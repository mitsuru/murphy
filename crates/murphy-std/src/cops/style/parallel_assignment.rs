//! `Style/ParallelAssignment` — flags simple usages of parallel assignment.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ParallelAssignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects parallel assignment when lhs count == rhs count, rhs is array,
//!   no splats on either side.
//!   Autocorrect: expands to sequential assignments with topological ordering
//!   when RHS elements reference LHS variables from earlier assignments.
//!   Swap patterns (a, b = b, a) form cyclic dependencies and are allowed (no offense).
//!   Modifier if/unless/while/until guard: autocorrect wraps in block form.
//!   Rescue modifier: autocorrect wraps in begin/rescue block.
//!   Indexed and attribute assignments (obj.attr=, ary[i]=) are supported in
//!   detection but lhs is represented as Unknown nodes; source-level correction
//!   uses raw source for those targets.
//!   Parity gap: `find_valid_order` returns nil for cyclic deps (swaps allowed).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! a, b, c = 1, 2, 3
//! a, b = [1, 2], {a: 1}
//!
//! # good (allowed)
//! a, b = b, a        # swap — cyclic dependency, allowed
//! a, b = foo         # rhs is not an array literal
//! a, *b = foo        # splat in lhs
//! a, b = 1, 2, 3     # mismatched count
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, Symbol, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct ParallelAssignment;

const MSG: &str = "Do not use parallel assignment.";

#[cop(
    name = "Style/ParallelAssignment",
    description = "Checks for simple usages of parallel assignment.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ParallelAssignment {
    #[on_node(kind = "masgn")]
    fn check_masgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Masgn { lhs, rhs } = *cx.kind(node) else {
        return;
    };

    // Peel rescue wrapper: `a, b = 1, 2 rescue foo` → rhs is the array inside.
    let (actual_rhs, rescue_node) = peel_rescue(rhs, cx);

    let NodeKind::Array(rhs_list) = *cx.kind(actual_rhs) else {
        // rhs is not an array literal (e.g. `a, b = foo`)
        return;
    };

    let NodeKind::Mlhs(lhs_list) = *cx.kind(lhs) else {
        return;
    };

    let lhs_elems = cx.list(lhs_list);
    let rhs_elems = cx.list(rhs_list);

    // allowed_lhs?: single element or any splat in lhs
    if lhs_elems.len() <= 1 {
        return;
    }
    if lhs_elems
        .iter()
        .any(|&id| matches!(*cx.kind(id), NodeKind::Splat(_)))
    {
        return;
    }

    // allowed_rhs?: any splat in rhs
    if rhs_elems
        .iter()
        .any(|&id| matches!(*cx.kind(id), NodeKind::Splat(_)))
    {
        return;
    }

    // Count mismatch → allowed
    if lhs_elems.len() != rhs_elems.len() {
        return;
    }

    // Build (lhs, rhs) pairs and attempt topological sort.
    let pairs: Vec<(NodeId, NodeId)> = lhs_elems
        .iter()
        .copied()
        .zip(rhs_elems.iter().copied())
        .collect();

    // allowed_masign?: cyclic dependency → no valid order → allowed (swap pattern)
    let Some(order) = find_valid_order(&pairs, cx) else {
        return;
    };

    // Compute offense range: from start of lhs to end of actual_rhs.
    let lhs_range = cx.range(lhs);
    let rhs_range = cx.range(actual_rhs);
    let offense_range = Range {
        start: lhs_range.start,
        end: rhs_range.end,
    };

    cx.emit_offense(offense_range, MSG, None);
    emit_correction(node, lhs, actual_rhs, rescue_node, &order, cx);
}

// ---------------------------------------------------------------------------
// Topological sort (Kahn's algorithm)
// ---------------------------------------------------------------------------

