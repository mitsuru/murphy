//! Pattern-name ↔ `NodeKindTag` resolution for `murphy-pattern`.
//!
//! `NodeKindTag` is the `u8` discriminant of a [`NodeKind`](crate::NodeKind)
//! variant (declaration order, frozen — see ADR 0037). The
//! `KIND_PATTERN_NAMES` table maps the snake_case node-type name a pattern
//! author writes (`send`, `lvasgn`, …) to that tag.

/// The `u8` discriminant of a [`NodeKind`](crate::NodeKind) variant.
///
/// `NodeKind` is `#[repr(C, u8)]` (ADR 0037, frozen layout), so its
/// first byte is the discriminant; [`NodeKindTag::of`] reads it.
///
/// Originally `murphy-plugin-api` shipped its own private copy of this
/// struct so the two crates could be merged in parallel (see the
/// `#[plugin-api]` `node_cop.rs` Task 8 note in the murphy-9cr.17 plan).
/// As of murphy-a70 the plugin-api re-exports this type instead, so
/// `murphy_plugin_api::NodeKindTag` and `murphy_ast::NodeKindTag` are
/// the same nominal type — required so that `def_node_matcher!`-generated
/// matchers (which compare `cx.kind(node).tag()` against a literal
/// tag) type-check across crates.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKindTag(pub u8);

impl NodeKindTag {
    /// The tag of a node kind. Reads the first byte of `kind`, which is
    /// the `#[repr(C, u8)]` discriminant (ADR 0037 frozen layout).
    pub fn of(kind: &crate::NodeKind) -> NodeKindTag {
        // Safety: the pointer has valid provenance from the `&NodeKind`
        // reference; `u8` has alignment 1 so the read cannot be misaligned.
        // `NodeKind` is `#[repr(C, u8)]` (ADR 0037 — frozen layout), so
        // its first byte is the discriminant.
        NodeKindTag(unsafe { *(kind as *const crate::NodeKind as *const u8) })
    }
}

/// Pattern-name → tag. Declaration order of `NodeKind` minus `Error` and
/// `Unknown` (you cannot meaningfully match an error or fallback node in a
/// pattern). Keep in sync with the `NodeKind` enum in `node.rs`; the
/// `table_matches_tag` test guards this.
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
    // tag 37 = `Unknown` — excluded (fallback sentinel, not matchable).
    ("gvasgn", 38),
    ("cvasgn", 39),
    ("optarg", 40),
    ("restarg", 41),
    ("kwarg", 42),
    ("kwoptarg", 43),
    ("kwrestarg", 44),
    ("blockarg", 45),
    ("kwsplat", 46),
    ("while", 47),
    ("until", 48),
    // `RangeExpr` collapses inclusive/exclusive ranges into one variant; the
    // parser gem splits them as `irange`/`erange`. `range` signals the merge.
    ("range", 49),
    ("sclass", 50),
    ("break", 51),
    ("next", 52),
    ("yield", 53),
    ("super", 54),
    ("zsuper", 55),
    // parser gem uses `defined?` with the `?`; `defined` keeps table style.
    ("defined", 56),
    ("rescue", 57),
    ("resbody", 58),
    ("ensure", 59),
    ("op_asgn", 60),
    ("or_asgn", 61),
    ("and_asgn", 62),
    ("dstr", 63),
    ("dsym", 64),
    ("xstr", 65),
    ("regexp", 66),
    ("masgn", 67),
    ("mlhs", 68),
    // ── murphy-w5ba HIGH-priority extensions (parser-only; subject-side
    // murphy-translate support lands per node kind as cops actually need it).
    // See docs/superpowers/specs/2026-05-29-rubocop-pattern-gap-survey.md.
    ("for", 69),
    ("lambda", 70),
    ("defs", 71),
    ("index", 72),
    ("indexasgn", 73),
    ("kwbegin", 74),
    ("cbase", 75),
    ("regopt", 76),
    ("rational", 77),
    ("complex", 78),
    ("not", 79),
    ("retry", 80),
    ("redo", 81),
    ("numblock", 82),
    ("procarg0", 83),
    ("forward_args", 84),
    ("forwarded_args", 85),
    // ── murphy-o57f MID-priority extensions (parser-only; subject-side
    // murphy-translate support lands per cop as actually needed). See
    // docs/superpowers/specs/2026-05-29-rubocop-pattern-gap-survey.md.
    ("case_match", 86),
    ("in_pattern", 87),
    ("array_pattern", 88),
    ("hash_pattern", 89),
    ("match_var", 90),
    ("itblock", 91),
    // ── murphy-s4b4 LOW-priority extensions (parser-only; subject-side
    // murphy-translate support lands per cop as actually needed).
    ("alias", 92),
    ("undef", 93),
    ("preexe", 94),
    ("postexe", 95),
    ("back_ref", 96),
    ("nth_ref", 97),
    ("shadowarg", 98),
    ("kwnilarg", 99),
    ("blocknilarg", 100),
    // murphy-jw5t pattern-match lowering extensions
    ("find_pattern", 101),
    ("match_alt", 102),
    // murphy-j1j2 PM-B array/hash pattern extensions
    ("match_rest", 103),
    ("match_nil_pattern", 104),
    ("array_pattern_with_tail", 105),
    // murphy-j1j2 PM-C one-liner pattern matching
    ("match_pattern_p", 106),
    ("match_pattern", 107),
];

