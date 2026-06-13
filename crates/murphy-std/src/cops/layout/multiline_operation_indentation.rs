//! `Layout/MultilineOperationIndentation` — checks the indentation of binary
//! operations (`&&`/`and`, `||`/`or`, and operator-method sends like `+`)
//! whose right operand begins its own line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineOperationIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports the core of RuboCop's `MultilineExpressionIndentation` for binary
//!   operations: `And`/`Or` nodes and operator-method `Send` nodes (with a
//!   receiver and a single argument, no leading dot). When the right operand
//!   begins its own line, the cop computes the correct column from
//!   `EnforcedStyle` (`aligned` default → the operation's start column;
//!   `indented` → `lhs indent + IndentationWidth`, doubled for a keyword
//!   condition) and flags a mismatch.
//!
//!   `should_align?` is ported whole — assignment-RHS, keyword-condition, and
//!   method-call-argument contexts all force alignment under `aligned` style —
//!   because it selects both the message and the correct column; a partial
//!   port would emit wrong offenses.
//!
//!   Gaps vs RuboCop (documented, not silently dropped):
//!     * `IndentationWidth` falls back to `Layout/IndentationWidth: Width` in
//!       RuboCop; Murphy uses this cop's own `IndentationWidth` option
//!       (default 2) and does not read the cross-cop value.
//!     * The `def_modifier?` / `postfix_conditional?` tails of `should_align?`
//!       / `correct_indentation` are not ported.
//!     * Autocorrect is not emitted (RuboCop shifts the operand via
//!       `AlignmentCorrector`); Murphy reports the offense only.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MultilineOperationIndentation;

#[derive(CopOptions)]
pub struct MultilineOperationIndentationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "aligned",
        description = "Whether multiline operands are aligned with the first operand or indented."
    )]
    pub enforced_style: OperationIndentStyle,
    // `Option<i64>` so the bundled default `IndentationWidth: ~` (JSON null)
    // decodes to `None` instead of erroring the option struct and discarding the
    // user's other keys; `None` falls back to width 2.
    #[option(
        name = "IndentationWidth",
        description = "Indentation width in spaces (null/unset falls back to RuboCop's default of 2)."
    )]
    pub indentation_width: Option<i64>,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum OperationIndentStyle {
    /// Continuation operands align with the first operand's column.
    #[option(value = "aligned")]
    Aligned,
    /// Continuation operands indent by `IndentationWidth` from the lhs.
    #[option(value = "indented")]
    Indented,
}

#[cop(
    name = "Layout/MultilineOperationIndentation",
    description = "Check indentation of binary operations that span more than one line.",
    default_severity = "warning",
    default_enabled = true,
    options = MultilineOperationIndentationOptions,
)]
impl MultilineOperationIndentation {
    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let Some((lhs, rhs)) = operands(node, cx) else {
        return;
    };
    if !relevant_node(node, cx) {
        return;
    }

    let opts = cx.options_or_default::<MultilineOperationIndentationOptions>();
    let width = opts.indentation_width.unwrap_or(2).max(0) as usize;
    let style = opts.enforced_style;
    let source = cx.source();

    let rhs_range = cx.range(rhs);

    // Gate 1: the rhs must begin its own line.
    if !begins_its_line(source, rhs_range.start) {
        return;
    }
    // Gate 2: skip when inside grouping / arg-list parentheses.
    if not_for_this_cop(node, cx) {
        return;
    }

    let rhs_col = column_of(source, rhs_range.start);
    let align = should_align(node, rhs, style, cx);
    let correct_col = if align {
        column_of(source, cx.range(node).start)
    } else {
        indentation(cx.range(lhs).start, source) + correct_indentation(node, width, cx)
    };

    if correct_col == rhs_col {
        return;
    }

    let message = if align {
        format!(
            "Align the operands of {} spanning multiple lines.",
            operation_description(node, rhs, cx)
        )
    } else {
        format!(
            "Use {correct_col} (not {rhs_col}) spaces for indenting {} spanning multiple lines.",
            operation_description(node, rhs, cx)
        )
    };
    // Offense range: the rhs operand's first line (RuboCop highlights the rhs
    // source range; for multiline operands that exceeds one line, so we trim).
    cx.emit_offense(first_line_range(source, rhs_range), &message, None);
}

