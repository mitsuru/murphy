//! `Layout/SpaceAroundOperators` — flags binary operators that lack
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAroundOperators
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-lfn4
//! notes: >
//!   Known gaps: index/call op-assign (Unknown nodes), setter-method `=`
//!   (x.y=2), pattern-match operators, AllowForAlignment runtime wiring,
//!   and EnforcedStyle* options.
//! ```
//!
//! surrounding whitespace or have more than one space on either side,
//! and autocorrects them to ` <op> `. Mirrors the missing-/extra-space
//! halves of RuboCop's same-named cop.
//!
//! ## Matched shapes
//!
//! - `a + b` style binary method calls — `Send` with `method` ∈
//!   { `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `===`, `<=>`, `<=`, `>=`,
//!   `<`, `>`, `&`, `|`, `^`, `<<`, `>>`, `=~`, `!~` }.
//! - `a && b` / `a and b` — `And`.
//! - `a || b` / `a or b` — `Or`.
//! - `a += b` (and other `op=` shapes Prism lowers to `OpAsgn`) — `OpAsgn`.
//! - `x = 0` / `@a = 0` / `@@c = 0` / `$g = 0` / `A = 0` — plain local,
//!   instance, class, global, and constant assignment (`Lvasgn`, `Ivasgn`,
//!   `Cvasgn`, `Gvasgn`, `Casgn`). Value-less targets inside `OpAsgn` /
//!   `OrAsgn` / `AndAsgn` / `Masgn` are skipped (no `=` token there).
//! - `a, b = 1, 2` — multiple assignment (`Masgn`), the `=` between `Mlhs`
//!   and the RHS.
//! - `x ||= 0` / `x &&= 0` — conditional assignment (`OrAsgn`, `AndAsgn`).
//! - `{ key => value }` — hash rocket pairs only (`Pair`; colon-style pairs
//!   are ignored).
//! - `rescue Exception => e` — rescue binding arrow (`Resbody`).
//! - `class C < D` — class inheritance operator (`Class`).
//! - `class << self` — singleton-class operator (`Sclass`).
//! - `a ? b : c` — ternary `?` and `:` (`If` with ternary form).
//!
//! ## Out of scope (remaining v1 limitations)
//!
//! - Index / call op-assign: `x[i] += 1` (`IndexOperatorWriteNode`) and
//!   `x.y += 1` (`CallOperatorWriteNode`). `murphy-translate` lowers both
//!   to `NodeKind::Unknown` in v1 so there is nothing for us to dispatch on.
//! - Setter-method assignment `x.y = 2` (a `Send` with a trailing-`=`
//!   method name) — not in the binary-operator method list and not a plain
//!   `Lvasgn`, so this shape is skipped.
//! - Optional-parameter defaults `def f(x=0)` — handled by
//!   `Style/SpaceAroundEqualsInParameterDefault` in RuboCop, so Murphy
//!   deliberately leaves `Optarg` / `Kwoptarg` to a separate cop.
//! - Pattern-matching `in`/`=>` / `|` — Murphy has no `MatchPattern` hook
//!   yet (see issue: AST mismatch for the cop's `on_match_pattern` /
//!   `on_match_alt` / `on_match_as` handlers).
//! - `**` (exponent) and `/` followed by a rational literal — RuboCop's
//!   defaults keep these flush; v1 does not honor
//!   `EnforcedStyleForExponentOperator` / `EnforcedStyleForRationalLiterals`
//!   so `**` is not dispatched and `/` is treated like any other binary op.
//! - `AllowForAlignment` — declared as a config key (default `true`,
//!   matching RuboCop) so the `murphy.toml` surface is frozen, but the
//!   v1 dispatch ignores the flag and flags vertical alignment as excess
//!   space. Tracked separately for runtime wiring.
//! - Trailing comment alignment after the operator (`foo +  # comment`).
//!
//! Users who hit a false positive can disable per project via
//! `[cops.rules."Layout/SpaceAroundOperators"] enabled = false`.
//!
//! ## Options (frozen v1 surface — not yet honored at runtime)
//!
//! The cop's option struct declares the three RuboCop keys with their
//! upstream defaults so `murphy.toml` can already reference them. The
//! v1 check ignores the values and behaves as if all three were at
//! their defaults; the schema is still exported so config validation
//! (murphy-9cr.9) fails closed on bad values:
//!
//! - `AllowForAlignment` (`bool`, default `true`).
//! - `EnforcedStyleForExponentOperator` (`no_space` | `space`,
//!   default `no_space`).
//! - `EnforcedStyleForRationalLiterals` (`no_space` | `space`,
//!   default `no_space`).
//!
//! Tracked follow-ups: option-to-logic wiring is `murphy-xszo`; lowering
//! `IndexOperatorWriteNode` + `CallOperatorWriteNode` out of
//! `NodeKind::Unknown` is `murphy-9vwq`.
//!
//! ## Autocorrect
//!
//! Replaces the operator together with its surrounding spaces / tabs with
//! ` <op> `. When the operator sits at the end of a line (continuation
//! shape `'a' +` newline `'b'`) the trailing space is dropped so the fix
//! does not introduce a `Layout/TrailingWhitespace` offense on the next
//! pass.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceAroundOperators;

