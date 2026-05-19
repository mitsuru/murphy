//! Table-driven fixpoint-loop tests for `run_to_fixpoint` (murphy-hwe.5;
//! design §5 step6, §6).
//!
//! ## Purpose
//!
//! Verify [`run_to_fixpoint`] behaviour with synthetic closures (no Ruby
//! parsing needed).  Tests are written RED-first (TDD) before the implementation.
//!
//! ## Test catalogue (DESIGN TDD 6 cases + APIN extras)
//!
//! 1. `converges`           — closure makes incremental edits until clean
//! 2. `already_clean`       — closure returns `[]` immediately (Converged, iter=0)
//! 3. `max_iter_cutoff`     — closure always returns edits (never settles)
//! 4. `oscillation`         — closure flips 'a'↔'b'; detected and stopped
//! 5. `conflict_passthrough`— closure emits overlapping edits every round
//! 6. `strong_idempotency`  — re-running fixpoint on converged output → iter=0
//! 7. `apin2_order`         — all edits conflict → next==state==source → Converged
//! 8. `apin3_max_zero`      — max==0 → MaxIterations, lint never called
//! 9. `apin3_already_clean_max_ge1` — already-clean + max≥1 → Converged, iter=0

use murphy_core::{Edit, FixpointStatus, Range, run_to_fixpoint};

// ---------------------------------------------------------------------------
// Helper: build an Edit that replaces the FIRST occurrence of `from` with `to`.
// Returns an empty vec if `from` is not found in source.
// ---------------------------------------------------------------------------

fn replace_first(source: &str, from: &str, to: &str) -> Vec<Edit> {
    if let Some(pos) = source.find(from) {
        vec![Edit {
            range: Range {
                start_offset: pos as u32,
                end_offset: (pos + from.len()) as u32,
            },
            replacement: to.to_owned(),
        }]
    } else {
        vec![]
    }
}

// ---------------------------------------------------------------------------
// 1. converges: closure replaces the first 'a'→'b' on each call until no 'a'
// remains.  Must terminate in ≤ `count_of('a')` rounds.
// ---------------------------------------------------------------------------

/// Closure that replaces the first ASCII 'a' with 'b' in the source.
/// Terminates naturally when no 'a' remains → Converged.
#[test]
fn converges() {
    let source = "aabaa";
    let max = 20;

    let outcome = run_to_fixpoint(source, |s| replace_first(s, "a", "b"), max);

    assert_eq!(
        outcome.status,
        FixpointStatus::Converged,
        "converges: expected Converged"
    );
    assert_eq!(
        outcome.corrected, "bbbbb",
        "converges: all 'a' replaced with 'b'"
    );
    // 5 'a's → 4 apply rounds (the 5th lint call returns [] → Converged with
    // no additional round counted).  Each apply round replaces one 'a' with 'b',
    // so 4 rounds produce the state-advancing changes; the 5th lint call sees
    // no 'a' and returns [] → the empty-edits branch fires with iterations==4.
    assert_eq!(
        outcome.iterations, 4,
        "converges: 4 apply rounds (5th lint returns [], not counted)"
    );
    assert!(
        outcome.conflicts.is_empty(),
        "converges: no conflicts expected"
    );
}

// ---------------------------------------------------------------------------
// 2. already_clean: closure returns [] immediately.
// Converged, corrected==source, iterations==0.
// ---------------------------------------------------------------------------

#[test]
fn already_clean() {
    let source = "clean source\n";

    let outcome = run_to_fixpoint(source, |_s| vec![], 10);

    assert_eq!(
        outcome.status,
        FixpointStatus::Converged,
        "already_clean: expected Converged"
    );
    assert_eq!(
        outcome.corrected, source,
        "already_clean: corrected must equal source"
    );
    assert_eq!(outcome.iterations, 0, "already_clean: no rounds performed");
    assert!(outcome.conflicts.is_empty(), "already_clean: no conflicts");
}

// ---------------------------------------------------------------------------
// 3. max_iter_cutoff: closure always appends " x" (always changes state, never
// converges).  Must stop at max_iterations rounds.
// ---------------------------------------------------------------------------

