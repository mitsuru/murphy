//! `Layout/MultilineAssignmentLayout` — checks for a newline after the
//! assignment operator in multi-line assignments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineAssignmentLayout
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports `check_assignment` over the `CheckAssignment` mixin's node set:
//!   every `*asgn` write (`lvasgn`/`ivasgn`/`cvasgn`/`gvasgn`/`casgn`/`masgn`/
//!   `op_asgn`/`or_asgn`/`and_asgn`) plus setter sends (`obj.foo = rhs`,
//!   guarded to a literal `=` operator). The right-hand side is extracted per
//!   `extract_rhs` (the assignment's value field, or a setter send's last
//!   argument).
//!
//!   An offense fires only when the RHS type is in `SupportedTypes` (default
//!   `block`/`case`/`class`/`if`/`kwbegin`/`module`; `block` expands to
//!   `block`/`numblock`/`itblock`) and the RHS is multi-line (or a block whose
//!   opener is on a different line from the assignment). `EnforcedStyle`:
//!
//!   - `new_line` (default): the RHS must NOT start on the operator's line.
//!   - `same_line`: the RHS MUST start on the operator's line.
//!
//!   Murphy node mappings: `unless` folds into `if`; `kwbegin` (`begin…end`)
//!   is a `begin` node that is not parenthesised (a parenthesised `( … )`,
//!   RuboCop's `:begin`, is excluded). Autocorrect: not implemented (v1 gap) —
//!   RuboCop inserts a newline after the operator (`new_line`) or collapses
//!   the gap to a single space (`same_line`).
//! ```
//!
//! ## Matched shapes
//!
//! `*asgn` writes and setter sends whose multi-line RHS is a supported control
//! structure laid out against the configured `EnforcedStyle`.

use crate::cops::util::{block_opener, first_line_range, gap_has_newline, is_parenthesized};
use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, cop};

const NEW_LINE_OFFENSE: &str = "Right hand side of multi-line assignment is on \
    the same line as the assignment operator `=`.";
const SAME_LINE_OFFENSE: &str = "Right hand side of multi-line assignment is not \
    on the same line as the assignment operator `=`.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineAssignmentLayout;

/// Options for [`MultilineAssignmentLayout`]. `EnforcedStyle` and
/// `SupportedTypes` match RuboCop verbatim.
#[derive(CopOptions)]
pub struct MultilineAssignmentLayoutOptions {
    #[option(
        name = "EnforcedStyle",
        default = "new_line",
        description = "Whether the RHS must be on a new line after `=` or on the same line."
    )]
    pub enforced_style: MultilineAssignmentLayoutStyle,
    #[option(
        name = "SupportedTypes",
        default = ["block", "case", "class", "if", "kwbegin", "module"],
        description = "RHS node types subject to this rule."
    )]
    pub supported_types: Vec<String>,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum MultilineAssignmentLayoutStyle {
    /// RHS begins on a new line after the operator.
    #[option(value = "new_line")]
    NewLine,
    /// RHS begins on the same line as the operator.
    #[option(value = "same_line")]
    SameLine,
}

