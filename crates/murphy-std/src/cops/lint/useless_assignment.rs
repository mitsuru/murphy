//! `Lint/UselessAssignment` — flags local-variable writes whose result is
//! never read inside the same lexical scope.
//!
//! ## Matched shapes
//!
//! - Plain `Lvasgn` (`x = 1`) — name range, "local variable" message.
//! - `Masgn` (`a, b = 1, 2`) — each LHS target is treated as its own
//!   `Lvasgn`-style write, name range, "local variable" message.
//! - `OpAsgn` / `OrAsgn` / `AndAsgn` (`x += 1`, `x ||= 1`, `x &&= 1`) —
//!   the whole operator-assignment range, "operator-assignment" message.
//!   The value-less `Lvasgn` *target* inside `OpAsgn`/`OrAsgn`/`AndAsgn`
//!   also counts as a read of the variable, so a preceding `x = 0` is
//!   not flagged just because `x += 1` follows.
//! - `Resbody.var` (`rescue => e`) — `e`'s name range, "exception
//!   variable" message.
//!
//! ## Known v1 limitations (Phase 4 — escalate to extend the AST)
//!
//! - `for x in xs` — Murphy's AST has no `For` node yet, so the iteration
//!   variable cannot be inspected.
//! - Pattern-match captures (`case ... in [a, b]`) — Murphy has no
//!   pattern-match nodes yet.
//! - Regexp implicit named captures (`/(?<name>…)/ =~ str`) — needs a
//!   `MatchWithLvasgn`-equivalent node.
//!
//! ## Autocorrect
//!
//! None. Removing or rewriting a dead assignment can change behaviour
//! when the RHS has side effects; the safe-removal heuristic lives in a
//! separate epic.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, Symbol, cop};

#[derive(Default)]
pub struct UselessAssignment;

#[cop(
    name = "Lint/UselessAssignment",
    description = "Flag local variable assignments that are never read in the same scope.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessAssignment {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        visit_scope(cx, cx.root());
    }
}

fn visit_scope(cx: &Cx<'_>, root: NodeId) {
    analyze_scope(cx, root);
    for node in scope_nodes(cx, root) {
        if node != root && is_scope(cx, node) {
            visit_scope(cx, node);
        }
    }
}

const MSG_LOCAL: &str = "Useless assignment to local variable";
const MSG_OPERATOR: &str = "Useless operator-assignment to local variable";
const MSG_EXCEPTION: &str = "Useless assignment to exception variable";

struct Write {
    name: Symbol,
    /// One-past-the-last byte of the assignment. Reads are considered
    /// "after" this point.
    end: u32,
    /// Range to emit the offense on.
    range: Range,
    message: &'static str,
}

struct Read {
    name: Symbol,
    /// Byte position of the read.
    pos: u32,
}

fn analyze_scope(cx: &Cx<'_>, root: NodeId) {
    let mut writes: Vec<Write> = Vec::new();
    let mut reads: Vec<Read> = Vec::new();
    for id in scope_nodes(cx, root) {
        classify(cx, id, &mut writes, &mut reads);
    }
    for write in &writes {
        let read_after = reads
            .iter()
            .any(|r| r.name == write.name && r.pos > write.end);
        if !read_after {
            cx.emit_offense(write.range, write.message, None);
        }
    }
}

fn classify(cx: &Cx<'_>, id: NodeId, writes: &mut Vec<Write>, reads: &mut Vec<Read>) {
    match *cx.kind(id) {
        NodeKind::Lvasgn { name, value } if value.get().is_some() => {
            // Plain `x = 1` (and the per-target `Lvasgn` nodes inside an
            // `Mlhs`, since `translate_target` emits a value-less form
            // for those — they are picked up in the `Masgn` arm).
            push_local_write(cx, id, name, cx.range(id).end, writes);
        }
        NodeKind::OpAsgn { target, .. }
        | NodeKind::OrAsgn { target, .. }
        | NodeKind::AndAsgn { target, .. } => {
            // `x op= rhs` is semantically `x = x op rhs`: record the read
            // of `x` and a write whose offense range is the whole op-asgn.
            if let NodeKind::Lvasgn { name, .. } = *cx.kind(target) {
                reads.push(Read {
                    name,
                    pos: cx.range(target).start,
                });
                if !cx.symbol_str(name).starts_with('_') {
                    writes.push(Write {
                        name,
                        end: cx.range(id).end,
                        range: cx.range(id),
                        message: MSG_OPERATOR,
                    });
                }
            }
        }
        NodeKind::Masgn { lhs, .. } => {
            if let NodeKind::Mlhs(list) = *cx.kind(lhs) {
                let asgn_end = cx.range(id).end;
                for &target in cx.list(list) {
                    if let NodeKind::Lvasgn { name, .. } = *cx.kind(target) {
                        push_local_write(cx, target, name, asgn_end, writes);
                    }
                }
            }
        }
        NodeKind::Resbody { var, .. } => {
            if let Some(var_id) = var.get()
                && let NodeKind::Lvasgn { name, .. } = *cx.kind(var_id)
                && !cx.symbol_str(name).starts_with('_')
            {
                writes.push(Write {
                    name,
                    end: cx.range(var_id).end,
                    range: cx.range(var_id),
                    message: MSG_EXCEPTION,
                });
            }
        }
        NodeKind::Lvar(name) => {
            reads.push(Read {
                name,
                pos: cx.range(id).start,
            });
        }
        _ => {}
    }
}

