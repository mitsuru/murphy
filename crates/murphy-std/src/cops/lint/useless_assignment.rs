//! `Lint/UselessAssignment` — flags local-variable writes whose result is
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessAssignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-ev4p
//! notes: >
//!   Known gaps remain around VariableForce-equivalent coverage and RuboCop parity details.
//! ```
//!
//! never read, *or* whose result is overwritten by a sibling write before
//! any read can observe it.
//!
//! ## Matched shapes
//!
//! - Plain `Lvasgn` (`x = 1`) — name range, RuboCop-compatible message.
//! - `Masgn` (`a, b = 1, 2`) — each LHS target is treated as its own
//!   `Lvasgn`-style write, name range, with RuboCop's underscore guidance.
//! - `OpAsgn` / `OrAsgn` / `AndAsgn` (`x += 1`, `x ||= 1`, `x &&= 1`) —
//!   the variable-name range, with RuboCop's operator guidance.
//!   The value-less `Lvasgn` *target* inside `OpAsgn`/`OrAsgn`/`AndAsgn`
//!   also counts as a read of the variable, so a preceding `x = 0` is
//!   not flagged just because `x += 1` follows.
//! - `Resbody.var` (`rescue => e`) — `e`'s name range.
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
//! `While` / `Until`). `Resbody` / `Rescue` / `Ensure` are *not*
//! barriers — exception flow carries partial begin-body writes into
//! the rescue handler, so treating those as exclusive arms would
//! produce false positives. Each chain entry is a pair
//! `(barrier_node, branch_child)` — the child of the barrier on the
//! path from the node — so two writes in different arms of the same
//! `if` get distinct chain entries even though they share the `If`
//! barrier.
//!
//! Two checks use these chains:
//!
//! * **Dominating overwrite** (for `w'` to *always* overwrite `w`): `w'`'s
//!   chain must be a prefix (outermost-first) of `w`'s. Same chunk or
//!   shallower-than-`w` qualifies; sibling sequential `if`s do not.
//! * **Read observation** (for `r` to *possibly* observe `w`): no shared
//!   barrier disagrees on its arm. Sequential `if`s are compatible
//!   (both can run on `a && b`); sibling arms of the same `if` are
//!   not (mutually exclusive at runtime).
//!
//! ### Known v1 limitations
//!
//! * Two writes in different `resbody`s of the same `Rescue` are not
//!   recognised as mutually exclusive (we conservatively treat them as
//!   compatible). The cop will not flag a write in one resbody as
//!   "overwritten" by a write in a sibling resbody.
//! * Loops are treated as a single barrier — we don't unroll. A write
//!   inside a `while` body is in the loop's chunk; we don't reason about
//!   iteration counts.
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
//! The cop mirrors RuboCop's safe local rewrites for exposed AST shapes:
//! plain local assignments drop the `name =` prefix, multiple-assignment
//! targets are renamed to `_`, and local `op=` assignments drop the `=`.
//! `||=` / `&&=` and sequential assignments remain report-only because the
//! rewrite can change local-variable declaration semantics or produce
//! invalid Ruby.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, Symbol, cop};

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

const MSG_PREFIX: &str = "Useless assignment to variable";

struct Write {
    name: Symbol,
    /// One-past-the-last byte of the assignment. Reads are considered
    /// "after" this point.
    end: u32,
    /// Range to emit the offense on.
    range: Range,
    kind: WriteKind,
    /// Node id used to compute the control-flow barrier chain when
    /// deciding whether two writes are on the same path.
    node: NodeId,
}

