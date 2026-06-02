//! `Style/IdenticalConditionalBranches` — flags identical leading or trailing
//! expressions across all branches of a conditional.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/IdenticalConditionalBranches
//! upstream_version_checked: 1.69.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection is implemented for `if/elsif/else`, `case/when/else`, and
//!   `case/in/else` (pattern matching). Identical tail (last expression) and
//!   head (first expression) expressions are flagged. Expression equality is
//!   compared via `raw_source`, which is whitespace-sensitive (a known gap vs
//!   RuboCop's structural node equality). The `last_child_of_parent` +
//!   `single_child_branch` guard prevents double-flagging when a single-statement
//!   branch would be flagged both as head and tail. The assignment-to-condition
//!   variable guard is implemented.
//!
//!   Autocorrect is not implemented (autocorrect for this cop is marked unsafe
//!   in RuboCop due to potential reordering of method calls with side effects).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};


const MSG: &str = "Move `%<source>s` out of the conditional.";

#[derive(Default)]
pub struct IdenticalConditionalBranches;

#[cop(
    name = "Style/IdenticalConditionalBranches",
    description = "Checks that conditional statements do not have an identical line at the end of each branch, which can be moved out.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl IdenticalConditionalBranches {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check_if(node, cx);
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        check_case(node, cx);
    }

    #[on_node(kind = "case_match")]
    fn check_case_match(&self, node: NodeId, cx: &Cx<'_>) {
        check_case_match(node, cx);
    }
}

fn check_if(node: NodeId, cx: &Cx<'_>) {
    // Skip elsif nodes (handled as part of the parent if's else chain).
    if cx.is_elsif(node) {
        return;
    }

    // Must have an else branch to have multiple branches to compare.
    if !cx.is_else(node) {
        return;
    }

    // Expand all branches: if_branch + any elsif bodies + else body.
    let branches = expand_if_branches(node, cx);
    check_branches(node, &branches, cx);
}

fn check_case(node: NodeId, cx: &Cx<'_>) {
    // Must have an else branch.
    let else_branch = match cx.case_else_branch(node).get() {
        Some(id) => id,
        None => return,
    };

    let when_branches = cx.case_when_branches(node);
    let mut branches: Vec<Option<NodeId>> = when_branches
        .iter()
        .map(|&w| cx.when_body(w).get())
        .collect();
    branches.push(Some(else_branch));

    check_branches(node, &branches, cx);
}

fn check_case_match(node: NodeId, cx: &Cx<'_>) {
    // Must have an else branch.
    let else_branch = match cx.case_match_else_branch(node).get() {
        Some(id) => id,
        None => return,
    };

    let in_branches = cx.in_pattern_branches(node);
    let mut branches: Vec<Option<NodeId>> = in_branches
        .iter()
        .map(|&p| cx.in_pattern_body(p).get())
        .collect();
    branches.push(Some(else_branch));

    check_branches(node, &branches, cx);
}

/// Expand an `if` node into its logical branches (if_branch, any elsif bodies,
/// else body). Returns `None` for any nil/missing branch.
fn expand_if_branches(node: NodeId, cx: &Cx<'_>) -> Vec<Option<NodeId>> {
    let mut result = Vec::new();

    // if_branch (the body run when condition is true for `if`).
    result.push(cx.if_branch(node).get());

    // Walk the else chain, collecting elsif bodies and the final else body.
    let mut current = cx.if_else_branch(node).get();
    loop {
        match current {
            None => {
                result.push(None);
                break;
            }
            Some(id) if matches!(cx.kind(id), NodeKind::If { .. }) && cx.is_elsif(id) => {
                // This is an elsif: push its body and continue with its else.
                result.push(cx.if_branch(id).get());
                current = cx.if_else_branch(id).get();
            }
            Some(id) => {
                // This is a plain else body.
                result.push(Some(id));
                break;
            }
        }
    }

    result
}

