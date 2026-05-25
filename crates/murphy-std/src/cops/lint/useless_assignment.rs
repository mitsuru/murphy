//! `Lint/UselessAssignment` — flags local-variable writes whose result is
//! never read, *or* whose result is overwritten by a sibling write before
//! any read can observe it.
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
//! ## Flow-sensitive dataflow (murphy-xek)
//!
//! The cop runs a single-pass intra-scope dataflow:
//!
//! 1. Collect every write (`Lvasgn`, `Masgn` target, `OpAsgn`/`OrAsgn`/
//!    `AndAsgn`, `Resbody.var`) and every read (`Lvar`, plus the implicit
//!    read inside `OpAsgn`/`OrAsgn`/`AndAsgn`) with byte positions.
//! 2. For each write `w` to a variable named `n`:
//!    a. If there is no later read of `n`, flag `w`.
//!    b. Else find the first later read of `n` and the first later write
//!    of `n`. If the later write happens *before* the later read **and**
//!    lies on the same control-flow path as `w`, flag `w` as
//!    overwrite-before-read.
//!
//! "Same control-flow path" is computed by walking up via `cx.parent` and
//! collecting branch-introducing ancestors (`If` / `Case` / `When` /
//! `While` / `Until` / `Rescue`). `Resbody` and `Ensure` are *not*
//! barriers — their children run sequentially, not exclusively, and
//! treating them as barriers would falsely cut the `rescue => e`
//! binding off from reads inside the body. Each chain entry
//! is a pair `(barrier_node, branch_child)` — the child of the barrier
//! on the path from the write — so two writes in different arms of the
//! same `if` get distinct chain entries even though they share the
//! `If` barrier. A write `w'` is guaranteed to be reached after `w`
//! iff `w'`'s chain (outermost-first) is a prefix of `w`'s chain;
//! that captures both the same-chunk case and the "exit some inner
//! branches then continue at a shallower level" case (e.g.
//! `x = 1; if c; x = 2; end; x = 3; x` flags `x = 1` because `x = 3`
//! is in a shallower-than-w2 chunk that is a prefix of `w1`'s).
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
    /// Node id used to compute the control-flow barrier chain when
    /// deciding whether two writes are on the same path.
    node: NodeId,
}

struct Read {
    name: Symbol,
    /// Byte position of the read.
    pos: u32,
    /// Node id used to compute the read's control-flow barrier chain so
    /// we can ask "is this read reachable from a particular write?"
    node: NodeId,
}

fn analyze_scope(cx: &Cx<'_>, root: NodeId) {
    let mut writes: Vec<Write> = Vec::new();
    let mut reads: Vec<Read> = Vec::new();
    for id in scope_nodes(cx, root) {
        classify(cx, id, &mut writes, &mut reads);
    }
    // Pre-compute each write's and read's branch chain so we don't pay
    // for the `cx.parent` walk in the O(W^2) / O(W*R) loops below.
    let write_chains: Vec<Vec<(NodeId, NodeId)>> = writes
        .iter()
        .map(|w| barrier_chain(cx, root, w.node))
        .collect();
    let read_chains: Vec<Vec<(NodeId, NodeId)>> = reads
        .iter()
        .map(|r| barrier_chain(cx, root, r.node))
        .collect();

    for (i, write) in writes.iter().enumerate() {
        // Earliest later read of the same name that's actually on a
        // control-flow path reachable from this write. A read inside
        // an exclusive branch (e.g. the `else` arm when the write is
        // in the `then`) can't observe the write, so it must not
        // suppress an overwrite-before-read flag.
        let next_read_pos = reads
            .iter()
            .enumerate()
            .filter(|(_, r)| r.name == write.name && r.pos > write.end)
            .filter(|(k, _)| chain_is_prefix(&read_chains[*k], &write_chains[i]))
            .map(|(_, r)| r.pos)
            .min();

        // Earliest later write of the same name whose chain is a
        // prefix of `write`'s — i.e. it is in the same chunk as `write`
        // or in a shallower one that `write` *must* fall through to.
        // Compare on `end` so the OpAsgn self-read at the same byte
        // position as the OpAsgn's start doesn't shadow its own write.
        let dominating_overwrite = writes
            .iter()
            .enumerate()
            .filter(|(j, w)| *j != i && w.name == write.name && w.end > write.end)
            .filter(|(j, _)| chain_is_prefix(&write_chains[*j], &write_chains[i]))
            .min_by_key(|(_, w)| w.end);

        match (next_read_pos, dominating_overwrite) {
            (None, _) => {
                // No later read of this name anywhere — classic useless
                // write (whether or not an overwrite follows).
                cx.emit_offense(write.range, write.message, None);
            }
            (Some(r), Some((_, w))) if w.end <= r => {
                // Dominating overwrite reaches before any read — this
                // write's value can never be observed.
                cx.emit_offense(write.range, write.message, None);
            }
            (Some(_), _) => {
                // A later read exists and no dominating overwrite
                // precedes it — the value is (potentially) used.
            }
        }
    }
}