/// Cop options for [`SpaceAroundOperators`].
///
/// All three keys mirror RuboCop's `Layout/SpaceAroundOperators` config
/// surface (key names and defaults included). The v1 cop body does not
/// yet branch on these values — the struct is declared up front so
/// `murphy.toml` users can reference the keys today without a future
/// config rename, and so the host-side validation gate (murphy-9cr.9)
/// can enforce the enum values via the generated schema. See the
/// "Options" section of the file's top doc-comment.
#[derive(CopOptions)]
pub struct SpaceAroundOperatorsOptions {
    #[option(
        name = "AllowForAlignment",
        default = true,
        description = "Allow extra spacing if used to align operators on adjacent lines."
    )]
    pub allow_for_alignment: bool,

    #[option(
        name = "EnforcedStyleForExponentOperator",
        default = "no_space",
        description = "Spacing around the `**` operator."
    )]
    pub enforced_style_for_exponent_operator: SpaceAroundOperatorsBinaryStyle,

    #[option(
        name = "EnforcedStyleForRationalLiterals",
        default = "no_space",
        description = "Spacing around `/` when the right-hand side is a rational literal."
    )]
    pub enforced_style_for_rational_literals: SpaceAroundOperatorsBinaryStyle,
}

/// Shared `no_space | space` enum reused by both
/// `EnforcedStyleForExponentOperator` and `EnforcedStyleForRationalLiterals`
/// — RuboCop documents identical accepted values for the two keys.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpaceAroundOperatorsBinaryStyle {
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "space")]
    Space,
}

