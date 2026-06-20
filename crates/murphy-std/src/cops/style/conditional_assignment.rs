//! `Style/ConditionalAssignment` — use return value of conditional for assignment.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ConditionalAssignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-7mc2]
//! notes: >
//!   Both `EnforcedStyle` directions are implemented.
//!
//!   `assign_to_condition` (default, `on_if`/`on_case`/`on_case_match`): flags
//!   `if`/`elsif`/`else`, `case`/`when`+`else`, and `case`/`in`+`else` where
//!   every branch's tail assigns to the same target with the same assignment
//!   node type. Recognised assignment kinds: `lvasgn`/`ivasgn`/`gvasgn`/
//!   `cvasgn`/`casgn` and `op_asgn`/`or_asgn`/`and_asgn` (matching RuboCop's
//!   `assignment_types_match?` — mixed `bar = 1`/`bar += 2` is not flagged).
//!   Single-expression parentheses are unwrapped. Ternary (`a ? b = 1 : b = 2`)
//!   is detected, gated by `IncludeTernaryExpressions` (default true).
//!   `SingleLineConditionsOnly` (default true) suppresses branches whose tail
//!   is a multi-statement begin, matching upstream.
//!
//!   `assign_inside_condition` (`on_lvasgn`/`ivasgn`/`gvasgn`/`cvasgn`/`casgn`
//!   plus `on_op_asgn`/`or_asgn`/`and_asgn`): flags `bar = if foo … end` /
//!   `bar += if … end` / `case` / `case`/`in` where the RHS is a conditional
//!   (not an allowed ternary), with the offense on the assignment.
//!   Honours `return unless else_branch` (so `x = if foo; 1; end` is not
//!   flagged) and `SingleLineConditionsOnly` (multi-statement branches suppress
//!   the offense by default), matching upstream.
//!
//!   Gaps vs upstream (all false-negatives — narrower than RuboCop, never
//!   firing where RuboCop would not) unless noted — murphy-7mc2:
//!   - The `on_send` comparison form (assignment-like sends: `<<`, `=~`, `!~`,
//!     `<=>`, `<`, `>`, `[]=`, setter `foo.bar = …`) is not modelled in either
//!     direction. `masgn` is likewise not handled.
//!   - Autocorrect is not implemented (report-only); upstream rewrites the tree
//!     with re-indentation and a `Layout/LineLength` guard.
//!   - The offense range is the conditional/assignment's first line (Murphy
//!     house style) rather than upstream's whole-node range.
//!   - `correction_exceeds_line_limit?` is not modelled: upstream suppresses
//!     when the rewrite would overflow `Layout/LineLength`, so Murphy may
//!     *over-fire* (false-positive) on very long branches.
//!   - Nested `assign_inside_condition` assignments double-fire: Murphy lacks
//!     upstream's `ignore_node`/`part_of_ignored_node?`, so a conditional whose
//!     branches are themselves `var = if … end` reports on both the outer and
//!     inner assignment (false-positive count, not false-positive location).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Use the return of the conditional for variable assignment and comparison.";
const ASSIGN_INSIDE_MSG: &str = "Assign variables inside of conditionals.";

#[derive(Default)]
pub struct ConditionalAssignment;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "assign_to_condition")]
    AssignToCondition,
    #[option(value = "assign_inside_condition")]
    AssignInsideCondition,
}

#[derive(CopOptions)]
pub struct ConditionalAssignmentOptions {
    #[option(
        name = "EnforcedStyle",
        default = "assign_to_condition",
        description = "Enforced style for conditional assignment."
    )]
    pub enforced_style: EnforcedStyle,
    #[option(
        name = "SingleLineConditionsOnly",
        default = true,
        description = "Whether to only flag conditionals whose branches are single statements."
    )]
    pub single_line_conditions_only: bool,
    #[option(
        name = "IncludeTernaryExpressions",
        default = true,
        description = "Whether to include ternary expressions in the check."
    )]
    pub include_ternary_expressions: bool,
}