/// Returns `Some(ordered_pairs)` if a valid sequential order exists,
/// or `None` if there is a cyclic dependency (swap pattern).
fn find_valid_order(pairs: &[(NodeId, NodeId)], cx: &Cx<'_>) -> Option<Vec<(NodeId, NodeId)>> {
    let n = pairs.len();
    // Build adjacency: edges[i] = set of j where pairs[i] must come AFTER pairs[j]
    // i.e., rhs[i] uses lhs[j] → pairs[j] must come before pairs[i]
    // so pairs[i] has a dependency on pairs[j]
    let mut in_edges: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (i, &(lhs_i, _)) in pairs.iter().enumerate() {
        for (j, &(_, rhs_j)) in pairs.iter().enumerate() {
            if i == j {
                continue;
            }
            if rhs_uses_lhs(rhs_j, lhs_i, cx) {
                // rhs_j references lhs_i → pairs[j] must come AFTER pairs[i]
                // equivalently, pairs[i] must come before pairs[j]
                // edge: i → j (j depends on i)
                in_edges[i].push(j);
            }
        }
    }

    // Kahn's algorithm
    let mut result = Vec::with_capacity(n);
    let mut in_degree: Vec<usize> = in_edges.iter().map(|e| e.len()).collect();
    let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();

    // Use a stable sort to keep original order for ties.
    while !queue.is_empty() {
        queue.sort_unstable();
        let i = queue.remove(0);
        result.push(pairs[i]);
        for j in 0..n {
            if in_edges[j].contains(&i) {
                in_degree[j] -= 1;
                if in_degree[j] == 0 {
                    queue.push(j);
                }
            }
        }
    }

    if result.len() == n {
        Some(result)
    } else {
        None // cyclic dependency
    }
}

/// Return `true` if `rhs_node` references the variable assigned by `lhs_node`.
fn rhs_uses_lhs(rhs_node: NodeId, lhs_node: NodeId, cx: &Cx<'_>) -> bool {
    let lhs_var = extract_lhs_var(lhs_node, cx);
    if lhs_var.is_none() {
        // Unknown lhs (indexed/attr): conservative — treat as no dependency for now
        return false;
    }
    let lhs_var = lhs_var.unwrap();
    node_uses_var(rhs_node, &lhs_var, cx)
}

/// A typed variable reference for dependency checking.
#[derive(Clone, PartialEq, Eq)]
enum Var {
    Local(Symbol),
    Instance(Symbol),
    Class(Symbol),
    Global(Symbol),
    Const(String), // const name as string
}

/// Extract the LHS variable from a simple assignment node.
fn extract_lhs_var(node: NodeId, cx: &Cx<'_>) -> Option<Var> {
    match *cx.kind(node) {
        NodeKind::Lvasgn { name, .. } => Some(Var::Local(name)),
        NodeKind::Ivasgn { name, .. } => Some(Var::Instance(name)),
        NodeKind::Cvasgn { name, .. } => Some(Var::Class(name)),
        NodeKind::Gvasgn { name, .. } => Some(Var::Global(name)),
        NodeKind::Casgn { .. } => cx.const_name(node).map(Var::Const),
        _ => None,
    }
}

/// Return `true` if `node` (or any descendant) reads the variable `var`.
fn node_uses_var(node: NodeId, var: &Var, cx: &Cx<'_>) -> bool {
    match (var, cx.kind(node)) {
        (Var::Local(sym), NodeKind::Lvar(s)) => s == sym,
        (Var::Instance(sym), NodeKind::Ivar(s)) => s == sym,
        (Var::Class(sym), NodeKind::Cvar(s)) => s == sym,
        (Var::Global(sym), NodeKind::Gvar(s)) => s == sym,
        (Var::Const(name), NodeKind::Const { .. }) => {
            cx.const_name(node).as_deref() == Some(name.as_str())
        }
        _ => {
            // Recurse into children.
            cx.children(node)
                .iter()
                .any(|&child| node_uses_var(child, var, cx))
        }
    }
}

// ---------------------------------------------------------------------------
// Autocorrect
// ---------------------------------------------------------------------------