/// RuboCop-compatible type-name aliases for APIs that accept parser node
/// families rather than exactly one node kind.
pub const GROUP_FOR_TYPE: &[(&str, &[NodeKindTag])] = &[
    ("nil", &[NodeKindTag(1)]),
    ("true", &[NodeKindTag(2)]),
    ("false", &[NodeKindTag(3)]),
    ("self", &[NodeKindTag(4)]),
    ("int", &[NodeKindTag(5)]),
    ("float", &[NodeKindTag(6)]),
    ("str", &[NodeKindTag(7)]),
    ("sym", &[NodeKindTag(8)]),
    ("lvar", &[NodeKindTag(9)]),
    ("ivar", &[NodeKindTag(10)]),
    ("cvar", &[NodeKindTag(11)]),
    ("gvar", &[NodeKindTag(12)]),
    ("const", &[NodeKindTag(13)]),
    ("lvasgn", &[NodeKindTag(14)]),
    ("ivasgn", &[NodeKindTag(15)]),
    ("casgn", &[NodeKindTag(16)]),
    ("send", &[NodeKindTag(17)]),
    ("csend", &[NodeKindTag(18)]),
    ("block", &[NodeKindTag(19)]),
    ("block_pass", &[NodeKindTag(20)]),
    ("splat", &[NodeKindTag(21)]),
    ("array", &[NodeKindTag(22)]),
    ("hash", &[NodeKindTag(23)]),
    ("pair", &[NodeKindTag(24)]),
    ("if", &[NodeKindTag(25)]),
    ("case", &[NodeKindTag(26)]),
    ("when", &[NodeKindTag(27)]),
    ("begin", &[NodeKindTag(28)]),
    ("return", &[NodeKindTag(29)]),
    ("and", &[NodeKindTag(30)]),
    ("or", &[NodeKindTag(31)]),
    ("def", &[NodeKindTag(32)]),
    ("class", &[NodeKindTag(33)]),
    ("module", &[NodeKindTag(34)]),
    ("args", &[NodeKindTag(35)]),
    ("arg", &[NodeKindTag(36)]),
    ("gvasgn", &[NodeKindTag(38)]),
    ("cvasgn", &[NodeKindTag(39)]),
    ("optarg", &[NodeKindTag(40)]),
    ("restarg", &[NodeKindTag(41)]),
    ("kwarg", &[NodeKindTag(42)]),
    ("kwoptarg", &[NodeKindTag(43)]),
    ("kwrestarg", &[NodeKindTag(44)]),
    ("blockarg", &[NodeKindTag(45)]),
    ("kwsplat", &[NodeKindTag(46)]),
    ("while", &[NodeKindTag(47)]),
    ("until", &[NodeKindTag(48)]),
    ("range", &[NodeKindTag(49)]),
    ("sclass", &[NodeKindTag(50)]),
    ("break", &[NodeKindTag(51)]),
    ("next", &[NodeKindTag(52)]),
    ("yield", &[NodeKindTag(53)]),
    ("super", &[NodeKindTag(54)]),
    ("zsuper", &[NodeKindTag(55)]),
    ("defined", &[NodeKindTag(56)]),
    ("rescue", &[NodeKindTag(57)]),
    ("resbody", &[NodeKindTag(58)]),
    ("ensure", &[NodeKindTag(59)]),
    ("op_asgn", &[NodeKindTag(60)]),
    ("or_asgn", &[NodeKindTag(61)]),
    ("and_asgn", &[NodeKindTag(62)]),
    ("dstr", &[NodeKindTag(63)]),
    ("dsym", &[NodeKindTag(64)]),
    ("xstr", &[NodeKindTag(65)]),
    ("regexp", &[NodeKindTag(66)]),
    ("masgn", &[NodeKindTag(67)]),
    ("mlhs", &[NodeKindTag(68)]),
    ("for", &[NodeKindTag(69)]),
    ("lambda", &[NodeKindTag(70)]),
    ("defs", &[NodeKindTag(71)]),
    ("index", &[NodeKindTag(72)]),
    ("indexasgn", &[NodeKindTag(73)]),
    ("kwbegin", &[NodeKindTag(74)]),
    ("cbase", &[NodeKindTag(75)]),
    ("regopt", &[NodeKindTag(76)]),
    ("rational", &[NodeKindTag(77)]),
    ("complex", &[NodeKindTag(78)]),
    ("not", &[NodeKindTag(79)]),
    ("retry", &[NodeKindTag(80)]),
    ("redo", &[NodeKindTag(81)]),
    ("numblock", &[NodeKindTag(82)]),
    ("procarg0", &[NodeKindTag(83)]),
    ("forward_args", &[NodeKindTag(84)]),
    ("forwarded_args", &[NodeKindTag(85)]),
    ("case_match", &[NodeKindTag(86)]),
    ("in_pattern", &[NodeKindTag(87)]),
    ("array_pattern", &[NodeKindTag(88)]),
    ("hash_pattern", &[NodeKindTag(89)]),
    ("match_var", &[NodeKindTag(90)]),
    ("itblock", &[NodeKindTag(91)]),
    ("alias", &[NodeKindTag(92)]),
    ("undef", &[NodeKindTag(93)]),
    ("preexe", &[NodeKindTag(94)]),
    ("postexe", &[NodeKindTag(95)]),
    ("back_ref", &[NodeKindTag(96)]),
    ("nth_ref", &[NodeKindTag(97)]),
    ("shadowarg", &[NodeKindTag(98)]),
    ("kwnilarg", &[NodeKindTag(99)]),
    ("blocknilarg", &[NodeKindTag(100)]),
    // murphy-jw5t pattern-match lowering extensions
    ("find_pattern", &[NodeKindTag(101)]),
    ("match_alt", &[NodeKindTag(102)]),
    ("call", &[NodeKindTag(17), NodeKindTag(18)]),
    (
        "any_block",
        &[NodeKindTag(19), NodeKindTag(82), NodeKindTag(91)],
    ),
    (
        "numeric",
        &[
            NodeKindTag(5),
            NodeKindTag(6),
            NodeKindTag(77),
            NodeKindTag(78),
        ],
    ),
    (
        "any_str",
        &[NodeKindTag(7), NodeKindTag(63), NodeKindTag(65)],
    ),
];