#[cop(
    name = "Style/ConditionalAssignment",
    description = "Use the return of conditional for variable assignment.",
    default_severity = "warning",
    default_enabled = true,
    options = ConditionalAssignmentOptions
)]
impl ConditionalAssignment {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ConditionalAssignmentOptions>();
        if opts.enforced_style != EnforcedStyle::AssignToCondition {
            return;
        }
        // `return if node.elsif?` — the chain is checked from its outermost `if`.
        if cx.is_elsif(node) {
            return;
        }
        // `return if allowed_ternary?(node)`.
        if cx.is_ternary(node) && !opts.include_ternary_expressions {
            return;
        }
        // `return if node.elsif?` is handled above; collect branch bodies.
        let Some(branches) = collect_branches(node, cx) else {
            return;
        };
        if branches_assign_same(&branches, &opts, cx) {
            cx.emit_offense(first_line_range(cx.range(node), cx.source()), MSG, None);
        }
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ConditionalAssignmentOptions>();
        if opts.enforced_style != EnforcedStyle::AssignToCondition {
            return;
        }
        let Some(branches) = collect_branches(node, cx) else {
            return;
        };
        if branches_assign_same(&branches, &opts, cx) {
            cx.emit_offense(first_line_range(cx.range(node), cx.source()), MSG, None);
        }
    }

    #[on_node(kind = "case_match")]
    fn check_case_match(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ConditionalAssignmentOptions>();
        if opts.enforced_style != EnforcedStyle::AssignToCondition {
            return;
        }
        let Some(branches) = collect_branches(node, cx) else {
            return;
        };
        if branches_assign_same(&branches, &opts, cx) {
            cx.emit_offense(first_line_range(cx.range(node), cx.source()), MSG, None);
        }
    }

    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    // Upstream aliases `on_op_asgn`/`on_or_asgn`/`on_and_asgn` to the same
    // handler, so `x += if … end` / `x ||= case … end` are reported on the
    // shorthand-assign node itself. The inner `*vasgn` write target is skipped
    // by the `shorthand_asgn?` guard in `check_assign_inside`, so the offense
    // fires exactly once, on the op-assign node.
    #[on_node(kind = "op_asgn")]
    fn check_op_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "or_asgn")]
    fn check_or_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "and_asgn")]
    fn check_and_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }
}

impl ConditionalAssignment {
    /// `EnforcedStyle: assign_inside_condition` — flag `bar = if foo … end`
    /// where the RHS is a conditional that is not an allowed ternary.
    fn check_assign_inside(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ConditionalAssignmentOptions>();
        if opts.enforced_style != EnforcedStyle::AssignInsideCondition {
            return;
        }
        // `return if node.parent&.shorthand_asgn?` — the value-less write
        // target of an `op_asgn`/`or_asgn`/`and_asgn` is itself a `*vasgn`
        // node; skip it so `x += if … end` is reported once, on the op-asgn.
        if let Some(parent) = cx.parent(node).get()
            && matches!(
                cx.kind(parent),
                NodeKind::OpAsgn { .. } | NodeKind::OrAsgn { .. } | NodeKind::AndAsgn { .. }
            )
        {
            return;
        }
        // The RHS conditional. `*vasgn`/`casgn` carry an `Option` value (the
        // value-less write target of a shorthand assign has `None`); the
        // shorthand-assign nodes themselves carry a plain `value`.
        let rhs = match *cx.kind(node) {
            NodeKind::Lvasgn { value, .. }
            | NodeKind::Ivasgn { value, .. }
            | NodeKind::Gvasgn { value, .. }
            | NodeKind::Cvasgn { value, .. }
            | NodeKind::Casgn { value, .. } => {
                let Some(rhs) = value.get() else {
                    return;
                };
                rhs
            }
            NodeKind::OpAsgn { value, .. }
            | NodeKind::OrAsgn { value, .. }
            | NodeKind::AndAsgn { value, .. } => value,
            _ => return,
        };
        let rhs = unwrap_single_begin(rhs, cx);
        // `candidate_condition?`: must be `if`/`case`/`case_match`, excluding a
        // ternary when `IncludeTernaryExpressions` is off.
        if !is_candidate_condition(rhs, &opts, cx) {
            return;
        }
        // Upstream `check_assignment_to_condition`:
        //   `_condition, *branches, else_branch = *assignment`
        //   `return unless else_branch`
        //   `return if allowed_single_line?([*branches, else_branch])`
        // `collect_branches` returns `None` when there is no final `else`
        // (e.g. `x = if foo; 1; end`), which RuboCop never flags.
        let Some(branches) = collect_branches(rhs, cx) else {
            return;
        };
        // `allowed_single_line?`: under `SingleLineConditionsOnly` (default
        // true), a multi-statement branch suppresses the offense.
        if opts.single_line_conditions_only
            && branches
                .iter()
                .any(|&b| matches!(multi_statement_tail(b, cx), MultiStatement::Begin(_)))
        {
            return;
        }
        cx.emit_offense(
            first_line_range(cx.range(node), cx.source()),
            ASSIGN_INSIDE_MSG,
            None,
        );
    }
}

