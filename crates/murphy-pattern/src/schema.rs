//! Per-NodeKind structural schema for the runtime matcher.
//!
//! The schema describes — at runtime — how a pattern's child list maps onto
//! one `NodeKind` variant's fields, in **parser-gem child order**: each fixed
//! slot is a `Node`, `OptNode`, or `Sym`, and an optional trailing `List`
//! slot consumes any rest. This mirrors the `SCHEMA_TABLE` the B-backend
//! proc macro uses to lower `def_node_matcher!` calls; the two backends MUST
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
/// backend's compile-time rejection. Atoms with no children (`int`,
/// `nil`, …) are *intentionally absent* — bare kind patterns (`int`)
/// and literal patterns (`5`) cover the v1 use cases. Variable-read
/// atoms (`lvar`/`ivar`/`cvar`/`gvar`) carry a `Symbol` payload, so
/// `(gvar :$stdout)` etc. is matchable through a single sym slot
/// (murphy-o5k).
const SUPPORTED_TAGS: &[u8] = &[
    9, 10, 11, 12, // Lvar / Ivar / Cvar / Gvar (one-slot sym pattern, murphy-o5k)
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
    86, 87, // CaseMatch / InPattern (murphy-j1j2 PM-F)
    90, // MatchVar (single sym slot, same shape as lvar/gvar)
];

/// `true` iff `(<kind>)` Node patterns are supported for `tag`.
pub fn supports_node_pattern(tag: murphy_ast::NodeKindTag) -> bool {
    SUPPORTED_TAGS.contains(&tag.0)
}

