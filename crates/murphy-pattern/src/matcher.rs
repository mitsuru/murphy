//! Runtime pattern matcher (C backend, murphy-9cr.19).
//!
//! Walks a [`PatternIr`] over an arena [`Ast`] and reports either a
//! successful match (with slot-indexed [`Captures`]) or no match. The
//! semantics MUST agree with the B-backend `node_pattern!` proc macro;
//! cross-backend conformance is guarded by `tests/conformance.rs`.
//!
//! The interpreter is recursive over the IR. Variable-length child lists
//! and `Rest` are handled in [`match_node_match`] / [`match_list_slot`].
//! Captures are written into a [`CaptureBuf`]; alternatives (`Union`,
//! negation, descend) clone the buffer so a failed arm leaves no trace.

use murphy_ast::{Ast, NodeId, NodeKind};

use crate::CaptureKind;
use crate::captures::{CaptureBuf, CaptureValue, Captures};
use crate::ir::{IrHead, IrNode, IrNodeId, PatternIr, StrRef};
use crate::schema::{PatChild, pattern_children};

/// Hook used by the matcher to evaluate `#predicate` calls.
///
/// The C backend defers `#predicate` resolution to the embedder — typically
/// the mruby bridge (murphy-9cr.24), which looks `name` up as a Ruby method
/// on the cop instance. [`NoPredicates`] is the trivial default ("every
/// predicate fails"), suitable for tests and standalone use.
pub trait PredicateHost {
    /// Look up the predicate by `name` and evaluate it on `node`. Returns
    /// `true` if the predicate accepts the node.
    fn call(&mut self, name: &str, node: NodeId) -> bool;
}

/// Predicate host that fails every predicate — useful for tests and any
/// caller that has not wired up a real predicate registry yet.
pub struct NoPredicates;

impl PredicateHost for NoPredicates {
    fn call(&mut self, _name: &str, _node: NodeId) -> bool {
        false
    }
}

/// Match `ir` against `node` in `ast`. Returns `Some(captures)` on a
/// successful match (with one [`CaptureValue`] per `$` capture, slot-indexed)
/// or `None` if the pattern does not match.
///
/// `predicates` is invoked for every `#name` predicate node reached during
/// the walk. Pass [`NoPredicates`] if the pattern has none.
pub fn matches<P: PredicateHost + ?Sized>(
    ir: &PatternIr,
    ast: &Ast,
    node: NodeId,
    predicates: &mut P,
) -> Option<Captures> {
    let mut buf = CaptureBuf::new(ir.captures.len());
    let ctx = MatcherCtx { ir, ast };
    if match_pat(&ctx, ir.root, node, &mut buf, predicates) {
        // `finish` may return `None` only if a capture slot was left
        // unwritten — the parser rejects every IR shape that can do
        // that, so on a normal `compile()`-produced IR this `?` is
        // never taken. It is the defense-in-depth fallback documented
        // on `CaptureBuf::finish`.
        buf.finish()
    } else {
        None
    }
}

/// Borrowed bundle threaded through the recursion. Splitting the IR/AST
/// borrows from the mutable `CaptureBuf` (which goes by `&mut` parameter)
/// keeps borrow-checking simple.
struct MatcherCtx<'a> {
    ir: &'a PatternIr,
    ast: &'a Ast,
}

impl<'a> MatcherCtx<'a> {
    fn ir_node(&self, id: IrNodeId) -> &'a IrNode {
        &self.ir.nodes[id.0 as usize]
    }

    fn pool(&self, r: StrRef) -> &'a str {
        &self.ir.str_pool[r.start as usize..(r.start + r.len) as usize]
    }
}

/// Resolve a node's structural pattern children against the matcher's AST.
fn schema_children<'a>(ctx: &MatcherCtx<'a>, node: NodeId) -> Option<Vec<PatChild<'a>>> {
    pattern_children(ctx.ast.kind(node), ctx.ast.raw_parts().node_lists)
}