#[test]
fn max_iter_cutoff() {
    let source = "start";
    let max = 5u32;

    // Each call appends " x" at the end (zero-width insert at source.len()).
    let outcome = run_to_fixpoint(
        source,
        |s| {
            vec![Edit {
                range: Range {
                    start_offset: s.len() as u32,
                    end_offset: s.len() as u32,
                },
                replacement: " x".to_owned(),
            }]
        },
        max,
    );

    assert_eq!(
        outcome.status,
        FixpointStatus::MaxIterations,
        "max_iter_cutoff: expected MaxIterations"
    );
    assert_eq!(
        outcome.iterations, max,
        "max_iter_cutoff: iterations must equal max"
    );
    // After `max` rounds each appending " x":
    // round 1: "start x", round 2: "start x x", ...
    let expected_suffix = " x".repeat(max as usize);
    assert!(
        outcome.corrected.ends_with(&expected_suffix),
        "max_iter_cutoff: corrected should have {} appended ' x' suffixes",
        max
    );
}

// ---------------------------------------------------------------------------
// 4. oscillation: closure flips 'a'↔'b'.
// After round 1: state="b..." (round 2 would return to "a..." which is already
// in seen) → Oscillation; corrected = re-visited state (APIN1: next, not prev).
// APIN1: corrected = the re-visited state at detection (NOT the round-1 output).
// ---------------------------------------------------------------------------

#[test]
fn oscillation() {
    // Source has one character only so flipping is trivially 2-cycle.
    let source = "a";
    let max = 20;

    let outcome = run_to_fixpoint(
        source,
        |s| {
            if s.contains('a') {
                replace_first(s, "a", "b")
            } else {
                replace_first(s, "b", "a")
            }
        },
        max,
    );

    assert_eq!(
        outcome.status,
        FixpointStatus::Oscillation,
        "oscillation: expected Oscillation"
    );
    // APIN1: corrected = the re-visited state (next, which was "a" the second time
    // we see it — the original source, not "b").
    // Round 1: state="a"→ apply (iterations=1) → next="b". seen={a,b}. state="b".
    // Round 2: state="b"→ apply (iterations=2) → next="a". "a" ∈ seen →
    //          Oscillation, corrected="a", iterations=2 (terminal round counted).
    assert_eq!(
        outcome.corrected, "a",
        "oscillation: APIN1 — corrected must be the re-visited state (next==\"a\")"
    );
    // Terminated (did not loop forever)
    assert!(
        outcome.iterations < max,
        "oscillation: must terminate before max iterations"
    );
    // APIN3: at least 1 apply round performed before detection
    assert!(
        outcome.iterations >= 1,
        "oscillation: at least 1 round at detection"
    );
}

/// Weak idempotency of oscillation: re-feeding the oscillation corrected
/// value to the same closure re-detects oscillation immediately (stable).
#[test]
fn oscillation_weak_idempotency() {
    let source = "a";
    let max = 20;

    let mk_closure = || {
        move |s: &str| {
            if s.contains('a') {
                replace_first(s, "a", "b")
            } else {
                replace_first(s, "b", "a")
            }
        }
    };

    let first = run_to_fixpoint(source, mk_closure(), max);
    assert_eq!(first.status, FixpointStatus::Oscillation);

    // Re-feed the oscillation corrected value.
    let second = run_to_fixpoint(&first.corrected, mk_closure(), max);
    // "a" re-fed → round 1: next="b" (new state), round 2: next="a" ∈ seen →
    // Oscillation again.  Must NOT panic or loop.
    assert_eq!(
        second.status,
        FixpointStatus::Oscillation,
        "oscillation_weak_idempotency: re-feeding corrected re-detects oscillation"
    );
}

// ---------------------------------------------------------------------------
// 5. conflict_passthrough: closure emits overlapping edits every round.
// apply_edits_logged handles conflict detection; outcome.conflicts non-empty;
// since all edits conflict (all dropped → next==state) → Converged.
// ---------------------------------------------------------------------------

