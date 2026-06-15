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

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

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
    // Returns None if any branch is missing (empty branch → no offense).
    if let Some(branches) = expand_if_branches(node, cx) {
        check_branches(node, &branches, cx);
    }
}

fn check_case(node: NodeId, cx: &Cx<'_>) {
    // Must have an else branch.
    let else_branch = match cx.case_else_branch(node).get() {
        Some(id) => id,
        None => return,
    };

    let when_branches = cx.case_when_branches(node);
    let mut branch_ids = Vec::with_capacity(when_branches.len() + 1);
    for &w in when_branches {
        match cx.when_body(w).get() {
            Some(id) => branch_ids.push(id),
            None => return, // empty branch → no offense
        }
    }
    branch_ids.push(else_branch);

    check_branches(node, &branch_ids, cx);
}

fn check_case_match(node: NodeId, cx: &Cx<'_>) {
    // Must have an else branch.
    let else_branch = match cx.case_match_else_branch(node).get() {
        Some(id) => id,
        None => return,
    };

    let in_branches = cx.in_pattern_branches(node);
    let mut branch_ids = Vec::with_capacity(in_branches.len() + 1);
    for &p in in_branches {
        match cx.in_pattern_body(p).get() {
            Some(id) => branch_ids.push(id),
            None => return, // empty branch → no offense
        }
    }
    branch_ids.push(else_branch);

    check_branches(node, &branch_ids, cx);
}

/// Expand an `if` node into its logical branch bodies (if_branch, any elsif
/// bodies, else body). Returns `None` if any branch is nil (empty branch →
/// no offense possible).
fn expand_if_branches(node: NodeId, cx: &Cx<'_>) -> Option<Vec<NodeId>> {
    let mut result = Vec::new();

    // if_branch (the body run when condition is true for `if`).
    result.push(cx.if_branch(node).get()?);

    // Walk the else chain, collecting elsif bodies and the final else body.
    let mut current = cx.if_else_branch(node).get();
    loop {
        match current {
            None => {
                return None;
            }
            Some(id) if matches!(cx.kind(id), NodeKind::If { .. }) && cx.is_elsif(id) => {
                // This is an elsif: push its body and continue with its else.
                result.push(cx.if_branch(id).get()?);
                current = cx.if_else_branch(id).get();
            }
            Some(id) => {
                // This is a plain else body.
                result.push(id);
                break;
            }
        }
    }

    Some(result)
}