/// Recursive match: pattern `pat_id` against arena `node`. Writes any
/// captures the pattern carries into `buf`; on a `false` return the buffer
/// may have been partially written (the caller is responsible for cloning
/// before exploring alternatives whose failures must not leak).
fn match_pat<P: PredicateHost + ?Sized>(
    ctx: &MatcherCtx,
    pat_id: IrNodeId,
    node: NodeId,
    buf: &mut CaptureBuf,
    predicates: &mut P,
) -> bool {
    match ctx.ir_node(pat_id) {
        IrNode::Wildcard => true,

        // `Rest` at top level is a parser-enforced error; reaching it here
        // means a caller bypassed `parse` or built an IR by hand.
        IrNode::Rest => false,

        // At top level (not on an `OptNode` slot) `nil?` matches only an
        // actual `Nil` node. The OptNode-slot "absent slot matches too"
        // case is handled in `match_fixed_slot`.
        IrNode::NilTest => matches!(*ctx.ast.kind(node), NodeKind::Nil),

        // Literal-vs-atom matches.
        IrNode::LitInt(v) => matches!(*ctx.ast.kind(node), NodeKind::Int(actual) if actual == *v),
        IrNode::LitFloat(v) => {
            matches!(*ctx.ast.kind(node), NodeKind::Float(actual) if actual == *v)
        }
        IrNode::LitStr(r) => match *ctx.ast.kind(node) {
            NodeKind::Str(sid) => ctx.ast.interner().resolve(sid.0) == ctx.pool(*r),
            _ => false,
        },
        IrNode::LitSym(r) => match *ctx.ast.kind(node) {
            NodeKind::Sym(s) => ctx.ast.interner().resolve(s.0) == ctx.pool(*r),
            _ => false,
        },
        IrNode::LitTrue => matches!(*ctx.ast.kind(node), NodeKind::True_),
        IrNode::LitFalse => matches!(*ctx.ast.kind(node), NodeKind::False_),
        IrNode::LitNil => matches!(*ctx.ast.kind(node), NodeKind::Nil),

        IrNode::Predicate(r) => predicates.call(ctx.pool(*r), node),

        IrNode::Kind(tag) => ctx.ast.kind(node).tag() == *tag,

        IrNode::Node { head, children } => {
            let pattern_kids: Vec<IrNodeId> = ctx.ir.children
                [children.start as usize..(children.start + children.len) as usize]
                .to_vec();
            match_node_match(ctx, *head, &pattern_kids, node, buf, predicates)
        }

        IrNode::Union(arms) => {
            // First arm to succeed wins. Each arm tries against a CLONED
            // buffer so a failed arm's partial writes do not leak; on
            // success the clone becomes the live buffer.
            let arm_ids: Vec<IrNodeId> =
                ctx.ir.children[arms.start as usize..(arms.start + arms.len) as usize].to_vec();
            for arm in arm_ids {
                let mut trial = buf.clone();
                if match_pat(ctx, arm, node, &mut trial, predicates) {
                    *buf = trial;
                    return true;
                }
            }
            false
        }

        IrNode::Not(body) => {
            // `!x` succeeds iff `x` fails. Captures inside `Not` are
            // structurally forbidden by the B backend (`lower_bool` route);
            // the C backend tolerates them at runtime by discarding the
            // trial buffer either way — the live `buf` is never touched.
            let mut trial = buf.clone();
            !match_pat(ctx, *body, node, &mut trial, predicates)
        }

        IrNode::Capture { slot, body } => {
            // Match the body first: a failing body must NOT register the
            // capture. The captured value is always the subject node id
            // for a `Node` slot; for a `Seq` slot the `$...` form goes
            // through `match_list_slot` and never reaches here.
            if !match_pat(ctx, *body, node, buf, predicates) {
                return false;
            }
            debug_assert_eq!(
                ctx.ir.captures[*slot as usize].kind,
                CaptureKind::Node,
                "Capture node reached for non-Node capture slot"
            );
            buf.set(*slot, CaptureValue::Node(node));
            true
        }

        IrNode::Parent(body) => match ctx.ast.parent(node).get() {
            None => false,
            Some(p) => match_pat(ctx, *body, p, buf, predicates),
        },

        IrNode::Descend(body) => {
            // ` `x ` matches iff any DFS descendant matches `body`. Like
            // `Not`, captures inside are structurally forbidden in B; the
            // C backend discards them by routing through a trial buffer.
            // A `false` return from any descendant trial is just "not this
            // one"; only one success is needed.
            for d in ctx.ast.descendants(node) {
                let mut trial = buf.clone();
                if match_pat(ctx, *body, d, &mut trial, predicates) {
                    return true;
                }
            }
            false
        }
        IrNode::Quantifier { .. } => {
            // Quantifiers are sequence operators, consumed by
            // `match_list_slot`. Reaching one as a scalar pattern means an
            // invalid hand-built IR or an unsupported fixed-slot shape.
            // Debug builds trip so layout drift is caught early; release
            // builds fall through to a silent miss to preserve the
            // historical no-panic contract for hand-built IR.
            debug_assert!(
                false,
                "quantifier IR reached scalar slot; only list slots dispatch quantifiers",
            );
            false
        }
        IrNode::AnyOrder { .. } => {
            // AnyOrder is a sequence operator; it is consumed by
            // `match_list_from`. Reaching it as a scalar pattern means
            // hand-built IR was used incorrectly.
            debug_assert!(false, "AnyOrder IR reached scalar slot");
            false
        }
    }
}

/// Match a `(head child...)` pattern against `node`.
fn match_node_match<P: PredicateHost + ?Sized>(
    ctx: &MatcherCtx,
    head: IrHead,
    pattern_kids: &[IrNodeId],
    node: NodeId,
    buf: &mut CaptureBuf,
    predicates: &mut P,
) -> bool {
    let actual_tag = ctx.ast.kind(node).tag();

    // Head: kind / tag-set check. `Any` / `OneOf` accept arbitrary kinds.
    match head {
        IrHead::Exact(t) => {
            if actual_tag != t {
                return false;
            }
        }
        IrHead::Any => {}
        IrHead::OneOf(s) => {
            let tags = &ctx.ir.tags[s.start as usize..(s.start + s.len) as usize];
            if !tags.contains(&actual_tag) {
                return false;
            }
        }
    }

    // `Any` / `OneOf` are kind-only: B backend accepts only an empty child
    // list or a single bare `...`. Either way the body is "any structure" —
    // succeed without dispatching children onto a slot schema.
    if matches!(head, IrHead::Any | IrHead::OneOf(_)) {
        return match pattern_kids {
            [] => true,
            [only] => matches!(ctx.ir_node(*only), IrNode::Rest),
            _ => false,
        };
    }

    // `Exact`: dispatch pattern children onto the kind's structural slots.
    let Some(slots) = schema_children(ctx, node) else {
        // The kind has no v1 pattern schema (e.g. `Error`, `Unknown`, or
        // an intentionally-unsupported variant). `(<kind> ...)` cannot
        // match such nodes.
        return false;
    };

    // Slot taxonomy: fixed = non-List, list_idx = index of the trailing
    // `List` slot (at most one, always last per v1 convention).
    let list_idx = slots.iter().position(|s| matches!(s, PatChild::List(_)));
    let fixed_count = list_idx.unwrap_or(slots.len());
    let has_list = list_idx.is_some();
    debug_assert!(
        list_idx.is_none_or(|i| i == slots.len() - 1),
        "schema invariant: the trailing List slot must be the last slot"
    );

    // Pattern-child count rules. With no `List` slot the counts must match
    // exactly; with one, the fixed slots take the first `fixed_count` and
    // any trailing pattern children flow into the `List` slot.
    if !has_list {
        if pattern_kids.len() != fixed_count {
            return false;
        }
    } else if pattern_kids.len() < fixed_count {
        return false;
    }

    // Fixed slots: positional match, child-by-slot.
    for (i, slot) in slots.iter().take(fixed_count).enumerate() {
        if !match_fixed_slot(ctx, *slot, pattern_kids[i], buf, predicates) {
            return false;
        }
    }

    // Trailing list slot (if any): the remaining pattern children match the
    // node's list elements, with at most one rest-like element in the
    // pattern (parser-guaranteed).
    if let Some(li) = list_idx {
        let PatChild::List(elems) = slots[li] else {
            unreachable!("list_idx points at a List slot")
        };
        let list_pat = &pattern_kids[fixed_count..];
        if !match_list_slot(ctx, list_pat, elems, buf, predicates) {
            return false;
        }
    }

    true
}

