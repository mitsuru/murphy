//! `Layout/SpaceAroundOperators` — flags binary operators that lack
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
//!
//! ## Out of scope (v1 limitations — see top of issue murphy-lpc.3.2)
//!
//! - Plain assignment `=`, conditional assignment `||=` / `&&=`, multiple
//!   assignment `a, b = …` (`Lvasgn`, `Ivasgn`, `Cvasgn`, `Gvasgn`, `Casgn`,
//!   `Masgn`, `OrAsgn`, `AndAsgn`). The dispatch list above does not include
//!   these kinds, so they remain unchecked.
//! - Index / call op-assign: `x[i] += 1` (`IndexOperatorWriteNode`) and
//!   `x.y += 1` (`CallOperatorWriteNode`). `murphy-translate` lowers both
//!   to `NodeKind::Unknown` in v1 so there is nothing for us to dispatch on.
//! - Hash rocket `=>` in pairs (`Pair`) and rescue clauses (`Resbody`).
//! - Class inheritance `<` (`Class`) and singleton-class `<<` (`Sclass`).
//! - Ternary `? :` (`If` with ternary form).
//! - Pattern-matching `in`/`=>` / `|` — Murphy has no `MatchPattern` hook yet
//!   (see issue: AST mismatch for the cop's `on_match_pattern` / `on_match_alt`
//!   / `on_match_as` handlers).
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
//! Tracked follow-ups: option-to-logic wiring is `murphy-xszo`; hook
//! expansion to `=` / `||=` / `&&=` / `=>` / `class<` / `class<<` /
//! ternary / match-pattern is `murphy-dvt8`; lowering
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
    use super::{
        SpaceAroundOperators, SpaceAroundOperatorsBinaryStyle, SpaceAroundOperatorsOptions,
    };
    use murphy_plugin_api::NodeCop;
    use murphy_plugin_api::test_support::{
        expect_correction, expect_no_corrections, expect_no_offenses, expect_offense, indoc,
        run_cop_with_edits,
    };

    // ---------- options surface (frozen v1 contract) ----------

    #[test]
    fn options_defaults_match_rubocop() {
        // Pin the public defaults — `[cops.rules."Layout/SpaceAroundOperators"]`
        // users today must see RuboCop-compatible defaults even though the
        // runtime check ignores the values.
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
        // The struct surface is frozen (murphy-lpc.3.2), the runtime
        // wiring lands in murphy-xszo. Until then, the v1 contract is:
        // setting `AllowForAlignment = false` or either `EnforcedStyle*`
        // = `space` must not change observable behaviour. Pin that here
        // via the typed-options test path so murphy-xszo's wiring shows
        // up as a deliberate test flip rather than a silent change.
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                a&&b
                 ^^ Surrounding space missing for operator `&&`.
            "#},
            &SpaceAroundOperatorsOptions {
                allow_for_alignment: false,
                enforced_style_for_exponent_operator: SpaceAroundOperatorsBinaryStyle::Space,
                enforced_style_for_rational_literals: SpaceAroundOperatorsBinaryStyle::Space,
            }
        );
    }

    /// Apply the cop's edits to `source` repeatedly until the result no
    /// longer changes. Used by the multi-operator-per-line tests where
    /// the `expect_offense!` annotation grammar's one-caret-per-line
    /// limit blocks direct expression — we still pin the corrected
    /// source for the canonical RuboCop case verbatim.
    fn apply_until_stable<T: NodeCop + Default>(source: &str) -> String {
        let mut current = source.to_string();
        for _ in 0..16 {
            let run = run_cop_with_edits::<T>(&current);
            if run.edits.is_empty() {
                return current;
            }
            let mut next = current.clone();
            // Apply non-overlapping edits in reverse so byte offsets
            // earlier in the source remain valid as we splice.
            let mut sorted = run.edits.clone();
            sorted.sort_by_key(|e| std::cmp::Reverse(e.range.start));
            for edit in sorted {
                let start = edit.range.start as usize;
                let end = edit.range.end as usize;
                next.replace_range(start..end, &edit.replacement);
            }
            if next == current {
                return next;
            }
            current = next;
        }
        panic!("apply_until_stable: fix-point did not converge in 16 iterations");
    }

    // ---------- Send: binary operator method calls ----------

    #[test]
    fn flags_missing_space_around_equals_equals() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                x==0
                 ^^ Surrounding space missing for operator `==`.
            "#}
        );
    }

    #[test]
    fn corrects_missing_space_around_equals_equals() {
        expect_correction!(
            SpaceAroundOperators,
            indoc! {r#"
                x==0
                 ^^ Surrounding space missing for operator `==`.
            "#},
            "x == 0\n"
        );
    }

    #[test]
    fn flags_missing_space_around_each_basic_binary_op() {
        // One operator per source line — the annotation grammar only
        // supports a single caret span per source line.
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
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
            "#}
        );
    }

    #[test]
    fn corrects_run_of_missing_spaces_on_one_line() {
        // Multi-operator single-line input — verified end-to-end via
        // `run_cop_with_edits` and an idempotent fix-point so we can
        // assert the corrected source while sidestepping the annotation
        // grammar's one-caret-per-line restriction.
        let before = "a+b-c*d/e%f\n";
        let after = apply_until_stable::<SpaceAroundOperators>(before);
        assert_eq!(after, "a + b - c * d / e % f\n");
    }

    #[test]
    fn flags_missing_space_around_equality_family() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                x==0
                 ^^ Surrounding space missing for operator `==`.
                y!=0
                 ^^ Surrounding space missing for operator `!=`.
                Hash===z
                    ^^^ Surrounding space missing for operator `===`.
            "#}
        );
    }

    #[test]
    fn flags_missing_space_around_match_operators() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                x=~/abc/
                 ^^ Surrounding space missing for operator `=~`.
            "#}
        );
    }

    #[test]
    fn flags_missing_space_around_shift_operator() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                x<<y
                 ^^ Surrounding space missing for operator `<<`.
            "#}
        );
    }

    #[test]
    fn flags_extra_space_around_binary_op() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                a  +  b
                   ^ Operator `+` should be surrounded by a single space.
            "#}
        );
    }

    #[test]
    fn corrects_extra_space_around_binary_op() {
        expect_correction!(
            SpaceAroundOperators,
            indoc! {r#"
                a  +  b
                   ^ Operator `+` should be surrounded by a single space.
            "#},
            "a + b\n"
        );
    }

    #[test]
    fn flags_extra_space_on_just_one_side() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                a +  b
                  ^ Operator `+` should be surrounded by a single space.
            "#}
        );
    }

    #[test]
    fn accepts_single_space_around_binary_op() {
        expect_no_offenses!(SpaceAroundOperators, "a + b\nx == 0\nh & i\n");
    }

    #[test]
    fn ignores_method_call_form() {
        expect_no_offenses!(SpaceAroundOperators, "Date.today.+(1).to_s\n");
    }

    #[test]
    fn ignores_operator_definitions() {
        expect_no_offenses!(
            SpaceAroundOperators,
            indoc! {r#"
                def +(other); end
                def self.===(other); end
            "#}
        );
    }

    #[test]
    fn ignores_unary_operators() {
        expect_no_offenses!(
            SpaceAroundOperators,
            indoc! {r#"
                x = +2
                y = -3
                arr.collect { |e| -e }
            "#}
        );
    }

    #[test]
    fn ignores_symbol_literals() {
        expect_no_offenses!(SpaceAroundOperators, "func(:-)\n");
    }

    #[test]
    fn ignores_ranges() {
        expect_no_offenses!(SpaceAroundOperators, "(1..2)\n(1...3)\n");
    }

    #[test]
    fn ignores_splat() {
        expect_no_offenses!(SpaceAroundOperators, "[*list, *tail]\n");
    }

    #[test]
    fn ignores_operator_at_end_of_line() {
        expect_no_offenses!(
            SpaceAroundOperators,
            indoc! {r#"
                'Here is a' +
                'joined string' +
                'across three lines'
            "#}
        );
    }

    #[test]
    fn flags_operator_at_end_of_line_when_missing_space_before_eol() {
        // `a+\n b` — leading space missing; trailing newline is OK but the
        // missing leading space is still flagged. Autocorrect should not
        // introduce trailing whitespace.
        expect_correction!(
            SpaceAroundOperators,
            indoc! {r#"
                'a'+
                   ^ Surrounding space missing for operator `+`.
                'b'
            "#},
            "'a' +\n'b'\n"
        );
    }

    #[test]
    fn ignores_operator_at_start_of_continuation_line() {
        expect_no_offenses!(
            SpaceAroundOperators,
            indoc! {r#"
                a = b \
                    && c
            "#}
        );
    }

    // ---------- And / Or ----------

    #[test]
    fn flags_missing_space_around_double_amp() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                a&&b
                 ^^ Surrounding space missing for operator `&&`.
            "#}
        );
    }

    #[test]
    fn corrects_missing_space_around_double_amp() {
        expect_correction!(
            SpaceAroundOperators,
            indoc! {r#"
                !a&&!b
                  ^^ Surrounding space missing for operator `&&`.
            "#},
            "!a && !b\n"
        );
    }

    #[test]
    fn flags_missing_space_around_double_pipe() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                a||b
                 ^^ Surrounding space missing for operator `||`.
            "#}
        );
    }

    #[test]
    fn accepts_word_form_and_or_with_spaces() {
        expect_no_offenses!(
            SpaceAroundOperators,
            indoc! {r#"
                a and b
                c or d
            "#}
        );
    }

    #[test]
    fn does_not_confuse_string_or_with_operator_or() {
        // `find_op_in_gap` searches the gap between `lhs.end` and
        // `rhs.start` for `||` then `or`. For an `Or` whose lhs is a
        // parenthesised string containing the substring `"or"`, the gap
        // does not include the string contents — it is just `) || `, so
        // we still find `||` and not the embedded `or`. Pin the contract.
        expect_no_offenses!(SpaceAroundOperators, "(x = \"or\") || y\n");
    }

    #[test]
    fn flags_extra_space_around_double_amp() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                a  &&  b
                   ^^ Operator `&&` should be surrounded by a single space.
            "#}
        );
    }

    // ---------- OpAsgn ----------

    #[test]
    fn flags_missing_space_around_plus_eq() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                x+=0
                 ^^ Surrounding space missing for operator `+=`.
            "#}
        );
    }

    #[test]
    fn corrects_missing_space_around_plus_eq() {
        expect_correction!(
            SpaceAroundOperators,
            indoc! {r#"
                y+= 0
                 ^^ Surrounding space missing for operator `+=`.
            "#},
            "y += 0\n"
        );
    }

    #[test]
    fn flags_missing_space_around_various_op_eq_shapes() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                z*=2
                 ^^ Surrounding space missing for operator `*=`.
                @a+=0
                  ^^ Surrounding space missing for operator `+=`.
                @@b-=0
                   ^^ Surrounding space missing for operator `-=`.
            "#}
        );
    }

    #[test]
    fn flags_extra_space_around_plus_eq() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                x  +=  0
                   ^^ Operator `+=` should be surrounded by a single space.
            "#}
        );
    }

    #[test]
    fn flags_missing_space_around_shift_eq() {
        expect_offense!(
            SpaceAroundOperators,
            indoc! {r#"
                x<<=2
                 ^^^ Surrounding space missing for operator `<<=`.
            "#}
        );
    }

    // ---------- documented v1 gaps — should not register offenses today ----------

    #[test]
    fn v1_gap_plain_assignment_not_flagged() {
        // Lvasgn — RuboCop flags `x=0`; v1 documents this as out of scope.
        expect_no_offenses!(SpaceAroundOperators, "x=0\n");
    }

    #[test]
    fn v1_gap_hash_rocket_not_flagged() {
        // Pair — RuboCop flags `{ 1=>2 }`; v1 does not dispatch on Pair.
        expect_no_offenses!(SpaceAroundOperators, "{ 1=>2 }\n");
    }

    #[test]
    fn v1_gap_conditional_assignment_not_flagged() {
        // OrAsgn / AndAsgn — RuboCop flags `x||=0`; v1 only dispatches on OpAsgn.
        expect_no_offenses!(SpaceAroundOperators, "x||=0\ny&&=0\n");
    }

    #[test]
    fn v1_gap_class_inheritance_not_flagged() {
        // Class — RuboCop flags `class C<D`; v1 does not dispatch on Class.
        expect_no_offenses!(
            SpaceAroundOperators,
            indoc! {r#"
                class C<D
                end
            "#}
        );
    }

    #[test]
    fn v1_gap_singleton_class_not_flagged() {
        // Sclass — RuboCop flags `class<<self`; v1 does not dispatch on Sclass.
        // Pins the v1 contract — if a future change starts dispatching on
        // Sclass without thinking it through, this test fails loudly.
        expect_no_offenses!(
            SpaceAroundOperators,
            indoc! {r#"
                class<<self
                end
            "#}
        );
    }

    #[test]
    fn v1_gap_ternary_not_flagged() {
        // If (ternary form) — RuboCop flags `x == 0?1:2`; v1 does not
        // dispatch on ternary `?` / `:` since `If` is not in our hook list.
        expect_no_offenses!(SpaceAroundOperators, "x == 0?1:2\n");
    }

    #[test]
    fn v1_gap_index_op_asgn_not_flagged() {
        // IndexOperatorWriteNode — `x[i] += 1` is currently lowered to
        // `Unknown` by murphy-translate; no OpAsgn fires, so the cop has
        // nothing to inspect.
        expect_no_offenses!(SpaceAroundOperators, "x[i]+=1\n");
    }

    // ---------- multiple offenses + idempotent autocorrect ----------

    #[test]
    fn corrects_run_of_op_asgn_and_binary() {
        // Same fix-point assertion shape as
        // `corrects_run_of_missing_spaces_on_one_line` — the canonical
        // RuboCop case `x+= a+b-c` has three operators on one line.
        let before = "x+= a+b-c\n";
        let after = apply_until_stable::<SpaceAroundOperators>(before);
        assert_eq!(after, "x += a + b - c\n");
    }

    #[test]
    fn leaves_clean_program_without_corrections() {
        expect_no_corrections!(
            SpaceAroundOperators,
            indoc! {r#"
                x = 1 + 2
                y = a && b
                z += 4
                w = c || d
            "#}
        );
    }
}
