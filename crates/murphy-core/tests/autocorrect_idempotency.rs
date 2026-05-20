//! Table-driven idempotency harness for `apply_edits` (murphy-hwe.3; design §7).
//!
//! ## Purpose
//!
//! CLAUDE.md mandate: pin the idempotency PROPERTY before writing apply logic.
//! This file is the TDD harness.  The apply body (`murphy-hwe.4`) is not here
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
//! ## What is `#[ignore]`'d until murphy-hwe.4
//!
//! * `apply_matches_expected` — verifies `apply_edits(input, edits) == expected`.
//!   This requires the descending-offset algorithm; un-ignored by `.4`.
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

use murphy_core::{Edit, Range, apply_edits};

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
/// Un-ignored and made green by murphy-hwe.4 when it implements the body of
/// `apply_edits`.
#[test]
#[ignore = "apply lands in murphy-hwe.4 (this harness pins the property first)"]
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
