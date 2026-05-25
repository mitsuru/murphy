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
            // A quantifier never matches at top level — it is only valid
            // as a direct child of a node match, where `match_node_match`
            // (PR #3) will dispatch it onto the child list. Reaching this
            // arm means a hand-built or malformed IR; PR #3 lands the
            // sibling-list backtracker that actually consumes it.
            todo!("PR #3: matcher backtracker for IrNode::Quantifier")
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
/// elements. At most one rest-like pattern child is permitted (parser-
/// enforced) and splits the remaining children into prefix + rest + suffix.
fn match_list_slot<P: PredicateHost + ?Sized>(
    ctx: &MatcherCtx,
    pattern_kids: &[IrNodeId],
    elems: &[NodeId],
    buf: &mut CaptureBuf,
    predicates: &mut P,
) -> bool {
    // Locate the at-most-one rest-like pattern child.
    let rest_at = pattern_kids
        .iter()
        .position(|p| rest_kind(ctx, *p).is_some());

    let Some(r) = rest_at else {
        // No rest: exact length, indexed match.
        if pattern_kids.len() != elems.len() {
            return false;
        }
        for (i, p) in pattern_kids.iter().enumerate() {
            if !match_pat(ctx, *p, elems[i], buf, predicates) {
                return false;
            }
        }
        return true;
    };

    // Rest-like child at index `r`. `non_rest = k - 1` non-rest pattern
    // children split into `r` prefix + `(non_rest - r)` suffix.
    let k = pattern_kids.len();
    let non_rest = k - 1;
    let suffix_count = non_rest - r;

    if elems.len() < non_rest {
        return false;
    }

    // Prefix.
    for (i, p) in pattern_kids.iter().take(r).enumerate() {
        if !match_pat(ctx, *p, elems[i], buf, predicates) {
            return false;
        }
    }
    // Suffix matches the last `suffix_count` elements.
    let len = elems.len();
    for (j, p) in pattern_kids.iter().skip(r + 1).enumerate() {
        let actual = elems[len - (suffix_count - j)];
        if !match_pat(ctx, *p, actual, buf, predicates) {
            return false;
        }
    }
    // Middle: the rest span `elems[r..len - suffix_count]`. A `$...`
    // capture binds it; a bare `...` binds nothing.
    let rest_slice = &elems[r..len - suffix_count];
    if let Some(Some(s)) = rest_kind(ctx, pattern_kids[r]) {
        buf.set(s, CaptureValue::Seq(rest_slice.to_vec()));
    }
    true
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

    // Pull in unused-import suppression for ergonomics.
    #[allow(dead_code)]
    fn _force_use_symbol(s: Symbol, l: NodeList) {
        let _ = (s, l);
    }
}