/// Match one pattern child against one fixed (non-List) slot value.
fn match_fixed_slot<P: PredicateHost + ?Sized>(
    ctx: &MatcherCtx,
    slot: PatChild<'_>,
    pat_id: IrNodeId,
    buf: &mut CaptureBuf,
    predicates: &mut P,
) -> bool {
    match slot {
        PatChild::Node(n) => match_pat(ctx, pat_id, n, buf, predicates),

        PatChild::OptNode(opt) => match (opt, ctx.ir_node(pat_id)) {
            // `nil?` on an `OptNode` slot is the ONLY place that "absent
            // slot" succeeds; elsewhere `nil?` requires an actual `Nil`
            // node. `Some(n)` falls through to the literal `Nil` check.
            (None, IrNode::NilTest) => true,
            (Some(n), IrNode::NilTest) => matches!(*ctx.ast.kind(n), NodeKind::Nil),
            // Any other pattern on an absent slot is a mismatch — the slot
            // has no node id to recurse into.
            (None, _) => false,
            (Some(n), _) => match_pat(ctx, pat_id, n, buf, predicates),
        },

        // Symbol slots accept `_`, a `:sym` literal, or a `{:a :b ...}`
        // union whose arms are all `:sym` literals (murphy-rs7) — same
        // surface as the B backend's `SlotTy::Sym`. Anything else is a
        // structural mismatch (no capture or recursion). The B backend
        // rejects non-sym union arms at compile time; the C backend
        // tolerates them at runtime by simply failing every comparison
        // (a defensive non-LitSym arm matches nothing).
        PatChild::Sym(actual_sym) => match ctx.ir_node(pat_id) {
            IrNode::Wildcard => true,
            IrNode::LitSym(r) => ctx.ast.interner().resolve(actual_sym.0) == ctx.pool(*r),
            IrNode::Union(arms) => {
                let actual = ctx.ast.interner().resolve(actual_sym.0);
                let arm_ids =
                    &ctx.ir.children[arms.start as usize..(arms.start + arms.len) as usize];
                arm_ids.iter().any(|id| match ctx.ir_node(*id) {
                    IrNode::LitSym(r) => ctx.pool(*r) == actual,
                    _ => false,
                })
            }
            _ => false,
        },

        // The matcher dispatches the `List` slot through `match_list_slot`;
        // it should never reach `match_fixed_slot`.
        PatChild::List(_) => unreachable!("List slot routed through match_list_slot"),
    }
}

/// Match the trailing `List` slot's pattern children against the node-list
/// elements. Handles bare rest, seq captures, and murphy-ycx postfix
/// quantifiers with greedy backtracking so a variable-length element can
/// give nodes back to a suffix pattern.
fn match_list_slot<P: PredicateHost + ?Sized>(
    ctx: &MatcherCtx,
    pattern_kids: &[IrNodeId],
    elems: &[NodeId],
    buf: &mut CaptureBuf,
    predicates: &mut P,
) -> bool {
    let mut trial = buf.clone();
    if match_list_from(ctx, pattern_kids, elems, &mut trial, predicates) {
        *buf = trial;
        true
    } else {
        false
    }
}

fn match_list_from<P: PredicateHost + ?Sized>(
    ctx: &MatcherCtx,
    pattern_kids: &[IrNodeId],
    elems: &[NodeId],
    buf: &mut CaptureBuf,
    predicates: &mut P,
) -> bool {
    let Some((&pat, rest)) = pattern_kids.split_first() else {
        return elems.is_empty();
    };

    if let Some(slot) = rest_kind(ctx, pat) {
        for count in (0..=elems.len()).rev() {
            let mut trial = buf.clone();
            if let Some(slot) = slot {
                trial.set(slot, CaptureValue::Seq(elems[..count].to_vec()));
            }
            if match_list_from(ctx, rest, &elems[count..], &mut trial, predicates) {
                *buf = trial;
                return true;
            }
        }
        return false;
    }

    if let Some(repeat) = repeat_kind(ctx, pat) {
        let max = repeat.max.unwrap_or(elems.len()).min(elems.len());
        let states = repeat_states(ctx, repeat.body, &elems[..max], buf, predicates);
        let upper = states.len() - 1;
        if upper < repeat.min {
            return false;
        }

        for count in (repeat.min..=upper).rev() {
            let mut trial = states[count].clone();
            if let Some(slot) = repeat.capture_slot {
                let value = if repeat.is_optional {
                    // `?` arity: count is always 0 or 1 because min=0, max=1.
                    CaptureValue::OptNode(if count == 1 { Some(elems[0]) } else { None })
                } else {
                    CaptureValue::Seq(elems[..count].to_vec())
                };
                trial.set(slot, value);
            }
            if match_list_from(ctx, rest, &elems[count..], &mut trial, predicates) {
                *buf = trial;
                return true;
            }
        }
        return false;
    }

    // AnyOrder: try to match the `<...>` block against a prefix of `elems`,
    // then continue with the suffix.
    if let IrNode::AnyOrder { children } = ctx.ir_node(pat) {
        let child_ids: Vec<IrNodeId> = ctx.ir.children
            [children.start as usize..(children.start + children.len) as usize]
            .to_vec();
        // Try every possible count of elements that the AnyOrder block could
        // consume.  The block must consume at least as many elements as there
        // are non-rest children; if there is a rest, it can consume more.
        let has_rest = child_ids.iter().any(|id| rest_kind(ctx, *id).is_some());
        let non_rest_ids: Vec<IrNodeId> = child_ids
            .iter()
            .copied()
            .filter(|id| rest_kind(ctx, *id).is_none())
            .collect();
        let min_consume = non_rest_ids.len();
        let max_consume = if has_rest { elems.len() } else { min_consume };

        for consume in min_consume..=max_consume {
            let (block, suffix) = elems.split_at(consume);
            let mut trial = buf.clone();
            if match_anyorder(ctx, &non_rest_ids, block, has_rest, &mut trial, predicates)
                && match_list_from(ctx, rest, suffix, &mut trial, predicates)
            {
                *buf = trial;
                return true;
            }
        }
        return false;
    }

    let Some((&actual, remaining)) = elems.split_first() else {
        return false;
    };
    let mut trial = buf.clone();
    if match_pat(ctx, pat, actual, &mut trial, predicates)
        && match_list_from(ctx, rest, remaining, &mut trial, predicates)
    {
        *buf = trial;
        true
    } else {
        false
    }
}

#[derive(Debug, Clone, Copy)]
struct RepeatPat {
    body: IrNodeId,
    min: usize,
    max: Option<usize>,
    capture_slot: Option<u16>,
    /// `?` arity (`min == 0`, `max == 1`). When the repeat is captured,
    /// `is_optional` selects `OptNode` over `Seq` so the caller sees a
    /// single-or-missing node rather than a 0-or-1-element sequence.
    is_optional: bool,
}