/// A conditional that `assign_inside_condition` should move the assignment
/// into: `if`/`case`/`case_match`, excluding a ternary when
/// `IncludeTernaryExpressions` is off. Mirrors upstream `candidate_condition?`.
fn is_candidate_condition(node: NodeId, opts: &ConditionalAssignmentOptions, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::If { .. } => opts.include_ternary_expressions || !cx.is_ternary(node),
        NodeKind::Case { .. } | NodeKind::CaseMatch { .. } => true,
        _ => false,
    }
}

/// Collect the body of every branch of an `if`/`case`/`case_match` conditional:
/// the `then`, each `elsif`, and the final `else` for `if`; each `when`/`in`
/// body and the `else` for `case`/`case_match`. Returns `None` when there is no
/// final `else` branch (upstream `return unless else_branch`), or when the node
/// is not a recognised conditional. Mirrors `expand_elses` + `expand_when_branches`.
fn collect_branches(node: NodeId, cx: &Cx<'_>) -> Option<Vec<NodeId>> {
    match *cx.kind(node) {
        NodeKind::If { then_, else_, .. } => {
            let then_id = then_.get()?;
            let mut branches: Vec<NodeId> = vec![then_id];
            let mut cursor = else_;
            loop {
                // No final `else` (or an `elsif` with no `else`) → not flagged.
                let branch = cursor.get()?;
                if matches!(cx.kind(branch), NodeKind::If { .. }) && cx.is_elsif(branch) {
                    let NodeKind::If { then_, else_, .. } = *cx.kind(branch) else {
                        return None;
                    };
                    branches.push(then_.get()?);
                    cursor = else_;
                } else {
                    branches.push(branch);
                    break;
                }
            }
            Some(branches)
        }
        NodeKind::Case { else_, whens, .. } => {
            let else_id = else_.get()?;
            let mut branches: Vec<NodeId> = Vec::with_capacity(cx.list(whens).len() + 1);
            for &wc in cx.list(whens) {
                let NodeKind::When { body, .. } = *cx.kind(wc) else {
                    return None;
                };
                branches.push(body.get()?);
            }
            branches.push(else_id);
            Some(branches)
        }
        NodeKind::CaseMatch { in_patterns, else_body, .. } => {
            let else_id = else_body.get()?;
            let mut branches: Vec<NodeId> = Vec::with_capacity(cx.list(in_patterns).len() + 1);
            for &ip in cx.list(in_patterns) {
                let NodeKind::InPattern { body, .. } = *cx.kind(ip) else {
                    return None;
                };
                branches.push(body.get()?);
            }
            branches.push(else_id);
            Some(branches)
        }
        _ => None,
    }
}