#[test]
fn conflict_passthrough() {
    let source = "abcdef";

    // Emit two overlapping edits every round.  After apply, next==state (only
    // one of the conflicting pair can land, and with the default sort order
    // the higher-start one wins — the lower-start is dropped every time).
    // In this case, edit_b [2,5) wins, edit_a [1,4) is always dropped.
    // BUT: applying edit_b changes "abcdef" → "abZZf", so state DOES change on
    // round 1 (next != state).  Round 2: source = "abZZf", edit_b = [2,5) → "ZZ"
    // but "abZZf" bytes 2..5 = "ZZf"[0..2] — wait, we need to make these
    // edits always conflict AND always produce next==state (all dropped).
    //
    // Simplest: emit an out-of-bounds edit (end > len) every round.
    let outcome = run_to_fixpoint(
        source,
        |s| {
            vec![Edit {
                range: Range {
                    start_offset: 0,
                    end_offset: (s.len() + 100) as u32, // always OOB
                },
                replacement: "X".to_owned(),
            }]
        },
        10,
    );

    // All edits are OOB → next==state every round → Converged immediately.
    assert_eq!(
        outcome.status,
        FixpointStatus::Converged,
        "conflict_passthrough: expected Converged (all edits OOB → no change)"
    );
    assert_eq!(
        outcome.corrected, source,
        "conflict_passthrough: corrected must equal source (no valid edit applied)"
    );
    // One apply round WAS performed (edits non-empty, all OOB conflicts) so it
    // is counted: iterations == 1 even though state did not advance (next==state
    // → step 4 Converged). `iterations` counts performed apply rounds, not
    // state-advancing rounds.
    assert_eq!(
        outcome.iterations, 1,
        "conflict_passthrough: 1 apply round performed (all conflicts, next==state)"
    );
    assert_eq!(
        outcome.conflicts.len(),
        1,
        "conflict_passthrough: one conflict logged (the OOB edit)"
    );
}

// ---------------------------------------------------------------------------
// 6. strong_idempotency: run fixpoint on already-converged output → Converged,
// corrected unchanged, iterations==0.
// This is the .3-deferred STRONG IDEMPOTENCY property, now encoded here.
// See also: tests/autocorrect_idempotency.rs (note updated to reference this).
// ---------------------------------------------------------------------------

/// Strong idempotency (the property deferred from murphy-hwe.3 to murphy-hwe.5):
/// `run_to_fixpoint(corrected, same_closure, max)` yields Converged,
/// corrected unchanged, iterations==0.
///
/// This is the correct re-derive-from-corrected-source form (contrast with the
/// vacuous "same byte-range edits twice" form which does NOT hold in general).
#[test]
fn strong_idempotency() {
    let source = "aabaa";
    let max = 20;

    // First run to fixpoint.
    let first = run_to_fixpoint(source, |s| replace_first(s, "a", "b"), max);
    assert_eq!(first.status, FixpointStatus::Converged);
    assert_eq!(first.corrected, "bbbbb");

    // Second run on the corrected output with the SAME closure.
    // No 'a' remains → closure returns [] immediately → Converged, iter=0.
    let second = run_to_fixpoint(&first.corrected, |s| replace_first(s, "a", "b"), max);

    assert_eq!(
        second.status,
        FixpointStatus::Converged,
        "strong_idempotency: re-running on corrected must be Converged"
    );
    assert_eq!(
        second.corrected, first.corrected,
        "strong_idempotency: corrected must be unchanged"
    );
    assert_eq!(
        second.iterations, 0,
        "strong_idempotency: iterations must be 0 (no edits needed)"
    );
    assert!(
        second.conflicts.is_empty(),
        "strong_idempotency: no conflicts"
    );
}

// ---------------------------------------------------------------------------
// 7. APIN2 step ordering: closure whose edits ALL conflict (OOB) so that
// next==state==source.  BOTH step 4 AND step 5 conditions would fire if
// checked in wrong order.  Correct order: step4 (next==state) checked FIRST
// → Converged.  NOT Oscillation.
// ---------------------------------------------------------------------------