#[cop(
    name = "Layout/MultilineAssignmentLayout",
    description = "Checks for a newline after the assignment operator in multi-line assignments.",
    default_severity = "warning",
    // RuboCop ships this cop `Enabled: false` (opt-in); the bundled
    // `default.yml` disables it too.
    default_enabled = false,
    options = MultilineAssignmentLayoutOptions,
)]
impl MultilineAssignmentLayout {
    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "masgn")]
    fn check_masgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "op_asgn")]
    fn check_op_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "or_asgn")]
    fn check_or_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "and_asgn")]
    fn check_and_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let is_send = matches!(cx.kind(node), NodeKind::Send { .. });

    // `extract_rhs`: setter send → last argument; assignment → value field.
    let Some(rhs) = extract_rhs(node, is_send, cx) else {
        return;
    };

    // RuboCop: `return if node.send_type? && node.loc.operator&.source != '='`.
    // A setter send only qualifies with a literal `=` operator. The operator
    // sits immediately before the RHS (the last argument) — covers both
    // `obj.foo = x` and the `[]=` index setter `foo[:x] = y`.
    let op = if is_send {
        let Some(eq) = setter_operator_range(node, rhs, cx) else {
            return;
        };
        eq
    } else {
        assignment_op_range(node, cx)
    };
    if op == Range::ZERO {
        return;
    }

    // `return unless supported_types.include?(rhs.type)`.
    let opts = cx.options_or_default::<MultilineAssignmentLayoutOptions>();
    if !rhs_type_supported(rhs, &opts.supported_types, cx) {
        return;
    }

    // `return if rhs.single_line? && (!rhs.block_type? || same_line?(node, rhs.loc.begin))`.
    let src = cx.source().as_bytes();
    if cx.is_single_line(rhs) {
        if !is_block(rhs, cx) {
            return;
        }
        let Some(begin) = block_opener(rhs, cx) else {
            return;
        };
        // `same_line?(node, rhs.loc.begin)` — node start and block opener share
        // a line iff no newline lies between them.
        if !gap_has_newline(src, cx.range(node).start, begin.start) {
            return;
        }
    }

    // The operator and the RHS share a line iff no newline lies between them
    // (the operator always precedes the RHS).
    let operator_and_rhs_same_line = !gap_has_newline(src, op.start, cx.range(rhs).start);

    match opts.enforced_style {
        MultilineAssignmentLayoutStyle::NewLine => {
            // `return unless same_line?(node.loc.operator, rhs)`.
            if operator_and_rhs_same_line {
                // RuboCop highlights the whole assignment node, which spans
                // multiple lines; clamp to its first line for the caret span.
                cx.emit_offense(first_line_range(node, cx), NEW_LINE_OFFENSE, None);
            }
        }
        MultilineAssignmentLayoutStyle::SameLine => {
            // `return unless node.loc.operator.line != rhs.first_line`.
            if !operator_and_rhs_same_line {
                cx.emit_offense(first_line_range(node, cx), SAME_LINE_OFFENSE, None);
            }
        }
    }
}

/// `extract_rhs`: a setter send's last argument, or an assignment's value.
fn extract_rhs(node: NodeId, is_send: bool, cx: &Cx<'_>) -> Option<NodeId> {
    if is_send {
        return cx.call_arguments(node).last().copied();
    }
    assignment_value(node, cx).get()
}

/// The RHS value of an `*asgn` node. `None` for a value-less write (a bare
/// `op_asgn` target, etc., which never occurs for these kinds).
fn assignment_value(node: NodeId, cx: &Cx<'_>) -> OptNodeId {
    match *cx.kind(node) {
        NodeKind::Lvasgn { value, .. }
        | NodeKind::Ivasgn { value, .. }
        | NodeKind::Cvasgn { value, .. }
        | NodeKind::Gvasgn { value, .. }
        | NodeKind::Casgn { value, .. } => value,
        NodeKind::Masgn { rhs, .. } => OptNodeId::some(rhs),
        NodeKind::OpAsgn { value, .. }
        | NodeKind::OrAsgn { value, .. }
        | NodeKind::AndAsgn { value, .. } => OptNodeId::some(value),
        _ => OptNodeId::NONE,
    }
}

/// The setter `=` operator of a send whose RHS is `rhs` (its last argument) —
/// RuboCop's `SendNode#loc.operator`. The operator is the last standalone `=`
/// token before the RHS; `None` if the send is not a setter (no such `=`,
/// e.g. an ordinary call). Handles both `obj.foo = x` and `foo[:x] = y`.
fn setter_operator_range(node: NodeId, rhs: NodeId, cx: &Cx<'_>) -> Option<Range> {
    // Begin searching after the call's receiver so a `=>`/`==` inside the
    // receiver chain cannot be considered. Receiver may be absent (e.g. a bare
    // `foo = x` is an `lvasgn`, not a send, so this only sees real setters).
    let search_from = cx
        .call_receiver(node)
        .get()
        .map_or(cx.range(node).start, |r| cx.range(r).end);
    let rhs_start = cx.range(rhs).start;
    let toks = cx.sorted_tokens();
    let lo = toks.partition_point(|t| t.range.start < search_from);
    let hi = toks.partition_point(|t| t.range.end <= rhs_start);
    if lo >= hi {
        return None;
    }
    toks[lo..hi]
        .iter()
        .rev()
        .find(|t| cx.raw_source(t.range) == "=")
        .map(|t| t.range)
}

