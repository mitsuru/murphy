//! Per-NodeKind structural schema for the runtime matcher.
//!
//! The schema describes — at runtime — how a pattern's child list maps onto
//! one `NodeKind` variant's fields, in **parser-gem child order**: each fixed
//! slot is a `Node`, `OptNode`, or `Sym`, and an optional trailing `List`
//! slot consumes any rest. This mirrors the `SCHEMA_TABLE` the B-backend
//! proc macro uses to lower `node_pattern!` calls; the two backends MUST
//! agree on slot order and type per kind so the same source pattern matches
//! the same set of nodes (design §4, "1 grammar, 2 backends").
//!
//! The B-backend's table lives in
//! `crates/murphy-plugin-macros/src/node_pattern.rs` and works in
//! proc-macro–only types (`FieldRef::Named` / `FieldRef::Pos`,
//! destructuring patterns). That format is not reachable from non-proc-macro
//! crates, so the runtime version is duplicated here intentionally; a
//! follow-up may lift a shared source. The drift guard is
//! `crates/murphy-plugin-macros/tests/cross_backend_conformance.rs`,
//! which exercises a representative pattern per backend feature against
//! both backends and asserts they agree on yes/no and capture values.

use murphy_ast::{NodeId, NodeKind, OptNodeId, Symbol};

/// The tag whitelist for `(kind ...)` Node patterns. **Must mirror the
/// B-backend `SCHEMA_TABLE` exactly.** Kinds outside this set deliberately
/// return `None` from [`pattern_children`] so that the matcher reports a
/// failed match for `(unsupported_kind …)` patterns, matching the B
/// backend's compile-time rejection. Atoms (`int`, `nil`, …) and unary
/// kinds (`lvar`, `splat`, …) are *intentionally absent* — bare kind
/// patterns (`int`) and literal patterns (`5`) cover the v1 use cases.
const SUPPORTED_TAGS: &[u8] = &[
    13, // Const
    14, 15, 16, // Lvasgn / Ivasgn / Casgn
    17, 18, 19, // Send / Csend / Block
    22, 23, 24, // Array / Hash / Pair
    25, 26, 27, // If / Case / When
    28, // Begin
    29, // Return
    30, 31, // And / Or
    32, 33, 34, // Def / Class / Module
    38, 39, // Gvasgn / Cvasgn
    47, 48, // While / Until
];

/// `true` iff `(<kind>)` Node patterns are supported for `tag`.
pub fn supports_node_pattern(tag: murphy_ast::NodeKindTag) -> bool {
    SUPPORTED_TAGS.contains(&tag.0)
}

/// One resolved slot value for a `NodeKind`, surfaced by [`pattern_children`].
///
/// Carries the *resolved* arena value — `OptNode` is `Option<NodeId>` (with
/// the `OptNodeId::NONE` sentinel collapsed), `List` is a borrowed slice into
/// the `node_lists` side table — so the matcher never re-derives them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatChild<'a> {
    /// A `NodeId` field (always present).
    Node(NodeId),
    /// An `OptNodeId` field, with the sentinel resolved.
    OptNode(Option<NodeId>),
    /// A `Symbol` field (e.g. `Send::method`, `Lvasgn::name`).
    Sym(Symbol),
    /// A `NodeList` field (e.g. `Send::args`). Borrowed against the
    /// `node_lists` side table that the matcher holds.
    List(&'a [NodeId]),
}