/// APIN2 order test: when all edits conflict, next==state==source.
/// source ∈ seen at this point (it was the initial value).  The correct result
/// is Converged (step 4 fires first), NOT Oscillation (step 5 would fire if
/// the check order were reversed).
#[test]
fn apin2_all_edits_conflict_converged_not_oscillation() {
    let source = "hello";
    // Emit a single OOB edit every round: end_offset = u32::MAX > source.len()
    // → dropped by apply_edits_logged → next == state == source.
    // source was inserted into `seen` at the start, so source ∈ seen → step 5 fires
    // IF evaluated before step 4.  Must be Converged.
    let outcome = run_to_fixpoint(
        source,
        |_s| {
            vec![Edit {
                range: Range {
                    start_offset: 0,
                    end_offset: u32::MAX,
                },
                replacement: "NEVER".to_owned(),
            }]
        },
        10,
    );

    assert_eq!(
        outcome.status,
        FixpointStatus::Converged,
        "apin2_order: all edits conflict → next==state → must be Converged, NOT Oscillation"
    );
    assert_eq!(
        outcome.corrected, source,
        "apin2_order: corrected must equal original source"
    );
    // iterations == 1: one apply round was performed and counted before the
    // step-4 check; next==state → Converged (NOT iterations==0).
    assert_eq!(
        outcome.iterations, 1,
        "apin2_order: 1 apply round performed (counted before step 4)"
    );
}

// ---------------------------------------------------------------------------
// 8. APIN3 max==0: zero budget → MaxIterations, corrected=source, iter=0,
// lint closure NEVER called.
// ---------------------------------------------------------------------------

#[test]
fn apin3_max_zero_no_lint_called() {
    let source = "anything";
    let mut lint_call_count = 0usize;

    let outcome = run_to_fixpoint(
        source,
        |_s| {
            lint_call_count += 1;
            vec![Edit {
                range: Range {
                    start_offset: 0,
                    end_offset: 1,
                },
                replacement: "X".to_owned(),
            }]
        },
        0, // max == 0
    );

    assert_eq!(
        outcome.status,
        FixpointStatus::MaxIterations,
        "apin3_max_zero: expected MaxIterations"
    );
    assert_eq!(
        outcome.corrected, source,
        "apin3_max_zero: corrected must equal source"
    );
    assert_eq!(
        outcome.iterations, 0,
        "apin3_max_zero: iterations must be 0"
    );
    assert!(outcome.conflicts.is_empty(), "apin3_max_zero: no conflicts");
    assert_eq!(
        lint_call_count, 0,
        "apin3_max_zero: lint closure must NOT be called when max==0"
    );
}

// ---------------------------------------------------------------------------
// 9. APIN3 max≥1 + already_clean: Converged, iterations==0, corrected==source.
// Invariant (corrected post-iter-2 fix): iterations==0 ⟹ first lint empty ⟹
// Converged && corrected==source. (The reverse is NOT an iff: corrected==source
// can also hold with iterations>=1 — an all-conflict no-op round, see
// conflict_passthrough.) This test pins the already-clean direction.
// ---------------------------------------------------------------------------

#[test]
fn apin3_already_clean_max_ge1() {
    let source = "already clean";

    let outcome = run_to_fixpoint(source, |_s| vec![], 5);

    assert_eq!(
        outcome.status,
        FixpointStatus::Converged,
        "apin3_already_clean: expected Converged"
    );
    assert_eq!(
        outcome.iterations, 0,
        "apin3_already_clean: iterations must be 0"
    );
    assert_eq!(
        outcome.corrected, source,
        "apin3_already_clean: corrected must equal source"
    );

    // Verify the iff invariant: (iterations==0 && Converged) ⇔ corrected==source
    let inv = outcome.iterations == 0
        && outcome.status == FixpointStatus::Converged
        && outcome.corrected == source;
    assert!(inv, "apin3_already_clean: iff invariant must hold");
}

// ---------------------------------------------------------------------------
// 10. APIN3 invariant: iterations <= max_iterations always.
// ---------------------------------------------------------------------------

#[test]
fn iterations_never_exceeds_max() {
    // Use a closure that always appends " x" (never converges) — iterations==max.
    let source = "x";
    let max = 7u32;

    let outcome = run_to_fixpoint(
        source,
        |s| {
            vec![Edit {
                range: Range {
                    start_offset: s.len() as u32,
                    end_offset: s.len() as u32,
                },
                replacement: " x".to_owned(),
            }]
        },
        max,
    );

    assert!(
        outcome.iterations <= max,
        "iterations_never_exceeds_max: {} > {}",
        outcome.iterations,
        max
    );
    assert_eq!(
        outcome.status,
        FixpointStatus::MaxIterations,
        "iterations_never_exceeds_max: expected MaxIterations"
    );
}