/// The assignment operator (`=`, `+=`, `||=`, …) of an `*asgn` node — the
/// token between the LHS write target and the RHS value. `Range::ZERO` if it
/// cannot be located.
fn assignment_op_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let Some(rhs) = assignment_value(node, cx).get() else {
        return Range::ZERO;
    };
    // The operator sits between the node's start and the RHS. Scan the gap for
    // the last `=` (handles `||=`/`&&=`/`+=`, whose final char is `=`).
    let node_start = cx.range(node).start as usize;
    let rhs_start = cx.range(rhs).start as usize;
    let src = cx.source().as_bytes();
    let gap = &src[node_start..rhs_start];
    gap.iter()
        .rposition(|&b| b == b'=')
        .map_or(Range::ZERO, |idx| {
            let pos = (node_start + idx) as u32;
            Range {
                start: pos,
                end: pos + 1,
            }
        })
}

/// `supported_types.include?(rhs.type)` with `block` expanded to all block
/// flavours and `class` covering singleton classes.
fn rhs_type_supported(rhs: NodeId, supported: &[String], cx: &Cx<'_>) -> bool {
    let Some(rhs_type) = rhs_type_name(rhs, cx) else {
        return false;
    };
    supported.iter().any(|t| {
        let sym = t.as_str();
        match sym {
            // `block` expands to `block`/`numblock`/`itblock` (RuboCop's
            // `BLOCK_TYPES`).
            "block" => matches!(rhs_type, "block" | "numblock" | "itblock"),
            other => other == rhs_type,
        }
    })
}