/// Walk up from `node` via `cx.parent`, collecting `(barrier, child)`
/// pairs at every branch-introducing ancestor up to (but not including)
/// the scope `root`. `child` is the direct child of `barrier` on the
/// path from `node`, so two writes in different arms of the same `if`
/// produce distinct chains even though they share the `If` barrier.
/// Returned chain is outermost-first so prefix comparisons read
/// naturally ("less-nested" ↔ "shorter prefix").
fn barrier_chain(cx: &Cx<'_>, root: NodeId, node: NodeId) -> Vec<(NodeId, NodeId)> {
    let mut chain: Vec<(NodeId, NodeId)> = Vec::new();
    let mut current = node;
    while let Some(parent) = cx.parent(current).get() {
        if parent == root {
            break;
        }
        if is_branch_barrier(cx, parent) {
            chain.push((parent, current));
        }
        current = parent;
    }
    chain.reverse();
    chain
}

fn is_branch_barrier(cx: &Cx<'_>, node: NodeId) -> bool {
    // `Resbody` and `Ensure` are *not* barriers: their children
    // (var/body for resbody, body/ensure_ for ensure) run sequentially,
    // not exclusively. Branch exclusivity at the rescue level is
    // captured by the enclosing `Rescue` instead.
    matches!(
        *cx.kind(node),
        NodeKind::If { .. }
            | NodeKind::Case { .. }
            | NodeKind::When { .. }
            | NodeKind::While { .. }
            | NodeKind::Until { .. }
            | NodeKind::Rescue { .. }
    )
}

/// `short` is a prefix of `long` (outermost-first comparison). When
/// `short = chain(w')` and `long = chain(w)`, this returns true iff
/// `w'` is guaranteed to be reached after `w` — either same chunk
/// (chains equal) or a strictly shallower chunk that `w` falls back
/// out to.
fn chain_is_prefix(short: &[(NodeId, NodeId)], long: &[(NodeId, NodeId)]) -> bool {
    short.len() <= long.len() && short == &long[..short.len()]
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
                    node: target,
                });
                if !cx.symbol_str(name).starts_with('_') {
                    writes.push(Write {
                        name,
                        end: cx.range(id).end,
                        range: cx.range(id),
                        message: MSG_OPERATOR,
                        node: id,
                    });
                }
            }
        }
        NodeKind::Masgn { lhs, .. } => {
            let asgn_end = cx.range(id).end;
            collect_mlhs_targets(cx, lhs, asgn_end, writes);
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
                    node: var_id,
                });
            }
        }
        NodeKind::Lvar(name) => {
            reads.push(Read {
                name,
                pos: cx.range(id).start,
                node: id,
            });
        }
        _ => {}
    }
}

