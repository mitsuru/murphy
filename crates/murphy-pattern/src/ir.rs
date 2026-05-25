//! `PatternIr` — a flat, pointer-free node array derived from `PatternAst`.
//! Consumed by the C backend interpreter (murphy-9cr.19). Mirrors the
//! `murphy-ast` arena design: nodes in a `Vec`, variable-length children in
//! a side table.

use crate::CaptureKind;
use crate::ast::{Head, Lit, Pat, PatKind, PatternAst};
use murphy_ast::NodeKindTag;

/// Index into [`PatternIr::nodes`].
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IrNodeId(pub u32);

/// A reference to a contiguous slice of a side table (`children` or `tags`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrSlice {
    pub start: u32,
    pub len: u32,
}

/// A reference to a `[start, start+len)` byte range of `str_pool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrRef {
    pub start: u32,
    pub len: u32,
}

/// A compiled pattern: a flat node array plus side tables. No pointers.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternIr {
    pub nodes: Vec<IrNode>,
    /// Side table for `IrNode::Node` children and `IrNode::Union` arms.
    pub children: Vec<IrNodeId>,
    /// Side table for `IrHead::OneOf` alternatives.
    pub tags: Vec<NodeKindTag>,
    /// Predicate names, string/symbol literals, capture names.
    pub str_pool: String,
    /// One entry per `$` capture, in slot order.
    pub captures: Vec<CaptureMeta>,
    /// The root node.
    pub root: IrNodeId,
}

/// Per-capture metadata, indexed by slot.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureMeta {
    pub kind: CaptureKind,
    /// `Some` for `$ident` named captures.
    pub name: Option<StrRef>,
}

/// A flat IR node. `PatKind` resolved and flattened; children by index.
#[derive(Debug, Clone, PartialEq)]
pub enum IrNode {
    Wildcard,
    /// `...` / `$...` — matches zero or more sibling nodes. The "rest-like
    /// element is valid only as a direct node child, and at most one per
    /// node child list" invariant is enforced by the parser
    /// (`validate_rest_placement`), NOT by this IR type — the `IrNode` enum
    /// does not structurally guarantee it, so consumers must not assume it.
    Rest,
    NilTest,
    LitInt(i64),
    LitFloat(f64),
    LitStr(StrRef),
    LitSym(StrRef),
    LitTrue,
    LitFalse,
    LitNil,
    Predicate(StrRef),
    Kind(NodeKindTag),
    Node {
        head: IrHead,
        children: IrSlice,
    },
    Union(IrSlice),
    Not(IrNodeId),
    Capture {
        slot: u16,
        body: IrNodeId,
    },
    Parent(IrNodeId),
    Descend(IrNodeId),
}

/// The head of an `IrNode::Node`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrHead {
    Exact(NodeKindTag),
    Any,
    OneOf(IrSlice),
}

/// Lower a [`PatternAst`] into the flat, pointer-free [`PatternIr`].
///
/// Infallible: all validation (unknown node types, duplicate capture names,
/// misplaced `...`, …) already happened in [`crate::parse`]. The recursive
/// tree is flattened post-order — every child is lowered and its [`IrNodeId`]
/// recorded before the parent node is pushed, so an `IrNodeId` always refers
/// to an already-present `nodes` entry.
///
/// Capture slot numbers are taken verbatim from `PatKind::Capture.slot` (the
/// parser assigned them in source order); they are never re-derived from
/// traversal order. `ir.captures` is pre-sized to `ast.n_captures()` so each
/// slot can be written by index.
pub fn lower(ast: &PatternAst) -> PatternIr {
    let mut ir = PatternIr {
        nodes: Vec::new(),
        children: Vec::new(),
        tags: Vec::new(),
        str_pool: String::new(),
        // Pre-sized so `ir.captures[slot]` indexes safely; every slot is
        // overwritten during the traversal.
        captures: vec![
            CaptureMeta {
                kind: CaptureKind::Node,
                name: None,
            };
            ast.n_captures()
        ],
        root: IrNodeId(0),
    };
    ir.root = lower_pat(&ast.root, ast, &mut ir);
    ir
}