/// Whether `tag`'s child slot at `child_idx` may host a bare predicate shorthand
/// identifier like `foo?` / `foo!` in a node-child position.
///
/// Symbol slots (`PatChild::Sym`) must not accept bare predicate shorthand,
/// because they are literal symbol paths (`:name`). For all other slot kinds,
/// including trailing list slots, bare predicates are valid in `node-match` child
/// positions.
pub fn node_child_allows_bare_predicate(tag: murphy_ast::NodeKindTag, child_idx: usize) -> bool {
    if !supports_node_pattern(tag) {
        // Unsupported node kinds have no schema table row in v1, so parser-time
        // slot typing is unavailable. Allow bare predicates in child position
        // and let runtime matching decide exact applicability.
        return true;
    }

    match tag.0 {
        // ── Symbol-only vars: `(:name)`
        9..=12 => false,

        // ── Single `OptNode + Sym` or sym-first variants
        13 => child_idx == 0,                   // const
        14 | 15 | 38 | 39 => child_idx == 1,    // lvasgn / ivasgn / gvasgn / cvasgn
        16 => child_idx == 0 || child_idx == 2, // casgn
        17 | 18 => child_idx != 1,              // send / csend

        // ── All Node/OptNode/Node list slots (non-Sym): all fixed children
        // plus trailing list children when present.
        19 => child_idx <= 2, // block (3 fixed, no trailing list)
        22 | 23 | 28 => true, // array / hash / begin (single trailing list)
        24 => child_idx <= 1, // pair (2 fixed, no trailing list)
        25 => child_idx <= 2, // if (3 fixed, no trailing list)
        26 => true,           // case (1 fixed + trailing list)
        27 => true,           // when (single trailing list)
        29 => child_idx == 0, // return (1 fixed, no trailing list)
        30 | 31 | 47 | 48 => child_idx <= 1, // and/or/while/until (2 fixed, no trailing list)
        32 => child_idx == 1 || child_idx == 2, // def (Sym, Node, OptNode — Sym at 0)
        33 => child_idx <= 2, // class (3 fixed, no trailing list)
        34 => child_idx <= 1, // module (2 fixed, no trailing list)

        // pattern-match nodes (murphy-j1j2 PM-F)
        // `case_match`: slot 0 = subject (Node), slot 1+ = in_patterns (List)
        86 => true,
        // `in_pattern`: slots 0-2 are pattern/guard/body (Node/OptNode/OptNode)
        87 => child_idx <= 2,
        // `match_var`: single sym slot — bare predicates not allowed
        90 => false,

        // Nothing else is supported in v1.
        _ => false,
    }
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
    ///
    /// **Invariant (v1):** no supported `NodeKind` has a `List<Sym>` slot —
    /// trailing lists carry `NodeId`s only. [`node_child_allows_bare_predicate`]
    /// relies on this to allow bare predicate shorthand at every trailing-list
    /// position. Adding a `List<Sym>`-shaped variant requires revisiting that
    /// helper and reflecting the per-slot Sym mask there.
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
        // ── Variable reads (single sym slot, murphy-o5k) ──────────────
        NodeKind::Lvar(name)
        | NodeKind::Ivar(name)
        | NodeKind::Cvar(name)
        | NodeKind::Gvar(name) => vec![PatChild::Sym(name)],

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

        // ── Pattern-match family (murphy-j1j2 PM-F) ─────────────────────────
        // `CaseMatch { subject, in_patterns, else_body }`: `else_body` follows
        // the `NodeList`, so it is omitted (covers_all_fields = false).
        NodeKind::CaseMatch {
            subject,
            in_patterns,
            ..
        } => vec![
            PatChild::Node(subject),
            PatChild::List(list(in_patterns, lists)),
        ],
        // `InPattern { pattern, guard, body }`: three fixed slots.
        NodeKind::InPattern {
            pattern,
            guard,
            body,
        } => vec![
            PatChild::Node(pattern),
            PatChild::OptNode(opt(guard)),
            PatChild::OptNode(opt(body)),
        ],
        // `MatchVar(Symbol)`: single sym slot — same shape as `Lvar`/`Gvar`.
        NodeKind::MatchVar(name) => vec![PatChild::Sym(name)],

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
    fn gvar_exposes_one_sym_slot_for_name_payload() {
        // `Gvar(:$stdout)` exposes its `Symbol` name through a single
        // `PatChild::Sym` slot, which is how `(gvar :$stdout)` patterns
        // filter on the variable name (murphy-o5k).
        let mut b = AstBuilder::new("$stdout", "t.rb");
        let s = b.intern_symbol("$stdout");
        let g = b.push(NodeKind::Gvar(s), r());
        let ast = b.finish(g);
        let kids = pattern_children(ast.kind(g), ast.raw_parts().node_lists).expect("gvar");
        assert_eq!(kids.len(), 1);
        assert!(matches!(kids[0], PatChild::Sym(_)));
        // The exposed `Symbol` round-trips to the original name.
        let PatChild::Sym(actual) = kids[0] else {
            unreachable!();
        };
        assert_eq!(ast.interner().resolve(actual.0), "$stdout");
    }

    #[test]
    fn lvar_ivar_cvar_each_expose_one_sym_slot() {
        // murphy-o5k extends the same single-sym-slot schema to all four
        // var-read atoms — Lvar, Ivar, Cvar (Gvar is covered above).
        let mut b = AstBuilder::new("x", "t.rb");
        let lx = b.intern_symbol("x");
        let l = b.push(NodeKind::Lvar(lx), r());
        let iat = b.intern_symbol("@x");
        let i = b.push(NodeKind::Ivar(iat), r());
        let cat = b.intern_symbol("@@c");
        let c = b.push(NodeKind::Cvar(cat), r());
        let ast = b.finish(l);
        for n in [l, i, c] {
            let kids = pattern_children(ast.kind(n), ast.raw_parts().node_lists).expect("var");
            assert_eq!(kids.len(), 1);
            assert!(matches!(kids[0], PatChild::Sym(_)));
        }
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
    fn node_child_allows_bare_predicate_only_for_node_like_slots() {
        assert!(!node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(9),
            0
        )); // lvar
        assert!(!node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(13),
            1
        )); // const method
        assert!(!node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(17),
            1
        )); // send method
        assert!(!node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(18),
            1
        )); // csend method
        assert!(node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(17),
            0
        )); // send receiver
        assert!(node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(17),
            2
        )); // send args list
        assert!(node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(17),
            99
        )); // send trailing-list child slot
        assert!(node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(22),
            0
        )); // array list
        assert!(node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(29),
            0
        )); // return
        assert!(!node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(29),
            1
        ));
        assert!(!node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(30),
            99
        )); // and no fixed slots beyond 1
        assert!(node_child_allows_bare_predicate(
            murphy_ast::NodeKindTag(57),
            0
        )); // unsupported tag: parser cannot know slot typing, so allow and defer
    }

    /// Build one representative instance of every `SUPPORTED_TAGS` variant in
    /// a single `Ast`. Used by the exhaustive cross-check to walk every
    /// supported kind's actual `pattern_children` output. Updating this list
    /// is forced whenever a tag joins or leaves `SUPPORTED_TAGS` — see
    /// `node_child_allows_bare_predicate_matches_pattern_children_slot_shape`.
    fn build_all_supported() -> (
        murphy_ast::Ast,
        Vec<(murphy_ast::NodeKindTag, murphy_ast::NodeId)>,
    ) {
        let mut b = AstBuilder::new("supported", "t.rb");
        // Placeholder atoms used as Node / OptNode fillers throughout.
        let a = b.push(NodeKind::Int(1), r());
        let bb = b.push(NodeKind::Int(2), r());
        let args = b.push(NodeKind::Args(murphy_ast::NodeList::EMPTY), r());
        let sym = b.intern_symbol("x");

        let mut out: Vec<(murphy_ast::NodeKindTag, murphy_ast::NodeId)> = Vec::new();
        let mut add = |b: &mut AstBuilder, tag: u8, id: murphy_ast::NodeId| {
            out.push((murphy_ast::NodeKindTag(tag), id));
            let _ = b; // keep the borrow lifetime explicit for clarity
        };

        let lvar = b.push(NodeKind::Lvar(sym), r());
        add(&mut b, 9, lvar);
        let ivar = b.push(NodeKind::Ivar(sym), r());
        add(&mut b, 10, ivar);
        let cvar = b.push(NodeKind::Cvar(sym), r());
        add(&mut b, 11, cvar);
        let gvar = b.push(NodeKind::Gvar(sym), r());
        add(&mut b, 12, gvar);

        let cst = b.push(
            NodeKind::Const {
                scope: OptNodeId::NONE,
                name: sym,
            },
            r(),
        );
        add(&mut b, 13, cst);

        let lvasgn = b.push(
            NodeKind::Lvasgn {
                name: sym,
                value: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 14, lvasgn);
        let ivasgn = b.push(
            NodeKind::Ivasgn {
                name: sym,
                value: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 15, ivasgn);
        let casgn = b.push(
            NodeKind::Casgn {
                scope: OptNodeId::NONE,
                name: sym,
                value: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 16, casgn);

        let send = b.push(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method: sym,
                args: murphy_ast::NodeList::EMPTY,
            },
            r(),
        );
        add(&mut b, 17, send);
        let csend = b.push(
            NodeKind::Csend {
                receiver: a,
                method: sym,
                args: murphy_ast::NodeList::EMPTY,
            },
            r(),
        );
        add(&mut b, 18, csend);
        let block = b.push(
            NodeKind::Block {
                call: send,
                args,
                body: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 19, block);

        let arr = b.push(NodeKind::Array(murphy_ast::NodeList::EMPTY), r());
        add(&mut b, 22, arr);
        let hash = b.push(NodeKind::Hash(murphy_ast::NodeList::EMPTY), r());
        add(&mut b, 23, hash);
        let pair = b.push(NodeKind::Pair { key: a, value: bb }, r());
        add(&mut b, 24, pair);

        let if_ = b.push(
            NodeKind::If {
                cond: a,
                then_: OptNodeId::NONE,
                else_: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 25, if_);
        let case = b.push(
            NodeKind::Case {
                subject: OptNodeId::NONE,
                whens: murphy_ast::NodeList::EMPTY,
                else_: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 26, case);
        let when_ = b.push(
            NodeKind::When {
                conds: murphy_ast::NodeList::EMPTY,
                body: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 27, when_);
        let begin = b.push(NodeKind::Begin(murphy_ast::NodeList::EMPTY), r());
        add(&mut b, 28, begin);

        let ret = b.push(NodeKind::Return(OptNodeId::NONE), r());
        add(&mut b, 29, ret);
        let and_ = b.push(NodeKind::And { lhs: a, rhs: bb }, r());
        add(&mut b, 30, and_);
        let or_ = b.push(NodeKind::Or { lhs: a, rhs: bb }, r());
        add(&mut b, 31, or_);

        let def = b.push(
            NodeKind::Def {
                receiver: OptNodeId::NONE,
                name: sym,
                args,
                body: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 32, def);
        let cls = b.push(
            NodeKind::Class {
                name: a,
                superclass: OptNodeId::NONE,
                body: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 33, cls);
        let mdl = b.push(
            NodeKind::Module {
                name: a,
                body: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 34, mdl);

        let gvasgn = b.push(
            NodeKind::Gvasgn {
                name: sym,
                value: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 38, gvasgn);
        let cvasgn = b.push(
            NodeKind::Cvasgn {
                name: sym,
                value: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 39, cvasgn);

        let while_ = b.push(
            NodeKind::While {
                cond: a,
                body: OptNodeId::NONE,
                post: false,
            },
            r(),
        );
        add(&mut b, 47, while_);
        let until_ = b.push(
            NodeKind::Until {
                cond: a,
                body: OptNodeId::NONE,
                post: false,
            },
            r(),
        );
        add(&mut b, 48, until_);

        // pattern-match family (murphy-j1j2 PM-F)
        let in_pats_list = b.push_list(&[]);
        let case_match = b.push(
            NodeKind::CaseMatch {
                subject: a,
                in_patterns: in_pats_list,
                else_body: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 86, case_match);
        let in_pat = b.push(
            NodeKind::InPattern {
                pattern: a,
                guard: OptNodeId::NONE,
                body: OptNodeId::NONE,
            },
            r(),
        );
        add(&mut b, 87, in_pat);
        let mv_sym = b.intern_symbol("x");
        let match_var = b.push(NodeKind::MatchVar(mv_sym), r());
        add(&mut b, 90, match_var);

        let ast = b.finish(a);
        (ast, out)
    }

    #[test]
    fn build_all_supported_covers_every_supported_tag() {
        // Cheap staleness guard: if `SUPPORTED_TAGS` grows but
        // `build_all_supported` is not updated, the exhaustive check below
        // would silently skip the new tag — fail loudly here instead.
        let (_, built) = build_all_supported();
        let built_tags: std::collections::BTreeSet<u8> = built.iter().map(|(t, _)| t.0).collect();
        let supported_tags: std::collections::BTreeSet<u8> =
            SUPPORTED_TAGS.iter().copied().collect();
        assert_eq!(
            built_tags, supported_tags,
            "build_all_supported must instantiate exactly the SUPPORTED_TAGS set",
        );
    }

    #[test]
    fn node_child_allows_bare_predicate_matches_pattern_children_slot_shape() {
        // For every supported kind, the parser-time helper
        // `node_child_allows_bare_predicate` must agree with the runtime
        // `pattern_children` output: `false` exactly at `Sym` slot indices,
        // `true` everywhere else (including trailing-list positions). This
        // is the structural drift guard between the two: a future variant
        // that reshuffles slot positions will break compilation of
        // `pattern_children` (exhaustive match) and break this test if the
        // helper isn't updated.
        let (ast, built) = build_all_supported();
        for (tag, id) in built {
            let kids = pattern_children(ast.kind(id), ast.raw_parts().node_lists)
                .unwrap_or_else(|| panic!("supported tag {tag:?} returned no children"));

            // Fixed slot positions: parser-time helper must agree with
            // runtime slot kind on each index.
            for (idx, child) in kids.iter().enumerate() {
                let actual = node_child_allows_bare_predicate(tag, idx);
                let expected = !matches!(child, PatChild::Sym(_));
                assert_eq!(
                    actual, expected,
                    "tag {tag:?} idx {idx} child {child:?}: helper={actual} expected={expected}",
                );
            }

            // Trailing-list positions: indices past the fixed slot count
            // continue the list. Per the `PatChild::List` invariant
            // (Node-only lists in v1), every such index must allow bare
            // predicates. For kinds without a trailing list, the helper
            // must reject indices past the last fixed slot.
            let last_is_list = matches!(kids.last(), Some(PatChild::List(_)));
            let past = kids.len() + 3;
            let actual_past = node_child_allows_bare_predicate(tag, past);
            assert_eq!(
                actual_past, last_is_list,
                "tag {tag:?} idx {past} (past fixed slots): helper={actual_past} expected={last_is_list}",
            );
        }
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
