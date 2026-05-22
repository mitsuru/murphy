//! Pattern-name ↔ `NodeKindTag` resolution for `murphy-pattern`.
//!
//! `NodeKindTag` is the `u8` discriminant of a [`NodeKind`](crate::NodeKind)
//! variant (declaration order, frozen — see ADR 0037). The
//! `KIND_PATTERN_NAMES` table maps the snake_case node-type name a pattern
//! author writes (`send`, `lvasgn`, …) to that tag.

/// The `u8` discriminant of a [`NodeKind`](crate::NodeKind) variant.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKindTag(pub u8);

/// Pattern-name → tag. Declaration order of `NodeKind` minus `Error`
/// (you cannot match an error node in a pattern). Keep in sync with the
/// `NodeKind` enum in `node.rs`; the `table_matches_tag` test guards this.
pub const KIND_PATTERN_NAMES: &[(&str, u8)] = &[
    ("nil", 1),
    ("true", 2),
    ("false", 3),
    ("self", 4),
    ("int", 5),
    ("float", 6),
    ("str", 7),
    ("sym", 8),
    ("lvar", 9),
    ("ivar", 10),
    ("cvar", 11),
    ("gvar", 12),
    ("const", 13),
    ("lvasgn", 14),
    ("ivasgn", 15),
    ("casgn", 16),
    ("send", 17),
    ("csend", 18),
    ("block", 19),
    ("block_pass", 20),
    ("splat", 21),
    ("array", 22),
    ("hash", 23),
    ("pair", 24),
    ("if", 25),
    ("case", 26),
    ("when", 27),
    ("begin", 28),
    ("return", 29),
    ("and", 30),
    ("or", 31),
    ("def", 32),
    ("class", 33),
    ("module", 34),
    ("args", 35),
    ("arg", 36),
];

/// Resolve a pattern node-type name to its tag. `None` for unknown names.
pub fn tag_from_pattern_name(name: &str) -> Option<NodeKindTag> {
    KIND_PATTERN_NAMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| NodeKindTag(*t))
}

/// The pattern node-type name for a tag (diagnostics / reverse lookup).
pub fn pattern_name(tag: NodeKindTag) -> Option<&'static str> {
    KIND_PATTERN_NAMES
        .iter()
        .find(|(_, t)| *t == tag.0)
        .map(|(n, _)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    /// One constructed instance of EVERY `NodeKind` variant, in declaration
    /// order. Adding a variant to `NodeKind` forces an update here — paired
    /// with the exhaustive `match` in `NodeKind::tag`, this is the staleness
    /// guard for `KIND_PATTERN_NAMES`.
    fn all_variants() -> Vec<NodeKind> {
        use crate::{NodeId, NodeList, OptNodeId, StringId, Symbol};
        let n = NodeId(0);
        let s = Symbol(0);
        vec![
            NodeKind::Error,
            NodeKind::Nil,
            NodeKind::True_,
            NodeKind::False_,
            NodeKind::SelfExpr,
            NodeKind::Int(0),
            NodeKind::Float(0.0),
            NodeKind::Str(StringId(0)),
            NodeKind::Sym(s),
            NodeKind::Lvar(s),
            NodeKind::Ivar(s),
            NodeKind::Cvar(s),
            NodeKind::Gvar(s),
            NodeKind::Const {
                scope: OptNodeId::NONE,
                name: s,
            },
            NodeKind::Lvasgn {
                name: s,
                value: OptNodeId::NONE,
            },
            NodeKind::Ivasgn {
                name: s,
                value: OptNodeId::NONE,
            },
            NodeKind::Casgn {
                scope: OptNodeId::NONE,
                name: s,
                value: OptNodeId::NONE,
            },
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method: s,
                args: NodeList::EMPTY,
            },
            NodeKind::Csend {
                receiver: n,
                method: s,
                args: NodeList::EMPTY,
            },
            NodeKind::Block {
                call: n,
                args: n,
                body: OptNodeId::NONE,
            },
            NodeKind::BlockPass(OptNodeId::NONE),
            NodeKind::Splat(OptNodeId::NONE),
            NodeKind::Array(NodeList::EMPTY),
            NodeKind::Hash(NodeList::EMPTY),
            NodeKind::Pair { key: n, value: n },
            NodeKind::If {
                cond: n,
                then_: OptNodeId::NONE,
                else_: OptNodeId::NONE,
            },
            NodeKind::Case {
                subject: OptNodeId::NONE,
                whens: NodeList::EMPTY,
                else_: OptNodeId::NONE,
            },
            NodeKind::When {
                conds: NodeList::EMPTY,
                body: OptNodeId::NONE,
            },
            NodeKind::Begin(NodeList::EMPTY),
            NodeKind::Return(OptNodeId::NONE),
            NodeKind::And { lhs: n, rhs: n },
            NodeKind::Or { lhs: n, rhs: n },
            NodeKind::Def {
                name: s,
                args: n,
                body: OptNodeId::NONE,
            },
            NodeKind::Class {
                name: n,
                superclass: OptNodeId::NONE,
                body: OptNodeId::NONE,
            },
            NodeKind::Module {
                name: n,
                body: OptNodeId::NONE,
            },
            NodeKind::Args(NodeList::EMPTY),
            NodeKind::Arg(s),
        ]
    }

    #[test]
    fn tag_is_declaration_order() {
        for (i, k) in all_variants().iter().enumerate() {
            assert_eq!(k.tag().0 as usize, i, "tag mismatch for {k:?}");
        }
    }

    #[test]
    fn table_matches_tag() {
        // Every table entry resolves to a real variant with that tag, and
        // every variant except Error (tag 0) has exactly one table entry.
        let variants = all_variants();
        for (name, tag) in KIND_PATTERN_NAMES {
            assert_eq!(variants[*tag as usize].tag().0, *tag, "table entry {name}");
        }
        for k in &variants {
            let t = k.tag();
            if t.0 == 0 {
                assert!(pattern_name(t).is_none(), "Error must have no pattern name");
            } else {
                assert!(pattern_name(t).is_some(), "missing table entry for {k:?}");
            }
        }
    }

    #[test]
    fn round_trip_and_unknown() {
        assert_eq!(tag_from_pattern_name("send"), Some(NodeKindTag(17)));
        assert_eq!(pattern_name(NodeKindTag(17)), Some("send"));
        assert_eq!(tag_from_pattern_name("sned"), None);
        assert_eq!(tag_from_pattern_name("error"), None);
    }

    #[test]
    fn tag_matches_serialize_discriminant() {
        // `tag()` and `serialize::write_node_kind` both assign discriminants;
        // this cross-checks them directly rather than via a round-trip.
        for k in all_variants() {
            let mut buf = vec![];
            crate::serialize::write_node_kind(&k, &mut buf);
            assert_eq!(buf[0], k.tag().0, "discriminant mismatch for {k:?}");
        }
    }
}