/// Classify a list-slot child as a postfix quantifier (`*` / `+` / `?`) or
/// a `$`-captured one. The parser only emits `Quantifier` directly or
/// wrapped in a single `Capture`; nested shapes (`Capture` inside
/// `Quantifier`, double-`Capture`, etc.) are deliberately unsupported and
/// fall through to `None`.
fn repeat_kind(ctx: &MatcherCtx, pat: IrNodeId) -> Option<RepeatPat> {
    let (capture_slot, q) = match ctx.ir_node(pat) {
        IrNode::Quantifier { .. } => (None, pat),
        IrNode::Capture { slot, body } => match ctx.ir_node(*body) {
            IrNode::Quantifier { .. } => (Some(*slot), *body),
            _ => return None,
        },
        _ => return None,
    };
    let IrNode::Quantifier { body, min, max } = ctx.ir_node(q) else {
        unreachable!("`q` was just classified as `Quantifier`");
    };
    let min = *min as usize;
    let max = (*max != u8::MAX).then_some(*max as usize);
    Some(RepeatPat {
        body: *body,
        min,
        max,
        capture_slot,
        is_optional: min == 0 && max == Some(1),
    })
}

fn repeat_states<P: PredicateHost + ?Sized>(
    ctx: &MatcherCtx,
    body: IrNodeId,
    elems: &[NodeId],
    buf: &CaptureBuf,
    predicates: &mut P,
) -> Vec<CaptureBuf> {
    let mut states = vec![buf.clone()];
    for elem in elems {
        let mut next = states.last().expect("seed state").clone();
        if !match_pat(ctx, body, *elem, &mut next, predicates) {
            break;
        }
        states.push(next);
    }
    states
}

/// Classify a `List`-slot pattern child as rest-like and, if so, report the
/// `$...` capture slot it binds.
///
/// Returns:
/// - `None` — not rest-like; matches one element by position.
/// - `Some(None)` — bare `...`, binds nothing.
/// - `Some(Some(slot))` — `$...`, binds the rest span to capture `slot`.
fn rest_kind(ctx: &MatcherCtx, pat: IrNodeId) -> Option<Option<u16>> {
    match ctx.ir_node(pat) {
        IrNode::Rest => Some(None),
        IrNode::Capture { slot, body } if matches!(ctx.ir_node(*body), IrNode::Rest) => {
            Some(Some(*slot))
        }
        _ => None,
    }
}

/// Match an `<...>` any-order block against `elems`.
///
/// `patterns` contains the non-rest children of the AnyOrder node (in
/// declaration order). `elems` is the slice of input elements that the block
/// is to consume (may be larger than `patterns.len()` when `has_rest` is
/// true). `has_rest` indicates whether the AnyOrder node has a trailing `...`.
///
/// Algorithm — backtracking with used-element bitmask (O(N! / elided) in the
/// worst case, but the parser enforces N ≤ 10 and typical patterns are tiny):
///
/// Walk patterns in declaration order. For each pattern try every input
/// element not yet used; on a match recurse into the next pattern. Captures
/// are written to `buf` in declaration order (the first-found permutation).
///
/// Two passes are used so captures are committed only on a full match:
///   Phase 1 (probe, no writes)  — find a valid assignment without touching `buf`.
///   Phase 2 (commit, with writes) — replay the found assignment, writing captures.
///
/// If `has_rest` is true the elements not assigned to any pattern are accepted
/// (they are the "rest"); otherwise the total consumed element count must equal
/// `patterns.len()`.
fn match_anyorder<P: PredicateHost + ?Sized>(
    ctx: &MatcherCtx,
    patterns: &[IrNodeId],
    elems: &[NodeId],
    has_rest: bool,
    buf: &mut CaptureBuf,
    predicates: &mut P,
) -> bool {
    let n = patterns.len();
    debug_assert!(n <= 10, "v1 limit: at most 10 non-rest children in <...>");

    if elems.len() < n {
        return false;
    }
    if !has_rest && elems.len() != n {
        return false;
    }

    // Phase 1: find a valid assignment without writing captures.
    // `assignment[pat_idx]` = chosen elem index; `u8::MAX` = unassigned.
    let mut assignment = [u8::MAX; 10];
    let mut used = 0u64; // bitmask of used element indices

    if !find_assignment(
        ctx,
        patterns,
        elems,
        0,
        &mut assignment,
        &mut used,
        buf,
        predicates,
    ) {
        return false;
    }

    // Phase 2: replay in declaration order, writing captures into `buf`.
    for (pat_idx, elem_idx) in assignment[..n].iter().enumerate() {
        let elem_idx = *elem_idx as usize;
        if !match_pat(ctx, patterns[pat_idx], elems[elem_idx], buf, predicates) {
            // Phase-1 confirmed this matches; failure here is a defensive guard.
            return false;
        }
    }

    true
}