fn push_local_write(
    cx: &Cx<'_>,
    node: NodeId,
    name: Symbol,
    asgn_end: u32,
    writes: &mut Vec<Write>,
) {
    let name_str = cx.symbol_str(name);
    if name_str.starts_with('_') {
        return;
    }
    writes.push(Write {
        name,
        end: asgn_end,
        range: assignment_name_range(cx, node, name_str),
        message: MSG_LOCAL,
    });
}

fn scope_nodes(cx: &Cx<'_>, root: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        out.push(node);
        if node != root && is_scope(cx, node) {
            continue;
        }
        let mut children = cx.children(node);
        children.reverse();
        stack.extend(children);
    }
    out
}

fn is_scope(cx: &Cx<'_>, node: NodeId) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Def { .. }
            | NodeKind::Block { .. }
            | NodeKind::Class { .. }
            | NodeKind::Module { .. }
            | NodeKind::Sclass { .. }
    )
}

/// `Lvasgn` ranges always start at the variable name: Prism emits
/// `[name_start, value_end)` for `x = 1` and `[name_start, name_end)` for
/// the LHS of `x += 1` (the enclosing `OpAsgn` carries the RHS).
fn assignment_name_range(cx: &Cx<'_>, node: NodeId, name: &str) -> Range {
    let start = cx.range(node).start;
    Range {
        start,
        end: start + name.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::UselessAssignment;
    use murphy_plugin_api::test_support::{
        expect_no_offenses, expect_offense, indoc, run_cop_with_edits,
    };

    #[test]
    fn flags_assignments_that_are_never_read() {
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
            used = 1
            unused = 2
            ^^^^^^ Useless assignment to local variable
            used
        "#}
        );
    }

    #[test]
    fn ignores_underscore_assignments_and_has_no_autocorrect() {
        expect_no_offenses!(UselessAssignment, "名前 = 1\n名前\n_unused = 2\n");
        let run = run_cop_with_edits::<UselessAssignment>("unused = 1\n");
        assert_eq!(run.edits.len(), 0);
    }

    #[test]
    fn nested_method_read_does_not_satisfy_outer_assignment() {
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
            outer = 1
            ^^^^^ Useless assignment to local variable
            def inner
              outer
            end
        "#}
        );
    }

    #[test]
    fn earlier_read_does_not_satisfy_later_assignment() {
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
            x = 0
            x
            x = 1
            ^ Useless assignment to local variable
        "#}
        );
    }

    // murphy-8k4y: cover Masgn / OpAsgn / OrAsgn / AndAsgn / Resbody shapes.

    #[test]
    fn masgn_flags_only_unused_targets() {
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
            a, b = 1, 2
            ^ Useless assignment to local variable
            b
        "#}
        );
    }

    #[test]
    fn masgn_with_all_used_targets_is_not_flagged() {
        expect_no_offenses!(
            UselessAssignment,
            indoc! {r#"
                a, b = 1, 2
                a + b
            "#}
        );
    }

    #[test]
    fn op_asgn_flags_only_the_op_asgn_when_result_unused() {
        // `x = 0` is read by `x += 1` (the operator-assignment implicitly
        // reads `x`); only the `x += 1` write is useless.
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
            x = 0
            x += 1
            ^^^^^^ Useless operator-assignment to local variable
        "#}
        );
    }

    #[test]
    fn or_asgn_uses_operator_message() {
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
            x = 0
            x ||= 1
            ^^^^^^^ Useless operator-assignment to local variable
        "#}
        );
    }

    #[test]
    fn and_asgn_uses_operator_message() {
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
            x = 0
            x &&= 1
            ^^^^^^^ Useless operator-assignment to local variable
        "#}
        );
    }

    #[test]
    fn op_asgn_target_counts_as_read_for_prior_assignment() {
        expect_no_offenses!(
            UselessAssignment,
            indoc! {r#"
                x = 0
                x += 1
                x
            "#}
        );
    }

    #[test]
    fn rescue_var_flags_when_unused() {
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
            begin
              raise
            rescue => e
                      ^ Useless assignment to exception variable
              :rescued
            end
        "#}
        );
    }

    #[test]
    fn rescue_var_used_is_not_flagged() {
        expect_no_offenses!(
            UselessAssignment,
            indoc! {r#"
                begin
                  raise
                rescue => e
                  e.message
                end
            "#}
        );
    }

    #[test]
    fn rescue_var_underscore_is_silenced() {
        expect_no_offenses!(
            UselessAssignment,
            indoc! {r#"
                begin
                  raise
                rescue => _err
                  :rescued
                end
            "#}
        );
    }
}
