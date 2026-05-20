//! Table-driven idempotency and conflict harness for `apply_edits` /
//! `apply_edits_logged` (murphy-hwe.3 harness; body landed in murphy-hwe.4;
//! design §5/§6/§7).
//!
//! ## Purpose
//!
//! CLAUDE.md mandate: pin the idempotency PROPERTY before writing apply logic.
//! This file is the TDD harness. The apply body (`murphy-hwe.4`) is not here
//! yet — tests that exercise non-empty apply are `#[ignore]`'d so they compile
//! (proving API compatibility) but don't run until `.4` un-ignores them.
//!
//! ## What is tested NOW (murphy-hwe.3, green)
//!
//! * **Empty-edits identity** — `apply_edits(any_src, &[]) == any_src`.
//!   This property holds trivially for all sources, including already-corrected
//!   ones.  It is the "weakly-pinnable form" from DECISION 4: we can assert
//!   `apply_edits(expected_corrected, &[]) == expected_corrected` today, which
//!   is the vacuous half of "re-running on corrected source yields no change".
//!
//! ## What was `#[ignore]`'d until murphy-hwe.4 (now green)
//!
//! * `apply_matches_expected` — verifies `apply_edits(input, edits) == expected`.
//!   This requires the descending-offset algorithm; un-ignored in `.4`.
//!
//! ## What is deferred to murphy-hwe.5
//!
//! True idempotency requires the **re-derive-from-corrected-source** form:
//! run all cops on `apply_edits(input, edits)`, get zero new edits, repeat
//! until stable.  That needs the reparse loop (`.5`) and is NOT encoded here.
//!
//! Note: the "same-edit-set twice" form does NOT hold in general (applying
//! the same byte-range edits to already-shifted source corrupts it), so it is
//! deliberately NOT in this harness.  murphy-hwe.5's reparse loop encodes the
//! correct "re-derive edits from corrected source → zero new edits" property.
//!
//! ## Fixture table
//!
//! Each [`Case`] row has:
//! - `name`: human-readable label (surfaced in every assertion failure message).
//! - `input`: Ruby source before correction.
//! - `edits`: the `Vec<Edit>` a cop would produce.
//! - `expected`: the corrected source after applying `edits` to `input`.
//!
//! Representative coverage:
//! - Single replace (one byte-range substitution).
//! - Multi-region replace (two non-overlapping byte-range substitutions).
//! - Delete (replacement is the empty string — shrinks source).

use murphy_core::{ConflictReason, Edit, Range, apply_edits, apply_edits_logged};

/// One fixture row.
struct Case {
    /// Human-readable row label.
    name: &'static str,
    /// Source before correction.
    input: &'static str,
    /// Edits a cop would emit for `input`.
    edits: Vec<Edit>,
    /// Expected corrected source after applying `edits` to `input`.
    expected: &'static str,
}