/// The (lhs, rhs) operands of a binary operation, or `None` if `node` is not a
/// relevant binary operation. For `And`/`Or` it's the two children; for an
/// operator-method `Send` it's the receiver and the single argument.
fn operands(node: NodeId, cx: &Cx<'_>) -> Option<(NodeId, NodeId)> {
    match *cx.kind(node) {
        NodeKind::And { lhs, rhs } | NodeKind::Or { lhs, rhs } => Some((lhs, rhs)),
        NodeKind::Send { .. } => {
            // Operator-method send: receiver + exactly one argument.
            if !cx.is_operator_method(node) {
                return None;
            }
            let recv = cx.call_receiver(node).get()?;
            let args = cx.call_arguments(node);
            if args.len() != 1 {
                return None;
            }
            Some((recv, args[0]))
        }
        _ => None,
    }
}

/// RuboCop's `relevant_node?`: skip unary operations and dotted calls
/// (`a.+(b)`). For `And`/`Or` there is no dot, so they always pass.
fn relevant_node(node: NodeId, cx: &Cx<'_>) -> bool {
    if matches!(*cx.kind(node), NodeKind::Send { .. }) {
        // A dotted operator call (`a.+ b`) is not this cop's concern.
        if cx.loc(node).dot() != Range::ZERO {
            return false;
        }
    }
    true
}

/// RuboCop's `should_align?`: assignment-RHS, keyword-condition, and
/// method-call-argument contexts force alignment under `aligned` style.
fn should_align(node: NodeId, rhs: NodeId, style: OperationIndentStyle, cx: &Cx<'_>) -> bool {
    let assignment = part_of_assignment_rhs(node, cx);
    if let Some(assignment) = assignment {
        // If the assignment's RHS itself begins its line, alignment is forced.
        if let Some(rhs_node) = assignment_rhs(assignment, cx)
            && begins_its_line(cx.source(), cx.range(rhs_node).start)
        {
            return true;
        }
    }

    if style != OperationIndentStyle::Aligned {
        return false;
    }

    if kw_node_with_special_indentation(node, cx).is_some() || assignment.is_some() {
        return true;
    }

    // Argument in a method call (with or without parentheses).
    let _ = rhs;
    argument_in_method_call(node, cx).is_some()
}

/// RuboCop's `correct_indentation`: the configured width, doubled when inside a
/// keyword condition with special indentation.
fn correct_indentation(node: NodeId, width: usize, cx: &Cx<'_>) -> usize {
    if kw_node_with_special_indentation(node, cx).is_some() {
        width * 2
    } else {
        width
    }
}

/// RuboCop's `operation_description`: the message tail describing the context.
fn operation_description(node: NodeId, _rhs: NodeId, cx: &Cx<'_>) -> String {
    if let Some(kw) = kw_node_with_special_indentation(node, cx) {
        return keyword_message_tail(kw, cx);
    }
    if part_of_assignment_rhs(node, cx).is_some() {
        return "an expression in an assignment".to_string();
    }
    "an expression".to_string()
}

/// RuboCop's `keyword_message_tail`: e.g. "a condition in an `if` statement".
fn keyword_message_tail(kw_node: NodeId, cx: &Cx<'_>) -> String {
    let keyword = keyword_of(kw_node, cx);
    let kind = if keyword == "for" {
        "collection"
    } else {
        "condition"
    };
    let article = if keyword.starts_with('i') || keyword.starts_with('u') {
        "an"
    } else {
        "a"
    };
    format!("a {kind} in {article} `{keyword}` statement")
}