/// All branch tails assign to the same target with the same assignment node
/// type, none is `masgn`, and (under `SingleLineConditionsOnly`) none is a
/// multi-statement begin. Mirrors upstream `allowed_statements?`.
fn branches_assign_same(
    branches: &[NodeId],
    opts: &ConditionalAssignmentOptions,
    cx: &Cx<'_>,
) -> bool {
    let mut first: Option<(String, u8)> = None;
    for &branch in branches {
        // `allowed_single_line?`: SingleLineConditionsOnly + a multi-statement
        // begin branch → not flagged. `tail` of a single-expr begin is that
        // expr; otherwise the whole branch.
        let tail = match multi_statement_tail(branch, cx) {
            MultiStatement::Single(id) => id,
            MultiStatement::Begin(id) => {
                if opts.single_line_conditions_only {
                    return false;
                }
                id
            }
        };
        let Some((lhs, tag)) = assignment_target(tail, cx) else {
            return false;
        };
        match &first {
            None => first = Some((lhs, tag)),
            Some((flhs, ftag)) => {
                if *flhs != lhs || *ftag != tag {
                    return false;
                }
            }
        }
    }
    first.is_some()
}

enum MultiStatement {
    /// Single-expression branch (after unwrapping single-expr begins).
    Single(NodeId),
    /// Multi-statement begin; the contained `NodeId` is its tail statement.
    Begin(NodeId),
}

/// Distinguish a single-statement branch from a multi-statement begin, and
/// return the tail statement either way. Mirrors upstream `tail(branch)`
/// (`branch.begin_type? ? Array(branch).last : branch`).
fn multi_statement_tail(node: NodeId, cx: &Cx<'_>) -> MultiStatement {
    if let NodeKind::Begin(list) = cx.kind(node) {
        let items = cx.list(*list);
        match items {
            [] => MultiStatement::Single(node),
            [single] => MultiStatement::Single(unwrap_single_begin(*single, cx)),
            [.., last] => MultiStatement::Begin(*last),
        }
    } else {
        MultiStatement::Single(node)
    }
}

/// Fully unwrap single-expression parentheses (`((expr))` → `expr`).
fn unwrap_single_begin(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    while let NodeKind::Begin(children) = cx.kind(node) {
        match cx.list(*children) {
            [single] => node = *single,
            _ => break,
        }
    }
    node
}

/// The canonical left-hand-side string and a node-type tag for an assignment.
/// `None` if `node` is not an assignment Murphy recognises. The tag enforces
/// upstream's `assignment_types_match?` (e.g. `bar = 1` and `bar += 2` differ).
fn assignment_target(node: NodeId, cx: &Cx<'_>) -> Option<(String, u8)> {
    match cx.kind(node) {
        NodeKind::Lvasgn { name, .. } => Some((cx.symbol_str(*name).to_string(), 1)),
        NodeKind::Ivasgn { name, .. } => Some((cx.symbol_str(*name).to_string(), 2)),
        NodeKind::Gvasgn { name, .. } => Some((cx.symbol_str(*name).to_string(), 3)),
        NodeKind::Cvasgn { name, .. } => Some((cx.symbol_str(*name).to_string(), 4)),
        NodeKind::Casgn { .. } => cx.const_name(node).map(|n| (n, 5)),
        NodeKind::OpAsgn { target, op, .. } => {
            let lhs = op_asgn_target_name(*target, cx)?;
            Some((format!("{lhs} {}=", cx.symbol_str(*op)), 6))
        }
        NodeKind::OrAsgn { target, .. } => {
            let lhs = op_asgn_target_name(*target, cx)?;
            Some((format!("{lhs} ||="), 7))
        }
        NodeKind::AndAsgn { target, .. } => {
            let lhs = op_asgn_target_name(*target, cx)?;
            Some((format!("{lhs} &&="), 8))
        }
        _ => None,
    }
}