#[cop(
    name = "Layout/SpaceAroundOperators",
    description = "Flag missing or extra whitespace around binary operators.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceAroundOperatorsOptions,
)]
impl SpaceAroundOperators {
    #[on_node(kind = "send", methods = [
        "+", "-", "*", "/", "%",
        "==", "!=", "===", "<=>",
        "<=", ">=", "<", ">",
        "&", "|", "^", "<<", ">>",
        "=~", "!~",
    ])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        // Unary `-x` / `+x` arrive with `receiver = NONE` and a `-@`/`+@`
        // method — the method-name filter already excludes the latter, but
        // an explicit guard keeps the intent obvious.
        let Some(receiver_id) = receiver.get() else {
            return;
        };
        let arg_ids = cx.list(args);
        // Binary operator dispatch only — multi-arg / zero-arg `Send`s with
        // matching method names (e.g. `def +(other, *rest)` calls) are not
        // ours.
        if arg_ids.len() != 1 {
            return;
        }
        let op_range = cx.loc(node).name;
        if op_range == Range::ZERO {
            return;
        }
        // `a.+(b)` — explicit method-call syntax. The selector range still
        // points at `+`, but RuboCop's `regular_operator?` excludes any
        // call that goes through a `.`. We mirror that by checking the gap
        // between the receiver end and the operator start.
        let recv_end = cx.range(receiver_id).end;
        if recv_end < op_range.start {
            let pre_op = Range {
                start: recv_end,
                end: op_range.start,
            };
            if cx.raw_source(pre_op).contains('.') {
                return;
            }
        }
        check_operator(cx, op_range);
    }

    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::And { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        let gap = Range {
            start: cx.range(lhs).end,
            end: cx.range(rhs).start,
        };
        if let Some(op_range) = find_op_in_gap(cx, gap, &["&&", "and"]) {
            check_operator(cx, op_range);
        }
    }

    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Or { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        let gap = Range {
            start: cx.range(lhs).end,
            end: cx.range(rhs).start,
        };
        if let Some(op_range) = find_op_in_gap(cx, gap, &["||", "or"]) {
            check_operator(cx, op_range);
        }
    }

    #[on_node(kind = "op_asgn")]
    fn check_op_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::OpAsgn { target, op, value } = *cx.kind(node) else {
            return;
        };
        let op_str = cx.symbol_str(op);
        // Prism's `binary_operator()` returns the op without the trailing `=`
        // (translate.rs: `let op = self.sym(&w.binary_operator())`), so
        // reattach it to get the full token: `+` -> `+=`, `<<` -> `<<=`.
        let full_op = format!("{op_str}=");
        let gap = Range {
            start: cx.range(target).end,
            end: cx.range(value).start,
        };
        let candidates = [full_op.as_str()];
        if let Some(op_range) = find_op_in_gap(cx, gap, &candidates) {
            check_operator(cx, op_range);
        }
    }

    // --- Plain assignment `=` ---

    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Lvasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(value_id) = value.get() {
            check_plain_asgn(cx, node, value_id);
        }
    }

    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Ivasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(value_id) = value.get() {
            check_plain_asgn(cx, node, value_id);
        }
    }

    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Gvasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(value_id) = value.get() {
            check_plain_asgn(cx, node, value_id);
        }
    }

    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Cvasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(value_id) = value.get() {
            check_plain_asgn(cx, node, value_id);
        }
    }

    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Casgn { value, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(value_id) = value.get() {
            check_plain_asgn(cx, node, value_id);
        }
    }

    #[on_node(kind = "masgn")]
    fn check_masgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Masgn { lhs: _, rhs } = *cx.kind(node) else {
            return;
        };
        // The Mlhs node's range covers the entire multi-assignment expression,
        // not just the LHS targets. Search from the node start (before the first
        // target) to the RHS start — the gap contains only targets, commas,
        // spaces, and the `=` token, none of which are a bare `=` except the
        // assignment operator.
        let gap = Range {
            start: cx.range(node).start,
            end: cx.range(rhs).start,
        };
        if let Some(op_range) = find_op_in_gap(cx, gap, &["="]) {
            check_operator(cx, op_range);
        }
    }

    // --- Conditional assignment `||=` / `&&=` ---

    #[on_node(kind = "or_asgn")]
    fn check_or_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::OrAsgn { target, value } = *cx.kind(node) else {
            return;
        };
        let gap = Range {
            start: cx.range(target).end,
            end: cx.range(value).start,
        };
        if let Some(op_range) = find_op_in_gap(cx, gap, &["||="]) {
            check_operator(cx, op_range);
        }
    }

    #[on_node(kind = "and_asgn")]
    fn check_and_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::AndAsgn { target, value } = *cx.kind(node) else {
            return;
        };
        let gap = Range {
            start: cx.range(target).end,
            end: cx.range(value).start,
        };
        if let Some(op_range) = find_op_in_gap(cx, gap, &["&&="]) {
            check_operator(cx, op_range);
        }
    }

    // --- Hash rocket `=>` ---

    #[on_node(kind = "pair")]
    fn check_pair(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.is_hash_rocket(node) {
            return;
        }
        let op_range = cx.pair_operator_loc(node);
        if op_range != Range::ZERO {
            check_operator(cx, op_range);
        }
    }

    // --- Rescue `=>` binding ---

    #[on_node(kind = "resbody")]
    fn check_resbody(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Resbody { exceptions, var, .. } = *cx.kind(node) else {
            return;
        };
        let Some(var_id) = var.get() else {
            return;
        };
        // Search from the last exception's end (or the node start when there
        // are no exceptions) to the binding variable's start.
        let from = cx
            .list(exceptions)
            .last()
            .map(|&e| cx.range(e).end)
            .unwrap_or_else(|| cx.range(node).start);
        let gap = Range {
            start: from,
            end: cx.range(var_id).start,
        };
        if let Some(op_range) = find_op_in_gap(cx, gap, &["=>"]) {
            check_operator(cx, op_range);
        }
    }

    // --- Class inheritance `<` ---

    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class {
            name, superclass, ..
        } = *cx.kind(node)
        else {
            return;
        };
        let Some(super_id) = superclass.get() else {
            return;
        };
        let gap = Range {
            start: cx.range(name).end,
            end: cx.range(super_id).start,
        };
        if let Some(op_range) = find_op_in_gap(cx, gap, &["<"]) {
            check_operator(cx, op_range);
        }
    }

    // --- Singleton-class `<<` ---

    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Sclass { expr, .. } = *cx.kind(node) else {
            return;
        };
        // Search from the node start (the `class` keyword) to the expression.
        let gap = Range {
            start: cx.range(node).start,
            end: cx.range(expr).start,
        };
        if let Some(op_range) = find_op_in_gap(cx, gap, &["<<"]) {
            check_operator(cx, op_range);
        }
    }

    // --- Ternary `?` and `:` ---

    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.is_ternary(node) {
            return;
        }
        let q_range = cx.ternary_question_loc(node);
        if q_range != Range::ZERO {
            check_operator(cx, q_range);
        }
        let c_range = cx.ternary_colon_loc(node);
        if c_range != Range::ZERO {
            check_operator(cx, c_range);
        }
    }
}