/// Append `s` to the IR string pool and return a [`StrRef`] for it.
///
/// v1 interning is a plain append — no deduplication; the C backend does not
/// need it and avoiding it keeps lowering allocation-free beyond the pool's
/// own growth.
fn intern(ir: &mut PatternIr, s: &str) -> StrRef {
    // v1 pattern sizes are tiny; the `as u32` casts below are sound only while
    // the pool stays under `u32::MAX` bytes. Document that invariant here.
    debug_assert!(ir.str_pool.len() <= u32::MAX as usize);
    let start = ir.str_pool.len() as u32;
    ir.str_pool.push_str(s);
    StrRef {
        start,
        len: s.len() as u32,
    }
}

/// Push `node` into `ir.nodes` and return the [`IrNodeId`] it now occupies.
fn push_node(ir: &mut PatternIr, node: IrNode) -> IrNodeId {
    // `IrNodeId` is a `u32`; the cast below is sound only while the node array
    // stays under `u32::MAX` entries. v1 patterns never approach that.
    debug_assert!(ir.nodes.len() <= u32::MAX as usize);
    let id = IrNodeId(ir.nodes.len() as u32);
    ir.nodes.push(node);
    id
}

/// Lower one `Head` into an `IrHead`, pushing `OneOf` tags into `ir.tags`.
fn lower_head(ir: &mut PatternIr, head: &Head) -> IrHead {
    match head {
        Head::Exact(tag) => IrHead::Exact(*tag),
        Head::Any => IrHead::Any,
        Head::OneOf(tags) => {
            // No recursion inside, so a single `extend` keeps the slice
            // contiguous regardless of surrounding child lowering.
            let start = ir.tags.len() as u32;
            ir.tags.extend_from_slice(tags);
            IrHead::OneOf(IrSlice {
                start,
                len: tags.len() as u32,
            })
        }
    }
}

/// Lower a sequence of child patterns into `ir.children`, returning the
/// [`IrSlice`] that addresses the resulting contiguous id run.
///
/// Each child is lowered first and its id collected; only then are the ids
/// appended in one block. Lowering a child may itself push grandchild ids
/// into `ir.children`, so collecting ids before extending is what keeps the
/// returned slice contiguous.
fn lower_children(children: &[Pat], ast: &PatternAst, ir: &mut PatternIr) -> IrSlice {
    let ids: Vec<IrNodeId> = children
        .iter()
        .map(|child| lower_pat(child, ast, ir))
        .collect();
    let start = ir.children.len() as u32;
    let len = ids.len() as u32;
    ir.children.extend(ids);
    IrSlice { start, len }
}