/// The variable name of an op-assign target (a value-less write node).
fn op_asgn_target_name(target: NodeId, cx: &Cx<'_>) -> Option<String> {
    match cx.kind(target) {
        NodeKind::Lvasgn { name, .. }
        | NodeKind::Ivasgn { name, .. }
        | NodeKind::Gvasgn { name, .. }
        | NodeKind::Cvasgn { name, .. } => Some(cx.symbol_str(*name).to_string()),
        NodeKind::Casgn { .. } => cx.const_name(target),
        _ => None,
    }
}

fn first_line_range(range: Range, source: &str) -> Range {
    let bytes = source.as_bytes();
    let mut end = range.start as usize;
    while end < range.end as usize && end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    Range { start: range.start, end: end as u32 }
}

#[cfg(test)]
mod tests {
    use super::{ConditionalAssignment, ConditionalAssignmentOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(
        style: EnforcedStyle,
        single_line_conditions_only: bool,
        include_ternary_expressions: bool,
    ) -> ConditionalAssignmentOptions {
        ConditionalAssignmentOptions {
            enforced_style: style,
            single_line_conditions_only,
            include_ternary_expressions,
        }
    }

    fn assign_inside() -> ConditionalAssignmentOptions {
        opts(EnforcedStyle::AssignInsideCondition, true, true)
    }

    #[test]
    fn flags_if_else_same_assignment() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              bar = 1
            else
              bar = 2
            end
        "});
    }