/// Recursive helper for the phase-1 permutation search.
///
/// `pat_idx` is the index of the next pattern to assign (0 = first).
/// `assignment` accumulates the chosen element index per pattern slot.
/// `used` is a bitmask of already-assigned element indices.
///
/// Returns `true` as soon as one valid full assignment is found.
#[allow(clippy::too_many_arguments)]
fn find_assignment<P: PredicateHost + ?Sized>(
    ctx: &MatcherCtx,
    patterns: &[IrNodeId],
    elems: &[NodeId],
    pat_idx: usize,
    assignment: &mut [u8; 10],
    used: &mut u64,
    buf: &CaptureBuf,
    predicates: &mut P,
) -> bool {
    if pat_idx == patterns.len() {
        return true; // all patterns assigned
    }
    for elem_idx in 0..elems.len() {
        if *used & (1u64 << elem_idx) != 0 {
            continue; // element already used by an earlier pattern
        }
        // Probe without writing captures (discard trial buf).
        let mut trial = buf.clone();
        if match_pat(
            ctx,
            patterns[pat_idx],
            elems[elem_idx],
            &mut trial,
            predicates,
        ) {
            assignment[pat_idx] = elem_idx as u8;
            *used |= 1u64 << elem_idx;
            if find_assignment(
                ctx,
                patterns,
                elems,
                pat_idx + 1,
                assignment,
                used,
                buf,
                predicates,
            ) {
                return true;
            }
            // Backtrack.
            *used &= !(1u64 << elem_idx);
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, lower, parse};
    use murphy_ast::{AstBuilder, NodeId, NodeKind, NodeList, OptNodeId, Range, Symbol};

    fn r() -> Range {
        Range { start: 0, end: 1 }
    }

    /// `puts(1)` as an arena: `Send { receiver: None, method: :puts, args: [1] }`.
    fn puts_one_ast() -> (Ast, NodeId) {
        let mut b = AstBuilder::new("puts(1)", "t.rb");
        let one = b.push(NodeKind::Int(1), r());
        let m = b.intern_symbol("puts");
        let args = b.push_list(&[one]);
        let send = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method: m,
                args,
            },
            r(),
        );
        let ast = b.finish(send);
        (ast, send)
    }

    /// `foo.bar(1, 2, 3)` shape: `Send { receiver: Some(foo), method: :bar, args: [1,2,3] }`.
    fn dotcall_three_args_ast() -> (Ast, NodeId) {
        let mut b = AstBuilder::new("foo.bar(1,2,3)", "t.rb");
        let foo_sym = b.intern_symbol("foo");
        let foo = b.push(NodeKind::Lvar(foo_sym), r());
        let m = b.intern_symbol("bar");
        let ints: Vec<NodeId> = (1..=3)
            .map(|i| b.push(NodeKind::Int(i as i64), r()))
            .collect();
        let args = b.push_list(&ints);
        let send = b.push(
            NodeKind::Send {
                receiver: OptNodeId::some(foo),
                method: m,
                args,
            },
            r(),
        );
        let ast = b.finish(send);
        (ast, send)
    }

    /// `[1, 2, 3]`.
    fn three_array_ast() -> (Ast, NodeId, Vec<NodeId>) {
        let mut b = AstBuilder::new("[1,2,3]", "t.rb");
        let ints: Vec<NodeId> = (1..=3)
            .map(|i| b.push(NodeKind::Int(i as i64), r()))
            .collect();
        let l = b.push_list(&ints);
        let arr = b.push(NodeKind::Array(l), r());
        let ast = b.finish(arr);
        (ast, arr, ints)
    }

    fn bare_send_ast<F>(src: &str, method: &str, build_args: F) -> (Ast, NodeId, Vec<NodeId>)
    where
        F: FnOnce(&mut AstBuilder) -> Vec<NodeId>,
    {
        let mut b = AstBuilder::new(src, "t.rb");
        let args_vec = build_args(&mut b);
        let m = b.intern_symbol(method);
        let args = b.push_list(&args_vec);
        let send = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method: m,
                args,
            },
            r(),
        );
        let ast = b.finish(send);
        (ast, send, args_vec)
    }

    // ────────────────────────────────────────────────────────────────────
    // Atom / wildcard / literal
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn wildcard_matches_anything() {
        let (ast, send) = puts_one_ast();
        let ir = compile("_").unwrap();
        let c = matches(&ir, &ast, send, &mut NoPredicates).expect("wildcard");
        assert!(c.is_empty());
    }

    #[test]
    fn bare_kind_matches_send() {
        let (ast, send) = puts_one_ast();
        let ir = compile("send").unwrap();
        assert!(matches(&ir, &ast, send, &mut NoPredicates).is_some());
    }

    #[test]
    fn bare_kind_rejects_other_kind() {
        let (ast, _) = puts_one_ast();
        let ir = compile("array").unwrap();
        assert!(matches(&ir, &ast, ast.root(), &mut NoPredicates).is_none());
    }

    #[test]
    fn int_literal_matches_value() {
        let mut b = AstBuilder::new("42", "t.rb");
        let n = b.push(NodeKind::Int(42), r());
        let ast = b.finish(n);
        assert!(matches(&compile("42").unwrap(), &ast, n, &mut NoPredicates).is_some());
        assert!(matches(&compile("43").unwrap(), &ast, n, &mut NoPredicates).is_none());
    }

    #[test]
    fn sym_literal_matches_interned_string() {
        let mut b = AstBuilder::new(":x", "t.rb");
        let s = b.intern_symbol("x");
        let n = b.push(NodeKind::Sym(s), r());
        let ast = b.finish(n);
        assert!(matches(&compile(":x").unwrap(), &ast, n, &mut NoPredicates).is_some());
        assert!(matches(&compile(":y").unwrap(), &ast, n, &mut NoPredicates).is_none());
    }

    #[test]
    fn str_literal_matches_interned_string() {
        let mut b = AstBuilder::new("\"a\"", "t.rb");
        let s = b.intern_string("a");
        let n = b.push(NodeKind::Str(s), r());
        let ast = b.finish(n);
        assert!(matches(&compile("\"a\"").unwrap(), &ast, n, &mut NoPredicates).is_some());
        assert!(matches(&compile("\"b\"").unwrap(), &ast, n, &mut NoPredicates).is_none());
    }

    #[test]
    fn keyword_literals_match_their_atoms() {
        for (src, kind) in &[
            ("true", NodeKind::True_),
            ("false", NodeKind::False_),
            ("nil", NodeKind::Nil),
        ] {
            let mut b = AstBuilder::new("x", "t.rb");
            let n = b.push(*kind, r());
            let ast = b.finish(n);
            assert!(
                matches(&compile(src).unwrap(), &ast, n, &mut NoPredicates).is_some(),
                "{src} should match"
            );
        }
    }

    #[test]
    fn nil_test_matches_nil_node_at_top_level() {
        let mut b = AstBuilder::new("nil", "t.rb");
        let n = b.push(NodeKind::Nil, r());
        let ast = b.finish(n);
        assert!(matches(&compile("nil?").unwrap(), &ast, n, &mut NoPredicates).is_some());
    }

    // ────────────────────────────────────────────────────────────────────
    // Node match: head + fixed slots
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn send_match_with_implicit_receiver_nil_test() {
        let (ast, send) = puts_one_ast();
        // `puts(1)` has no receiver — `nil?` must match the absent slot.
        let ir = compile("(send nil? :puts _)").unwrap();
        assert!(matches(&ir, &ast, send, &mut NoPredicates).is_some());
    }

    #[test]
    fn send_method_slot_union_matches_any_listed_sym() {
        // murphy-rs7: `{:puts :print}` at the send method slot accepts
        // either name. `puts(1)` has method `:puts` — must hit.
        let (ast, send) = puts_one_ast();
        let ir = compile("(send nil? {:puts :print} ...)").unwrap();
        assert!(matches(&ir, &ast, send, &mut NoPredicates).is_some());
    }

    #[test]
    fn send_method_slot_union_misses_unlisted_sym() {
        // `foo.bar(...)` — `:bar` is not in `{:puts :print}`.
        let (ast, send) = dotcall_three_args_ast();
        let ir = compile("(send _ {:puts :print} ...)").unwrap();
        assert!(matches(&ir, &ast, send, &mut NoPredicates).is_none());
    }

    #[test]
    fn gvar_sym_slot_union_filters_on_name_membership() {
        // murphy-rs7 on top of murphy-o5k: `{:$stdout :$stderr}` at
        // a Gvar sym slot accepts either name and misses others.
        let mut b = AstBuilder::new("$stdout", "t.rb");
        let s = b.intern_symbol("$stdout");
        let g = b.push(NodeKind::Gvar(s), r());
        let ast = b.finish(g);
        let un = compile("(gvar {:$stdout :$stderr})").unwrap();
        assert!(matches(&un, &ast, g, &mut NoPredicates).is_some());
    }

    #[test]
    fn gvar_atom_sym_slot_filters_on_name() {
        // murphy-o5k: `(gvar :$stdout)` matches a `Gvar(:$stdout)` only.
        let mut b = AstBuilder::new("$stdout", "t.rb");
        let s = b.intern_symbol("$stdout");
        let g = b.push(NodeKind::Gvar(s), r());
        let ast = b.finish(g);
        let hit = compile("(gvar :$stdout)").unwrap();
        let miss = compile("(gvar :$stderr)").unwrap();
        let wild = compile("(gvar _)").unwrap();
        assert!(matches(&hit, &ast, g, &mut NoPredicates).is_some());
        assert!(matches(&miss, &ast, g, &mut NoPredicates).is_none());
        assert!(matches(&wild, &ast, g, &mut NoPredicates).is_some());
    }

    #[test]
    fn send_match_rejects_wrong_method_sym() {
        let (ast, send) = puts_one_ast();
        let ir = compile("(send nil? :raise _)").unwrap();
        assert!(matches(&ir, &ast, send, &mut NoPredicates).is_none());
    }

    #[test]
    fn send_match_with_explicit_receiver() {
        let (ast, send) = dotcall_three_args_ast();
        let ir = compile("(send _ :bar _ _ _)").unwrap();
        assert!(matches(&ir, &ast, send, &mut NoPredicates).is_some());
    }

    #[test]
    fn any_head_matches_arbitrary_kind() {
        let (ast, send) = puts_one_ast();
        assert!(matches(&compile("(_)").unwrap(), &ast, send, &mut NoPredicates).is_some());
        assert!(matches(&compile("(_ ...)").unwrap(), &ast, send, &mut NoPredicates).is_some());
    }

    #[test]
    fn oneof_head_matches_listed_kinds() {
        let (ast, send) = puts_one_ast();
        let ir = compile("({send csend} ...)").unwrap();
        assert!(matches(&ir, &ast, send, &mut NoPredicates).is_some());
        let ir_other = compile("({array hash} ...)").unwrap();
        assert!(matches(&ir_other, &ast, send, &mut NoPredicates).is_none());
    }

    // ────────────────────────────────────────────────────────────────────
    // List slot + Rest
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn send_with_seq_capture_collects_args() {
        let (ast, send) = dotcall_three_args_ast();
        let ir = compile("(send _ :bar $...)").unwrap();
        let c = matches(&ir, &ast, send, &mut NoPredicates).expect("ok");
        let CaptureValue::Seq(ids) = c.get(0).unwrap() else {
            panic!("expected Seq");
        };
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn array_pattern_with_prefix_rest_suffix() {
        let (ast, arr, ints) = three_array_ast();
        // `(array 1 ... 3)` — prefix [1], rest [2], suffix [3].
        let ir = compile("(array 1 ... 3)").unwrap();
        assert!(matches(&ir, &ast, arr, &mut NoPredicates).is_some());
        // Confirm that the boundaries are real.
        let bad = compile("(array 2 ... 3)").unwrap();
        assert!(matches(&bad, &ast, arr, &mut NoPredicates).is_none());
        // And that arg-by-arg works.
        let exact = compile("(array 1 2 3)").unwrap();
        assert!(matches(&exact, &ast, arr, &mut NoPredicates).is_some());
        let _ = ints;
    }

    #[test]
    fn array_rejects_wrong_length() {
        let (ast, arr, _) = three_array_ast();
        let ir = compile("(array 1 2)").unwrap();
        assert!(matches(&ir, &ast, arr, &mut NoPredicates).is_none());
    }

    #[test]
    fn array_plus_quantifier_matches_one_or_more_elements() {
        let (ast, arr, _) = three_array_ast();
        let ir = compile("(array int+)").unwrap();
        assert!(matches(&ir, &ast, arr, &mut NoPredicates).is_some());

        let mut b = AstBuilder::new("[]", "t.rb");
        let empty_list = b.push_list(&[]);
        let empty = b.push(NodeKind::Array(empty_list), r());
        let empty_ast = b.finish(empty);
        assert!(matches(&ir, &empty_ast, empty, &mut NoPredicates).is_none());
    }

    #[test]
    fn send_list_quantifiers_match_optional_and_repeated_args() {
        let (many_ast, many_send, _) = bare_send_ast("foo(1,2,\"x\")", "foo", |b| {
            let one = b.push(NodeKind::Int(1), r());
            let two = b.push(NodeKind::Int(2), r());
            let x = b.intern_string("x");
            let str_x = b.push(NodeKind::Str(x), r());
            vec![one, two, str_x]
        });
        let (str_ast, str_send, _) = bare_send_ast("foo(\"x\")", "foo", |b| {
            let x = b.intern_string("x");
            vec![b.push(NodeKind::Str(x), r())]
        });
        let (empty_ast, empty_send, _) = bare_send_ast("foo()", "foo", |_| vec![]);

        let int_star_str = compile("(send nil? :foo int* str)").unwrap();
        assert!(matches(&int_star_str, &many_ast, many_send, &mut NoPredicates).is_some());
        assert!(matches(&int_star_str, &str_ast, str_send, &mut NoPredicates).is_some());
        assert!(matches(&int_star_str, &empty_ast, empty_send, &mut NoPredicates).is_none());

        let pluck_sym_plus = compile("(send nil? :pluck sym+)").unwrap();
        let (pluck_ast, pluck_send, _) = bare_send_ast("pluck(:a,:b)", "pluck", |b| {
            let a = b.intern_symbol("a");
            let b_sym = b.intern_symbol("b");
            vec![
                b.push(NodeKind::Sym(a), r()),
                b.push(NodeKind::Sym(b_sym), r()),
            ]
        });
        let (pluck_empty_ast, pluck_empty_send, _) = bare_send_ast("pluck()", "pluck", |_| vec![]);
        assert!(matches(&pluck_sym_plus, &pluck_ast, pluck_send, &mut NoPredicates).is_some());
        assert!(
            matches(
                &pluck_sym_plus,
                &pluck_empty_ast,
                pluck_empty_send,
                &mut NoPredicates
            )
            .is_none()
        );

        let update_hash_optional = compile("(send nil? :update_columns hash?)").unwrap();
        let (hash_ast, hash_send, _) =
            bare_send_ast("update_columns({a:1})", "update_columns", |b| {
                let a = b.intern_symbol("a");
                let key = b.push(NodeKind::Sym(a), r());
                let value = b.push(NodeKind::Int(1), r());
                let pair = b.push(NodeKind::Pair { key, value }, r());
                let pairs = b.push_list(&[pair]);
                vec![b.push(NodeKind::Hash(pairs), r())]
            });
        let (no_hash_ast, no_hash_send, _) =
            bare_send_ast("update_columns()", "update_columns", |_| vec![]);
        assert!(
            matches(
                &update_hash_optional,
                &hash_ast,
                hash_send,
                &mut NoPredicates
            )
            .is_some()
        );
        assert!(
            matches(
                &update_hash_optional,
                &no_hash_ast,
                no_hash_send,
                &mut NoPredicates
            )
            .is_some()
        );
    }

    #[test]
    fn quantifier_backtracks_to_allow_suffix_match_and_captures() {
        let mut b = AstBuilder::new("[1,2,1]", "t.rb");
        let first = b.push(NodeKind::Int(1), r());
        let second = b.push(NodeKind::Int(2), r());
        let last = b.push(NodeKind::Int(1), r());
        let elems = b.push_list(&[first, second, last]);
        let arr = b.push(NodeKind::Array(elems), r());
        let ast = b.finish(arr);

        let greedy_suffix = compile("(array int+ int)").unwrap();
        assert!(matches(&greedy_suffix, &ast, arr, &mut NoPredicates).is_some());

        let captured = compile("(array $int+ $1)").unwrap();
        let c = matches(&captured, &ast, arr, &mut NoPredicates).expect("captures");
        let CaptureValue::Seq(seq) = c.get(0).unwrap() else {
            panic!("expected Seq capture");
        };
        assert_eq!(seq, &vec![first, second]);
        let CaptureValue::Node(id) = c.get(1).unwrap() else {
            panic!("expected Node capture");
        };
        assert_eq!(*id, last);
    }

    #[test]
    fn optional_quantifier_capture_records_some_and_none() {
        let (str_ast, str_send, args) = bare_send_ast("foo(\"x\")", "foo", |b| {
            let x = b.intern_string("x");
            vec![b.push(NodeKind::Str(x), r())]
        });
        let (empty_ast, empty_send, _) = bare_send_ast("foo()", "foo", |_| vec![]);
        let ir = compile("(send nil? :foo $str?)").unwrap();

        let some = matches(&ir, &str_ast, str_send, &mut NoPredicates).expect("some");
        assert_eq!(
            some.get(0),
            Some(&CaptureValue::OptNode(Some(args[0]))),
            "$str? should capture the present arg"
        );

        let none = matches(&ir, &empty_ast, empty_send, &mut NoPredicates).expect("none");
        assert_eq!(
            none.get(0),
            Some(&CaptureValue::OptNode(None)),
            "$str? should write an explicit None capture"
        );
    }

    #[test]
    fn rest_and_quantifier_can_coexist_in_list_slot() {
        let (ast, send, _) = bare_send_ast("foo(\"x\",1,2)", "foo", |b| {
            let x = b.intern_string("x");
            let str_x = b.push(NodeKind::Str(x), r());
            let one = b.push(NodeKind::Int(1), r());
            let two = b.push(NodeKind::Int(2), r());
            vec![str_x, one, two]
        });
        let (miss_ast, miss_send, _) = bare_send_ast("foo(\"x\")", "foo", |b| {
            let x = b.intern_string("x");
            vec![b.push(NodeKind::Str(x), r())]
        });
        let ir = compile("(send nil? :foo ... int+)").unwrap();
        assert!(matches(&ir, &ast, send, &mut NoPredicates).is_some());
        assert!(matches(&ir, &miss_ast, miss_send, &mut NoPredicates).is_none());
    }

    // ────────────────────────────────────────────────────────────────────
    // Union / Not
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn union_matches_any_arm() {
        let (ast, send) = puts_one_ast();
        let ir = compile("{array send}").unwrap();
        assert!(matches(&ir, &ast, send, &mut NoPredicates).is_some());
    }

    #[test]
    fn union_fails_when_no_arm_matches() {
        let (ast, send) = puts_one_ast();
        let ir = compile("{array hash}").unwrap();
        assert!(matches(&ir, &ast, send, &mut NoPredicates).is_none());
    }

    #[test]
    fn not_inverts_match() {
        let (ast, send) = puts_one_ast();
        assert!(matches(&compile("!array").unwrap(), &ast, send, &mut NoPredicates).is_some());
        assert!(matches(&compile("!send").unwrap(), &ast, send, &mut NoPredicates).is_none());
    }

    // ────────────────────────────────────────────────────────────────────
    // Capture
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn anonymous_node_capture_records_subject_id() {
        let (ast, send) = puts_one_ast();
        let ir = compile("(send nil? :puts $_)").unwrap();
        let c = matches(&ir, &ast, send, &mut NoPredicates).expect("ok");
        assert_eq!(c.len(), 1);
        let CaptureValue::Node(id) = c.get(0).unwrap() else {
            panic!("expected Node capture");
        };
        // The captured node is the `1` argument: same parent as `send`,
        // exactly one int child.
        assert_eq!(*ast.kind(*id), NodeKind::Int(1));
    }

    #[test]
    fn named_capture_writes_implicit_wildcard_body() {
        let (ast, send) = dotcall_three_args_ast();
        let ir = compile("(send $receiver _ _ _ _)").unwrap();
        // `(send <recv> <sym> <args...>)` — 3 fixed + list. Pattern child
        // count 5 → fixed [$receiver, _, _], list [_, _]. Must match.
        let c = matches(&ir, &ast, send, &mut NoPredicates).expect("ok");
        let CaptureValue::Node(_) = c.get(0).unwrap() else {
            panic!("expected Node capture");
        };
    }

    #[test]
    fn nested_capture_via_literal_and_wildcard() {
        // v1 does not support `(int ...)` Node patterns — atoms are matched
        // by literal (`5`) or bare kind (`int`). The supported way to
        // capture a specific int receiver here is `$1`: the literal `1`
        // matches an `Int(1)` node, and the surrounding `$` records the id.
        let mut b = AstBuilder::new("1 + 2", "t.rb");
        let one = b.push(NodeKind::Int(1), r());
        let two = b.push(NodeKind::Int(2), r());
        let plus = b.intern_symbol("+");
        let args = b.push_list(&[two]);
        let send = b.push(
            NodeKind::Send {
                receiver: OptNodeId::some(one),
                method: plus,
                args,
            },
            r(),
        );
        let ast = b.finish(send);
        // `$1` — capture wrapping the literal-1 sub-pattern.
        let ok = compile("(send $1 :+ _)").unwrap();
        let c = matches(&ok, &ast, send, &mut NoPredicates).expect("match");
        let CaptureValue::Node(id) = c.get(0).unwrap() else {
            panic!("expected Node capture");
        };
        assert_eq!(*id, one);
        // A literal-2 capture in the receiver position must NOT match.
        let bad = compile("(send $2 :+ _)").unwrap();
        assert!(matches(&bad, &ast, send, &mut NoPredicates).is_none());
    }

    #[test]
    fn unsupported_kind_node_pattern_silently_fails() {
        // `(int ...)` patterns are outside the v1 surface — the matcher
        // never matches them. Mirrors the B backend's compile-time error.
        let mut b = AstBuilder::new("5", "t.rb");
        let n = b.push(NodeKind::Int(5), r());
        let ast = b.finish(n);
        let ir = compile("(int)").unwrap();
        assert!(matches(&ir, &ast, n, &mut NoPredicates).is_none());
    }

    #[test]
    fn failed_top_level_match_returns_none_without_capture_panic() {
        // Even though `$_` declares a capture, a failed match must NOT
        // attempt `CaptureBuf::finish` (which would panic on the unwritten
        // slot). This test fails if the matcher tries to finalize on
        // mismatch.
        let (ast, send) = puts_one_ast();
        let ir = compile("(array $_)").unwrap();
        assert!(matches(&ir, &ast, send, &mut NoPredicates).is_none());
    }

    // ────────────────────────────────────────────────────────────────────
    // Parent / Descend
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn parent_walks_one_level_up() {
        // `Begin [ Int(1) ]`, root = Begin. `^begin` from the int matches.
        let mut b = AstBuilder::new("1", "t.rb");
        let one = b.push(NodeKind::Int(1), r());
        let list = b.push_list(&[one]);
        let begin = b.push(NodeKind::Begin(list), r());
        let ast = b.finish(begin);
        let ir = compile("^begin").unwrap();
        assert!(matches(&ir, &ast, one, &mut NoPredicates).is_some());
        // From the root, there's no parent — must fail.
        assert!(matches(&ir, &ast, begin, &mut NoPredicates).is_none());
    }

    #[test]
    fn descend_finds_a_matching_descendant() {
        // `Begin [ Int(7), Int(99) ]` — `` `int `` from the root finds the
        // first int descendant via bare-kind match.
        let mut b = AstBuilder::new("7; 99", "t.rb");
        let a = b.push(NodeKind::Int(7), r());
        let c = b.push(NodeKind::Int(99), r());
        let list = b.push_list(&[a, c]);
        let begin = b.push(NodeKind::Begin(list), r());
        let ast = b.finish(begin);
        let ir = compile("`int").unwrap();
        assert!(matches(&ir, &ast, begin, &mut NoPredicates).is_some());
        // The leaf int `99` has no descendants (excludes self), so descend
        // never finds anything from there.
        assert!(matches(&ir, &ast, c, &mut NoPredicates).is_none());
        // ` `99 ` — literal descend matches the second int.
        let lit = compile("`99").unwrap();
        assert!(matches(&lit, &ast, begin, &mut NoPredicates).is_some());
    }

    // ────────────────────────────────────────────────────────────────────
    // Predicates
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn predicate_calls_host_with_pool_name_and_subject_id() {
        let (ast, send) = puts_one_ast();
        let int_id = match *ast.kind(send) {
            NodeKind::Send { args, .. } => {
                let arr = ast.raw_parts().node_lists;
                arr[args.start as usize]
            }
            _ => unreachable!(),
        };
        struct Recording {
            seen: Vec<(String, NodeId)>,
            answer: bool,
        }
        impl PredicateHost for Recording {
            fn call(&mut self, name: &str, node: NodeId) -> bool {
                self.seen.push((name.to_owned(), node));
                self.answer
            }
        }
        let ir = compile("(send nil? :puts #is_one?)").unwrap();
        let mut host = Recording {
            seen: vec![],
            answer: true,
        };
        assert!(matches(&ir, &ast, send, &mut host).is_some());
        assert_eq!(host.seen, vec![("is_one?".to_owned(), int_id)]);

        let mut host_false = Recording {
            seen: vec![],
            answer: false,
        };
        assert!(matches(&ir, &ast, send, &mut host_false).is_none());
    }

    // ────────────────────────────────────────────────────────────────────
    // Cross-cuts: compile / lower equivalence
    // ────────────────────────────────────────────────────────────────────

    #[test]
    fn compile_is_parse_plus_lower() {
        // Confirms the matcher works equally on either `compile` or the
        // two-step `lower(&parse(...))`.
        let (ast, send) = puts_one_ast();
        let ir1 = compile("(send nil? :puts _)").unwrap();
        let ir2 = lower(&parse("(send nil? :puts _)").unwrap());
        assert!(matches(&ir1, &ast, send, &mut NoPredicates).is_some());
        assert!(matches(&ir2, &ast, send, &mut NoPredicates).is_some());
    }

    // ────────────────────────────────────────────────────────────────────
    // murphy-ycx PR #3: quantifier IR outside list dispatch
    // ────────────────────────────────────────────────────────────────────
    //
    // In debug builds the scalar-slot branch trips a `debug_assert!` so
    // layout drift is caught early; in release builds it silently misses
    // (`false`) to preserve the historical no-panic contract for hand-built
    // IR. The two test arms below pin both shapes.

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "quantifier IR reached scalar slot")]
    fn quantifier_as_scalar_pattern_panics_in_debug() {
        let (ast, arr, _ints) = three_array_ast();
        let ir = lower(&parse("(array int+)").unwrap());
        let IrNode::Node { children, .. } = ir.nodes[ir.root.0 as usize] else {
            panic!("expected node root");
        };
        let quantifier = ir.children[children.start as usize];
        let mut buf = CaptureBuf::new(0);
        let _ = match_pat(
            &MatcherCtx { ir: &ir, ast: &ast },
            quantifier,
            arr,
            &mut buf,
            &mut NoPredicates,
        );
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn quantifier_as_scalar_pattern_silently_misses_in_release() {
        let (ast, arr, _ints) = three_array_ast();
        let ir = lower(&parse("(array int+)").unwrap());
        let IrNode::Node { children, .. } = ir.nodes[ir.root.0 as usize] else {
            panic!("expected node root");
        };
        let quantifier = ir.children[children.start as usize];
        let mut buf = CaptureBuf::new(0);
        assert!(!match_pat(
            &MatcherCtx { ir: &ir, ast: &ast },
            quantifier,
            arr,
            &mut buf,
            &mut NoPredicates,
        ));
    }

    // Pull in unused-import suppression for ergonomics.
    #[allow(dead_code)]
    fn _force_use_symbol(s: Symbol, l: NodeList) {
        let _ = (s, l);
    }
}