/// The matcher's per-NodeKind slot dispatch. Returns the `kind`'s
/// pattern-children in parser-gem order, **including the trailing list slot
/// if any**. `None` for variants that the v1 pattern surface does not match
/// (e.g. `Error`, `Unknown`, or variants whose schema is omitted on purpose,
/// like `Rescue` — see the design notes inline). A `None` here means "this
/// kind is not matchable by `(<kind> ...)`"; the matcher reports it as a
/// failed match, not a panic.
///
/// The exhaustive `match` is intentional: a new `NodeKind` variant breaks
/// compilation here, forcing a deliberate decision about how (or whether) to
/// expose it to patterns.
pub fn pattern_children<'a>(kind: &'a NodeKind, lists: &'a [NodeId]) -> Option<Vec<PatChild<'a>>> {
    fn opt(o: OptNodeId) -> Option<NodeId> {
        o.get()
    }
    fn list(l: murphy_ast::NodeList, lists: &[NodeId]) -> &[NodeId] {
        let s = l.start as usize;
        &lists[s..s + l.len as usize]
    }

    // Gate every variant on the supported-tags whitelist so the matcher
    // mirrors the B backend's compile-time rejection — `(int)` / `(splat)`
    // / `(rescue)` etc. deterministically fail to match. Atoms are still
    // matchable as bare kind patterns (`int`) and literal patterns (`5`).
    if !supports_node_pattern(kind.tag()) {
        return None;
    }

    let slots = match *kind {
        // ── Variable reads with payload ────────────────────────────────
        NodeKind::Const { scope, name } => vec![PatChild::OptNode(opt(scope)), PatChild::Sym(name)],

        // ── Assignments ───────────────────────────────────────────────
        NodeKind::Lvasgn { name, value }
        | NodeKind::Ivasgn { name, value }
        | NodeKind::Gvasgn { name, value }
        | NodeKind::Cvasgn { name, value } => {
            vec![PatChild::Sym(name), PatChild::OptNode(opt(value))]
        }
        NodeKind::Casgn { scope, name, value } => vec![
            PatChild::OptNode(opt(scope)),
            PatChild::Sym(name),
            PatChild::OptNode(opt(value)),
        ],

        // ── Calls / blocks ────────────────────────────────────────────
        NodeKind::Send {
            receiver,
            method,
            args,
        } => vec![
            PatChild::OptNode(opt(receiver)),
            PatChild::Sym(method),
            PatChild::List(list(args, lists)),
        ],
        NodeKind::Csend {
            receiver,
            method,
            args,
        } => vec![
            PatChild::Node(receiver),
            PatChild::Sym(method),
            PatChild::List(list(args, lists)),
        ],
        NodeKind::Block { call, args, body } => vec![
            PatChild::Node(call),
            PatChild::Node(args),
            PatChild::OptNode(opt(body)),
        ],
        // ── Collections ───────────────────────────────────────────────
        NodeKind::Array(l) | NodeKind::Hash(l) | NodeKind::Begin(l) => {
            vec![PatChild::List(list(l, lists))]
        }
        NodeKind::Pair { key, value } => vec![PatChild::Node(key), PatChild::Node(value)],

        // ── Control flow ──────────────────────────────────────────────
        NodeKind::If { cond, then_, else_ } => vec![
            PatChild::Node(cond),
            PatChild::OptNode(opt(then_)),
            PatChild::OptNode(opt(else_)),
        ],
        // `Case { subject, whens, else_ }`: `else_` follows the `NodeList`,
        // but the v1 slot convention allows at most one trailing `List`. The
        // B backend therefore omits `else_` from `case`'s schema, so `case`
        // patterns expose only `subject` + `whens`. Mirrored here.
        NodeKind::Case { subject, whens, .. } => vec![
            PatChild::OptNode(opt(subject)),
            PatChild::List(list(whens, lists)),
        ],
        // `When { conds, body }`: `body` follows the `NodeList`; same reason
        // as `Case::else_` above — omitted to keep the trailing-List rule.
        NodeKind::When { conds, .. } => vec![PatChild::List(list(conds, lists))],
        NodeKind::Return(o) => vec![PatChild::OptNode(opt(o))],
        NodeKind::And { lhs, rhs } | NodeKind::Or { lhs, rhs } => {
            vec![PatChild::Node(lhs), PatChild::Node(rhs)]
        }

        // ── Definitions ───────────────────────────────────────────────
        // `Def { receiver, name, args, body }`: `receiver` (singleton-method
        // discrimination) is out of v1 pattern scope — omitted to mirror B
        // backend. `def` patterns expose `name`/`args`/`body`.
        NodeKind::Def {
            name, args, body, ..
        } => vec![
            PatChild::Sym(name),
            PatChild::Node(args),
            PatChild::OptNode(opt(body)),
        ],
        NodeKind::Class {
            name,
            superclass,
            body,
        } => vec![
            PatChild::Node(name),
            PatChild::OptNode(opt(superclass)),
            PatChild::OptNode(opt(body)),
        ],
        NodeKind::Module { name, body } => {
            vec![PatChild::Node(name), PatChild::OptNode(opt(body))]
        }

        // ── Loops ────────────────────────────────────────────────────
        // `While { cond, body, post }` / `Until { ... }`: `post: bool` is a
        // flag, not a child — omitted to mirror B backend.
        NodeKind::While { cond, body, .. } | NodeKind::Until { cond, body, .. } => {
            vec![PatChild::Node(cond), PatChild::OptNode(opt(body))]
        }

        // The early `supports_node_pattern` gate above has already returned
        // `None` for every other variant; reaching this arm means a tag was
        // added to `SUPPORTED_TAGS` without a matching schema branch.
        _ => unreachable!(
            "SUPPORTED_TAGS lists tag {} but pattern_children has no schema",
            kind.tag().0
        ),
    };
    Some(slots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_ast::{AstBuilder, NodeKind, OptNodeId, Range, Symbol};

    fn r() -> Range {
        Range { start: 0, end: 1 }
    }

    #[test]
    fn send_schema_is_receiver_method_args() {
        // `puts(1)` shaped as `Send { receiver: None, method: :puts, args: [1] }`.
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

        let kids = pattern_children(ast.kind(send), ast.raw_parts().node_lists).expect("send");
        assert_eq!(kids.len(), 3);
        assert!(matches!(kids[0], PatChild::OptNode(None)));
        assert!(matches!(kids[1], PatChild::Sym(Symbol(_))));
        let PatChild::List(args_slice) = kids[2] else {
            panic!("expected List slot, got {:?}", kids[2]);
        };
        assert_eq!(args_slice.len(), 1);
        assert_eq!(args_slice[0], one);
    }

    #[test]
    fn error_and_unknown_are_unmatchable() {
        let mut b = AstBuilder::new("x", "t.rb");
        let e = b.push(NodeKind::Error, r());
        let u = b.push(NodeKind::Unknown, r());
        // The root must be ANY node; pick `e` arbitrarily.
        let ast = b.finish(e);
        assert!(pattern_children(ast.kind(e), ast.raw_parts().node_lists).is_none());
        assert!(pattern_children(ast.kind(u), ast.raw_parts().node_lists).is_none());
    }

    #[test]
    fn atom_is_outside_node_pattern_surface() {
        // `nil` is a bare-kind / literal pattern, not a `(nil)` Node pattern
        // (B backend has no SCHEMA_TABLE entry for it). `pattern_children`
        // must report `None`, mirroring B's "kind not supported" rejection.
        let mut b = AstBuilder::new("nil", "t.rb");
        let n = b.push(NodeKind::Nil, r());
        let ast = b.finish(n);
        assert!(pattern_children(ast.kind(n), ast.raw_parts().node_lists).is_none());
    }

    #[test]
    fn int_is_outside_node_pattern_surface() {
        // Same as `nil` — `(int 5)` is not a v1 pattern surface; literal `5`
        // is the way to match an `Int(5)` node.
        let mut b = AstBuilder::new("5", "t.rb");
        let n = b.push(NodeKind::Int(5), r());
        let ast = b.finish(n);
        assert!(pattern_children(ast.kind(n), ast.raw_parts().node_lists).is_none());
    }

    #[test]
    fn rescue_and_friends_are_unsupported_in_v1() {
        // Variants outside `SUPPORTED_TAGS` deterministically report `None`
        // — mirrors the B backend's compile-time "kind not supported" error.
        let mut b = AstBuilder::new("x", "t.rb");
        let res = b.push(
            NodeKind::Rescue {
                body: OptNodeId::NONE,
                resbodies: murphy_ast::NodeList::EMPTY,
                else_: OptNodeId::NONE,
            },
            r(),
        );
        let ast = b.finish(res);
        assert!(pattern_children(ast.kind(res), ast.raw_parts().node_lists).is_none());
    }

    #[test]
    fn supports_node_pattern_covers_b_backend_whitelist() {
        // Cheap exhaustive cross-check: every tag in the supported list
        // must round-trip through `supports_node_pattern`.
        for &t in SUPPORTED_TAGS {
            assert!(
                supports_node_pattern(murphy_ast::NodeKindTag(t)),
                "tag {t} missing from supports_node_pattern",
            );
        }
        // And a known-unsupported tag fails.
        assert!(!supports_node_pattern(murphy_ast::NodeKindTag(5))); // Int
        assert!(!supports_node_pattern(murphy_ast::NodeKindTag(57))); // Rescue
    }

    #[test]
    fn array_exposes_trailing_list_slot() {
        let mut b = AstBuilder::new("[1,2]", "t.rb");
        let a = b.push(NodeKind::Int(1), r());
        let c = b.push(NodeKind::Int(2), r());
        let l = b.push_list(&[a, c]);
        let arr = b.push(NodeKind::Array(l), r());
        let ast = b.finish(arr);

        let kids = pattern_children(ast.kind(arr), ast.raw_parts().node_lists).expect("array");
        assert_eq!(kids.len(), 1);
        let PatChild::List(s) = kids[0] else {
            panic!("expected List, got {:?}", kids[0]);
        };
        assert_eq!(s, &[a, c]);
    }

    #[test]
    fn casgn_exposes_three_slots_with_sym_in_the_middle() {
        // `casgn` is the canonical 3-slot pattern: `(scope OptNode, name Sym,
        // value OptNode)`. This guards against the assignments' `Sym`-first
        // shape being copied into `casgn` (`Sym` is in position 1, not 0).
        let mut b = AstBuilder::new("Foo = 1", "t.rb");
        let one = b.push(NodeKind::Int(1), r());
        let n = b.intern_symbol("Foo");
        let casgn = b.push(
            NodeKind::Casgn {
                scope: OptNodeId::NONE,
                name: n,
                value: OptNodeId::some(one),
            },
            r(),
        );
        let ast = b.finish(casgn);
        let kids = pattern_children(ast.kind(casgn), ast.raw_parts().node_lists).expect("casgn");
        assert_eq!(kids.len(), 3);
        assert!(matches!(kids[0], PatChild::OptNode(None)));
        assert!(matches!(kids[1], PatChild::Sym(_)));
        assert!(matches!(kids[2], PatChild::OptNode(Some(_))));
    }

    #[test]
    fn case_when_omit_trailing_optnode_slots() {
        // `Case { subject, whens, else_ }` and `When { conds, body }` both
        // have a trailing `OptNodeId` AFTER a `NodeList`. The v1 slot
        // convention allows at most one trailing `List`, so `else_`/`body`
        // are dropped — the schema exposes only the slots up to and
        // including the `List`. Guards against drift back to a 3-slot
        // `case` / 2-slot `when` schema.
        let mut b = AstBuilder::new("case x; when y; end", "t.rb");
        let x = b.push(NodeKind::Nil, r());
        let y = b.push(NodeKind::Nil, r());
        let conds = b.push_list(&[y]);
        let when_ = b.push(
            NodeKind::When {
                conds,
                body: OptNodeId::some(x),
            },
            r(),
        );
        let whens = b.push_list(&[when_]);
        let case = b.push(
            NodeKind::Case {
                subject: OptNodeId::some(x),
                whens,
                else_: OptNodeId::some(x),
            },
            r(),
        );
        let ast = b.finish(case);
        let case_kids = pattern_children(ast.kind(case), ast.raw_parts().node_lists).expect("case");
        assert_eq!(case_kids.len(), 2);
        assert!(matches!(case_kids[1], PatChild::List(_)));
        let when_kids =
            pattern_children(ast.kind(when_), ast.raw_parts().node_lists).expect("when");
        assert_eq!(when_kids.len(), 1);
        assert!(matches!(when_kids[0], PatChild::List(_)));
    }
}