    #[test]
    fn flags_nested_parenthesized_assignments() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              ((bar = 1))
            else
              ((bar = 2))
            end
        "});
    }

    #[test]
    fn accepts_if_else_different_vars() {
        test::<ConditionalAssignment>()
            .expect_no_offenses("if foo\n  bar = 1\nelse\n  baz = 2\nend\n");
    }

    #[test]
    fn accepts_direct_assignment() {
        test::<ConditionalAssignment>()
            .expect_no_offenses("bar = if foo\n  1\nelse\n  2\nend\n");
    }

    #[test]
    fn flags_case_when_same_assignment() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            case foo
            ^^^^^^^^ Use the return of the conditional for variable assignment and comparison.
            when 'a'
              bar = 1
            else
              bar = 2
            end
        "});
    }

    #[test]
    fn flags_if_elsif_else_chain() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              bar = 1
            elsif baz
              bar = 2
            else
              bar = 3
            end
        "});
    }

    #[test]
    fn accepts_elsif_chain_with_mismatched_var() {
        test::<ConditionalAssignment>().expect_no_offenses(indoc! {"
            if foo
              bar = 1
            elsif baz
              other = 2
            else
              bar = 3
            end
        "});
    }

    #[test]
    fn flags_op_asgn_branches() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              bar += 1
            else
              bar += 2
            end
        "});
    }

    #[test]
    fn accepts_mixed_assignment_types() {
        // `assignment_types_match?`: `bar = 1` and `bar += 2` differ.
        test::<ConditionalAssignment>()
            .expect_no_offenses("if foo\n  bar = 1\nelse\n  bar += 2\nend\n");
    }

    #[test]
    fn flags_case_in_same_assignment() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            case foo
            ^^^^^^^^ Use the return of the conditional for variable assignment and comparison.
            in 1
              bar = 1
            else
              bar = 2
            end
        "});
    }

    #[test]
    fn flags_ternary_assignment() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            foo? ? bar = 1 : bar = 2
            ^^^^^^^^^^^^^^^^^^^^^^^^ Use the return of the conditional for variable assignment and comparison.
        "});
    }

    #[test]
    fn accepts_ternary_when_include_ternary_disabled() {
        test::<ConditionalAssignment>()
            .with_options(&opts(EnforcedStyle::AssignToCondition, true, false))
            .expect_no_offenses("foo? ? bar = 1 : bar = 2\n");
    }

    #[test]
    fn accepts_multi_statement_branch_with_single_line_only() {
        // SingleLineConditionsOnly: true (default) → multi-statement branch
        // tail is not flagged.
        test::<ConditionalAssignment>().expect_no_offenses(indoc! {"
            if foo
              do_something
              bar = 1
            else
              do_other
              bar = 2
            end
        "});
    }

    #[test]
    fn flags_multi_statement_branch_when_single_line_only_disabled() {
        test::<ConditionalAssignment>()
            .with_options(&opts(EnforcedStyle::AssignToCondition, false, true))
            .expect_offense(indoc! {"
                if foo
                ^^^^^^ Use the return of the conditional for variable assignment and comparison.
                  do_something
                  bar = 1
                else
                  do_other
                  bar = 2
                end
            "});
    }

    #[test]
    fn accepts_if_without_else_branch() {
        test::<ConditionalAssignment>().expect_no_offenses("if foo\n  bar = 1\nend\n");
    }

    // --- EnforcedStyle: assign_inside_condition ---

    #[test]
    fn assign_inside_flags_assign_to_if() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                bar = if foo
                ^^^^^^^^^^^^ Assign variables inside of conditionals.
                  1
                else
                  2
                end
            "});
    }

    #[test]
    fn assign_inside_flags_assign_to_case() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                bar = case foo
                ^^^^^^^^^^^^^^ Assign variables inside of conditionals.
                when 'a'
                  1
                else
                  2
                end
            "});
    }

    #[test]
    fn assign_inside_flags_op_asgn_to_if() {
        // Upstream aliases `on_op_asgn` to the same handler. Verified against
        // rubocop 1.87: `bar += if … end` reports on the op-assign node.
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                bar += if foo
                ^^^^^^^^^^^^^ Assign variables inside of conditionals.
                  1
                else
                  2
                end
            "});
    }

    #[test]
    fn assign_inside_flags_or_asgn_to_case() {
        // `bar ||= case … end` must also fire (on_or_asgn alias).
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                bar ||= case foo
                ^^^^^^^^^^^^^^^^ Assign variables inside of conditionals.
                when 'a'
                  1
                else
                  2
                end
            "});
    }

    #[test]
    fn assign_inside_accepts_if_without_else() {
        // Verified against rubocop 1.87: `x = if foo; 1; end` is silent.
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_no_offenses("bar = if foo\n  1\nend\n");
    }

    #[test]
    fn assign_inside_accepts_multi_statement_branch_by_default() {
        // SingleLineConditionsOnly defaults true; a multi-statement branch
        // suppresses the offense. Verified against rubocop 1.87 (silent).
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_no_offenses("bar = if foo\n  a\n  b\nelse\n  c\nend\n");
    }

    #[test]
    fn assign_inside_flags_multi_statement_branch_when_single_line_only_disabled() {
        test::<ConditionalAssignment>()
            .with_options(&opts(EnforcedStyle::AssignInsideCondition, false, true))
            .expect_offense(indoc! {"
                bar = if foo
                ^^^^^^^^^^^^ Assign variables inside of conditionals.
                  a
                  b
                else
                  c
                end
            "});
    }

    #[test]
    fn assign_inside_default_style_does_not_flag_assign_to_condition() {
        // Default style is assign_to_condition; `bar = if … end` is the *good*
        // form and must not be reported.
        test::<ConditionalAssignment>().expect_no_offenses("bar = if foo\n  1\nelse\n  2\nend\n");
    }

    #[test]
    fn assign_to_condition_style_does_not_flag_inside_assignment() {
        // Inverse: default style must not fire on the inside form's good output.
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_no_offenses("if foo\n  bar = 1\nelse\n  bar = 2\nend\n");
    }
}
murphy_plugin_api::submit_cop!(ConditionalAssignment);
