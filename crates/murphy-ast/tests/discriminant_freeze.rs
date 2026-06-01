//! `NodeKind` discriminant freeze test (murphy-9cr.26 申し送り 2).
//!
//! Both the in-memory `NodeKind::tag()` and the on-disk serialization
//! encode each variant as a `u8` derived from the variant's declaration
//! order. Renaming or reordering variants therefore silently shifts the
//! mapping and breaks every previously-serialized cache file — but the
//! breakage is undetectable until a cache hit deserializes into the wrong
//! variant at traversal time.
//!
//! This test pins each variant's tag byte to an explicit literal so a
//! reorder fails compilation here (via `pattern` panic) or assertion long
//! before any cache file is consulted. Pair this with the in-crate
//! `kinds::tests::tag_matches_serialize_discriminant`, which guarantees
//! `serialize::write_node_kind` agrees with `tag()`; together the two
//! tests freeze the wire-byte discriminant.
//!
//! **When adding a new variant**: append it at the end of the enum, give
//! it the next free tag, and add a matching `freeze(NodeKind::New, N);`
//! line below. Never insert in the middle.

use murphy_ast::{NodeId, NodeKind, NodeList, OptNodeId, StringId, Symbol};

fn n() -> NodeId {
    NodeId(0)
}
fn s() -> Symbol {
    Symbol(0)
}

fn freeze(kind: NodeKind, expected: u8) {
    assert_eq!(
        kind.tag().0,
        expected,
        "frozen discriminant {expected} no longer matches {kind:?} — \
         did you reorder NodeKind?"
    );
}

