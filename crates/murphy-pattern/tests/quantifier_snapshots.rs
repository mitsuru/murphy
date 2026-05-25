//! Quantifier parse-shape snapshots (murphy-ycx, PR #1).
//!
//! Pins the `PatternAst` shape for each DESIGN-blessed quantifier source,
//! plus capture-slot upgrades and the five canonical parse-error cases.
//! These cover the same surfaces as the parser unit tests, but here we
//! assert the *full* tree shape — so a future lowering / IR refactor that
//! accidentally restructures the AST is loud at the parse boundary.

use murphy_pattern::{CaptureKind, Pat, PatKind, parse};

/// Walk into a top-level `(...)` and return its children, panicking with the
/// source on the way down if the shape is not a `Node`.
fn children_of(src: &str) -> Vec<Pat> {
    let p = parse(src).unwrap_or_else(|e| panic!("parse `{src}`: {e:?}"));
    match p.root.kind {
        PatKind::Node { children, .. } => children,
        other => panic!("`{src}` should be a Node, got {other:?}"),
    }
}

// =====================================================================
// (A) Positive snapshots — DESIGN's acceptance examples
// =====================================================================

#[test]
fn snapshot_array_int_plus_one_or_more_int_children() {
    // `(array int+)` — the one child is a Quantifier wrapping a bare-kind
    // body (`Kind(int)`) with `min=1, max=u8::MAX`.
    let cs = children_of("(array int+)");
    assert_eq!(cs.len(), 1);
    match &cs[0].kind {
        PatKind::Quantifier { body, min, max } => {
            assert_eq!(*min, 1);
            assert_eq!(*max, u8::MAX);
            assert!(
                matches!(body.kind, PatKind::Kind(_)),
                "body should be a bare Kind, was {:?}",
                body.kind
            );
        }
        other => panic!("expected Quantifier, got {other:?}"),
    }
}

#[test]
fn snapshot_send_foo_int_star_str_zero_or_more_then_one() {
    // `(send _ :foo int* str)` — three children: wildcard, sym lit,
    // Quantifier(Kind(int), *), Kind(str).
    let cs = children_of("(send _ :foo int* str)");
    assert_eq!(cs.len(), 4);
    assert!(matches!(cs[0].kind, PatKind::Wildcard));
    assert!(matches!(cs[1].kind, PatKind::Lit(_)));
    match &cs[2].kind {
        PatKind::Quantifier { min, max, .. } => {
            assert_eq!(*min, 0);
            assert_eq!(*max, u8::MAX);
        }
        other => panic!("expected Quantifier(*), got {other:?}"),
    }
    assert!(matches!(cs[3].kind, PatKind::Kind(_)));
}

#[test]
fn snapshot_pluck_sym_plus_one_or_more_sym() {
    // `(send _ :pluck sym+)` — Quantifier on `Kind(sym)`.
    let cs = children_of("(send _ :pluck sym+)");
    match &cs[2].kind {
        PatKind::Quantifier { min, .. } => assert_eq!(*min, 1),
        other => panic!("expected Quantifier(+), got {other:?}"),
    }
}

#[test]
fn snapshot_update_columns_hash_question_optional_one_hash() {
    // `(send _ :update_columns hash?)` — Quantifier(Kind(hash), 0..=1).
    let cs = children_of("(send _ :update_columns hash?)");
    match &cs[2].kind {
        PatKind::Quantifier { min, max, .. } => {
            assert_eq!(*min, 0);
            assert_eq!(*max, 1);
        }
        other => panic!("expected Quantifier(?), got {other:?}"),
    }
}

#[test]
fn snapshot_rest_and_quantifier_coexist_in_same_child_list() {
    // `(send _ :foo ... int+)` — DESIGN mixing rule: at most one rest, but
    // quantifiers may appear alongside it.
    let cs = children_of("(send _ :foo ... int+)");
    assert!(matches!(cs[2].kind, PatKind::Rest));
    assert!(matches!(cs[3].kind, PatKind::Quantifier { .. }));
}

// =====================================================================
// (B) Capture-slot upgrades — `$pat+/*` -> Seq, `$pat?` -> OptNode
// =====================================================================

#[test]
fn snapshot_dollar_int_plus_upgrades_slot_to_seq() {
    // `(array $int+)` — anonymous capture whose body is `Quantifier(int, +)`;
    // slot kind becomes `Seq` so the matcher returns a slice.
    let p = parse("(array $int+)").expect("parse ok");
    assert_eq!(p.capture_kinds(), &[CaptureKind::Seq]);
}

#[test]
fn snapshot_dollar_int_star_upgrades_slot_to_seq() {
    let p = parse("(array $int*)").expect("parse ok");
    assert_eq!(p.capture_kinds(), &[CaptureKind::Seq]);
}

#[test]
fn snapshot_dollar_int_question_upgrades_slot_to_optnode() {
    // `(send _ :update_columns $hash?)` — `?` produces `OptNode`.
    let p = parse("(send _ :update_columns $hash?)").expect("parse ok");
    assert_eq!(p.capture_kinds(), &[CaptureKind::OptNode]);
}

#[test]
fn snapshot_seq_capture_via_ellipsis_still_works() {
    // Regression: the existing `$...` -> Seq path is unchanged by the
    // body-shape based upgrade logic.
    let p = parse("(send nil :puts $...)").expect("parse ok");
    assert_eq!(p.capture_kinds(), &[CaptureKind::Seq]);
}

#[test]
fn snapshot_named_capture_without_postfix_is_node_slot() {
    // Regression: `$ident` with no trailing postfix is still a named
    // capture (body = Wildcard, slot = Node).
    let p = parse("(send $receiver _)").expect("parse ok");
    assert_eq!(p.capture_kinds(), &[CaptureKind::Node]);
}

// =====================================================================
// (C) The five canonical parse failures
// =====================================================================

#[test]
fn snapshot_error_quantifier_at_top_level() {
    // `int+` — quantifier outside a node child list.
    let e = parse("int+").unwrap_err();
    assert!(e.message.contains("direct child of a node match"));
}

#[test]
fn snapshot_error_quantifier_in_union_arm() {
    // `{int+ sym}` — union arms are not node child lists.
    let e = parse("{int+ sym}").unwrap_err();
    assert!(e.message.contains("direct child of a node match"));
}

#[test]
fn snapshot_error_quantifier_under_sigils() {
    // `!int+` / `^int+` / `` `int+ `` — sigil bodies are not node child lists.
    for src in ["!int+", "^int+", "`int+"] {
        let e = parse(src).unwrap_err();
        assert!(
            e.message.contains("direct child of a node match"),
            "`{src}` message: {}",
            e.message
        );
    }
}

#[test]
fn snapshot_error_chained_postfix() {
    // `int++` and `int*?` — only one postfix per pattern.
    for src in ["(array int++)", "(array int*?)"] {
        let e = parse(src).unwrap_err();
        assert!(
            e.message.contains("chained") || e.message.contains("at most one"),
            "`{src}` message: {}",
            e.message
        );
    }
}

#[test]
fn snapshot_error_capture_inside_quantifier_body() {
    // `(array (send _ $_)+)` — `$` inside a quantifier body.
    let e = parse("(array (send _ $_)+)").unwrap_err();
    assert!(e.message.contains("quantifier body"));
}