fn emit_correction(
    node: NodeId,
    _lhs: NodeId,
    _actual_rhs: NodeId,
    rescue_node: Option<NodeId>,
    order: &[(NodeId, NodeId)],
    cx: &Cx<'_>,
) {
    // Compute indentation from the start of the line containing the masgn.
    let node_range = cx.range(node);
    let indent = compute_indent(node_range.start, cx);

    // Build the sequential assignments.
    let assignments: Vec<String> = order
        .iter()
        .map(|(lhs_n, rhs_n)| {
            format!(
                "{}{} = {}",
                indent,
                lhs_source(*lhs_n, cx),
                rhs_source(*rhs_n, cx)
            )
        })
        .collect();

    // Determine the correction form based on context.
    let parent = cx.parent(node).get();

    if let Some(rescue_node_id) = rescue_node {
        // `a, b = 1, 2 rescue foo` → `begin; ...; rescue; foo; end`
        emit_rescue_correction(node, rescue_node_id, &assignments, &indent, cx);
    } else if let Some(p) = parent {
        if is_modifier_conditional(p, node, cx) {
            emit_modifier_correction(p, node, &assignments, cx);
            return;
        }
        // default: replace just the masgn node
        let joined = assignments.join("\n");
        cx.emit_edit(node_range, &joined);
    } else {
        let joined = assignments.join("\n");
        cx.emit_edit(node_range, &joined);
    }
}

/// Emit correction for rescue modifier: `a, b = 1, 2 rescue foo` → begin/rescue block.
fn emit_rescue_correction(
    node: NodeId,
    rescue_id: NodeId,
    assignments: &[String],
    indent: &str,
    cx: &Cx<'_>,
) {
    // rescue_id is the Rescue node wrapping the rhs array.
    // Its parent is the Masgn node.
    // We need to find the rescue handler body.
    let handler_body = rescue_handler_body(rescue_id, cx);
    let inner_indent = format!("{}  ", indent);

    let asgn_lines: Vec<String> = assignments
        .iter()
        .map(|a| {
            // Strip the leading indent from the assignment since we'll re-add the inner indent.
            let trimmed = a.trim_start();
            format!("{}{}", inner_indent, trimmed)
        })
        .collect();

    let rescue_body = handler_body
        .map(|b| cx.raw_source(cx.range(b)).to_owned())
        .unwrap_or_default();

    let replacement = format!(
        "begin\n{}\n{}rescue\n{}{}\n{}end",
        asgn_lines.join("\n"),
        indent,
        inner_indent,
        rescue_body,
        indent,
    );

    // Replace the whole Masgn's parent's range (which includes the rescue keyword).
    // The masgn node includes the rescue node in its rhs range, so use the masgn range.
    // Actually, the `a, b = 1, 2 rescue foo` node: the Masgn's rhs is the Rescue node.
    // The Masgn's range covers `a, b = 1, 2 rescue foo`.
    cx.emit_edit(cx.range(node), &replacement);
}

/// Get the rescue handler's body node from a Rescue node.
fn rescue_handler_body(rescue_id: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Rescue { resbodies, .. } = *cx.kind(rescue_id) else {
        return None;
    };
    let resbody_ids = cx.list(resbodies);
    let first = resbody_ids.first()?;
    let NodeKind::Resbody { body, .. } = *cx.kind(*first) else {
        return None;
    };
    body.get()
}

/// Emit correction for modifier conditional: `a, b = 1, 2 if foo` → `if foo; ...; end`.
fn emit_modifier_correction(
    parent: NodeId,
    node: NodeId,
    assignments: &[String],
    cx: &Cx<'_>,
) {
    let indent = compute_indent(cx.range(parent).start, cx);
    let inner_indent = format!("{}  ", indent);

    let asgn_lines: Vec<String> = assignments
        .iter()
        .map(|a| {
            let trimmed = a.trim_start();
            format!("{}{}", inner_indent, trimmed)
        })
        .collect();

    // The modifier form: `a, b = 1, 2 if foo` (or unless/while/until).
    // We need the keyword and condition from the parent If node.
    let NodeKind::If { cond, then_: _, else_ } = *cx.kind(parent) else {
        // Not an if/unless — fall back to simple replacement
        let joined = assignments.join("\n");
        cx.emit_edit(cx.range(node), &joined);
        return;
    };

    // Determine keyword: `unless` if the masgn is in else_, `if` if in then_.
    let keyword = if else_.get() == Some(node) { "unless" } else { "if" };
    let cond_src = cx.raw_source(cx.range(cond)).to_owned();

    let replacement = format!(
        "{} {}\n{}\n{}end",
        keyword,
        cond_src,
        asgn_lines.join("\n"),
        indent,
    );

    cx.emit_edit(cx.range(parent), &replacement);
}