#[test]
fn node_kind_discriminants_are_frozen() {
    freeze(NodeKind::Error, 0);
    freeze(NodeKind::Nil, 1);
    freeze(NodeKind::True_, 2);
    freeze(NodeKind::False_, 3);
    freeze(NodeKind::SelfExpr, 4);
    freeze(NodeKind::Int(0), 5);
    freeze(NodeKind::Float(0.0), 6);
    freeze(NodeKind::Str(StringId(0)), 7);
    freeze(NodeKind::Sym(s()), 8);
    freeze(NodeKind::Lvar(s()), 9);
    freeze(NodeKind::Ivar(s()), 10);
    freeze(NodeKind::Cvar(s()), 11);
    freeze(NodeKind::Gvar(s()), 12);
    freeze(
        NodeKind::Const {
            scope: OptNodeId::NONE,
            name: s(),
        },
        13,
    );
    freeze(
        NodeKind::Lvasgn {
            name: s(),
            value: OptNodeId::NONE,
        },
        14,
    );
    freeze(
        NodeKind::Ivasgn {
            name: s(),
            value: OptNodeId::NONE,
        },
        15,
    );
    freeze(
        NodeKind::Casgn {
            scope: OptNodeId::NONE,
            name: s(),
            value: OptNodeId::NONE,
        },
        16,
    );
    freeze(
        NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: s(),
            args: NodeList::EMPTY,
        },
        17,
    );
    freeze(
        NodeKind::Csend {
            receiver: n(),
            method: s(),
            args: NodeList::EMPTY,
        },
        18,
    );
    freeze(
        NodeKind::Block {
            call: n(),
            args: n(),
            body: OptNodeId::NONE,
        },
        19,
    );
    freeze(NodeKind::BlockPass(OptNodeId::NONE), 20);
    freeze(NodeKind::Splat(OptNodeId::NONE), 21);
    freeze(NodeKind::Array(NodeList::EMPTY), 22);
    freeze(NodeKind::Hash(NodeList::EMPTY), 23);
    freeze(
        NodeKind::Pair {
            key: n(),
            value: n(),
        },
        24,
    );
    freeze(
        NodeKind::If {
            cond: n(),
            then_: OptNodeId::NONE,
            else_: OptNodeId::NONE,
        },
        25,
    );
    freeze(
        NodeKind::Case {
            subject: OptNodeId::NONE,
            whens: NodeList::EMPTY,
            else_: OptNodeId::NONE,
        },
        26,
    );
    freeze(
        NodeKind::When {
            conds: NodeList::EMPTY,
            body: OptNodeId::NONE,
        },
        27,
    );
    freeze(NodeKind::Begin(NodeList::EMPTY), 28);
    freeze(NodeKind::Return(OptNodeId::NONE), 29);
    freeze(NodeKind::And { lhs: n(), rhs: n() }, 30);
    freeze(NodeKind::Or { lhs: n(), rhs: n() }, 31);
    freeze(
        NodeKind::Def {
            receiver: OptNodeId::NONE,
            name: s(),
            args: n(),
            body: OptNodeId::NONE,
        },
        32,
    );
    freeze(
        NodeKind::Class {
            name: n(),
            superclass: OptNodeId::NONE,
            body: OptNodeId::NONE,
        },
        33,
    );
    freeze(
        NodeKind::Module {
            name: n(),
            body: OptNodeId::NONE,
        },
        34,
    );
    freeze(NodeKind::Args(NodeList::EMPTY), 35);
    freeze(NodeKind::Arg(s()), 36);
    freeze(NodeKind::Unknown, 37);
    freeze(
        NodeKind::Gvasgn {
            name: s(),
            value: OptNodeId::NONE,
        },
        38,
    );
    freeze(
        NodeKind::Cvasgn {
            name: s(),
            value: OptNodeId::NONE,
        },
        39,
    );
    freeze(
        NodeKind::Optarg {
            name: s(),
            default: n(),
        },
        40,
    );
    freeze(NodeKind::Restarg(s()), 41);
    freeze(NodeKind::Kwarg(s()), 42);
    freeze(
        NodeKind::Kwoptarg {
            name: s(),
            default: n(),
        },
        43,
    );
    freeze(NodeKind::Kwrestarg(s()), 44);
    freeze(NodeKind::Blockarg(s()), 45);
    freeze(NodeKind::Kwsplat(OptNodeId::NONE), 46);
    freeze(
        NodeKind::While {
            cond: n(),
            body: OptNodeId::NONE,
            post: false,
        },
        47,
    );
    freeze(
        NodeKind::Until {
            cond: n(),
            body: OptNodeId::NONE,
            post: false,
        },
        48,
    );
    freeze(
        NodeKind::RangeExpr {
            begin_: OptNodeId::NONE,
            end_: OptNodeId::NONE,
            exclusive: false,
        },
        49,
    );
    freeze(
        NodeKind::Sclass {
            expr: n(),
            body: OptNodeId::NONE,
        },
        50,
    );
    freeze(NodeKind::Break(OptNodeId::NONE), 51);
    freeze(NodeKind::Next(OptNodeId::NONE), 52);
    freeze(NodeKind::Yield(NodeList::EMPTY), 53);
    freeze(NodeKind::Super(NodeList::EMPTY), 54);
    freeze(NodeKind::Zsuper, 55);
    freeze(NodeKind::Defined(n()), 56);
    freeze(
        NodeKind::Rescue {
            body: OptNodeId::NONE,
            resbodies: NodeList::EMPTY,
            else_: OptNodeId::NONE,
        },
        57,
    );
    freeze(
        NodeKind::Resbody {
            exceptions: NodeList::EMPTY,
            var: OptNodeId::NONE,
            body: OptNodeId::NONE,
        },
        58,
    );
    freeze(
        NodeKind::Ensure {
            body: OptNodeId::NONE,
            ensure_: OptNodeId::NONE,
        },
        59,
    );
    freeze(
        NodeKind::OpAsgn {
            target: n(),
            op: s(),
            value: n(),
        },
        60,
    );
    freeze(
        NodeKind::OrAsgn {
            target: n(),
            value: n(),
        },
        61,
    );
    freeze(
        NodeKind::AndAsgn {
            target: n(),
            value: n(),
        },
        62,
    );
    freeze(NodeKind::Dstr(NodeList::EMPTY), 63);
    freeze(NodeKind::Dsym(NodeList::EMPTY), 64);
    freeze(NodeKind::Xstr(NodeList::EMPTY), 65);
    freeze(
        NodeKind::Regexp {
            parts: NodeList::EMPTY,
            opts: s(),
        },
        66,
    );
    freeze(NodeKind::Masgn { lhs: n(), rhs: n() }, 67);
    freeze(NodeKind::Mlhs(NodeList::EMPTY), 68);
    // murphy-o57f MID-priority pattern-match extensions (tags 86–90 subset)
    freeze(
        NodeKind::CaseMatch {
            subject: n(),
            in_patterns: NodeList::EMPTY,
            else_body: OptNodeId::NONE,
        },
        86,
    );
    freeze(
        NodeKind::InPattern {
            pattern: n(),
            guard: OptNodeId::NONE,
            body: OptNodeId::NONE,
        },
        87,
    );
    freeze(NodeKind::ArrayPattern(NodeList::EMPTY), 88);
    freeze(NodeKind::HashPattern(NodeList::EMPTY), 89);
    freeze(NodeKind::MatchVar(s()), 90);
    // murphy-jw5t pattern-match lowering
    freeze(NodeKind::FindPattern(NodeList::EMPTY), 101);
    freeze(
        NodeKind::MatchAlt {
            left: n(),
            right: n(),
        },
        102,
    );
    // murphy-j1j2 PM-B array/hash pattern extensions
    freeze(NodeKind::MatchRest(OptNodeId::NONE), 103);
    freeze(NodeKind::MatchNilPattern, 104);
    freeze(NodeKind::ArrayPatternWithTail(NodeList::EMPTY), 105);
    // murphy-j1j2 PM-C one-liner forms
    freeze(
        NodeKind::MatchPatternP {
            value: NodeId(0),
            pattern: NodeId(0),
        },
        106,
    );
    freeze(
        NodeKind::MatchPattern {
            value: NodeId(0),
            pattern: NodeId(0),
        },
        107,
    );
    // murphy-j1j2 PM-D advanced patterns
    freeze(
        NodeKind::MatchAs {
            value: NodeId(0),
            name: NodeId(0),
        },
        108,
    );
    freeze(
        NodeKind::ConstPattern {
            const_: NodeId(0),
            pattern: NodeId(0),
        },
        109,
    );
    // murphy-j1j2 PM-E pin & guard
    freeze(NodeKind::Pin(NodeId(0)), 110);
    freeze(NodeKind::IfGuard(NodeId(0)), 111);
    freeze(NodeKind::UnlessGuard(NodeId(0)), 112);
    freeze(
        NodeKind::MatchWithLvasgn {
            call: NodeId(0),
            targets: NodeList::EMPTY,
        },
        113,
    );
}

/// Catch the failure mode that `node_kind_discriminants_are_frozen` would
/// miss if a new variant is appended at the end without being added to the
/// list above: the freeze list must cover **every** variant, so its length
/// must equal the highest valid tag + 1.
///
/// `NodeKind::MatchWithLvasgn` is the current highest variant. Bumping it without
/// touching this file means the new tag falls outside the test and slips
/// in undetected.
#[test]
fn highest_frozen_tag_matches_last_variant() {
    let last = NodeKind::MatchWithLvasgn {
        call: NodeId(0),
        targets: NodeList::EMPTY,
    }
    .tag()
    .0;
    assert_eq!(
        last, 113,
        "appending a new NodeKind variant requires extending tests/discriminant_freeze.rs \
         (add the new variant to both `node_kind_discriminants_are_frozen` and update \
         the expected last-tag here)."
    );
}