#[derive(Clone, Copy)]
enum WriteKind {
    Local { value: OptNodeId },
    Multiple,
    Operator { op: &'static str, autocorrect: bool },
    Exception,
}

struct Read {
    name: Symbol,
    /// Byte position of the read.
    pos: u32,
    /// Node id used to compute the read's control-flow barrier chain so
    /// we can ask "is this read reachable from a particular write?"
    node: NodeId,
}

struct Candidate {
    name: Symbol,
}

fn analyze_scope(cx: &Cx<'_>, root: NodeId) {
    let mut writes: Vec<Write> = Vec::new();
    let mut reads: Vec<Read> = Vec::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    for id in scope_nodes(cx, root) {
        classify(cx, id, &mut writes, &mut reads, &mut candidates);
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
        // suppress an overwrite-before-read flag. The compatibility
        // test is *either direction* of prefix: a read at the same
        // level as the write or deeper-into-a-conditional (write's
        // chain is a prefix of read's) is reachable when that branch
        // runs; a read in a shallower chunk (read's chain is a prefix
        // of write's) is reachable after exiting w's branches.
        let next_read_pos = reads
            .iter()
            .enumerate()
            .filter(|(_, r)| r.name == write.name && r.pos > write.end)
            .filter(|(k, _)| paths_compatible(&read_chains[*k], &write_chains[i]))
            .map(|(_, r)| r.pos)
            .min();

        // Earliest later write of the same name whose chain is a
        // prefix of `write`'s — i.e. it is in the same chunk as `write`
        // or in a shallower one that `write` *must* fall through to.
        // Compare on `end` so the OpAsgn self-read at the same byte
        // position as the OpAsgn's start doesn't shadow its own write.
        //
        // Writes inside a `begin`/`rescue`/`ensure`-protected body are
        // not eligible dominators: an exception in the body can skip
        // the rest of it, so we can't guarantee the candidate actually
        // executes. This is conservative — purely-side-effect-free
        // writes between two assignments wouldn't really raise — but
        // it keeps us false-positive-free against exception flow.
        let dominating_overwrite = writes
            .iter()
            .enumerate()
            .filter(|(j, w)| *j != i && w.name == write.name && w.end > write.end)
            .filter(|(j, _)| chain_is_prefix(&write_chains[*j], &write_chains[i]))
            .filter(|(_, w)| !is_in_protected_begin_body(cx, root, w.node))
            .min_by_key(|(_, w)| w.end);

        match (next_read_pos, dominating_overwrite) {
            (None, _) => {
                // No later read of this name anywhere — classic useless
                // write (whether or not an overwrite follows).
                emit_useless_assignment(cx, write, &writes, &reads, &candidates);
            }
            (Some(r), Some((_, w))) if w.end <= r => {
                // Dominating overwrite reaches before any read — this
                // write's value can never be observed.
                emit_useless_assignment(cx, write, &writes, &reads, &candidates);
            }
            (Some(_), _) => {
                // A later read exists and no dominating overwrite
                // precedes it — the value is (potentially) used.
            }
        }
    }
}

fn emit_useless_assignment(
    cx: &Cx<'_>,
    write: &Write,
    writes: &[Write],
    reads: &[Read],
    candidates: &[Candidate],
) {
    let name = cx.symbol_str(write.name);
    let mut message = format!("{MSG_PREFIX} - `{name}`.");
    match write.kind {
        WriteKind::Multiple => {
            message.push_str(&format!(
                " Use `_` or `_{name}` as a variable name to indicate that it won't be used."
            ));
        }
        WriteKind::Operator { op, .. } => {
            message.push_str(&format!(" Use `{op}` instead of `{op}=`."));
        }
        WriteKind::Local { .. } | WriteKind::Exception => {
            if let Some(similar) = similar_name(cx, name, writes, reads, candidates) {
                message.push_str(&format!(" Did you mean `{similar}`?"));
            }
        }
    }

    cx.emit_offense(write.range, &message, None);
    emit_autocorrect(cx, write);
}

fn emit_autocorrect(cx: &Cx<'_>, write: &Write) {
    match write.kind {
        WriteKind::Local { value } => {
            let Some(value) = value.get() else {
                return;
            };
            if contains_assignment(cx, value) || is_part_of_sequential_assignment(cx, write.node) {
                return;
            }
            cx.emit_edit(
                Range {
                    start: write.range.start,
                    end: cx.range(value).start,
                },
                "",
            );
        }
        WriteKind::Multiple => {
            cx.emit_edit(write.range, "_");
        }
        WriteKind::Operator {
            autocorrect: true, ..
        } => {
            if let Some(eq) = operator_equals_range(cx, write) {
                cx.emit_edit(eq, "");
            }
        }
        WriteKind::Operator {
            autocorrect: false, ..
        }
        | WriteKind::Exception => {}
    }
}

fn is_part_of_sequential_assignment(cx: &Cx<'_>, node: NodeId) -> bool {
    let mut current = node;
    while let Some(parent) = cx.parent(current).get() {
        match *cx.kind(parent) {
            NodeKind::Lvasgn { value, .. } if value.get() == Some(current) => return true,
            NodeKind::Array(_) => {
                current = parent;
            }
            _ => return false,
        }
    }
    false
}

fn operator_equals_range(cx: &Cx<'_>, write: &Write) -> Option<Range> {
    let src = cx.source().as_bytes();
    let start = write.range.end as usize;
    let end = write.end as usize;
    let rel = src.get(start..end)?.iter().position(|b| *b == b'=')?;
    let pos = (start + rel) as u32;
    Some(Range {
        start: pos,
        end: pos + 1,
    })
}

fn contains_assignment(cx: &Cx<'_>, node: NodeId) -> bool {
    let mut stack = vec![node];
    while let Some(id) = stack.pop() {
        if matches!(
            *cx.kind(id),
            NodeKind::Lvasgn { .. }
                | NodeKind::Masgn { .. }
                | NodeKind::OpAsgn { .. }
                | NodeKind::OrAsgn { .. }
                | NodeKind::AndAsgn { .. }
        ) {
            return true;
        }
        stack.extend(cx.children(id));
    }
    false
}

fn similar_name<'a>(
    cx: &'a Cx<'_>,
    name: &str,
    writes: &'a [Write],
    reads: &'a [Read],
    candidates: &'a [Candidate],
) -> Option<&'a str> {
    let mut best: Option<(&str, usize)> = None;
    for candidate in writes
        .iter()
        .map(|w| cx.symbol_str(w.name))
        .chain(reads.iter().map(|r| cx.symbol_str(r.name)))
        .chain(candidates.iter().map(|c| cx.symbol_str(c.name)))
    {
        if candidate == name
            || candidate.starts_with('_')
            || candidate.contains(name)
            || name.contains(candidate)
        {
            continue;
        }
        let dist = levenshtein(name, candidate);
        if dist <= 2 && best.is_none_or(|(_, best_dist)| dist < best_dist) {
            best = Some((candidate, dist));
        }
    }
    best.map(|(candidate, _)| candidate)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0; b_chars.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b_chars.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_chars.len()]
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
    // `Rescue` / `Resbody` / `Ensure` are *not* barriers. A write in
    // the begin body can be observed by a read in the rescue handler
    // (exception flow carries the partially-executed body state into
    // the handler), so treating them as exclusive arms would produce
    // false positives — the cop would flag the begin write as never
    // observed even though the rescue read sees it. The known v1
    // limitation is that two writes in different resbodies of the
    // same Rescue are *not* recognised as mutually exclusive; that
    // sits in the doc-comment so users see it.
    matches!(
        *cx.kind(node),
        NodeKind::If { .. }
            | NodeKind::Case { .. }
            | NodeKind::When { .. }
            | NodeKind::While { .. }
            | NodeKind::Until { .. }
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

/// Whether `node` is inside the `body` arm of an enclosing `Rescue` or
/// `Ensure`. A write here can be interrupted by an exception, so we
/// can't claim it always executes — exclude it from dominating-overwrite
/// candidates.
fn is_in_protected_begin_body(cx: &Cx<'_>, root: NodeId, node: NodeId) -> bool {
    let mut current = node;
    while let Some(parent) = cx.parent(current).get() {
        if parent == root {
            return false;
        }
        let parent_kind = *cx.kind(parent);
        let body = match parent_kind {
            NodeKind::Rescue { body, .. } | NodeKind::Ensure { body, .. } => body,
            _ => {
                current = parent;
                continue;
            }
        };
        if body.get() == Some(current) {
            return true;
        }
        current = parent;
    }
    false
}

/// Two chains describe compatible control-flow paths iff no shared
/// branch barrier disagrees on which arm each chain is inside.
/// Sequential `if`s (different barriers) are compatible; sibling
/// arms of the *same* barrier are not.
fn paths_compatible(a: &[(NodeId, NodeId)], b: &[(NodeId, NodeId)]) -> bool {
    for (barrier_a, arm_a) in a {
        for (barrier_b, arm_b) in b {
            if barrier_a == barrier_b && arm_a != arm_b {
                return false;
            }
        }
    }
    true
}

fn classify(
    cx: &Cx<'_>,
    id: NodeId,
    writes: &mut Vec<Write>,
    reads: &mut Vec<Read>,
    candidates: &mut Vec<Candidate>,
) {
    match *cx.kind(id) {
        NodeKind::Lvasgn { name, value } if value.get().is_some() => {
            // Plain `x = 1` (and the per-target `Lvasgn` nodes inside an
            // `Mlhs`, since `translate_target` emits a value-less form
            // for those — they are picked up in the `Masgn` arm).
            push_local_write(cx, id, name, value, cx.range(id).end, writes);
        }
        NodeKind::OpAsgn { target, .. }
        | NodeKind::OrAsgn { target, .. }
        | NodeKind::AndAsgn { target, .. } => {
            // `x op= rhs` is semantically `x = x op rhs`: record the read
            // of `x` and a write whose offense range is the variable name.
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
                        range: cx.range(target),
                        kind: WriteKind::Operator {
                            op: operator_text(cx, id),
                            autocorrect: matches!(*cx.kind(id), NodeKind::OpAsgn { .. }),
                        },
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
                    kind: WriteKind::Exception,
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
        NodeKind::Send {
            receiver,
            method,
            args,
        } if receiver == OptNodeId::NONE && cx.list(args).is_empty() => {
            candidates.push(Candidate { name: method });
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
            push_multiple_write(cx, lhs, name, asgn_end, writes);
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
    value: OptNodeId,
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
        kind: WriteKind::Local { value },
        node,
    });
}

fn push_multiple_write(
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
        kind: WriteKind::Multiple,
        node,
    });
}