// ---------------------------------------------------------------------------
// Source extraction helpers
// ---------------------------------------------------------------------------

/// Get the source text for an lhs target node.
fn lhs_source(node: NodeId, cx: &Cx<'_>) -> String {
    match *cx.kind(node) {
        NodeKind::Lvasgn { name, .. }
        | NodeKind::Ivasgn { name, .. }
        | NodeKind::Cvasgn { name, .. }
        | NodeKind::Gvasgn { name, .. } => cx.symbol_str(name).to_owned(),
        _ => {
            // Unknown (indexed/attr): use raw source up to `=` token (or the full range).
            // For the mlhs context, the node is the whole target without `=`.
            cx.raw_source(cx.range(node)).to_owned()
        }
    }
}

/// Get the source text for an rhs value node.
fn rhs_source(node: NodeId, cx: &Cx<'_>) -> String {
    match *cx.kind(node) {
        // `__FILE__` is a Str node without a begin delimiter — use raw source.
        // Sym nodes without a begin delimiter (bare symbol) need `:` prefix.
        NodeKind::Sym(_) => {
            let src = cx.raw_source(cx.range(node));
            // If it doesn't start with `:` or `%s`, it's a bare symbol — add colon.
            if src.starts_with(':') || src.starts_with('%') {
                src.to_owned()
            } else {
                format!(":{}", src)
            }
        }
        _ => cx.raw_source(cx.range(node)).to_owned(),
    }
}