/// Resolve a pattern node-type name to its tag. `None` for unknown names.
pub fn tag_from_pattern_name(name: &str) -> Option<NodeKindTag> {
    KIND_PATTERN_NAMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| NodeKindTag(*t))
}

/// Resolve a pattern node-type name or alias group to one or more tags.
pub fn tags_for_type_name(name: &str) -> &'static [NodeKindTag] {
    GROUP_FOR_TYPE
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, tags)| *tags)
        .unwrap_or(&[])
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
                receiver: OptNodeId::NONE,
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
            NodeKind::Unknown,
            NodeKind::Gvasgn {
                name: s,
                value: OptNodeId::NONE,
            },
            NodeKind::Cvasgn {
                name: s,
                value: OptNodeId::NONE,
            },
            NodeKind::Optarg {
                name: s,
                default: n,
            },
            NodeKind::Restarg(s),
            NodeKind::Kwarg(s),
            NodeKind::Kwoptarg {
                name: s,
                default: n,
            },
            NodeKind::Kwrestarg(s),
            NodeKind::Blockarg(s),
            NodeKind::Kwsplat(OptNodeId::NONE),
            NodeKind::While {
                cond: n,
                body: OptNodeId::NONE,
                post: false,
            },
            NodeKind::Until {
                cond: n,
                body: OptNodeId::NONE,
                post: false,
            },
            NodeKind::RangeExpr {
                begin_: OptNodeId::NONE,
                end_: OptNodeId::NONE,
                exclusive: false,
            },
            NodeKind::Sclass {
                expr: n,
                body: OptNodeId::NONE,
            },
            NodeKind::Break(OptNodeId::NONE),
            NodeKind::Next(OptNodeId::NONE),
            NodeKind::Yield(NodeList::EMPTY),
            NodeKind::Super(NodeList::EMPTY),
            NodeKind::Zsuper,
            NodeKind::Defined(n),
            NodeKind::Rescue {
                body: OptNodeId::NONE,
                resbodies: NodeList::EMPTY,
                else_: OptNodeId::NONE,
            },
            NodeKind::Resbody {
                exceptions: NodeList::EMPTY,
                var: OptNodeId::NONE,
                body: OptNodeId::NONE,
            },
            NodeKind::Ensure {
                body: OptNodeId::NONE,
                ensure_: OptNodeId::NONE,
            },
            NodeKind::OpAsgn {
                target: n,
                op: s,
                value: n,
            },
            NodeKind::OrAsgn {
                target: n,
                value: n,
            },
            NodeKind::AndAsgn {
                target: n,
                value: n,
            },
            NodeKind::Dstr(NodeList::EMPTY),
            NodeKind::Dsym(NodeList::EMPTY),
            NodeKind::Xstr(NodeList::EMPTY),
            NodeKind::Regexp {
                parts: NodeList::EMPTY,
                opts: s,
            },
            NodeKind::Masgn { lhs: n, rhs: n },
            NodeKind::Mlhs(NodeList::EMPTY),
            // murphy-w5ba HIGH-priority extensions
            NodeKind::For {
                var: n,
                iter: n,
                body: OptNodeId::NONE,
            },
            NodeKind::Lambda,
            NodeKind::Defs {
                receiver: n,
                name: s,
                args: n,
                body: OptNodeId::NONE,
            },
            NodeKind::Index {
                receiver: n,
                args: NodeList::EMPTY,
            },
            NodeKind::IndexAsgn {
                receiver: n,
                args: NodeList::EMPTY,
                value: n,
            },
            NodeKind::Kwbegin(NodeList::EMPTY),
            NodeKind::Cbase,
            NodeKind::Regopt(s),
            NodeKind::Rational(StringId(0)),
            NodeKind::Complex(StringId(0)),
            NodeKind::Not(n),
            NodeKind::Retry,
            NodeKind::Redo,
            NodeKind::Numblock {
                send: n,
                max_n: 0,
                body: OptNodeId::NONE,
            },
            NodeKind::Procarg0(NodeList::EMPTY),
            NodeKind::ForwardArgs,
            NodeKind::ForwardedArgs,
            // murphy-o57f MID-priority extensions
            NodeKind::CaseMatch {
                subject: n,
                in_patterns: NodeList::EMPTY,
                else_body: OptNodeId::NONE,
            },
            NodeKind::InPattern {
                pattern: n,
                guard: OptNodeId::NONE,
                body: OptNodeId::NONE,
            },
            NodeKind::ArrayPattern(NodeList::EMPTY),
            NodeKind::HashPattern(NodeList::EMPTY),
            NodeKind::MatchVar(s),
            NodeKind::Itblock {
                send: n,
                body: OptNodeId::NONE,
            },
            // murphy-s4b4 LOW-priority extensions
            NodeKind::Alias {
                new_name: n,
                old_name: n,
            },
            NodeKind::Undef(NodeList::EMPTY),
            NodeKind::Preexe(OptNodeId::NONE),
            NodeKind::Postexe(OptNodeId::NONE),
            NodeKind::BackRef(s),
            NodeKind::NthRef(0),
            NodeKind::Shadowarg(s),
            NodeKind::Kwnilarg,
            NodeKind::Blocknilarg,
            // murphy-jw5t pattern-match lowering extensions (tags 101, 102)
            NodeKind::FindPattern(NodeList::EMPTY),
            NodeKind::MatchAlt { left: n, right: n },
            // murphy-j1j2 PM-B array/hash pattern extensions (tags 103-105)
            NodeKind::MatchRest(OptNodeId::NONE),
            NodeKind::MatchNilPattern,
            NodeKind::ArrayPatternWithTail(NodeList::EMPTY),
            // murphy-j1j2 PM-C one-liner pattern matching (tags 106-107)
            NodeKind::MatchPatternP { value: n, pattern: n },
            NodeKind::MatchPattern { value: n, pattern: n },
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
        // every variant except Error (tag 0) and Unknown (tag 37) has
        // exactly one table entry — both are fallback/sentinel kinds that
        // cannot be matched in a pattern.
        let variants = all_variants();
        for (name, tag) in KIND_PATTERN_NAMES {
            assert_eq!(variants[*tag as usize].tag().0, *tag, "table entry {name}");
        }
        let error_tag = NodeKind::Error.tag().0;
        let unknown_tag = NodeKind::Unknown.tag().0;
        for k in &variants {
            let t = k.tag();
            if t.0 == error_tag || t.0 == unknown_tag {
                assert!(
                    pattern_name(t).is_none(),
                    "Error/Unknown must have no pattern name, got {k:?}"
                );
            } else {
                assert!(pattern_name(t).is_some(), "missing table entry for {k:?}");
            }
        }
    }

    /// The expected pattern name for each variant — an independent,
    /// exhaustive cross-check of `KIND_PATTERN_NAMES`. The `match` is
    /// exhaustive so a new `NodeKind` variant forces an update here too.
    fn expected_pattern_name(k: &NodeKind) -> Option<&'static str> {
        Some(match k {
            NodeKind::Error => return None,
            NodeKind::Nil => "nil",
            NodeKind::True_ => "true",
            NodeKind::False_ => "false",
            NodeKind::SelfExpr => "self",
            NodeKind::Int(_) => "int",
            NodeKind::Float(_) => "float",
            NodeKind::Str(_) => "str",
            NodeKind::Sym(_) => "sym",
            NodeKind::Lvar(_) => "lvar",
            NodeKind::Ivar(_) => "ivar",
            NodeKind::Cvar(_) => "cvar",
            NodeKind::Gvar(_) => "gvar",
            NodeKind::Const { .. } => "const",
            NodeKind::Lvasgn { .. } => "lvasgn",
            NodeKind::Ivasgn { .. } => "ivasgn",
            NodeKind::Casgn { .. } => "casgn",
            NodeKind::Send { .. } => "send",
            NodeKind::Csend { .. } => "csend",
            NodeKind::Block { .. } => "block",
            NodeKind::BlockPass(_) => "block_pass",
            NodeKind::Splat(_) => "splat",
            NodeKind::Array(_) => "array",
            NodeKind::Hash(_) => "hash",
            NodeKind::Pair { .. } => "pair",
            NodeKind::If { .. } => "if",
            NodeKind::Case { .. } => "case",
            NodeKind::When { .. } => "when",
            NodeKind::Begin(_) => "begin",
            NodeKind::Return(_) => "return",
            NodeKind::And { .. } => "and",
            NodeKind::Or { .. } => "or",
            NodeKind::Def { .. } => "def",
            NodeKind::Class { .. } => "class",
            NodeKind::Module { .. } => "module",
            NodeKind::Args(_) => "args",
            NodeKind::Arg(_) => "arg",
            NodeKind::Unknown => return None,
            NodeKind::Gvasgn { .. } => "gvasgn",
            NodeKind::Cvasgn { .. } => "cvasgn",
            NodeKind::Optarg { .. } => "optarg",
            NodeKind::Restarg(_) => "restarg",
            NodeKind::Kwarg(_) => "kwarg",
            NodeKind::Kwoptarg { .. } => "kwoptarg",
            NodeKind::Kwrestarg(_) => "kwrestarg",
            NodeKind::Blockarg(_) => "blockarg",
            NodeKind::Kwsplat(_) => "kwsplat",
            NodeKind::While { .. } => "while",
            NodeKind::Until { .. } => "until",
            NodeKind::RangeExpr { .. } => "range",
            NodeKind::Sclass { .. } => "sclass",
            NodeKind::Break(_) => "break",
            NodeKind::Next(_) => "next",
            NodeKind::Yield(_) => "yield",
            NodeKind::Super(_) => "super",
            NodeKind::Zsuper => "zsuper",
            NodeKind::Defined(_) => "defined",
            NodeKind::Rescue { .. } => "rescue",
            NodeKind::Resbody { .. } => "resbody",
            NodeKind::Ensure { .. } => "ensure",
            NodeKind::OpAsgn { .. } => "op_asgn",
            NodeKind::OrAsgn { .. } => "or_asgn",
            NodeKind::AndAsgn { .. } => "and_asgn",
            NodeKind::Dstr(_) => "dstr",
            NodeKind::Dsym(_) => "dsym",
            NodeKind::Xstr(_) => "xstr",
            NodeKind::Regexp { .. } => "regexp",
            NodeKind::Masgn { .. } => "masgn",
            NodeKind::Mlhs(_) => "mlhs",
            // murphy-w5ba HIGH-priority extensions
            NodeKind::For { .. } => "for",
            NodeKind::Lambda => "lambda",
            NodeKind::Defs { .. } => "defs",
            NodeKind::Index { .. } => "index",
            NodeKind::IndexAsgn { .. } => "indexasgn",
            NodeKind::Kwbegin(_) => "kwbegin",
            NodeKind::Cbase => "cbase",
            NodeKind::Regopt(_) => "regopt",
            NodeKind::Rational(_) => "rational",
            NodeKind::Complex(_) => "complex",
            NodeKind::Not(_) => "not",
            NodeKind::Retry => "retry",
            NodeKind::Redo => "redo",
            NodeKind::Numblock { .. } => "numblock",
            NodeKind::Procarg0(_) => "procarg0",
            NodeKind::ForwardArgs => "forward_args",
            NodeKind::ForwardedArgs => "forwarded_args",
            // murphy-o57f MID-priority extensions
            NodeKind::CaseMatch { .. } => "case_match",
            NodeKind::InPattern { .. } => "in_pattern",
            NodeKind::ArrayPattern(_) => "array_pattern",
            NodeKind::HashPattern(_) => "hash_pattern",
            NodeKind::MatchVar(_) => "match_var",
            NodeKind::Itblock { .. } => "itblock",
            // murphy-s4b4 LOW-priority extensions
            NodeKind::Alias { .. } => "alias",
            NodeKind::Undef(_) => "undef",
            NodeKind::Preexe(_) => "preexe",
            NodeKind::Postexe(_) => "postexe",
            NodeKind::BackRef(_) => "back_ref",
            NodeKind::NthRef(_) => "nth_ref",
            NodeKind::Shadowarg(_) => "shadowarg",
            NodeKind::Kwnilarg => "kwnilarg",
            NodeKind::Blocknilarg => "blocknilarg",
            // murphy-jw5t pattern-match lowering extensions
            NodeKind::FindPattern(_) => "find_pattern",
            NodeKind::MatchAlt { .. } => "match_alt",
            // murphy-j1j2 PM-B array/hash pattern extensions
            NodeKind::MatchRest(_) => "match_rest",
            NodeKind::MatchNilPattern => "match_nil_pattern",
            NodeKind::ArrayPatternWithTail(_) => "array_pattern_with_tail",
            // murphy-j1j2 PM-C one-liner pattern matching
            NodeKind::MatchPatternP { .. } => "match_pattern_p",
            NodeKind::MatchPattern { .. } => "match_pattern",
        })
    }

    #[test]
    fn table_name_matches_variant() {
        // Catches a MISLABELED table entry (e.g. lvar/ivar swapped), which
        // `table_matches_tag` alone does not detect.
        for k in all_variants() {
            assert_eq!(pattern_name(k.tag()), expected_pattern_name(&k), "{k:?}");
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
    fn tags_for_type_name_resolves_alias_groups_and_pattern_names() {
        assert_eq!(tags_for_type_name("range"), &[NodeKindTag(49)]);
        assert_eq!(
            tags_for_type_name("call"),
            &[NodeKindTag(17), NodeKindTag(18)]
        );
        assert_eq!(
            tags_for_type_name("any_block"),
            &[NodeKindTag(19), NodeKindTag(82), NodeKindTag(91)]
        );
        assert_eq!(
            tags_for_type_name("numeric"),
            &[
                NodeKindTag(5),
                NodeKindTag(6),
                NodeKindTag(77),
                NodeKindTag(78)
            ]
        );
        assert_eq!(
            tags_for_type_name("any_str"),
            &[NodeKindTag(7), NodeKindTag(63), NodeKindTag(65)]
        );
        assert_eq!(tags_for_type_name("send"), &[NodeKindTag(17)]);
        assert_eq!(tags_for_type_name("sned"), &[]);
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