/// Post-order flatten one `Pat` into `ir`, returning its [`IrNodeId`].
fn lower_pat(pat: &Pat, ast: &PatternAst, ir: &mut PatternIr) -> IrNodeId {
    match &pat.kind {
        PatKind::Wildcard => push_node(ir, IrNode::Wildcard),
        PatKind::Rest => push_node(ir, IrNode::Rest),
        PatKind::NilTest => push_node(ir, IrNode::NilTest),
        PatKind::Lit(lit) => {
            let node = match lit {
                Lit::Int(v) => IrNode::LitInt(*v),
                Lit::Float(v) => IrNode::LitFloat(*v),
                Lit::Str(s) => IrNode::LitStr(intern(ir, s)),
                Lit::Sym(s) => IrNode::LitSym(intern(ir, s)),
                Lit::True => IrNode::LitTrue,
                Lit::False => IrNode::LitFalse,
                Lit::Nil => IrNode::LitNil,
            };
            push_node(ir, node)
        }
        PatKind::Predicate(name) => {
            let r = intern(ir, name);
            push_node(ir, IrNode::Predicate(r))
        }
        PatKind::Kind(tag) => push_node(ir, IrNode::Kind(*tag)),
        PatKind::Node { head, children } => {
            // Lower children first (post-order), then the head's tag table.
            let child_slice = lower_children(children, ast, ir);
            let head = lower_head(ir, head);
            push_node(
                ir,
                IrNode::Node {
                    head,
                    children: child_slice,
                },
            )
        }
        PatKind::Union(alts) => {
            let arm_slice = lower_children(alts, ast, ir);
            push_node(ir, IrNode::Union(arm_slice))
        }
        PatKind::Not(inner) => {
            let body = lower_pat(inner, ast, ir);
            push_node(ir, IrNode::Not(body))
        }
        PatKind::Capture { slot, name, body } => {
            let body_id = lower_pat(body, ast, ir);
            // Trust the parser-assigned slot — do NOT renumber by traversal.
            // Every parser-assigned slot is `< ast.n_captures()`, so both
            // `ir.captures` (pre-sized to `n_captures()`) and `ast.captures`
            // index safely.
            debug_assert!(
                (*slot as usize) < ast.captures.len(),
                "capture slot out of range"
            );
            let name_ref = name.as_deref().map(|n| intern(ir, n));
            ir.captures[*slot as usize] = CaptureMeta {
                kind: ast.captures[*slot as usize],
                name: name_ref,
            };
            push_node(
                ir,
                IrNode::Capture {
                    slot: *slot,
                    body: body_id,
                },
            )
        }
        PatKind::Parent(inner) => {
            let body = lower_pat(inner, ast, ir);
            push_node(ir, IrNode::Parent(body))
        }
        PatKind::Descend(inner) => {
            let body = lower_pat(inner, ast, ir);
            push_node(ir, IrNode::Descend(body))
        }
        PatKind::Quantifier { .. } => {
            // PR #2 (murphy-ycx) lowers quantifiers into the C-backend IR.
            // PR #1 only covers parse + validation, so reaching here means a
            // quantifier-bearing pattern was handed to lowering before its
            // PR landed.
            todo!("PR #2: lower PatKind::Quantifier into PatternIr")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    /// `NodeKindTag` for `send` in the current tag table. Localized here so a
    /// future tag-table reorder breaks in one obvious place.
    const SEND_TAG: murphy_ast::NodeKindTag = murphy_ast::NodeKindTag(17);
    /// `NodeKindTag` for `csend` in the current tag table.
    const CSEND_TAG: murphy_ast::NodeKindTag = murphy_ast::NodeKindTag(18);

    #[test]
    fn ir_construction_smoke() {
        let ir = PatternIr {
            nodes: vec![IrNode::Wildcard],
            children: vec![],
            tags: vec![],
            str_pool: String::new(),
            captures: vec![],
            root: IrNodeId(0),
        };
        assert_eq!(ir.nodes.len(), 1);
        assert_eq!(ir.root, IrNodeId(0));
    }

    /// Read the [`StrRef`] range out of the IR's string pool.
    fn pooled(ir: &PatternIr, r: StrRef) -> &str {
        &ir.str_pool[r.start as usize..(r.start + r.len) as usize]
    }

    #[test]
    fn lowers_wildcard() {
        let ir = lower(&parse("_").unwrap());
        assert_eq!(ir.nodes, vec![IrNode::Wildcard]);
        assert_eq!(ir.root, IrNodeId(0));
    }

    #[test]
    fn lowers_node_children_into_side_table() {
        let ir = lower(&parse("(send nil :puts)").unwrap());
        // root is a Node; its children live in the `children` side table.
        let root = &ir.nodes[ir.root.0 as usize];
        match root {
            IrNode::Node { head, children } => {
                assert_eq!(*head, IrHead::Exact(SEND_TAG));
                assert_eq!(children.len, 2);
                // The side-table ids must point at the lowered children:
                // a `nil` literal and a `:puts` symbol literal.
                let start = children.start as usize;
                let len = children.len as usize;
                let ids = &ir.children[start..start + len];
                assert_eq!(ir.nodes[ids[0].0 as usize], IrNode::LitNil);
                match ir.nodes[ids[1].0 as usize] {
                    IrNode::LitSym(r) => assert_eq!(pooled(&ir, r), "puts"),
                    ref other => panic!("expected LitSym, got {other:?}"),
                }
            }
            other => panic!("expected Node, got {other:?}"),
        }
    }

    #[test]
    fn lowers_strings_into_pool() {
        let ir = lower(&parse(":puts").unwrap());
        match ir.nodes[ir.root.0 as usize] {
            IrNode::LitSym(r) => {
                let s = &ir.str_pool[r.start as usize..(r.start + r.len) as usize];
                assert_eq!(s, "puts");
            }
            ref other => panic!("expected LitSym, got {other:?}"),
        }
    }

    #[test]
    fn lowers_capture_slots_and_meta() {
        let ir = lower(&parse("(send $receiver $...)").unwrap());
        assert_eq!(ir.captures.len(), 2);
        assert_eq!(ir.captures[0].kind, CaptureKind::Node);
        assert_eq!(ir.captures[1].kind, CaptureKind::Seq);
        // named capture's name is in the pool
        let r = ir.captures[0].name.expect("named");
        assert_eq!(
            &ir.str_pool[r.start as usize..(r.start + r.len) as usize],
            "receiver"
        );
        assert!(ir.captures[1].name.is_none());
    }

    #[test]
    fn lowers_oneof_head_tags() {
        let ir = lower(&parse("({send csend} _)").unwrap());
        match ir.nodes[ir.root.0 as usize] {
            IrNode::Node {
                head: IrHead::OneOf(s),
                ..
            } => {
                let tags = &ir.tags[s.start as usize..(s.start + s.len) as usize];
                assert_eq!(tags, &[SEND_TAG, CSEND_TAG]);
            }
            ref other => panic!("expected OneOf, got {other:?}"),
        }
    }

    #[test]
    fn lower_capture_slots_match_pattern_ast() {
        // Nested captures: outer $(...) = 0, $named = 1, $tail = 2. IR capture
        // metadata must be slot-indexed and agree with the PatternAst.
        let p = parse("$(send $named (send $tail))").expect("ok");
        let ir = lower(&p);
        assert_eq!(ir.captures.len(), p.n_captures());
        let ir_kinds: Vec<_> = ir.captures.iter().map(|c| c.kind).collect();
        assert_eq!(ir_kinds.as_slice(), p.capture_kinds());
    }

    #[test]
    fn lower_trusts_source_order_slots_not_post_order() {
        // `$(send $... $named)` — outer anonymous `$(...)` is slot 0 (Node),
        // `$...` is slot 1 (Seq), `$named` is slot 2 (Node). A post-order
        // traversal would visit `$...` and `$named` BEFORE the outer capture,
        // so a bug that renumbered by traversal order would produce a
        // different kind sequence. Slot order must be `[Node, Seq, Node]`.
        let p = parse("$(send $... $named)").expect("ok");
        let ir = lower(&p);
        let ir_kinds: Vec<_> = ir.captures.iter().map(|c| c.kind).collect();
        assert_eq!(
            ir_kinds.as_slice(),
            &[CaptureKind::Node, CaptureKind::Seq, CaptureKind::Node]
        );
        assert_eq!(ir_kinds.as_slice(), p.capture_kinds());
        // Slot 2 is the named one; its name is in the pool, slots 0/1 are not.
        assert!(ir.captures[0].name.is_none());
        assert!(ir.captures[1].name.is_none());
        let r = ir.captures[2].name.expect("named");
        assert_eq!(pooled(&ir, r), "named");
        // The outer node's `IrNode::Capture` must carry slot 0 verbatim.
        match ir.nodes[ir.root.0 as usize] {
            IrNode::Capture { slot, .. } => assert_eq!(slot, 0),
            ref other => panic!("expected outer Capture, got {other:?}"),
        }
    }

    // --- additional lowering coverage ------------------------------------

    #[test]
    fn lowers_int_and_float_literals() {
        let ir = lower(&parse("42").unwrap());
        assert_eq!(ir.nodes[ir.root.0 as usize], IrNode::LitInt(42));
        let ir = lower(&parse("1.5").unwrap());
        assert_eq!(ir.nodes[ir.root.0 as usize], IrNode::LitFloat(1.5));
    }

    #[test]
    fn lowers_keyword_literals_and_niltest() {
        assert_eq!(lower(&parse("true").unwrap()).nodes[0], IrNode::LitTrue);
        assert_eq!(lower(&parse("false").unwrap()).nodes[0], IrNode::LitFalse);
        assert_eq!(lower(&parse("nil").unwrap()).nodes[0], IrNode::LitNil);
        assert_eq!(lower(&parse("nil?").unwrap()).nodes[0], IrNode::NilTest);
    }

    #[test]
    fn lowers_kind() {
        let ir = lower(&parse("send").unwrap());
        assert_eq!(ir.nodes[ir.root.0 as usize], IrNode::Kind(SEND_TAG));
    }

    #[test]
    fn lowers_union_arms_into_side_table() {
        let ir = lower(&parse("{send csend}").unwrap());
        match ir.nodes[ir.root.0 as usize] {
            IrNode::Union(s) => {
                assert_eq!(s.len, 2);
                let arms = &ir.children[s.start as usize..(s.start + s.len) as usize];
                assert_eq!(ir.nodes[arms[0].0 as usize], IrNode::Kind(SEND_TAG));
                assert_eq!(ir.nodes[arms[1].0 as usize], IrNode::Kind(CSEND_TAG));
            }
            ref other => panic!("expected Union, got {other:?}"),
        }
    }

    #[test]
    fn lowers_predicate_into_pool() {
        let ir = lower(&parse("#odd?").unwrap());
        match ir.nodes[ir.root.0 as usize] {
            IrNode::Predicate(r) => assert_eq!(pooled(&ir, r), "odd?"),
            ref other => panic!("expected Predicate, got {other:?}"),
        }
    }

    #[test]
    fn lowers_not_parent_descend() {
        // `!_` -> Not(Wildcard).
        let ir = lower(&parse("!_").unwrap());
        match ir.nodes[ir.root.0 as usize] {
            IrNode::Not(body) => {
                assert_eq!(ir.nodes[body.0 as usize], IrNode::Wildcard)
            }
            ref other => panic!("expected Not, got {other:?}"),
        }
        // `^_` -> Parent(Wildcard).
        let ir = lower(&parse("^_").unwrap());
        match ir.nodes[ir.root.0 as usize] {
            IrNode::Parent(body) => {
                assert_eq!(ir.nodes[body.0 as usize], IrNode::Wildcard)
            }
            ref other => panic!("expected Parent, got {other:?}"),
        }
        // `` `_ `` -> Descend(Wildcard).
        let ir = lower(&parse("`_").unwrap());
        match ir.nodes[ir.root.0 as usize] {
            IrNode::Descend(body) => {
                assert_eq!(ir.nodes[body.0 as usize], IrNode::Wildcard)
            }
            ref other => panic!("expected Descend, got {other:?}"),
        }
    }

    #[test]
    fn lowers_any_head() {
        let ir = lower(&parse("(_ _)").unwrap());
        match ir.nodes[ir.root.0 as usize] {
            IrNode::Node {
                head: IrHead::Any, ..
            } => {}
            ref other => panic!("expected Any head, got {other:?}"),
        }
    }

    #[test]
    fn lowers_two_distinct_strings_into_pool() {
        // Two distinct symbols at distinct offsets in the pool.
        let ir = lower(&parse("(send :foo :bar)").unwrap());
        let root = &ir.nodes[ir.root.0 as usize];
        let IrNode::Node { children, .. } = root else {
            panic!("expected Node, got {root:?}");
        };
        let start = children.start as usize;
        let ids = &ir.children[start..start + children.len as usize];
        let (IrNode::LitSym(r0), IrNode::LitSym(r1)) =
            (&ir.nodes[ids[0].0 as usize], &ir.nodes[ids[1].0 as usize])
        else {
            panic!("expected two LitSym children");
        };
        assert_eq!(pooled(&ir, *r0), "foo");
        assert_eq!(pooled(&ir, *r1), "bar");
        // Distinct ranges — v1 interns by append, so offsets differ.
        assert_ne!(r0.start, r1.start);
    }

    #[test]
    fn lowers_nested_node_children_offsets_are_correct() {
        // `(send (send nil :a) :b)` — the first child is itself a node whose
        // own children get pushed into the side table BEFORE the outer node's
        // child-id slice. The outer slice must still be contiguous and point
        // at the right ids.
        let ir = lower(&parse("(send (send nil :a) :b)").unwrap());
        let root = &ir.nodes[ir.root.0 as usize];
        let IrNode::Node {
            children: outer, ..
        } = root
        else {
            panic!("expected outer Node, got {root:?}");
        };
        assert_eq!(outer.len, 2);
        let outer_ids = &ir.children[outer.start as usize..(outer.start + outer.len) as usize];
        // First outer child is the inner `(send nil :a)` node.
        let inner = &ir.nodes[outer_ids[0].0 as usize];
        let IrNode::Node {
            children: inner_kids,
            ..
        } = inner
        else {
            panic!("expected inner Node, got {inner:?}");
        };
        assert_eq!(inner_kids.len, 2);
        let inner_ids =
            &ir.children[inner_kids.start as usize..(inner_kids.start + inner_kids.len) as usize];
        assert_eq!(ir.nodes[inner_ids[0].0 as usize], IrNode::LitNil);
        match ir.nodes[inner_ids[1].0 as usize] {
            IrNode::LitSym(r) => assert_eq!(pooled(&ir, r), "a"),
            ref other => panic!("expected LitSym, got {other:?}"),
        }
        // Second outer child is the `:b` symbol.
        match ir.nodes[outer_ids[1].0 as usize] {
            IrNode::LitSym(r) => assert_eq!(pooled(&ir, r), "b"),
            ref other => panic!("expected LitSym, got {other:?}"),
        }
    }

    #[test]
    fn lowers_capture_with_body() {
        // `$(send)` — anonymous capture wrapping a Node body.
        let ir = lower(&parse("$(send)").unwrap());
        match ir.nodes[ir.root.0 as usize] {
            IrNode::Capture { slot, body } => {
                assert_eq!(slot, 0);
                assert!(matches!(ir.nodes[body.0 as usize], IrNode::Node { .. }));
                assert_eq!(ir.captures[0].kind, CaptureKind::Node);
                assert!(ir.captures[0].name.is_none());
            }
            ref other => panic!("expected Capture, got {other:?}"),
        }
    }

    #[test]
    fn lowers_empty_child_list() {
        // `(send)` — a node with no children still gets a valid (len 0) slice.
        let ir = lower(&parse("(send)").unwrap());
        match ir.nodes[ir.root.0 as usize] {
            IrNode::Node { children, .. } => assert_eq!(children.len, 0),
            ref other => panic!("expected Node, got {other:?}"),
        }
    }

    #[test]
    fn lowers_rest_to_ir_rest_node() {
        // `...` in a node child list lowers to `IrNode::Rest`. (A bare `...`
        // at top level is a parse error, so it must be tested inside a node.)
        let ir = lower(&parse("(send ... _)").unwrap());
        let root = &ir.nodes[ir.root.0 as usize];
        let IrNode::Node { children, .. } = root else {
            panic!("expected Node, got {root:?}");
        };
        let first_child = ir.children[children.start as usize];
        assert_eq!(ir.nodes[first_child.0 as usize], IrNode::Rest);
    }
}