/// Core check: given a slice of branch body node IDs, check if tails or heads
/// are identical across all branches.
fn check_branches(node: NodeId, branch_ids: &[NodeId], cx: &Cx<'_>) {
    // Check tails (last expressions).
    let tails: Vec<NodeId> = branch_ids.iter().map(|&b| tail_of(b, cx)).collect();
    if duplicated_expressions(node, &tails, cx) {
        for &expr in &tails {
            let src = cx.raw_source(cx.range(expr));
            let msg = format!("Move `{src}` out of the conditional.");
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
            let src = cx.raw_source(cx.range(expr));
            let msg = format!("Move `{src}` out of the conditional.");
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

/// Collect the body source range of every heredoc in the file, paired with the
/// byte offset of each heredoc's opener (`<<~LABEL`). The opener offset lets a
/// caller associate a heredoc body with the expression that contains its opener.
///
/// Heredoc openers and terminators are matched **by label**, not by stack
/// position. A position-based stack is only correct for one nesting direction:
/// same-line sibling heredocs (`foo(<<~A, <<~B)`) close in FIFO order (A's body
/// comes first), while a heredoc opened inside another's interpolated body closes
/// in LIFO order — so neither `pop()` (LIFO) nor `remove(0)` (FIFO) alone is right
/// for both. Matching each `HeredocEnd` label to its `HeredocStart` label (FIFO
/// among the rare same-label case) pairs them correctly regardless of nesting.
///
/// The body spans from the line *after* the opener's line to the start of the
/// terminator's line (so the squiggly closing-label indentation is excluded).
fn heredoc_body_ranges(cx: &Cx<'_>) -> Vec<(u32, Range)> {
    let source = cx.source().as_bytes();
    // (label, opener_start) for each heredoc whose terminator hasn't been seen.
    let mut open: Vec<(&str, u32)> = Vec::new();
    let mut ranges: Vec<(u32, Range)> = Vec::new();

    for tok in cx.sorted_tokens() {
        match tok.kind {
            SourceTokenKind::HeredocStart => {
                open.push((heredoc_label(cx.raw_source(tok.range)), tok.range.start));
            }
            SourceTokenKind::HeredocEnd => {
                let label = heredoc_label(cx.raw_source(tok.range));
                // FIFO among same-label openers (the first-opened body closes
                // first); distinct labels match unambiguously.
                let Some(i) = open.iter().position(|&(l, _)| l == label) else {
                    continue;
                };
                let (_, opener_start) = open.remove(i);
                // Body starts on the line after the opener's line.
                let body_start = source[opener_start as usize..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(source.len() as u32, |p| opener_start + p as u32 + 1);
                // The body ends at the start of the terminator's line so the
                // squiggly indentation of the closing label is excluded.
                let line_start = source[..tok.range.start as usize]
                    .iter()
                    .rposition(|&b| b == b'\n')
                    .map_or(0, |p| p as u32 + 1);
                ranges.push((
                    opener_start,
                    Range {
                        start: body_start,
                        end: line_start,
                    },
                ));
            }
            _ => {}
        }
    }
    ranges
}

/// Extract a heredoc's label from a `HeredocStart` (`<<~LABEL`, `<<-'LABEL'`, …)
/// or `HeredocEnd` (the bare terminator) token's source, so the two can be
/// matched. Strips `<<`, the optional squiggly/dash, surrounding quotes, and any
/// indentation/whitespace.
fn heredoc_label(src: &str) -> &str {
    src.trim()
        .trim_start_matches('<')
        .trim_start_matches(['~', '-'])
        .trim_matches(['\'', '"', '`'])
        .trim()
}

/// Build the comparison key for a branch expression: its raw source plus the
/// content of any heredoc bodies whose opener lies within the expression's range.
///
/// `heredoc_bodies` is the file-wide list produced by [`heredoc_body_ranges`].
fn expr_comparison_key(expr: NodeId, heredoc_bodies: &[(u32, Range)], cx: &Cx<'_>) -> String {
    let expr_range = cx.range(expr);
    let mut key = cx.raw_source(expr_range).to_owned();
    for &(opener_start, body) in heredoc_bodies {
        if opener_start >= expr_range.start && opener_start < expr_range.end {
            key.push('\u{0}'); // separator that cannot appear in Ruby source
            // Skip an empty body (`<<~A\nA`): body-start (line after the opener)
            // equals the terminator line, so the range is empty. `raw_source`
            // would also panic on `start > end`, so slice only when valid.
            if body.start < body.end {
                key.push_str(cx.raw_source(body));
            }
        }
    }
    key
}

/// Returns true if all expressions in the slice have the same source text,
/// and the expression (if it's an assignment) doesn't assign to a variable
/// that appears in the condition.
fn duplicated_expressions(node: NodeId, exprs: &[NodeId], cx: &Cx<'_>) -> bool {
    if exprs.is_empty() {
        return false;
    }

    // All expressions must have the same comparison key. The key is the
    // expression's raw source plus the content of any heredoc bodies it opens.
    // `raw_source(range(expr))` for a heredoc-bearing call (`puts <<~MSG`) covers
    // only the opener line, not the body that follows after the line's `\n`, so
    // two branches with identical openers but different heredoc bodies would
    // otherwise compare equal. Including the body text restores correctness.
    let heredoc_bodies = heredoc_body_ranges(cx);
    let first_key = expr_comparison_key(exprs[0], &heredoc_bodies, cx);
    if !exprs[1..]
        .iter()
        .all(|&e| expr_comparison_key(e, &heredoc_bodies, cx) == first_key)
    {
        return false;
    }

    // If the expression is an assignment, check the assignment guard.
    let expr = exprs[0];
    if is_assignment(expr, cx) {
        let condition_variable = assignable_condition_value(node, cx);
        let assigned_var = assigned_lhs_name(expr, cx);
        if let (Some(cond_var), Some(asgn_var)) = (condition_variable, assigned_var)
            && cond_var == asgn_var {
                return false;
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
    // For Begin/Kwbegin parents, use cx.list for zero-copy traversal.
    match cx.kind(parent) {
        NodeKind::Begin(list) | NodeKind::Kwbegin(list) => {
            cx.list(*list).last().is_some_and(|&last| last == node)
        }
        _ => {
            let children = cx.children(parent);
            children.last().is_some_and(|&last| last == node)
        }
    }
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

    // --- heredoc bodies: differ → no offense ---

    #[test]
    fn accepts_identical_openers_with_different_heredoc_bodies() {
        // Both branches call `puts <<~MSG`, so the opener line (the only part
        // covered by `raw_source(range(expr))`) is identical — but the heredoc
        // bodies differ, so the expressions are NOT identical.
        test::<IdenticalConditionalBranches>().expect_no_offenses(indoc! {"
            if cond
              puts <<~MSG
                first
              MSG
            else
              puts <<~MSG
                second different
              MSG
            end
        "});
    }

    // --- heredoc bodies: identical → still flag ---

    #[test]
    fn flags_identical_openers_with_identical_heredoc_bodies() {
        test::<IdenticalConditionalBranches>().expect_offense(indoc! {"
            if cond
              puts <<~MSG
              ^^^^^^^^^^^ Move `puts <<~MSG` out of the conditional.
                same
              MSG
            else
              puts <<~MSG
              ^^^^^^^^^^^ Move `puts <<~MSG` out of the conditional.
                same
              MSG
            end
        "});
    }

    // --- same-line sibling heredocs: matched by label, not stack position ---

    #[test]
    fn accepts_same_line_sibling_heredocs_with_different_bodies() {
        // Two heredocs share an opener line (`process(<<~A, <<~B)`). In Ruby they
        // terminate in FIFO order, which a LIFO `pop()` would mispair. Bodies
        // differ between the branches, so this must NOT be flagged.
        test::<IdenticalConditionalBranches>().expect_no_offenses(indoc! {"
            if cond
              process(<<~A, <<~B)
                alpha
              A
                beta
              B
            else
              process(<<~A, <<~B)
                gamma
              A
                delta
              B
            end
        "});
    }

    #[test]
    fn flags_same_line_sibling_heredocs_with_identical_bodies() {
        test::<IdenticalConditionalBranches>().expect_offense(indoc! {"
            if cond
              process(<<~A, <<~B)
              ^^^^^^^^^^^^^^^^^^^ Move `process(<<~A, <<~B)` out of the conditional.
                alpha
              A
                beta
              B
            else
              process(<<~A, <<~B)
              ^^^^^^^^^^^^^^^^^^^ Move `process(<<~A, <<~B)` out of the conditional.
                alpha
              A
                beta
              B
            end
        "});
    }

    #[test]
    fn accepts_nested_heredocs_with_different_inner_bodies() {
        // A heredoc opened inside another's interpolated body terminates in LIFO
        // order; label matching pairs both correctly. The inner bodies differ, so
        // the branches are not identical.
        test::<IdenticalConditionalBranches>().expect_no_offenses(indoc! {"
            if cond
              puts <<~OUTER
                pre #{<<~INNER}
                  first inner
                INNER
              OUTER
            else
              puts <<~OUTER
                pre #{<<~INNER}
                  second inner
                INNER
              OUTER
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(IdenticalConditionalBranches);