/// Walk an `Mlhs` (or anything that decomposes into target write nodes)
/// and push every `Lvasgn` target as a local-variable write. Nested
/// destructuring `a, (b, c) = …` produces nested `Mlhs`, so this
/// recurses through inner `Mlhs` nodes.
fn collect_mlhs_targets(cx: &Cx<'_>, lhs: NodeId, asgn_end: u32, writes: &mut Vec<Write>) {
    match *cx.kind(lhs) {
        NodeKind::Mlhs(list) => {
            for &target in cx.list(list) {
                collect_mlhs_targets(cx, target, asgn_end, writes);
            }
        }
        NodeKind::Lvasgn { name, .. } => {
            push_local_write(cx, lhs, name, asgn_end, writes);
        }
        _ => {
            // Other target kinds (`Ivasgn`, `Casgn`, …) are not local
            // variables, so this cop does not flag them.
        }
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
        node,
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
        expect_no_offenses, expect_offense, indoc, run_cop, run_cop_with_edits,
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
    fn masgn_nested_lhs_targets_are_inspected() {
        // `a, (b, c) = [1, [2, 3]]` — the inner `(b, c)` is its own
        // `Mlhs`. `b` is used, `a` and `c` are not. The `expect_offense!`
        // caret grammar can't annotate two offsets on one line, so check
        // via `run_cop` directly.
        let offenses = run_cop::<UselessAssignment>("a, (b, c) = [1, [2, 3]]\nb\n");
        let names: Vec<&str> = offenses
            .iter()
            .map(|o| {
                let r = o.range;
                &"a, (b, c) = [1, [2, 3]]\nb\n"[r.start as usize..r.end as usize]
            })
            .collect();
        assert_eq!(
            names,
            vec!["a", "c"],
            "expected only `a` and `c` flagged, got {names:?} (offenses={offenses:?})",
        );
        for offense in &offenses {
            assert_eq!(offense.message, "Useless assignment to local variable");
        }
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

    // murphy-xek: flow-sensitive dataflow — overwrite-before-read +
    // branch-aware barriers.

    #[test]
    fn overwrite_before_read_flags_the_overwritten_write() {
        // `x = 1` is overwritten by `x = 2` before any read.
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
                x = 1
                ^ Useless assignment to local variable
                x = 2
                x
            "#}
        );
    }

    #[test]
    fn overwrite_with_no_read_flags_both_writes() {
        let offenses =
            murphy_plugin_api::test_support::run_cop::<UselessAssignment>("x = 1\nx = 2\n");
        assert_eq!(
            offenses.len(),
            2,
            "expected both writes flagged, got {offenses:?}"
        );
    }

    #[test]
    fn conditional_overwrite_does_not_flag_outer_write() {
        // `x = 1` may survive — the `x = 2` is inside an `if`, so on
        // the `cond == false` path the final `x` reads `1`.
        expect_no_offenses!(
            UselessAssignment,
            indoc! {r#"
                x = 1
                if cond
                  x = 2
                end
                x
            "#}
        );
    }

    #[test]
    fn overwrite_inside_same_branch_still_flags() {
        // Both writes live inside the same `if` body — straight-line
        // within the branch, so the first is still overwritten.
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
                if cond
                  x = 1
                  ^ Useless assignment to local variable
                  x = 2
                  x
                end
            "#}
        );
    }

    #[test]
    fn overwrite_separated_by_use_does_not_flag() {
        // `x = 1; foo(x); x = 2` — the call reads `x`, so `x = 1` is
        // used.
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
                x = 1
                foo(x)
                x = 2
                ^ Useless assignment to local variable
            "#}
        );
    }

    #[test]
    fn shallower_overwrite_after_branch_dominates_outer_write() {
        // `x = 1` is overwritten by `x = 3` regardless of whether the
        // `if` body runs, because `x = 3` is in a shallower (and thus
        // unconditionally reached) chunk. The `x = 2` inside the `if`
        // is *also* useless for the same reason — `x = 3` dominates it
        // through the shallower prefix of its chain. Fix for PR #70
        // review job 1158 finding 1.
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
                x = 1
                ^ Useless assignment to local variable
                if cond
                  x = 2
                  ^ Useless assignment to local variable
                end
                x = 3
                x
            "#}
        );
    }

    #[test]
    fn exclusive_branches_of_an_if_are_not_overwrites_of_each_other() {
        // `x = 1` and `x = 2` live in different arms of the same `if`,
        // so neither overwrites the other at runtime — they are
        // mutually exclusive. The fix for PR #70 review job 1158
        // finding 2.
        expect_no_offenses!(
            UselessAssignment,
            indoc! {r#"
                if cond
                  x = 1
                else
                  x = 2
                end
                x
            "#}
        );
    }

    #[test]
    fn read_in_exclusive_branch_does_not_save_a_write_from_another_branch() {
        // `x = 1` is in the `then` arm; `puts x` is in the `else` arm.
        // They are mutually exclusive at runtime, so the `puts x` does
        // *not* observe `x = 1`. The dominating `x = 2` afterward
        // overwrites `x = 1` unconditionally → flag.
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
                if cond
                  x = 1
                  ^ Useless assignment to local variable
                else
                  puts x
                end
                x = 2
                x
            "#}
        );
    }

    #[test]
    fn op_asgn_overwriting_a_prior_write_does_not_flag_the_prior_write() {
        // Already covered by the murphy-8k4y tests, but pin again under
        // the dataflow framing: `x = 0; x += 1` — only the OpAsgn is
        // useless.
        expect_offense!(
            UselessAssignment,
            indoc! {r#"
                x = 0
                x += 1
                ^^^^^^ Useless operator-assignment to local variable
            "#}
        );
    }
}