fn operator_text(cx: &Cx<'_>, node: NodeId) -> &'static str {
    match *cx.kind(node) {
        NodeKind::OpAsgn { op, .. } => match cx.symbol_str(op) {
            "+" => "+",
            "-" => "-",
            "*" => "*",
            "/" => "/",
            "%" => "%",
            "**" => "**",
            "&" => "&",
            "|" => "|",
            "^" => "^",
            "<<" => "<<",
            ">>" => ">>",
            _ => "operator",
        },
        NodeKind::OrAsgn { .. } => "||",
        NodeKind::AndAsgn { .. } => "&&",
        _ => "operator",
    }
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
    use murphy_plugin_api::test_support::{indoc, run_cop, run_cop_with_edits, test};

    #[test]
    fn flags_assignments_that_are_never_read() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            used = 1
            unused = 2
            ^^^^^^ Useless assignment to variable - `unused`.
            used
        "#});
    }

    #[test]
    fn ignores_underscore_assignments() {
        test::<UselessAssignment>().expect_no_offenses("名前 = 1\n名前\n_unused = 2\n");
    }

    #[test]
    fn autocorrects_plain_assignment_by_removing_lhs() {
        test::<UselessAssignment>().expect_correction(
            indoc! {r#"
                unused = 1
                ^^^^^^ Useless assignment to variable - `unused`.
            "#},
            "1\n",
        );
    }

    #[test]
    fn autocorrects_assignment_inside_call_arguments() {
        test::<UselessAssignment>().expect_correction(
            indoc! {r#"
                some_method(unused = 1) do
                            ^^^^^^ Useless assignment to variable - `unused`.
                end
            "#},
            "some_method(1) do\nend\n",
        );
    }

    #[test]
    fn skips_autocorrect_for_sequential_assignment() {
        let run = run_cop_with_edits::<UselessAssignment>("foo = 1, bar = 2\n");
        assert_eq!(run.edits.len(), 0);
    }

    #[test]
    fn suggests_similar_variable_like_names() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            environment = nil
            enviromnent = {}
            ^^^^^^^^^^^ Useless assignment to variable - `enviromnent`. Did you mean `environment`?
            puts environment
        "#});
    }

    #[test]
    fn suggests_similar_bare_method_names() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            enviromnent = {}
            ^^^^^^^^^^^ Useless assignment to variable - `enviromnent`. Did you mean `environment`?
            another_symbol
            puts environment
        "#});
    }

    #[test]
    fn nested_method_read_does_not_satisfy_outer_assignment() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            outer = 1
            ^^^^^ Useless assignment to variable - `outer`.
            def inner
              outer
            end
        "#});
    }

    #[test]
    fn earlier_read_does_not_satisfy_later_assignment() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            x = 0
            x
            x = 1
            ^ Useless assignment to variable - `x`.
        "#});
    }

    // murphy-8k4y: cover Masgn / OpAsgn / OrAsgn / AndAsgn / Resbody shapes.

    #[test]
    fn masgn_flags_only_unused_targets() {
        test::<UselessAssignment>().expect_correction(
            indoc! {r#"
                a, b = 1, 2
                ^ Useless assignment to variable - `a`. Use `_` or `_a` as a variable name to indicate that it won't be used.
                b
            "#},
            "_, b = 1, 2\nb\n",
        );
    }

    #[test]
    fn masgn_nested_lhs_targets_are_inspected() {
        // `a, (b, c) = [1, [2, 3]]` — the inner `(b, c)` is its own
        // `Mlhs`. `b` is used, `a` and `c` are not. The builder-based
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
            assert!(
                offense
                    .message
                    .starts_with("Useless assignment to variable - `"),
                "unexpected message: {}",
                offense.message
            );
        }
    }

    #[test]
    fn masgn_with_all_used_targets_is_not_flagged() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                a, b = 1, 2
                a + b
            "#});
    }

    #[test]
    fn op_asgn_flags_only_the_op_asgn_when_result_unused() {
        // `x = 0` is read by `x += 1` (the operator-assignment implicitly
        // reads `x`); only the `x += 1` write is useless.
        test::<UselessAssignment>().expect_correction(
            indoc! {r#"
                x = 0
                x += 1
                ^ Useless assignment to variable - `x`. Use `+` instead of `+=`.
            "#},
            "x = 0\nx + 1\n",
        );
    }

    #[test]
    fn or_asgn_uses_operator_message() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            x = 0
            x ||= 1
            ^ Useless assignment to variable - `x`. Use `||` instead of `||=`.
        "#});
        let run = run_cop_with_edits::<UselessAssignment>("x = 0\nx ||= 1\n");
        assert_eq!(run.edits.len(), 0);
    }

    #[test]
    fn and_asgn_uses_operator_message() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            x = 0
            x &&= 1
            ^ Useless assignment to variable - `x`. Use `&&` instead of `&&=`.
        "#});
    }

    #[test]
    fn op_asgn_target_counts_as_read_for_prior_assignment() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                x = 0
                x += 1
                x
            "#});
    }

    #[test]
    fn rescue_var_flags_when_unused() {
        test::<UselessAssignment>().expect_offense(indoc! {r#"
            begin
              raise
            rescue => e
                      ^ Useless assignment to variable - `e`.
              :rescued
            end
        "#});
    }

    #[test]
    fn rescue_var_used_is_not_flagged() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                begin
                  raise
                rescue => e
                  e.message
                end
            "#});
    }

    #[test]
    fn rescue_var_underscore_is_silenced() {
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                begin
                  raise
                rescue => _err
                  :rescued
                end
            "#});
    }

    // murphy-xek: flow-sensitive dataflow — overwrite-before-read +
    // branch-aware barriers.

    #[test]
    fn overwrite_before_read_flags_the_overwritten_write() {
        // `x = 1` is overwritten by `x = 2` before any read.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                x = 1
                ^ Useless assignment to variable - `x`.
                x = 2
                x
            "#});
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
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                x = 1
                if cond
                  x = 2
                end
                x
            "#});
    }

    #[test]
    fn overwrite_inside_same_branch_still_flags() {
        // Both writes live inside the same `if` body — straight-line
        // within the branch, so the first is still overwritten.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                if cond
                  x = 1
                  ^ Useless assignment to variable - `x`.
                  x = 2
                  x
                end
            "#});
    }

    #[test]
    fn overwrite_separated_by_use_does_not_flag() {
        // `x = 1; foo(x); x = 2` — the call reads `x`, so `x = 1` is
        // used.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                x = 1
                foo(x)
                x = 2
                ^ Useless assignment to variable - `x`.
            "#});
    }

    #[test]
    fn shallower_overwrite_after_branch_dominates_outer_write() {
        // `x = 1` is overwritten by `x = 3` regardless of whether the
        // `if` body runs, because `x = 3` is in a shallower (and thus
        // unconditionally reached) chunk. The `x = 2` inside the `if`
        // is *also* useless for the same reason — `x = 3` dominates it
        // through the shallower prefix of its chain. Fix for PR #70
        // review job 1158 finding 1.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                x = 1
                ^ Useless assignment to variable - `x`.
                if cond
                  x = 2
                  ^ Useless assignment to variable - `x`.
                end
                x = 3
                x
            "#});
    }

    #[test]
    fn exclusive_branches_of_an_if_are_not_overwrites_of_each_other() {
        // `x = 1` and `x = 2` live in different arms of the same `if`,
        // so neither overwrites the other at runtime — they are
        // mutually exclusive. The fix for PR #70 review job 1158
        // finding 2.
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                if cond
                  x = 1
                else
                  x = 2
                end
                x
            "#});
    }

    #[test]
    fn read_in_a_sibling_conditional_observes_the_outer_write() {
        // `x = 1` is set when `a` is true; `puts x` runs when `b` is
        // true. The path `a && b` actually observes `x = 1`. The cop
        // must not flag `x = 1` as useless. (PR #70 review job 1163
        // finding 1.)
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                if a
                  x = 1
                end
                if b
                  puts x
                end
            "#});
    }

    #[test]
    fn begin_body_write_interrupted_by_exception_is_observed_by_rescue() {
        // `may_raise` can throw between `x = 1` and `x = 2`. On the
        // exception path, `rescue` reads x = 1 — the value survives.
        // The cop must not flag x = 1 as overwritten by x = 2.
        // (PR #70 review job 1165.)
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                begin
                  x = 1
                  may_raise
                  x = 2
                rescue
                  puts x
                end
            "#});
    }

    #[test]
    fn rescue_handler_read_observes_begin_body_write() {
        // Exception flow carries the partial begin-body write into
        // the rescue handler — `puts x` can see `x = 1`. (PR #70
        // review job 1163 finding 2.)
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                begin
                  x = 1
                  raise
                rescue
                  puts x
                end
            "#});
    }

    #[test]
    fn read_inside_a_later_conditional_observes_the_outer_write() {
        // The `puts x` runs when `cond` is true — that path observes
        // the outer `x = 1`, so the write must not be flagged.
        // (PR #70 review job 1162.)
        test::<UselessAssignment>().expect_no_offenses(indoc! {r#"
                x = 1
                if cond
                  puts x
                end
            "#});
    }

    #[test]
    fn read_in_exclusive_branch_does_not_save_a_write_from_another_branch() {
        // `x = 1` is in the `then` arm; `puts x` is in the `else` arm.
        // They are mutually exclusive at runtime, so the `puts x` does
        // *not* observe `x = 1`. The dominating `x = 2` afterward
        // overwrites `x = 1` unconditionally → flag.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                if cond
                  x = 1
                  ^ Useless assignment to variable - `x`.
                else
                  puts x
                end
                x = 2
                x
            "#});
    }

    #[test]
    fn op_asgn_overwriting_a_prior_write_does_not_flag_the_prior_write() {
        // Already covered by the murphy-8k4y tests, but pin again under
        // the dataflow framing: `x = 0; x += 1` — only the OpAsgn is
        // useless.
        test::<UselessAssignment>().expect_offense(indoc! {r#"
                x = 0
                x += 1
                ^ Useless assignment to variable - `x`. Use `+` instead of `+=`.
            "#});
    }
}