/// Build the canonical fixture table used by all test functions.
///
/// Fixtures are defined once here to avoid duplication across test functions.
fn cases() -> Vec<Case> {
    vec![
        // Single replace: `puts "hello"` → `logger.info "hello"`.
        // `puts` spans bytes [0, 4) (4 bytes).
        Case {
            name: "single replace — puts → logger.info",
            input: "puts \"hello\"\n",
            edits: vec![Edit {
                range: Range {
                    start_offset: 0,
                    end_offset: 4,
                },
                replacement: "logger.info".into(),
            }],
            expected: "logger.info \"hello\"\n",
        },
        // Multi-region replace: two non-overlapping substitutions.
        // Source: `puts 1\nputs 2\n`
        //   first `puts` → bytes [0, 4); second `puts` → bytes [7, 11).
        // Edits are listed in ascending offset order here (the harness
        // documents that apply_edits must sort them into descending order
        // before applying).
        Case {
            name: "multi-region replace — two puts → logger.info",
            input: "puts 1\nputs 2\n",
            edits: vec![
                Edit {
                    range: Range {
                        start_offset: 0,
                        end_offset: 4,
                    },
                    replacement: "logger.info".into(),
                },
                Edit {
                    range: Range {
                        start_offset: 7,
                        end_offset: 11,
                    },
                    replacement: "logger.info".into(),
                },
            ],
            expected: "logger.info 1\nlogger.info 2\n",
        },
        // Delete: remove a byte range (replacement = "").
        // Source: `x = puts\n`; delete ` = puts` = bytes [1, 8) → `x\n`.
        Case {
            name: "delete — remove assignment value",
            input: "x = puts\n",
            edits: vec![Edit {
                range: Range {
                    start_offset: 1,
                    end_offset: 8,
                },
                replacement: "".into(),
            }],
            expected: "x\n",
        },
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Empty-edits identity holds for all fixture rows (no `#[ignore]` — green now).
///
/// `apply_edits(src, &[])` must return `src` verbatim for any source string,
/// whether that source is already corrected or not.  This is the green-today
/// half of the idempotency property.
#[test]
fn empty_edits_is_identity() {
    for case in &cases() {
        // Empty edits leave input unchanged.
        let input_unchanged = apply_edits(case.input, &[]);
        assert_eq!(
            input_unchanged, case.input,
            "empty-edits identity failed for case {:?}: \
             apply_edits(input, &[]) should equal input",
            case.name
        );

        // Weak idempotency form (DECISION 4): corrected source + empty edits
        // = corrected source.  This is `apply_edits(expected, &[]) == expected`.
        let re_applied = apply_edits(case.expected, &[]);
        assert_eq!(
            re_applied, case.expected,
            "empty-edits identity failed for case {:?}: \
             apply_edits(expected, &[]) should equal expected",
            case.name
        );
    }
}

/// Applying edits to `input` produces `expected` (requires descending-offset
/// apply — murphy-hwe.4).
///
/// Un-ignored in murphy-hwe.4 when the body of `apply_edits` is implemented.
#[test]
fn apply_matches_expected() {
    for case in &cases() {
        let corrected = apply_edits(case.input, &case.edits);
        assert_eq!(
            corrected, case.expected,
            "apply correctness failed for case {:?}",
            case.name
        );
    }
}

// ---------------------------------------------------------------------------
// Conflict detection tests (murphy-hwe.4: apply_edits_logged)
// ---------------------------------------------------------------------------

/// Two overlapping edits: only the one that is earlier in the stable total
/// order (highest start_offset wins first, then end_offset, then original
/// index tiebreak) is applied; the other is logged as `Overlap`.
///
/// Source: `"abcdef"` (6 bytes)
///   edit A: bytes [1,4) → "XY"  (original index 0)
///   edit B: bytes [2,5) → "ZZ"  (original index 1)
///
/// Total order (start DESC, end DESC, index ASC tiebreak):
///   B.start=2 > A.start=1 → B is first in sort order.
///   B is applied first: source "abcdef" → "ab" + "ZZ" + "f" = "abZZf".
///   Then A's range [1,4) overlaps B's [2,5): overlap = (1 < 5 && 2 < 4) = true.
///   A is dropped with reason=Overlap, conflicts_with=Some(B).
///
/// Corrected: "ab" + "ZZ" + "f" = "abZZf"
#[test]
fn overlap_later_wins_earlier_dropped() {
    let source = "abcdef";
    let edit_a = Edit {
        range: Range {
            start_offset: 1,
            end_offset: 4,
        },
        replacement: "XY".into(),
    };
    let edit_b = Edit {
        range: Range {
            start_offset: 2,
            end_offset: 5,
        },
        replacement: "ZZ".into(),
    };

    let outcome = apply_edits_logged(source, &[edit_a.clone(), edit_b.clone()]);

    // edit_b wins (higher start in total order → applied first)
    // "abcdef": [2,5) → "ZZ" gives "ab"+"ZZ"+"f" = "abZZf"
    assert_eq!(
        outcome.corrected, "abZZf",
        "overlap: edit_b (start=2) should win over edit_a (start=1)"
    );
    assert_eq!(
        outcome.conflicts.len(),
        1,
        "overlap: exactly one conflict logged"
    );
    let conflict = &outcome.conflicts[0];
    assert_eq!(conflict.dropped, edit_a, "overlap: edit_a is dropped");
    assert_eq!(
        conflict.reason,
        ConflictReason::Overlap,
        "overlap: reason is Overlap"
    );
    assert!(
        conflict.conflicts_with.is_some(),
        "overlap: conflicts_with should be Some(winner)"
    );
    assert_eq!(
        conflict.conflicts_with.as_ref().unwrap(),
        &edit_b,
        "overlap: conflicts_with should be the winning edit_b"
    );
}

/// Adjacent edits: `a.end == b.start` is NOT a conflict (half-open [start,end)).
/// Both edits must be applied and produce a clean concatenation.
///
/// Source: `"hello world"` (11 bytes)
///   edit A: bytes [0,5) → "goodbye"  (replaces "hello")
///   edit B: bytes [5,6) → "_"        (replaces " ")
///
/// Corrected: "goodbye" + "_" + "world" = "goodbye_world"
#[test]
fn adjacent_edits_both_applied() {
    let source = "hello world";
    let edit_a = Edit {
        range: Range {
            start_offset: 0,
            end_offset: 5,
        },
        replacement: "goodbye".into(),
    };
    let edit_b = Edit {
        range: Range {
            start_offset: 5,
            end_offset: 6,
        },
        replacement: "_".into(),
    };

    let outcome = apply_edits_logged(source, &[edit_a, edit_b]);

    assert_eq!(
        outcome.corrected, "goodbye_world",
        "adjacent: both edits must be applied (a.end==b.start is not overlap)"
    );
    assert!(
        outcome.conflicts.is_empty(),
        "adjacent: no conflicts expected"
    );
}

/// Out-of-bounds edit: `end_offset > source.len()` → dropped with
/// reason=OutOfBounds, `conflicts_with=None`, other edits + corrected survive.
///
/// Source: `"abc"` (3 bytes)
///   edit A (valid):      bytes [0,1) → "X"
///   edit B (out-of-bounds): bytes [1,10) → "Y"  (10 > 3)
///
/// Corrected: "Xbc"  (only A applied)
#[test]
fn out_of_bounds_edit_dropped() {
    let source = "abc";
    let edit_valid = Edit {
        range: Range {
            start_offset: 0,
            end_offset: 1,
        },
        replacement: "X".into(),
    };
    let edit_oob = Edit {
        range: Range {
            start_offset: 1,
            end_offset: 10,
        },
        replacement: "Y".into(),
    };

    let outcome = apply_edits_logged(source, &[edit_valid.clone(), edit_oob.clone()]);

    assert_eq!(
        outcome.corrected, "Xbc",
        "out-of-bounds: only the valid edit should be applied"
    );
    assert_eq!(
        outcome.conflicts.len(),
        1,
        "out-of-bounds: exactly one conflict logged"
    );
    let conflict = &outcome.conflicts[0];
    assert_eq!(
        conflict.dropped, edit_oob,
        "out-of-bounds: the OOB edit should be dropped"
    );
    assert_eq!(
        conflict.reason,
        ConflictReason::OutOfBounds,
        "out-of-bounds: reason must be OutOfBounds"
    );
    assert!(
        conflict.conflicts_with.is_none(),
        "out-of-bounds: conflicts_with must be None (not an overlap)"
    );
}

/// Out-of-bounds: `start_offset > source.len()` → dropped with OutOfBounds.
#[test]
fn out_of_bounds_start_dropped() {
    let source = "abc";
    let edit_oob = Edit {
        range: Range {
            start_offset: 10,
            end_offset: 12,
        },
        replacement: "Z".into(),
    };

    let outcome = apply_edits_logged(source, std::slice::from_ref(&edit_oob));

    assert_eq!(
        outcome.corrected, "abc",
        "out-of-bounds start: corrected should be unchanged"
    );
    assert_eq!(outcome.conflicts.len(), 1);
    assert_eq!(outcome.conflicts[0].reason, ConflictReason::OutOfBounds);
    assert!(outcome.conflicts[0].conflicts_with.is_none());
}

/// Non-char-boundary: an edit whose start/end lands inside a multibyte
/// codepoint is dropped with reason=NonCharBoundary, `conflicts_with=None`.
///
/// "日" is 3 bytes (UTF-8: E6 97 A5). An edit with start_offset=1 lands
/// inside the codepoint — not a char boundary.
///
/// Source: `"日abc"` — "日"=bytes[0,3), 'a'=3, 'b'=4, 'c'=5
///   edit A (NCB): start=1, end=3  → middle of "日"
///   edit B (valid): start=3, end=4 → replaces 'a'
///
/// Corrected: "日Xbc"  (only B applied)
#[test]
fn non_char_boundary_edit_dropped() {
    let source = "日abc";
    // "日" is at bytes [0,3); start=1 is inside the codepoint.
    let edit_ncb = Edit {
        range: Range {
            start_offset: 1,
            end_offset: 3,
        },
        replacement: "X".into(),
    };
    let edit_valid = Edit {
        range: Range {
            start_offset: 3,
            end_offset: 4,
        },
        replacement: "X".into(),
    };

    let outcome = apply_edits_logged(source, &[edit_ncb.clone(), edit_valid.clone()]);

    assert_eq!(
        outcome.corrected, "日Xbc",
        "non-char-boundary: only the valid edit should be applied"
    );
    assert_eq!(
        outcome.conflicts.len(),
        1,
        "non-char-boundary: exactly one conflict logged"
    );
    let conflict = &outcome.conflicts[0];
    assert_eq!(
        conflict.dropped, edit_ncb,
        "non-char-boundary: the NCB edit should be dropped"
    );
    assert_eq!(
        conflict.reason,
        ConflictReason::NonCharBoundary,
        "non-char-boundary: reason must be NonCharBoundary"
    );
    assert!(
        conflict.conflicts_with.is_none(),
        "non-char-boundary: conflicts_with must be None"
    );
}

/// Shuffle determinism: the same set of edits applied in different input
/// orders must produce identical `ApplyOutcome` (corrected string AND conflicts
/// log).  This exercises the stable total-order tiebreak (PIN 3).
///
/// Source: `"abcdefgh"` (8 bytes, indices: a=0,b=1,c=2,d=3,e=4,f=5,g=6,h=7)
///   edit A: [0,2) → "AA"   replaces "ab"
///   edit B: [3,6) → "BB"   replaces "def"
///   edit C: [4,7) → "CC"   replaces "efg"  (overlaps B: 3<7 && 4<6 → true)
///
/// Total sort order (start DESC, end DESC, original-index ASC tiebreak):
///   C(start=4) > B(start=3) > A(start=0)
///   → C applied first: "abcd" + "CC" + "h" = "abcdCCh"
///   → B overlaps C (3<7 && 4<6 = true) → B dropped, Overlap, conflicts_with=Some(C)
///   → A applied: "AA" + "cdCCh" = "AAcdCCh"
///
/// Corrected: "AAcdCCh"  (not "AAcCCh" — B's "d" at index 3 is NOT in C's range [4,7))
#[test]
fn shuffle_determinism() {
    let source = "abcdefgh";
    let edit_a = Edit {
        range: Range {
            start_offset: 0,
            end_offset: 2,
        },
        replacement: "AA".into(),
    };
    let edit_b = Edit {
        range: Range {
            start_offset: 3,
            end_offset: 6,
        },
        replacement: "BB".into(),
    };
    let edit_c = Edit {
        range: Range {
            start_offset: 4,
            end_offset: 7,
        },
        replacement: "CC".into(),
    };

    // All 6 permutations of [A, B, C]
    let permutations: Vec<Vec<Edit>> = vec![
        vec![edit_a.clone(), edit_b.clone(), edit_c.clone()],
        vec![edit_a.clone(), edit_c.clone(), edit_b.clone()],
        vec![edit_b.clone(), edit_a.clone(), edit_c.clone()],
        vec![edit_b.clone(), edit_c.clone(), edit_a.clone()],
        vec![edit_c.clone(), edit_a.clone(), edit_b.clone()],
        vec![edit_c.clone(), edit_b.clone(), edit_a.clone()],
    ];

    let reference = apply_edits_logged(source, &permutations[0]);
    // C applied first: [4,7)→"CC" gives "abcd"+"CC"+"h" = "abcdCCh"
    // then A applied:  [0,2)→"AA" gives "AA"+"cdCCh" = "AAcdCCh"
    assert_eq!(
        reference.corrected, "AAcdCCh",
        "shuffle: corrected output sanity check"
    );

    for (i, perm) in permutations.iter().enumerate() {
        let outcome = apply_edits_logged(source, perm);
        assert_eq!(
            outcome.corrected, reference.corrected,
            "shuffle: corrected differs for permutation {i}"
        );
        assert_eq!(
            outcome.conflicts.len(),
            reference.conflicts.len(),
            "shuffle: conflicts count differs for permutation {i}"
        );
        // Compare conflicts element-by-element (order is deterministic = sort order)
        for (j, (got, exp)) in outcome
            .conflicts
            .iter()
            .zip(reference.conflicts.iter())
            .enumerate()
        {
            assert_eq!(
                got, exp,
                "shuffle: conflicts[{j}] differs for permutation {i}"
            );
        }
    }
}

/// Zero-width insertions at the same point are NOT a conflict.
/// Half-open predicate: a.start < b.end && b.start < a.end.
/// For two zero-width at same point: start=5, end=5 for both.
/// 5 < 5 is false → no overlap.
#[test]
fn zero_width_same_point_not_conflict() {
    let source = "hello world";
    let edit_a = Edit {
        range: Range {
            start_offset: 5,
            end_offset: 5,
        },
        replacement: "---".into(),
    };
    let edit_b = Edit {
        range: Range {
            start_offset: 5,
            end_offset: 5,
        },
        replacement: "+++".into(),
    };

    let outcome = apply_edits_logged(source, &[edit_a, edit_b]);

    // No conflict (zero-width at same point → not overlap by half-open predicate)
    assert!(
        outcome.conflicts.is_empty(),
        "zero-width insertions at same point must not conflict: {:?}",
        outcome.conflicts
    );
    // Both applied: sort order is (start DESC, end DESC, idx ASC) → both have
    // same start+end, so idx tiebreak: idx=0 comes first in total order (wins
    // first apply position), idx=1 also applies.
    // Both are zero-width: corrected contains both replacements.
    assert!(
        outcome.corrected.contains("---") && outcome.corrected.contains("+++"),
        "zero-width: both insertions must appear in corrected: {:?}",
        outcome.corrected
    );
}