/// Find the `=` operator token in the gap between a plain-assignment node's
/// start and the value's start, then check spacing around it.
///
/// Called for `Lvasgn`, `Ivasgn`, `Gvasgn`, `Cvasgn`, `Casgn` — all have
/// the `=` immediately after the target name (or scope path). The gap from
/// `node.start` to `value.start` safely bounds the search because none of
/// those names / paths can contain a bare `=` token.
fn check_plain_asgn(cx: &Cx<'_>, node: NodeId, value_id: NodeId) {
    let gap = Range {
        start: cx.range(node).start,
        end: cx.range(value_id).start,
    };
    if let Some(op_range) = find_op_in_gap(cx, gap, &["="]) {
        check_operator(cx, op_range);
    }
}

/// Inspect the operator at `op_range` and emit an offense + autocorrect
/// edit if it is missing surrounding space or has more than one space on
/// either side. Operators at the start of a line (continuation shape) are
/// silently accepted, matching RuboCop's
/// `with_space.source.start_with?("\n")` early return.
fn check_operator(cx: &Cx<'_>, op_range: Range) {
    let src = cx.source().as_bytes();
    let op_start = op_range.start as usize;
    let op_end = op_range.end as usize;
    if op_start >= op_end || op_end > src.len() {
        return;
    }

    // Expand leading whitespace. Spaces / tabs only — a `\n` stops the
    // expansion so the next byte before `leading_start` tells us whether
    // the operator is at the start of a line.
    let mut leading_start = op_start;
    while leading_start > 0 && matches!(src[leading_start - 1], b' ' | b'\t') {
        leading_start -= 1;
    }
    // Expand trailing whitespace.
    let mut trailing_end = op_end;
    while trailing_end < src.len() && matches!(src[trailing_end], b' ' | b'\t') {
        trailing_end += 1;
    }

    // Operator at the start of a line (possibly indented continuation) is
    // accepted — both RuboCop and Murphy assume the previous line carries
    // the missing-space context (`a = b \` + newline + `    && c`).
    if leading_start == 0 || matches!(src[leading_start - 1], b'\n' | b'\r') {
        return;
    }

    let leading_count = op_start - leading_start;
    let trailing_count = trailing_end - op_end;
    let at_eol = trailing_end >= src.len() || matches!(src[trailing_end], b'\n' | b'\r');

    let op_text = match std::str::from_utf8(&src[op_start..op_end]) {
        Ok(t) => t,
        Err(_) => return,
    };

    if leading_count == 0 || (trailing_count == 0 && !at_eol) {
        emit_fix(
            cx,
            op_range,
            leading_start,
            trailing_end,
            op_text,
            at_eol,
            &format!("Surrounding space missing for operator `{op_text}`."),
        );
    } else if leading_count > 1 || (trailing_count > 1 && !at_eol) {
        emit_fix(
            cx,
            op_range,
            leading_start,
            trailing_end,
            op_text,
            at_eol,
            &format!("Operator `{op_text}` should be surrounded by a single space."),
        );
    }
}

/// Emit the offense at the operator range and the autocorrect edit that
/// rewrites `[leading_start, trailing_end)` to ` <op>` (drop trailing space
/// at EOL so we don't introduce trailing whitespace) or ` <op> `.
fn emit_fix(
    cx: &Cx<'_>,
    op_range: Range,
    leading_start: usize,
    trailing_end: usize,
    op_text: &str,
    at_eol: bool,
    message: &str,
) {
    cx.emit_offense(op_range, message, None);
    let trailing_space = if at_eol { "" } else { " " };
    let replacement = format!(" {op_text}{trailing_space}");
    cx.emit_edit(
        Range {
            start: leading_start as u32,
            end: trailing_end as u32,
        },
        &replacement,
    );
}