/// Compute the indentation (leading whitespace) of the line containing `offset`.
fn compute_indent(offset: u32, cx: &Cx<'_>) -> String {
    let source = cx.source().as_bytes();
    let start = offset as usize;
    let line_start = source[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    let indent_bytes = source[line_start..start]
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .count();
    String::from_utf8_lossy(&source[line_start..line_start + indent_bytes]).into_owned()
}

/// Peel a Rescue node from the rhs of a Masgn, returning (actual_rhs, Some(rescue_node)).
/// If no rescue, returns (rhs, None).
fn peel_rescue(rhs: NodeId, cx: &Cx<'_>) -> (NodeId, Option<NodeId>) {
    if let NodeKind::Rescue { body, .. } = *cx.kind(rhs)
        && let Some(inner) = body.get()
    {
        return (inner, Some(rhs));
    }
    (rhs, None)
}

/// Return `true` if `parent` is a modifier conditional (`if`/`unless`/`while`/`until`)
/// and `child` is the guarded body.
fn is_modifier_conditional(parent: NodeId, child: NodeId, cx: &Cx<'_>) -> bool {
    // Modifier form: keyword loc is ZERO for modifier if/unless
    let kw = cx.loc(parent).keyword();
    if kw != Range::ZERO {
        return false; // block form
    }
    match *cx.kind(parent) {
        NodeKind::If { then_, else_, .. } => {
            then_.get() == Some(child) || else_.get() == Some(child)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::ParallelAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- basic offense cases ---

    #[test]
    fn flags_simple_parallel_assignment() {
        test::<ParallelAssignment>().expect_offense(indoc! {"
            a, b, c = 1, 2, 3
            ^^^^^^^^^^^^^^^^^ Do not use parallel assignment.
        "});
    }

    #[test]
    fn corrects_simple_parallel_assignment() {
        test::<ParallelAssignment>().expect_correction(
            indoc! {"
                a, b, c = 1, 2, 3
                ^^^^^^^^^^^^^^^^^ Do not use parallel assignment.
            "},
            "a = 1\nb = 2\nc = 3\n",
        );
    }

    #[test]
    fn flags_parallel_assignment_with_arrays_on_rhs() {
        test::<ParallelAssignment>().expect_offense(indoc! {"
            a, b, c = [1, 2], [3, 4], [5, 6]
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use parallel assignment.
        "});
    }

    #[test]
    fn corrects_parallel_assignment_with_arrays_on_rhs() {
        test::<ParallelAssignment>().expect_correction(
            indoc! {"
                a, b, c = [1, 2], [3, 4], [5, 6]
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use parallel assignment.
            "},
            "a = [1, 2]\nb = [3, 4]\nc = [5, 6]\n",
        );
    }

    #[test]
    fn flags_parallel_assignment_with_hashes_on_rhs() {
        test::<ParallelAssignment>().expect_offense(indoc! {"
            a, b, c = {a: 1}, {b: 2}, {c: 3}
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use parallel assignment.
        "});
    }

    #[test]
    fn corrects_parallel_assignment_with_hashes_on_rhs() {
        test::<ParallelAssignment>().expect_correction(
            indoc! {"
                a, b, c = {a: 1}, {b: 2}, {c: 3}
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use parallel assignment.
            "},
            "a = {a: 1}\nb = {b: 2}\nc = {c: 3}\n",
        );
    }

    #[test]
    fn flags_parallel_assignment_with_constants_on_rhs() {
        test::<ParallelAssignment>().expect_offense(indoc! {"
            a, b, c = CONSTANT1, CONSTANT2, CONSTANT3
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use parallel assignment.
        "});
    }

    #[test]
    fn corrects_parallel_assignment_with_constants_on_rhs() {
        test::<ParallelAssignment>().expect_correction(
            indoc! {"
                a, b, c = CONSTANT1, CONSTANT2, CONSTANT3
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use parallel assignment.
            "},
            "a = CONSTANT1\nb = CONSTANT2\nc = CONSTANT3\n",
        );
    }

    #[test]
    fn flags_when_assignments_must_be_reordered_to_preserve_meaning() {
        test::<ParallelAssignment>().expect_offense(indoc! {"
            a, b = 1, a
            ^^^^^^^^^^^ Do not use parallel assignment.
        "});
    }

    #[test]
    fn corrects_reordering_to_preserve_meaning() {
        test::<ParallelAssignment>().expect_correction(
            indoc! {"
                a, b = 1, a
                ^^^^^^^^^^^ Do not use parallel assignment.
            "},
            "b = a\na = 1\n",
        );
    }

    #[test]
    fn flags_assigning_to_same_variables_in_same_order() {
        test::<ParallelAssignment>().expect_offense(indoc! {"
            a, b = a, b
            ^^^^^^^^^^^ Do not use parallel assignment.
        "});
    }

    #[test]
    fn corrects_same_order_assignment() {
        test::<ParallelAssignment>().expect_correction(
            indoc! {"
                a, b = a, b
                ^^^^^^^^^^^ Do not use parallel assignment.
            "},
            "a = a\nb = b\n",
        );
    }

    // --- no-offense cases ---

    #[test]
    fn allows_swap() {
        test::<ParallelAssignment>().expect_no_offenses("a, b = b, a\n");
    }

    #[test]
    fn allows_non_array_rhs() {
        test::<ParallelAssignment>().expect_no_offenses("a, b = foo\n");
    }

    #[test]
    fn allows_splat_in_lhs() {
        test::<ParallelAssignment>().expect_no_offenses("a, *b = 1, 2, 3\n");
    }

    #[test]
    fn allows_splat_in_rhs() {
        test::<ParallelAssignment>().expect_no_offenses("a, b = 1, *foo\n");
    }

    #[test]
    fn allows_more_left_than_right() {
        test::<ParallelAssignment>().expect_no_offenses("a, b, c, d = 1, 2\n");
    }

    #[test]
    fn allows_more_right_than_left() {
        test::<ParallelAssignment>().expect_no_offenses("a, b = 1, 2, 3\n");
    }

    #[test]
    fn allows_expanding_an_assigned_var() {
        test::<ParallelAssignment>().expect_no_offenses(indoc! {"
            foo = [1, 2, 3]
            a, b, c = foo
        "});
    }
}
murphy_plugin_api::submit_cop!(ParallelAssignment);