/// Core check: given a list of branch body node IDs (Some(id) or None),
/// check if tails or heads are identical across all branches.
fn check_branches(node: NodeId, branches: &[Option<NodeId>], cx: &Cx<'_>) {
    // If any branch is nil, skip (an empty branch means no offense is possible).
    if branches.iter().any(|b| b.is_none()) {
        return;
    }

    let branch_ids: Vec<NodeId> = branches.iter().map(|b| b.unwrap()).collect();

    // Check tails (last expressions).
    let tails: Vec<NodeId> = branch_ids.iter().map(|&b| tail_of(b, cx)).collect();
    if duplicated_expressions(node, &tails, cx) {
        for &expr in &tails {
            let src = cx.raw_source(cx.range(expr)).to_owned();
            let msg = MSG.replace("%<source>s", &src);
            cx.emit_offense(cx.range(expr), &msg, None);
        }
    }

    // Guard: if the conditional is the last child of its parent and any branch
    // is a single-statement branch, skip the head check to avoid double-flagging
    // when tail == head (e.g. `if foo; x; else; x; end` — only tail fires).
    if last_child_of_parent(node, cx) && branch_ids.iter().any(|&b| single_child_branch(b, cx)) {
        return;
    }

    // Check heads (first expressions).
    let heads: Vec<NodeId> = branch_ids.iter().map(|&b| head_of(b, cx)).collect();
    if duplicated_expressions(node, &heads, cx) {
        for &expr in &heads {
            let src = cx.raw_source(cx.range(expr)).to_owned();
            let msg = MSG.replace("%<source>s", &src);
            cx.emit_offense(cx.range(expr), &msg, None);
        }
    }
}

/// Returns the last statement in a branch body.
/// If the body is a `begin` (multi-statement), returns the last child.
/// Otherwise returns the node itself.
fn tail_of(node: NodeId, cx: &Cx<'_>) -> NodeId {
    if let NodeKind::Begin(list) = cx.kind(node) {
        let children = cx.list(*list);
        if let Some(&last) = children.last() {
            return last;
        }
    }
    node
}

/// Returns the first statement in a branch body.
/// If the body is a `begin` (multi-statement), returns the first child.
/// Otherwise returns the node itself.
fn head_of(node: NodeId, cx: &Cx<'_>) -> NodeId {
    if let NodeKind::Begin(list) = cx.kind(node) {
        let children = cx.list(*list);
        if let Some(&first) = children.first() {
            return first;
        }
    }
    node
}

/// Returns true if all expressions in the slice have the same source text,
/// and the expression (if it's an assignment) doesn't assign to a variable
/// that appears in the condition.
fn duplicated_expressions(node: NodeId, exprs: &[NodeId], cx: &Cx<'_>) -> bool {
    if exprs.is_empty() {
        return false;
    }

    // All expressions must have the same raw source.
    let first_src = cx.raw_source(cx.range(exprs[0]));
    if !exprs[1..].iter().all(|&e| cx.raw_source(cx.range(e)) == first_src) {
        return false;
    }

    // If the expression is an assignment, check the assignment guard.
    let expr = exprs[0];
    if is_assignment(expr, cx) {
        let condition_variable = assignable_condition_value(node, cx);
        let assigned_var = assigned_lhs_name(expr, cx);
        if let (Some(cond_var), Some(asgn_var)) = (condition_variable, assigned_var) {
            if cond_var == asgn_var {
                return false;
            }
        }
    }

    true
}

/// Returns the string name of the variable on the left-hand side of an
/// assignment node, if it's a simple variable assignment.
fn assigned_lhs_name<'a>(node: NodeId, cx: &'a Cx<'_>) -> Option<&'a str> {
    match cx.kind(node) {
        NodeKind::Lvasgn { name, .. }
        | NodeKind::Ivasgn { name, .. }
        | NodeKind::Cvasgn { name, .. }
        | NodeKind::Gvasgn { name, .. } => Some(cx.symbol_str(*name)),
        _ => None,
    }
}

/// Returns true if the node is a simple variable assignment.
fn is_assignment(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Lvasgn { .. }
            | NodeKind::Ivasgn { .. }
            | NodeKind::Cvasgn { .. }
            | NodeKind::Gvasgn { .. }
            | NodeKind::Casgn { .. }
    )
}