/// The RuboCop node-type name of `rhs` for the supported-type check. Only the
/// types reachable as a multi-line assignment RHS are mapped; everything else
/// returns `None` (never supported).
fn rhs_type_name(rhs: NodeId, cx: &Cx<'_>) -> Option<&'static str> {
    Some(match *cx.kind(rhs) {
        NodeKind::Block { .. } => "block",
        NodeKind::Numblock { .. } => "numblock",
        NodeKind::Itblock { .. } => "itblock",
        NodeKind::Case { .. } => "case",
        // `unless` folds into `if` in Murphy.
        NodeKind::If { .. } => "if",
        // RuboCop's `:class`. A singleton class (`class << x`) is `:sclass`,
        // which is not in the default supported set, so it is not mapped here.
        NodeKind::Class { .. } => "class",
        NodeKind::Module { .. } => "module",
        // `begin…end` is a non-parenthesised `begin` node (RuboCop's
        // `:kwbegin`); a parenthesised `( … )` is RuboCop's `:begin`, which is
        // not in the default supported set.
        NodeKind::Begin(_) if !is_parenthesized(rhs, cx) => "kwbegin",
        _ => return None,
    })
}

/// `rhs.block_type?` — any block flavour.
fn is_block(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::{MultilineAssignmentLayout, MultilineAssignmentLayoutOptions, MultilineAssignmentLayoutStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn same_line() -> MultilineAssignmentLayoutOptions {
        MultilineAssignmentLayoutOptions {
            enforced_style: MultilineAssignmentLayoutStyle::SameLine,
            supported_types: vec![
                "block".into(),
                "case".into(),
                "class".into(),
                "if".into(),
                "kwbegin".into(),
                "module".into(),
            ],
        }
    }

    #[test]
    fn new_line_flags_rhs_on_operator_line() {
        test::<MultilineAssignmentLayout>().expect_offense(indoc! {"
            foo = if expression
            ^^^^^^^^^^^^^^^^^^^ Right hand side of multi-line assignment is on the same line as the assignment operator `=`.
              'bar'
            end
        "});
    }

    #[test]
    fn new_line_accepts_rhs_on_new_line() {
        test::<MultilineAssignmentLayout>().expect_no_offenses(indoc! {"
            foo =
              if expression
                'bar'
              end
        "});
    }

    #[test]
    fn new_line_accepts_begin_rescue_on_new_line() {
        test::<MultilineAssignmentLayout>().expect_no_offenses(indoc! {"
            foo =
              begin
                compute
              rescue => e
                nil
              end
        "});
    }

    #[test]
    fn new_line_flags_begin_on_operator_line() {
        test::<MultilineAssignmentLayout>().expect_offense(indoc! {"
            foo = begin
            ^^^^^^^^^^^ Right hand side of multi-line assignment is on the same line as the assignment operator `=`.
              compute
            end
        "});
    }

    #[test]
    fn accepts_single_line_assignment() {
        test::<MultilineAssignmentLayout>().expect_no_offenses("foo = bar\n");
    }

    #[test]
    fn accepts_multiline_rhs_not_in_supported_types() {
        // An array RHS spanning multiple lines is not a supported type.
        test::<MultilineAssignmentLayout>().expect_no_offenses(indoc! {"
            foo = [
              1,
              2
            ]
        "});
    }

    #[test]
    fn ignores_parenthesized_begin_rhs() {
        // A parenthesised `( … )` is RuboCop's `:begin`, not `:kwbegin`.
        test::<MultilineAssignmentLayout>().expect_no_offenses(indoc! {"
            foo = (
              bar
            )
        "});
    }

    #[test]
    fn new_line_flags_setter_send() {
        test::<MultilineAssignmentLayout>().expect_offense(indoc! {"
            obj.foo = if expression
            ^^^^^^^^^^^^^^^^^^^^^^^ Right hand side of multi-line assignment is on the same line as the assignment operator `=`.
              'bar'
            end
        "});
    }

    // Index setters (`foo[:x] = …`) parse to a `[]=` send and are handled by
    // the `send` hook (RuboCop's `CheckAssignment#on_send`).
    #[test]
    fn new_line_flags_index_setter() {
        test::<MultilineAssignmentLayout>().expect_offense(indoc! {"
            foo[:x] = if expression
            ^^^^^^^^^^^^^^^^^^^^^^^ Right hand side of multi-line assignment is on the same line as the assignment operator `=`.
              'bar'
            end
        "});
    }

    #[test]
    fn new_line_flags_masgn() {
        test::<MultilineAssignmentLayout>().expect_offense(indoc! {"
            a, b = if expression
            ^^^^^^^^^^^^^^^^^^^^ Right hand side of multi-line assignment is on the same line as the assignment operator `=`.
              [1, 2]
            end
        "});
    }

    #[test]
    fn same_line_flags_rhs_on_new_line() {
        test::<MultilineAssignmentLayout>()
            .with_options(&same_line())
            .expect_offense(indoc! {"
                foo =
                ^^^^^ Right hand side of multi-line assignment is not on the same line as the assignment operator `=`.
                  if expression
                    'bar'
                  end
            "});
    }

    #[test]
    fn same_line_accepts_rhs_on_operator_line() {
        test::<MultilineAssignmentLayout>()
            .with_options(&same_line())
            .expect_no_offenses(indoc! {"
                foo = if expression
                  'bar'
                end
            "});
    }

    // A single-line block whose opener is on a different line from the
    // assignment node still triggers a `new_line` offense.
    #[test]
    fn new_line_flags_block_rhs() {
        test::<MultilineAssignmentLayout>().expect_offense(indoc! {"
            foo = [1].map do |i|
            ^^^^^^^^^^^^^^^^^^^^ Right hand side of multi-line assignment is on the same line as the assignment operator `=`.
              i + 1
            end
        "});
    }

    #[test]
    fn new_line_accepts_block_rhs_on_new_line() {
        test::<MultilineAssignmentLayout>().expect_no_offenses(indoc! {"
            foo =
              [1].map do |i|
                i + 1
              end
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineAssignmentLayout);