/// The keyword text of a special-indentation ancestor.
fn keyword_of(node: NodeId, cx: &Cx<'_>) -> &'static str {
    match *cx.kind(node) {
        NodeKind::If { .. } => "if",
        NodeKind::While { .. } => "while",
        NodeKind::Until { .. } => "until",
        NodeKind::For { .. } => "for",
        NodeKind::Return(_) => "return",
        _ => "if",
    }
}

/// Find a `KEYWORD_ANCESTOR_TYPES` (for/if/while/until/return) ancestor whose
/// condition/collection contains `node`, excluding ternary `if`.
fn kw_node_with_special_indentation(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let node_range = cx.range(node);
    for ancestor in cx.ancestors(node) {
        let cond = match *cx.kind(ancestor) {
            NodeKind::If { cond, .. } => {
                // Exclude ternaries (no `end` keyword).
                if cx.loc(ancestor).end_keyword() == Range::ZERO {
                    continue;
                }
                cond
            }
            NodeKind::While { cond, .. } | NodeKind::Until { cond, .. } => cond,
            NodeKind::For { iter, .. } => iter,
            NodeKind::Return(inner) => match inner.get() {
                Some(v) => v,
                None => continue,
            },
            _ => continue,
        };
        if within_node(node_range, cx.range(cond)) {
            return Some(ancestor);
        }
    }
    None
}

/// Find an assignment ancestor whose RHS contains `node`.
fn part_of_assignment_rhs(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let node_range = cx.range(node);
    for ancestor in cx.ancestors(node) {
        // Disqualify when crossing a control-flow / array / kwbegin boundary
        // (RuboCop's UNALIGNED_RHS_TYPES) — the operand is no longer a direct
        // assignment RHS.
        if matches!(
            *cx.kind(ancestor),
            NodeKind::If { .. }
                | NodeKind::While { .. }
                | NodeKind::Until { .. }
                | NodeKind::For { .. }
                | NodeKind::Return(_)
                | NodeKind::Array(_)
                | NodeKind::Kwbegin(_)
        ) {
            return None;
        }
        if cx.is_assignment(ancestor)
            && let Some(rhs) = assignment_rhs(ancestor, cx)
            && within_node(node_range, cx.range(rhs))
        {
            return Some(ancestor);
        }
    }
    None
}

/// The RHS value of an assignment node.
fn assignment_rhs(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(node) {
        NodeKind::Lvasgn { value, .. }
        | NodeKind::Ivasgn { value, .. }
        | NodeKind::Cvasgn { value, .. }
        | NodeKind::Gvasgn { value, .. }
        | NodeKind::Casgn { value, .. } => value.get(),
        NodeKind::OpAsgn { value, .. }
        | NodeKind::OrAsgn { value, .. }
        | NodeKind::AndAsgn { value, .. } => Some(value),
        NodeKind::Masgn { rhs, .. } => Some(rhs),
        _ => None,
    }
}

/// Find a `Send` ancestor (stopping at a block boundary) whose argument list
/// contains `node` — RuboCop's `argument_in_method_call(node,
/// :with_or_without_parentheses)`.
fn argument_in_method_call(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let node_range = cx.range(node);
    for ancestor in cx.ancestors(node) {
        if cx.is_any_block_type(ancestor) {
            return None;
        }
        if matches!(
            *cx.kind(ancestor),
            NodeKind::Send { .. } | NodeKind::Csend { .. }
        ) {
            // Skip setter methods (`a.b = c`).
            if cx.is_assignment_method(ancestor) {
                continue;
            }
            if cx
                .call_arguments(ancestor)
                .iter()
                .any(|&arg| within_node(node_range, cx.range(arg)))
            {
                return Some(ancestor);
            }
        }
    }
    None
}

/// RuboCop's `not_for_this_cop?`: skip when an ancestor is a grouped
/// (parenthesized) expression or `node` is inside an arg-list's parentheses.
fn not_for_this_cop(node: NodeId, cx: &Cx<'_>) -> bool {
    let node_range = cx.range(node);
    for ancestor in cx.ancestors(node) {
        if crate::cops::util::is_parenthesized(ancestor, cx) {
            return true;
        }
        if matches!(*cx.kind(ancestor), NodeKind::Send { .. })
            && inside_arg_list_parentheses(node_range, ancestor, cx)
        {
            return true;
        }
    }
    false
}

/// True if `node_range` sits strictly inside the parenthesized argument list of
/// `ancestor` (a parenthesized send).
fn inside_arg_list_parentheses(node_range: Range, ancestor: NodeId, cx: &Cx<'_>) -> bool {
    let open = cx.loc(ancestor).begin();
    let close = cx.loc(ancestor).end();
    if open == Range::ZERO || close == Range::ZERO {
        return false;
    }
    node_range.start > open.start && node_range.end < close.end
}

/// True if `inner` is fully contained in `outer`.
fn within_node(inner: Range, outer: Range) -> bool {
    inner.start >= outer.start && inner.end <= outer.end
}

/// RuboCop's `begins_its_line?`: `offset` is the first non-whitespace on its
/// line.
fn begins_its_line(source: &str, offset: u32) -> bool {
    let line_start = line_start_of(source, offset) as usize;
    source.as_bytes()[line_start..offset as usize]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
}

/// RuboCop's `indentation(node)`: the column of the first non-whitespace on the
/// node's line (i.e. the line's indentation).
fn indentation(node_start: u32, source: &str) -> usize {
    let line_start = line_start_of(source, node_start) as usize;
    source.as_bytes()[line_start..]
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .count()
}

fn line_start_of(source: &str, offset: u32) -> u32 {
    source.as_bytes()[..offset as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1) as u32
}

/// 0-based column (character count) of `offset`.
fn column_of(source: &str, offset: u32) -> usize {
    let line_start = line_start_of(source, offset) as usize;
    source[line_start..offset as usize].chars().count()
}

/// The first physical line of `range`.
fn first_line_range(source: &str, range: Range) -> Range {
    let bytes = source.as_bytes();
    let end = bytes[range.start as usize..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(range.end, |i| range.start + i as u32)
        .min(range.end);
    Range {
        start: range.start,
        end,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MultilineOperationIndentation, MultilineOperationIndentationOptions, OperationIndentStyle,
    };
    use murphy_plugin_api::test_support::{indoc, test};
    use murphy_plugin_api::CopOptions;

    /// Regression (sweep #384 follow-up): the bundled default
    /// `IndentationWidth: ~` merges to JSON `null`. With an `Option<i64>` field
    /// it must decode rather than error the whole struct and silently discard the
    /// user's `EnforcedStyle`.
    #[test]
    fn null_indentation_width_preserves_other_keys() {
        let opts = <MultilineOperationIndentationOptions as CopOptions>::from_config_json(
            br#"{"EnforcedStyle":"indented","IndentationWidth":null}"#,
        )
        .expect("null IndentationWidth must decode, not discard the struct");
        let reference = <MultilineOperationIndentationOptions as CopOptions>::from_config_json(
            br#"{"EnforcedStyle":"indented","IndentationWidth":4}"#,
        )
        .unwrap();
        assert!(opts.enforced_style == reference.enforced_style);
    }

    fn indented() -> MultilineOperationIndentationOptions {
        MultilineOperationIndentationOptions {
            enforced_style: OperationIndentStyle::Indented,
            indentation_width: Some(2),
        }
    }

    // ----- aligned style (default) ----------------------------------

    #[test]
    fn flags_misaligned_operand_in_method_arg() {
        // `puts a, 1 +\n  2` — `1 + 2` is an argument, so alignment is forced.
        test::<MultilineOperationIndentation>().expect_offense(indoc! {"
            puts a, 1 +
              2
              ^ Align the operands of an expression spanning multiple lines.
        "});
    }

    #[test]
    fn accepts_aligned_operand_in_method_arg() {
        test::<MultilineOperationIndentation>().expect_no_offenses(indoc! {"
            puts a, 1 +
                    2
        "});
    }

    #[test]
    fn flags_underindented_bare_operation() {
        // Top-level `a ||\n   b` (3 spaces): no alignment base, so the indented
        // message applies; correct is 2.
        test::<MultilineOperationIndentation>().expect_offense(indoc! {"
            a ||
               b
               ^ Use 2 (not 3) spaces for indenting an expression spanning multiple lines.
        "});
    }

    #[test]
    fn flags_misaligned_string_concat_in_method_arg() {
        // RuboCop spec: `it "..." +\n  "..."` — `+` chain as a method argument
        // forces alignment; the misindented operand is flagged.
        test::<MultilineOperationIndentation>().expect_offense(indoc! {r#"
            it "should convert " +
              "a to "
              ^^^^^^^ Align the operands of an expression spanning multiple lines.
        "#});
    }

    #[test]
    fn accepts_correctly_indented_bare_operation() {
        test::<MultilineOperationIndentation>().expect_no_offenses(indoc! {"
            a ||
              b
        "});
    }

    #[test]
    fn accepts_single_line_operation() {
        test::<MultilineOperationIndentation>().expect_no_offenses("a || b\n");
    }

    #[test]
    fn ignores_dotted_operator_call() {
        // `a.+(b)` is a dotted call — not this cop's concern.
        test::<MultilineOperationIndentation>().expect_no_offenses("a.+(b)\n");
    }

    #[test]
    fn ignores_parenthesized_grouping() {
        test::<MultilineOperationIndentation>().expect_no_offenses(indoc! {"
            (a ||
              b)
        "});
    }

    // ----- indented style -------------------------------------------

    #[test]
    fn flags_under_indented_if_condition_indented_style() {
        // `if a +\n   b` (3 spaces) under indented style: a condition gets
        // width*2 = 4.
        test::<MultilineOperationIndentation>()
            .with_options(&indented())
            .expect_offense(indoc! {"
                if a +
                   b
                   ^ Use 4 (not 3) spaces for indenting a condition in an `if` statement spanning multiple lines.
                  something
                end
            "});
    }

    #[test]
    fn flags_over_indented_assignment_indented_style() {
        // `a = b +\n      c` (6 spaces) under indented style: correct is 2.
        test::<MultilineOperationIndentation>()
            .with_options(&indented())
            .expect_offense(indoc! {"
                a = b +
                      c
                      ^ Use 2 (not 6) spaces for indenting an expression in an assignment spanning multiple lines.
            "});
    }

    #[test]
    fn accepts_correctly_indented_assignment_indented_style() {
        test::<MultilineOperationIndentation>()
            .with_options(&indented())
            .expect_no_offenses(indoc! {"
                a = b +
                  c
            "});
    }
}

murphy_plugin_api::submit_cop!(MultilineOperationIndentation);