/// Search `gap`'s source for the first occurrence of one of `candidates`
/// (operator literals — `&&` / `and` for `And`, `||` / `or` for `Or`,
/// `+=` / `<<=` / … for `OpAsgn`). Alphabetic keyword candidates (`and`,
/// `or`) require word boundaries so we don't catch `andante` mid-string;
/// in practice the gap between an `And`'s `lhs.end` and `rhs.start` is
/// just whitespace and the keyword.
fn find_op_in_gap(cx: &Cx<'_>, gap: Range, candidates: &[&str]) -> Option<Range> {
    if gap.start >= gap.end {
        return None;
    }
    let gap_text = cx.raw_source(gap);
    let gap_bytes = gap_text.as_bytes();
    let mut best: Option<(usize, usize)> = None;
    for op in candidates {
        let bytes = op.as_bytes();
        if bytes.is_empty() || bytes.len() > gap_bytes.len() {
            continue;
        }
        let alphabetic = bytes.iter().all(|b| b.is_ascii_alphabetic());
        let upper_bound = gap_bytes.len() - bytes.len() + 1;
        for i in 0..upper_bound {
            if &gap_bytes[i..i + bytes.len()] != bytes {
                continue;
            }
            if alphabetic {
                let before_ok = i == 0 || !is_word_char(gap_bytes[i - 1]);
                let after_ok =
                    i + bytes.len() >= gap_bytes.len() || !is_word_char(gap_bytes[i + bytes.len()]);
                if !before_ok || !after_ok {
                    continue;
                }
            }
            if best.map(|(bi, _)| i < bi).unwrap_or(true) {
                best = Some((i, bytes.len()));
            }
            break;
        }
    }
    best.map(|(i, len)| Range {
        start: gap.start + i as u32,
        end: gap.start + (i + len) as u32,
    })
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    //! Tests use the tester-builder API (`test::<T>().expect_*()`) so the
    //! cop's `Options` struct flows through the chain typed — see the
    //! `options_are_accepted_but_not_yet_honored` test for the
    //! non-default-options shape.

    use super::{
        SpaceAroundOperators, SpaceAroundOperatorsBinaryStyle, SpaceAroundOperatorsOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    // ---------- options surface (frozen v1 contract) ----------

    #[test]
    fn options_defaults_match_rubocop() {
        let d = SpaceAroundOperatorsOptions::default();
        assert!(d.allow_for_alignment);
        assert_eq!(
            d.enforced_style_for_exponent_operator,
            SpaceAroundOperatorsBinaryStyle::NoSpace
        );
        assert_eq!(
            d.enforced_style_for_rational_literals,
            SpaceAroundOperatorsBinaryStyle::NoSpace
        );
    }

    #[test]
    fn options_are_accepted_but_not_yet_honored() {
        test::<SpaceAroundOperators>()
            .with_options(&SpaceAroundOperatorsOptions {
                allow_for_alignment: false,
                enforced_style_for_exponent_operator: SpaceAroundOperatorsBinaryStyle::Space,
                enforced_style_for_rational_literals: SpaceAroundOperatorsBinaryStyle::Space,
            })
            .expect_offense(indoc! {r#"
                a&&b
                 ^^ Surrounding space missing for operator `&&`.
            "#});
    }

    // ---------- Send: binary operator method calls ----------

    #[test]
    fn flags_and_corrects_missing_space_around_equals_equals() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                x==0
                 ^^ Surrounding space missing for operator `==`.
            "#})
            .expect_correction(
                indoc! {r#"
                    x==0
                     ^^ Surrounding space missing for operator `==`.
                "#},
                "x == 0\n",
            );
    }

    #[test]
    fn flags_missing_space_around_each_basic_binary_op() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            a+b
             ^ Surrounding space missing for operator `+`.
            c-d
             ^ Surrounding space missing for operator `-`.
            e*f
             ^ Surrounding space missing for operator `*`.
            g/h
             ^ Surrounding space missing for operator `/`.
            i%j
             ^ Surrounding space missing for operator `%`.
            k&l
             ^ Surrounding space missing for operator `&`.
            m|n
             ^ Surrounding space missing for operator `|`.
            o^p
             ^ Surrounding space missing for operator `^`.
        "#});
    }

    #[test]
    fn corrects_run_of_missing_spaces_on_one_line() {
        test::<SpaceAroundOperators>().expect_correction(
            indoc! {r#"
                a+b-c*d/e%f
                 ^ Surrounding space missing for operator `+`.
                   ^ Surrounding space missing for operator `-`.
                     ^ Surrounding space missing for operator `*`.
                       ^ Surrounding space missing for operator `/`.
                         ^ Surrounding space missing for operator `%`.
            "#},
            "a + b - c * d / e % f\n",
        );
    }

    #[test]
    fn flags_missing_space_around_equality_family() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            x==0
             ^^ Surrounding space missing for operator `==`.
            y!=0
             ^^ Surrounding space missing for operator `!=`.
            Hash===z
                ^^^ Surrounding space missing for operator `===`.
        "#});
    }

    #[test]
    fn flags_missing_space_around_match_operators() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            x=~/abc/
             ^^ Surrounding space missing for operator `=~`.
        "#});
    }

    #[test]
    fn flags_missing_space_around_shift_operator() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            x<<y
             ^^ Surrounding space missing for operator `<<`.
        "#});
    }

    #[test]
    fn flags_and_corrects_extra_space_around_binary_op() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                a  +  b
                   ^ Operator `+` should be surrounded by a single space.
            "#})
            .expect_correction(
                indoc! {r#"
                    a  +  b
                       ^ Operator `+` should be surrounded by a single space.
                "#},
                "a + b\n",
            );
    }

    #[test]
    fn flags_extra_space_on_just_one_side() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            a +  b
              ^ Operator `+` should be surrounded by a single space.
        "#});
    }

    #[test]
    fn accepts_canonical_shapes() {
        test::<SpaceAroundOperators>()
            .expect_no_offenses("a + b\nx == 0\nh & i\n")
            .expect_no_offenses("Date.today.+(1).to_s\n")
            .expect_no_offenses(indoc! {r#"
                def +(other); end
                def self.===(other); end
            "#})
            .expect_no_offenses(indoc! {r#"
                x = +2
                y = -3
                arr.collect { |e| -e }
            "#})
            .expect_no_offenses("func(:-)\n")
            .expect_no_offenses("(1..2)\n(1...3)\n")
            .expect_no_offenses("[*list, *tail]\n");
    }

    #[test]
    fn ignores_operator_at_end_of_line() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            'Here is a' +
            'joined string' +
            'across three lines'
        "#});
    }

    #[test]
    fn flags_operator_at_end_of_line_when_missing_space_before_eol() {
        test::<SpaceAroundOperators>().expect_correction(
            indoc! {r#"
                'a'+
                   ^ Surrounding space missing for operator `+`.
                'b'
            "#},
            "'a' +\n'b'\n",
        );
    }

    #[test]
    fn ignores_operator_at_start_of_continuation_line() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            a = b \
                && c
        "#});
    }

    // ---------- And / Or ----------

    #[test]
    fn flags_and_corrects_missing_space_around_double_amp() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                a&&b
                 ^^ Surrounding space missing for operator `&&`.
            "#})
            .expect_correction(
                indoc! {r#"
                    !a&&!b
                      ^^ Surrounding space missing for operator `&&`.
                "#},
                "!a && !b\n",
            );
    }

    #[test]
    fn flags_missing_space_around_double_pipe() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            a||b
             ^^ Surrounding space missing for operator `||`.
        "#});
    }

    #[test]
    fn accepts_word_form_and_or_with_spaces() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            a and b
            c or d
        "#});
    }

    #[test]
    fn does_not_confuse_string_or_with_operator_or() {
        test::<SpaceAroundOperators>().expect_no_offenses("(x = \"or\") || y\n");
    }

    #[test]
    fn flags_extra_space_around_double_amp() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            a  &&  b
               ^^ Operator `&&` should be surrounded by a single space.
        "#});
    }

    // ---------- OpAsgn ----------

    #[test]
    fn flags_and_corrects_missing_space_around_plus_eq() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                x+=0
                 ^^ Surrounding space missing for operator `+=`.
            "#})
            .expect_correction(
                indoc! {r#"
                    y+= 0
                     ^^ Surrounding space missing for operator `+=`.
                "#},
                "y += 0\n",
            );
    }

    #[test]
    fn flags_missing_space_around_various_op_eq_shapes() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            z*=2
             ^^ Surrounding space missing for operator `*=`.
            @a+=0
              ^^ Surrounding space missing for operator `+=`.
            @@b-=0
               ^^ Surrounding space missing for operator `-=`.
        "#});
    }

    #[test]
    fn flags_extra_space_around_plus_eq() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            x  +=  0
               ^^ Operator `+=` should be surrounded by a single space.
        "#});
    }

    #[test]
    fn flags_missing_space_around_shift_eq() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            x<<=2
             ^^^ Surrounding space missing for operator `<<=`.
        "#});
    }

    #[test]
    fn flags_missing_space_around_call_and_index_op_assign() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                x.y+=1
                   ^^ Surrounding space missing for operator `+=`.
            "#})
            .expect_correction(
                indoc! {r#"
                    x[i]+=1
                        ^^ Surrounding space missing for operator `+=`.
                "#},
                "x[i] += 1\n",
            );
    }

    // ---------- Plain assignment `=` ----------

    #[test]
    fn flags_and_corrects_missing_space_around_lvasgn() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                x=0
                 ^ Surrounding space missing for operator `=`.
            "#})
            .expect_correction(
                indoc! {r#"
                    x=0
                     ^ Surrounding space missing for operator `=`.
                "#},
                "x = 0\n",
            );
    }

    #[test]
    fn flags_missing_space_around_ivasgn() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            @a=0
              ^ Surrounding space missing for operator `=`.
        "#});
    }

    #[test]
    fn flags_missing_space_around_cvasgn() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            @@b=0
               ^ Surrounding space missing for operator `=`.
        "#});
    }

    #[test]
    fn flags_missing_space_around_gvasgn() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            $g=0
              ^ Surrounding space missing for operator `=`.
        "#});
    }

    #[test]
    fn flags_missing_space_around_casgn() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            A=0
             ^ Surrounding space missing for operator `=`.
        "#});
    }

    #[test]
    fn flags_missing_space_around_scoped_casgn() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            Foo::BAR=0
                    ^ Surrounding space missing for operator `=`.
        "#});
    }

    #[test]
    fn flags_missing_space_around_masgn() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            a, b=1, 2
                ^ Surrounding space missing for operator `=`.
        "#});
    }

    #[test]
    fn accepts_well_spaced_plain_assignments() {
        test::<SpaceAroundOperators>()
            .expect_no_offenses("x = 0\n")
            .expect_no_offenses("@a = 0\n")
            .expect_no_offenses("@@b = 0\n")
            .expect_no_offenses("$g = 0\n")
            .expect_no_offenses("A = 0\n")
            .expect_no_offenses("Foo::BAR = 0\n")
            .expect_no_offenses("a, b = 1, 2\n");
    }

    #[test]
    fn skips_value_less_lvasgn_targets_inside_op_asgn() {
        // `x += 1` is OpAsgn whose target is a value-less Lvasgn; the
        // plain-assignment handler must not fire on that target.
        test::<SpaceAroundOperators>().expect_no_offenses("x += 1\n");
    }

    // ---------- Conditional assignment `||=` / `&&=` ----------

    #[test]
    fn flags_and_corrects_missing_space_around_or_asgn() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                x||=0
                 ^^^ Surrounding space missing for operator `||=`.
            "#})
            .expect_correction(
                indoc! {r#"
                    x||=0
                     ^^^ Surrounding space missing for operator `||=`.
                "#},
                "x ||= 0\n",
            );
    }

    #[test]
    fn flags_missing_space_around_and_asgn() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            y&&=0
             ^^^ Surrounding space missing for operator `&&=`.
        "#});
    }

    #[test]
    fn flags_missing_space_around_ivar_or_asgn() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            @a||=0
              ^^^ Surrounding space missing for operator `||=`.
        "#});
    }

    #[test]
    fn flags_extra_space_around_or_asgn() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            x  ||=  0
               ^^^ Operator `||=` should be surrounded by a single space.
        "#});
    }

    #[test]
    fn accepts_well_spaced_conditional_assignments() {
        test::<SpaceAroundOperators>()
            .expect_no_offenses("x ||= 0\n")
            .expect_no_offenses("y &&= 0\n");
    }

    // ---------- Hash rocket `=>` ----------

    #[test]
    fn flags_and_corrects_missing_space_around_hash_rocket() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                { 1=>2 }
                   ^^ Surrounding space missing for operator `=>`.
            "#})
            .expect_correction(
                indoc! {r#"
                    { 1=>2 }
                       ^^ Surrounding space missing for operator `=>`.
                "#},
                "{ 1 => 2 }\n",
            );
    }

    #[test]
    fn flags_extra_space_around_hash_rocket() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            { 1  =>  2 }
                 ^^ Operator `=>` should be surrounded by a single space.
        "#});
    }

    #[test]
    fn accepts_well_spaced_hash_rocket() {
        test::<SpaceAroundOperators>().expect_no_offenses("{ 1 => 2 }\n");
    }

    #[test]
    fn ignores_colon_style_hash_pairs() {
        test::<SpaceAroundOperators>().expect_no_offenses("{ a: 1, b: 2 }\n");
    }

    // ---------- Rescue `=>` ----------

    #[test]
    fn flags_and_corrects_missing_space_around_rescue_rocket() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                begin
                rescue Exception=>e
                                ^^ Surrounding space missing for operator `=>`.
                end
            "#})
            .expect_correction(
                indoc! {r#"
                    begin
                    rescue Exception=>e
                                    ^^ Surrounding space missing for operator `=>`.
                    end
                "#},
                "begin\nrescue Exception => e\nend\n",
            );
    }

    #[test]
    fn flags_missing_space_around_rescue_rocket_no_exception_class() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            begin
            rescue=>e
                  ^^ Surrounding space missing for operator `=>`.
            end
        "#});
    }

    #[test]
    fn accepts_well_spaced_rescue_rocket() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            begin
            rescue Exception => e
            end
        "#});
    }

    #[test]
    fn accepts_rescue_without_binding() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            begin
            rescue Exception
            end
        "#});
    }

    // ---------- Class inheritance `<` ----------

    #[test]
    fn flags_and_corrects_missing_space_around_class_lt() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                class C<D
                       ^ Surrounding space missing for operator `<`.
                end
            "#})
            .expect_correction(
                indoc! {r#"
                    class C<D
                           ^ Surrounding space missing for operator `<`.
                    end
                "#},
                "class C < D\nend\n",
            );
    }

    #[test]
    fn flags_extra_space_around_class_lt() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            class C  <  D
                     ^ Operator `<` should be surrounded by a single space.
            end
        "#});
    }

    #[test]
    fn accepts_well_spaced_class_inheritance() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            class C < D
            end
        "#});
    }

    #[test]
    fn accepts_class_without_superclass() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            class C
            end
        "#});
    }

    // ---------- Singleton class `<<` ----------

    #[test]
    fn flags_and_corrects_missing_space_around_sclass_shovel() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                class<<self
                     ^^ Surrounding space missing for operator `<<`.
                end
            "#})
            .expect_correction(
                indoc! {r#"
                    class<<self
                         ^^ Surrounding space missing for operator `<<`.
                    end
                "#},
                "class << self\nend\n",
            );
    }

    #[test]
    fn flags_extra_space_around_sclass_shovel() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            class  <<  self
                   ^^ Operator `<<` should be surrounded by a single space.
            end
        "#});
    }

    #[test]
    fn accepts_well_spaced_singleton_class() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            class << self
            end
        "#});
    }

    // ---------- Ternary `?` and `:` ----------

    #[test]
    fn flags_and_corrects_missing_space_around_ternary() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                x == 0?1:2
                      ^ Surrounding space missing for operator `?`.
                        ^ Surrounding space missing for operator `:`.
            "#})
            .expect_correction(
                indoc! {r#"
                    x == 0?1:2
                          ^ Surrounding space missing for operator `?`.
                            ^ Surrounding space missing for operator `:`.
                "#},
                "x == 0 ? 1 : 2\n",
            );
    }

    #[test]
    fn flags_extra_space_around_ternary() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            x == 0  ?  1  :  2
                    ^ Operator `?` should be surrounded by a single space.
                          ^ Operator `:` should be surrounded by a single space.
        "#});
    }

    #[test]
    fn accepts_well_spaced_ternary() {
        test::<SpaceAroundOperators>().expect_no_offenses("x == 0 ? 1 : 2\n");
    }

    #[test]
    fn accepts_non_ternary_if() {
        test::<SpaceAroundOperators>()
            .expect_no_offenses(indoc! {r#"
                if x == 0
                  1
                else
                  2
                end
            "#})
            .expect_no_offenses("1 if x == 0\n");
    }

    // ---------- remaining v1 gaps — should not register offenses ----------

    #[test]
    fn v1_gaps_are_silently_accepted() {
        // Optarg defaults are handled by SpaceAroundEqualsInParameterDefault.
        // Pattern-match operators remain out of scope (no MatchPattern node yet).
        test::<SpaceAroundOperators>().expect_no_offenses("def f(x=0); end\n");

    }

    // ---------- multiple offenses + idempotent autocorrect ----------

    #[test]
    fn corrects_run_of_op_asgn_and_binary() {
        test::<SpaceAroundOperators>().expect_correction(
            indoc! {r#"
                x+= a+b-c
                 ^^ Surrounding space missing for operator `+=`.
                     ^ Surrounding space missing for operator `+`.
                       ^ Surrounding space missing for operator `-`.
            "#},
            "x += a + b - c\n",
        );
    }

    #[test]
    fn leaves_clean_program_without_corrections() {
        test::<SpaceAroundOperators>().expect_no_corrections(indoc! {r#"
            x = 1 + 2
            y = a && b
            z += 4
            w = c || d
        "#});
    }
}
