//! `Layout/SpaceAroundOperators` — flags binary operators that lack
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAroundOperators
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-9vwq
//! notes: >
//!   Remaining gap: index/call op-assign (x[i]+=1, x.y+=1) — murphy-translate
//!   lowers IndexOperatorWriteNode/CallOperatorWriteNode to NodeKind::Unknown
//!   (murphy-9vwq).
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
//! - `x.y = 2` — setter-method `=` (`Send` with trailing-`=` method name
//!   and `cx.is_setter_method` true).
//! - `case x in A | B` — pattern alternation `|` (`MatchAlt`).
//! - `case x in Integer => n` — capture pattern `=>` (`MatchAs`).
//! - `x => n` — one-liner assignment pattern `=>` (`MatchPattern`).
//!
//! ## Out of scope (remaining limitations)
//!
//! - Index / call op-assign: `x[i] += 1` (`IndexOperatorWriteNode`) and
//!   `x.y += 1` (`CallOperatorWriteNode`). `murphy-translate` lowers both
//!   to `NodeKind::Unknown` so there is nothing to dispatch on (murphy-9vwq).
//! - Optional-parameter defaults `def f(x=0)` — handled by
//!   `Style/SpaceAroundEqualsInParameterDefault` in RuboCop; Murphy
//!   deliberately delegates `Optarg` / `Kwoptarg` to a separate cop.
//! - `x in Integer` — one-liner boolean pattern match (`MatchPatternP`);
//!   the `in` keyword spacing is handled by `SpaceAroundKeyword` in RuboCop.
//! - Trailing comment after the operator (`foo +  # comment`) — the extra
//!   space before `#` is silently accepted.
//!
//! Users who hit a false positive can disable per project via
//! `[cops.rules."Layout/SpaceAroundOperators"] enabled = false`.
//!
//! ## Options
//!
//! - `AllowForAlignment` (`bool`, default `true`) — when `true`, extra spaces
//!   that vertically align an operator with one on an adjacent line are
//!   silently accepted. The check looks one line up and one line down: if the
//!   same column holds a non-whitespace character on either neighbour, the
//!   spacing is treated as intentional alignment.
//! - `EnforcedStyleForExponentOperator` (`no_space` | `space`, default
//!   `no_space`) — with `no_space` (default), any space around `**` is an
//!   offense ("Space around operator `**` detected."); with `space`, missing
//!   space is an offense ("Surrounding space missing for operator `**`.").
//! - `EnforcedStyleForRationalLiterals` (`no_space` | `space`, default
//!   `no_space`) — controls `/` when the right-hand side is a rational
//!   literal (`1r`, `2.5r`). Same semantics as the exponent style.
//!
//! Tracked follow-up: lowering `IndexOperatorWriteNode` +
//! `CallOperatorWriteNode` out of `NodeKind::Unknown` is `murphy-9vwq`.
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
    // Binary-operator Send nodes and setter-method Send nodes are both
    // dispatched here. The `methods = [...]` filter would exclude setter
    // methods (`y=`) since they are not in the operator whitelist, so
    // we omit the filter and perform the method-name check manually.
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            args,
            ..
        } = *cx.kind(node)
        else {
            return;
        };

        let opts = cx.options_or_default::<SpaceAroundOperatorsOptions>();

        // --- Setter-method branch: `x.y = 2` ---
        // RuboCop's `setter_method?` gate: `loc?(:operator)` is set when
        // the call carries a standalone `=` operator token.
        if cx.is_setter_method(node) {
            let op_range = cx.assignment_operator_loc(node);
            if op_range != Range::ZERO {
                check_operator(cx, op_range, opts.allow_for_alignment);
            }
            return;
        }

        // --- Binary-operator branch ---
        let method_str = cx.symbol_str(method);
        // Only process recognised binary-operator method names.
        const BINARY_OPS: &[&str] = &[
            "+", "-", "*", "**", "/", "%", "==", "!=", "===", "<=>", "<=", ">=", "<", ">", "&",
            "|", "^", "<<", ">>", "=~", "!~",
        ];
        if !BINARY_OPS.contains(&method_str) {
            return;
        }
        // Unary `-x` / `+x` arrive with `receiver = NONE` — exclude.
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
        // `**` uses `EnforcedStyleForExponentOperator` (default `no_space`).
        if method_str == "**" {
            if opts.enforced_style_for_exponent_operator == SpaceAroundOperatorsBinaryStyle::NoSpace
            {
                check_operator_no_space(cx, op_range);
            } else {
                check_operator(cx, op_range, opts.allow_for_alignment);
            }
            return;
        }
        // `/` with a rational-literal RHS uses `EnforcedStyleForRationalLiterals`
        // (default `no_space`): `a/1r` is correct, `a / 1r` is an offense.
        if method_str == "/" {
            let rhs_id = arg_ids[0];
            if matches!(*cx.kind(rhs_id), NodeKind::Rational(_)) {
                if opts.enforced_style_for_rational_literals
                    == SpaceAroundOperatorsBinaryStyle::NoSpace
                {
                    check_operator_no_space(cx, op_range);
                } else {
                    check_operator(cx, op_range, opts.allow_for_alignment);
                }
                return;
            }
        }
        check_operator(cx, op_range, opts.allow_for_alignment);
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
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        if let Some(op_range) = find_op_in_gap(cx, gap, &["&&", "and"]) {
            check_operator(cx, op_range, afa);
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
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        if let Some(op_range) = find_op_in_gap(cx, gap, &["||", "or"]) {
            check_operator(cx, op_range, afa);
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
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        let candidates = [full_op.as_str()];
        if let Some(op_range) = find_op_in_gap(cx, gap, &candidates) {
            check_operator(cx, op_range, afa);
        }
    }

    // --- Plain assignment `=` ---

    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Lvasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(value_id) = value.get() {
            let afa = cx
                .options_or_default::<SpaceAroundOperatorsOptions>()
                .allow_for_alignment;
            check_plain_asgn(cx, node, value_id, afa);
        }
    }

    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Ivasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(value_id) = value.get() {
            let afa = cx
                .options_or_default::<SpaceAroundOperatorsOptions>()
                .allow_for_alignment;
            check_plain_asgn(cx, node, value_id, afa);
        }
    }

    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Gvasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(value_id) = value.get() {
            let afa = cx
                .options_or_default::<SpaceAroundOperatorsOptions>()
                .allow_for_alignment;
            check_plain_asgn(cx, node, value_id, afa);
        }
    }

    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Cvasgn { value, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(value_id) = value.get() {
            let afa = cx
                .options_or_default::<SpaceAroundOperatorsOptions>()
                .allow_for_alignment;
            check_plain_asgn(cx, node, value_id, afa);
        }
    }

    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Casgn { value, .. } = *cx.kind(node) else {
            return;
        };
        if let Some(value_id) = value.get() {
            let afa = cx
                .options_or_default::<SpaceAroundOperatorsOptions>()
                .allow_for_alignment;
            check_plain_asgn(cx, node, value_id, afa);
        }
    }

    #[on_node(kind = "masgn")]
    fn check_masgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Masgn { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        // The Mlhs range covers the entire multi-assignment node, not just the
        // LHS targets. Use the last LHS target's range end to bound the search
        // so that inner assignments inside complex targets (e.g. `a[i=1], b =
        // rhs`) do not produce a false positive by matching the inner `=`.
        let NodeKind::Mlhs(list) = *cx.kind(lhs) else {
            return;
        };
        let lhs_end = cx
            .list(list)
            .last()
            .map(|&t| cx.range(t).end)
            .unwrap_or(cx.range(node).start);
        let gap = Range {
            start: lhs_end,
            end: cx.range(rhs).start,
        };
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        if let Some(op_range) = find_op_in_gap(cx, gap, &["="]) {
            check_operator(cx, op_range, afa);
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
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        if let Some(op_range) = find_op_in_gap(cx, gap, &["||="]) {
            check_operator(cx, op_range, afa);
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
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        if let Some(op_range) = find_op_in_gap(cx, gap, &["&&="]) {
            check_operator(cx, op_range, afa);
        }
    }

    // --- Hash rocket `=>` ---

    #[on_node(kind = "pair")]
    fn check_pair(&self, node: NodeId, cx: &Cx<'_>) {
        let op_range = cx.pair_operator_loc(node);
        if op_range != Range::ZERO && cx.raw_source(op_range) == "=>" {
            let afa = cx
                .options_or_default::<SpaceAroundOperatorsOptions>()
                .allow_for_alignment;
            check_operator(cx, op_range, afa);
        }
    }

    // --- Rescue `=>` binding ---

    #[on_node(kind = "resbody")]
    fn check_resbody(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Resbody {
            exceptions, var, ..
        } = *cx.kind(node)
        else {
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
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        if let Some(op_range) = find_op_in_gap(cx, gap, &["=>"]) {
            check_operator(cx, op_range, afa);
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
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        if let Some(op_range) = find_op_in_gap(cx, gap, &["<"]) {
            check_operator(cx, op_range, afa);
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
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        if let Some(op_range) = find_op_in_gap(cx, gap, &["<<"]) {
            check_operator(cx, op_range, afa);
        }
    }

    // --- Ternary `?` and `:` ---

    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let q_range = cx.ternary_question_loc(node);
        if q_range == Range::ZERO {
            return;
        }
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        check_operator(cx, q_range, afa);
        let c_range = cx.ternary_colon_loc(node);
        if c_range != Range::ZERO {
            check_operator(cx, c_range, afa);
        }
    }

    // --- Pattern-match alternation `|` ---

    #[on_node(kind = "match_alt")]
    fn check_match_alt(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::MatchAlt { left, right } = *cx.kind(node) else {
            return;
        };
        let gap = Range {
            start: cx.range(left).end,
            end: cx.range(right).start,
        };
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        if let Some(op_range) = find_op_in_gap(cx, gap, &["|"]) {
            check_operator(cx, op_range, afa);
        }
    }

    // --- Pattern-match capture `=>` (`Integer => n` inside `in`) ---

    #[on_node(kind = "match_as")]
    fn check_match_as(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::MatchAs { value, name } = *cx.kind(node) else {
            return;
        };
        let gap = Range {
            start: cx.range(value).end,
            end: cx.range(name).start,
        };
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        if let Some(op_range) = find_op_in_gap(cx, gap, &["=>"]) {
            check_operator(cx, op_range, afa);
        }
    }

    // --- One-liner assignment pattern `x => n` (`MatchPattern`) ---

    #[on_node(kind = "match_pattern")]
    fn check_match_pattern(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::MatchPattern { value, pattern } = *cx.kind(node) else {
            return;
        };
        let gap = Range {
            start: cx.range(value).end,
            end: cx.range(pattern).start,
        };
        let afa = cx
            .options_or_default::<SpaceAroundOperatorsOptions>()
            .allow_for_alignment;
        if let Some(op_range) = find_op_in_gap(cx, gap, &["=>"]) {
            check_operator(cx, op_range, afa);
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
fn check_plain_asgn(cx: &Cx<'_>, node: NodeId, value_id: NodeId, allow_for_alignment: bool) {
    let gap = Range {
        start: cx.range(node).start,
        end: cx.range(value_id).start,
    };
    if let Some(op_range) = find_op_in_gap(cx, gap, &["="]) {
        check_operator(cx, op_range, allow_for_alignment);
    }
}

/// Inspect the operator at `op_range` and emit an offense + autocorrect
/// edit if it is missing surrounding space or has more than one space on
/// either side. Operators at the start of a line (continuation shape) are
/// silently accepted. When `allow_for_alignment` is `true` (the default
/// `AllowForAlignment` setting), extra spaces that vertically align the
/// operator with one on an adjacent line are also accepted.
fn check_operator(cx: &Cx<'_>, op_range: Range, allow_for_alignment: bool) {
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
    // RuboCop's `check_operator` returns when a trailing comment begins where the
    // operator's surrounding space ends (`with_space.last_column == comment.loc.column`):
    // the right operand is on the next line and the excess space merely reaches the
    // comment. Treat a following `#` like end-of-line so the trailing-space excess
    // is not flagged.
    let at_eol =
        trailing_end >= src.len() || matches!(src[trailing_end], b'\n' | b'\r' | b'#');

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
        // AllowForAlignment: skip the offense when the extra space aligns the
        // operator with one on an adjacent source line — either by start column
        // (`aligned_words?`) or by an assignment/comparison operator's trailing
        // `=` end column (`aligned_equals_operator?`).
        if allow_for_alignment
            && (is_alignment_spacing(src, op_start)
                || crate::cops::util::is_equals_aligned(cx, op_range))
        {
            return;
        }
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

/// Emit an offense when there IS space around an operator that should have
/// none (`EnforcedStyleForExponentOperator: no_space` for `**`, etc.).
/// Autocorrect removes all surrounding spaces.
fn check_operator_no_space(cx: &Cx<'_>, op_range: Range) {
    let src = cx.source().as_bytes();
    let op_start = op_range.start as usize;
    let op_end = op_range.end as usize;
    if op_start >= op_end || op_end > src.len() {
        return;
    }

    let mut leading_start = op_start;
    while leading_start > 0 && matches!(src[leading_start - 1], b' ' | b'\t') {
        leading_start -= 1;
    }
    let mut trailing_end = op_end;
    while trailing_end < src.len() && matches!(src[trailing_end], b' ' | b'\t') {
        trailing_end += 1;
    }

    let leading_count = op_start - leading_start;
    let trailing_count = trailing_end - op_end;
    let at_eol = trailing_end >= src.len() || matches!(src[trailing_end], b'\n' | b'\r');

    if leading_count == 0 && (trailing_count == 0 || at_eol) {
        return;
    }

    let op_text = match std::str::from_utf8(&src[op_start..op_end]) {
        Ok(t) => t,
        Err(_) => return,
    };

    cx.emit_offense(
        op_range,
        &format!("Space around operator `{op_text}` detected."),
        None,
    );
    cx.emit_edit(
        Range {
            start: leading_start as u32,
            end: trailing_end as u32,
        },
        op_text,
    );
}

/// Returns `true` when extra spacing around an operator looks like vertical
/// alignment: a non-whitespace character sits at the same column on the
/// immediately preceding or following source line. Mirrors RuboCop's
/// `AllowForAlignment` check.
fn is_alignment_spacing(src: &[u8], op_start: usize) -> bool {
    crate::cops::util::is_alignment_at_column(src, op_start)
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
    fn allow_for_alignment_skips_extra_space_when_operator_is_aligned() {
        // Default AllowForAlignment: true — extra spaces used for vertical
        // alignment with an adjacent line are NOT flagged.
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
                x   = 1
                foo = 2
            "#});
    }

    #[test]
    fn allow_for_alignment_false_flags_all_extra_spaces() {
        test::<SpaceAroundOperators>()
            .with_options(&SpaceAroundOperatorsOptions {
                allow_for_alignment: false,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                x   = 1
                    ^ Operator `=` should be surrounded by a single space.
                foo = 2
            "#});
    }

    #[test]
    fn allow_for_alignment_still_flags_isolated_extra_space() {
        // A single line with extra space but no aligned neighbour is always flagged.
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            a  +  b
               ^ Operator `+` should be surrounded by a single space.
        "#});
    }

    /// Mastodon FP: an op-assignment `+=` aligned by its trailing `=` END column
    /// with a preceding `=` is RuboCop's `aligned_equals_operator?` case. The
    /// `+=`'s `=` ends at the same column as the `=` of `@http_client =`, so the
    /// extra space before `+=` is intentional alignment and must not be flagged.
    #[test]
    fn allow_for_alignment_accepts_equals_end_column_alignment() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            @http_client = http_client
            retries     += 1
        "#});
    }

    /// Mastodon FP: the right operand of `||` is on the next line and the excess
    /// trailing space before a `#` comment reaches the comment column. RuboCop
    /// suppresses this (`with_space.last_column == comment.loc.column`); treating
    /// a following `#` as end-of-line does the same.
    #[test]
    fn accepts_trailing_space_before_comment_with_operand_next_line() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            x = a ||          # c
                b
        "#});
    }

    #[test]
    fn exponent_operator_no_space_default_flags_space_around_star_star() {
        // Default no_space style: `a ** b` has space → offense.
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                a ** b
                  ^^ Space around operator `**` detected.
            "#})
            .expect_correction(
                indoc! {r#"
                    a ** b
                      ^^ Space around operator `**` detected.
                "#},
                "a**b
",
            );
    }

    #[test]
    fn exponent_operator_no_space_default_accepts_no_space() {
        test::<SpaceAroundOperators>().expect_no_offenses(
            "a**b
",
        );
    }

    #[test]
    fn exponent_operator_space_style_flags_missing_space_around_star_star() {
        // space style: `a**b` missing space → offense.
        test::<SpaceAroundOperators>()
            .with_options(&SpaceAroundOperatorsOptions {
                enforced_style_for_exponent_operator: SpaceAroundOperatorsBinaryStyle::Space,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                a**b
                 ^^ Surrounding space missing for operator `**`.
            "#});
    }

    #[test]
    fn rational_literals_no_space_default_flags_space_around_slash() {
        // Default no_space: `a / 1r` has space → offense.
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                a / 1r
                  ^ Space around operator `/` detected.
            "#})
            .expect_correction(
                indoc! {r#"
                    a / 1r
                      ^ Space around operator `/` detected.
                "#},
                "a/1r
",
            );
    }

    #[test]
    fn rational_literals_no_space_default_accepts_no_space() {
        test::<SpaceAroundOperators>().expect_no_offenses(
            "a/1r
",
        );
    }

    #[test]
    fn rational_literals_space_style_flags_missing_space_before_rational() {
        test::<SpaceAroundOperators>()
            .with_options(&SpaceAroundOperatorsOptions {
                enforced_style_for_rational_literals: SpaceAroundOperatorsBinaryStyle::Space,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                a/1r
                 ^ Surrounding space missing for operator `/`.
            "#});
    }

    #[test]
    fn rational_literals_style_does_not_affect_regular_division() {
        // Non-rational RHS: normal space rules apply regardless of style.
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            a/b
             ^ Surrounding space missing for operator `/`.
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
    fn masgn_catches_separator_even_with_complex_lhs_target() {
        // `a[i = 1]` is an index-assignment target containing an inner `=`.
        // With node.start as the gap start, the inner `=` in `i = 1` (properly
        // spaced) would be found first — no offense — silently missing the real
        // masgn `=` that lacks spacing.  The lhs_end boundary (end of the last
        // Mlhs child) bounds the search to after `b` and correctly flags the
        // actual masgn separator.
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                a[i = 1], b=2
                           ^ Surrounding space missing for operator `=`.
            "#})
            .expect_no_offenses("a[i = 1], b = 2\n");
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
        test::<SpaceAroundOperators>().expect_no_offenses("def f(x=0); end\n");
    }

    // ---------- setter-method = (murphy-70ej) ----------

    #[test]
    fn flags_missing_space_around_setter_equals() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            x.y=2
               ^ Surrounding space missing for operator `=`.
        "#});
    }

    #[test]
    fn flags_and_corrects_setter_equals_missing_space() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                obj.name=value
                        ^ Surrounding space missing for operator `=`.
            "#})
            .expect_correction(
                indoc! {r#"
                    obj.name=value
                            ^ Surrounding space missing for operator `=`.
                "#},
                "obj.name = value\n",
            );
    }

    #[test]
    fn flags_extra_space_around_setter_equals() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            x.y  =  2
                 ^ Operator `=` should be surrounded by a single space.
        "#});
    }

    #[test]
    fn accepts_well_spaced_setter_equals() {
        test::<SpaceAroundOperators>().expect_no_offenses("x.y = 2\n");
    }

    // ---------- pattern-match operators: match_alt | and match_as => (murphy-9jge) ----------

    #[test]
    fn flags_missing_space_around_match_alt_pipe() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            case x
            in A|B
                ^ Surrounding space missing for operator `|`.
            end
        "#});
    }

    #[test]
    fn flags_and_corrects_match_alt_pipe() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                case x
                in A|B
                    ^ Surrounding space missing for operator `|`.
                end
            "#})
            .expect_correction(
                indoc! {r#"
                    case x
                    in A|B
                        ^ Surrounding space missing for operator `|`.
                    end
                "#},
                "case x\nin A | B\nend\n",
            );
    }

    #[test]
    fn flags_extra_space_around_match_alt_pipe() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            case x
            in A  |  B
                  ^ Operator `|` should be surrounded by a single space.
            end
        "#});
    }

    #[test]
    fn accepts_well_spaced_match_alt_pipe() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            case x
            in A | B
            end
        "#});
    }

    #[test]
    fn flags_missing_space_around_match_as_rocket() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            case x
            in Integer=>n
                      ^^ Surrounding space missing for operator `=>`.
            end
        "#});
    }

    #[test]
    fn flags_and_corrects_match_as_rocket() {
        test::<SpaceAroundOperators>()
            .expect_offense(indoc! {r#"
                case x
                in Integer=>n
                          ^^ Surrounding space missing for operator `=>`.
                end
            "#})
            .expect_correction(
                indoc! {r#"
                    case x
                    in Integer=>n
                              ^^ Surrounding space missing for operator `=>`.
                    end
                "#},
                "case x\nin Integer => n\nend\n",
            );
    }

    #[test]
    fn accepts_well_spaced_match_as_rocket() {
        test::<SpaceAroundOperators>().expect_no_offenses(indoc! {r#"
            case x
            in Integer => n
            end
        "#});
    }

    #[test]
    fn flags_missing_space_around_one_liner_match_pattern_rocket() {
        test::<SpaceAroundOperators>().expect_offense(indoc! {r#"
            x=>n
             ^^ Surrounding space missing for operator `=>`.
        "#});
    }

    #[test]
    fn accepts_well_spaced_one_liner_match_pattern_rocket() {
        test::<SpaceAroundOperators>().expect_no_offenses("x => n\n");
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
murphy_plugin_api::submit_cop!(SpaceAroundOperators);