/// Returns the variable name used in the condition expression, to guard against
/// moving assignments that would change semantics.
///
/// Mirrors RuboCop's `assignable_condition_value`.
fn assignable_condition_value<'a>(node: NodeId, cx: &'a Cx<'_>) -> Option<&'a str> {
    let cond = match cx.kind(node) {
        NodeKind::If { cond, .. } => *cond,
        NodeKind::Case { subject, .. } => subject.get()?,
        NodeKind::CaseMatch { subject, .. } => *subject,
        _ => return None,
    };

    match cx.kind(cond) {
        NodeKind::Lvar(name)
        | NodeKind::Ivar(name)
        | NodeKind::Cvar(name)
        | NodeKind::Gvar(name) => Some(cx.symbol_str(*name)),
        NodeKind::Send { receiver, .. } => {
            let recv = receiver.get()?;
            match cx.kind(recv) {
                NodeKind::Lvar(name)
                | NodeKind::Ivar(name)
                | NodeKind::Cvar(name)
                | NodeKind::Gvar(name) => Some(cx.symbol_str(*name)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Returns true if the conditional node is the last child of its parent.
fn last_child_of_parent(node: NodeId, cx: &Cx<'_>) -> bool {
    let parent = match cx.parent(node).get() {
        Some(p) => p,
        None => return true,
    };
    let children = cx.children(parent);
    children.last().map_or(false, |&last| last == node)
}

/// Returns true if the branch body is a single-statement (not a begin with multiple children).
fn single_child_branch(branch: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(branch) {
        NodeKind::Begin(list) => cx.list(*list).len() == 1,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::IdenticalConditionalBranches;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- if/else: identical tail ---

    #[test]
    fn flags_identical_tail_in_both_branches_of_if_else() {
        test::<IdenticalConditionalBranches>().expect_offense(indoc! {"
            if condition
              do_x
              do_z
              ^^^^ Move `do_z` out of the conditional.
            else
              do_y
              do_z
              ^^^^ Move `do_z` out of the conditional.
            end
        "});
    }

    // --- if/else: identical head ---

    #[test]
    fn flags_identical_head_in_if_else() {
        test::<IdenticalConditionalBranches>().expect_offense(indoc! {"
            if condition
              do_z
              ^^^^ Move `do_z` out of the conditional.
              do_x
            else
              do_z
              ^^^^ Move `do_z` out of the conditional.
              do_y
            end
        "});
    }

    // --- if with elsif ---

    #[test]
    fn flags_identical_tail_in_if_elsif_else() {
        test::<IdenticalConditionalBranches>().expect_offense(indoc! {"
            if condition_a
              do_a
              do_z
              ^^^^ Move `do_z` out of the conditional.
            elsif condition_b
              do_b
              do_z
              ^^^^ Move `do_z` out of the conditional.
            else
              do_c
              do_z
              ^^^^ Move `do_z` out of the conditional.
            end
        "});
    }

    // --- single-statement: only tail fires, not head ---

    #[test]
    fn flags_only_tail_for_single_statement_branch() {
        // Single-statement branch: head == tail; only tail flagged (not both).
        test::<IdenticalConditionalBranches>().expect_offense(indoc! {"
            if condition
              do_x
              ^^^^ Move `do_x` out of the conditional.
            else
              do_x
              ^^^^ Move `do_x` out of the conditional.
            end
        "});
    }

    // --- case/when/else ---

    #[test]
    fn flags_identical_tail_in_case_when_else() {
        test::<IdenticalConditionalBranches>().expect_offense(indoc! {"
            case foo
            when 1
              do_x
              ^^^^ Move `do_x` out of the conditional.
            when 2
              do_x
              ^^^^ Move `do_x` out of the conditional.
            else
              do_x
              ^^^^ Move `do_x` out of the conditional.
            end
        "});
    }

    // --- case/in/else (pattern matching) ---

    #[test]
    fn flags_identical_tail_in_case_in_else() {
        test::<IdenticalConditionalBranches>().expect_offense(indoc! {"
            case foo
            in 1
              do_x
              ^^^^ Move `do_x` out of the conditional.
            in 2
              do_x
              ^^^^ Move `do_x` out of the conditional.
            else
              do_x
              ^^^^ Move `do_x` out of the conditional.
            end
        "});
    }

    // --- No offense cases ---

    #[test]
    fn accepts_if_without_else() {
        test::<IdenticalConditionalBranches>().expect_no_offenses(indoc! {"
            if condition
              do_x
            end
        "});
    }

    #[test]
    fn accepts_different_tails() {
        test::<IdenticalConditionalBranches>().expect_no_offenses(indoc! {"
            if condition
              do_x
              do_z
            else
              do_y
              do_w
            end
        "});
    }

    #[test]
    fn accepts_different_heads() {
        test::<IdenticalConditionalBranches>().expect_no_offenses(indoc! {"
            if condition
              do_a
              do_x
            else
              do_b
              do_y
            end
        "});
    }

    #[test]
    fn accepts_case_without_else() {
        test::<IdenticalConditionalBranches>().expect_no_offenses(indoc! {"
            case foo
            when 1
              do_x
            when 2
              do_y
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(IdenticalConditionalBranches);
