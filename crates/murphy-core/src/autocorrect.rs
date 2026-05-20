//! Autocorrect apply engine for Murphy (design §5, §6, §7).
//!
//! ## Responsibilities
//!
//! [`apply_edits`] is the frozen public contract that transforms source text by
//! applying a slice of [`Edit`] records produced by cops with a `fix` block.
//! The real engine is [`apply_edits_logged`], which returns an [`ApplyOutcome`]
//! with both the corrected string and a conflict log. `apply_edits` is a thin
//! wrapper that discards the conflict log.
//!
//! ## Conflict detection and the conflict log
//!
//! [`Conflict`] and [`ConflictReason`] are **internal debug/observability types**
//! for the `.6 --debug` surface.  They are **NOT** the `Offense.autocorrect`
//! wire contract (design §5 / ADR 0006); do not conflate them.
//!
//! Three reasons an edit is dropped rather than applied:
//!
//! - **[`ConflictReason::OutOfBounds`]** — `start > source.len()` or
//!   `end > source.len()`.  Checked first (pre-validation).
//! - **[`ConflictReason::NonCharBoundary`]** — `!source.is_char_boundary(start)`
//!   or `!source.is_char_boundary(end)`.  Checked after bounds (pre-validation).
//!   With these two pre-checks passing, every slice cuts on a valid char boundary,
//!   so the replacement strings (valid UTF-8 by `.2` contract) can be concatenated
//!   via `&str` splice into a valid `String` — no `Vec<u8>+from_utf8` needed.
//! - **[`ConflictReason::Overlap`]** — the edit's byte range overlaps an already-
//!   accepted edit in the stable total order.  Half-open `[start, end)` rule:
//!   `a.start < b.end && b.start < a.end` is overlap; `a.end == b.start` is NOT.
//!
//! ## Stable total order (PIN 3)
//!
//! Edits are sorted in **descending** `(start_offset, end_offset)` order so the
//! algorithm splices from the highest byte offset first, keeping all lower
//! offsets valid on the running string.  Ties are broken by the original slice
//! index (ascending), using `Vec::sort_by` which is stable — so identical-key
//! edits land in their original relative order.  The shuffle-determinism test
//! asserts that any permutation of the input produces an identical [`ApplyOutcome`].
//!
//! ## What is pinned in murphy-hwe.3
//!
//! * **Function signature** — `apply_edits(source, edits) -> String` is frozen.
//! * **Empty-edits identity** — when `edits` is empty, `apply_edits` returns
//!   the source unchanged.
//!
//! ## What is deferred to murphy-hwe.5
//!
//! True idempotency requires a **reparse-and-re-derive loop**. That is `.5`.

use crate::offense::Edit;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Reason an [`Edit`] was not applied (internal debug/observability type for
/// `.6 --debug`).
///
/// **This is NOT the `Offense.autocorrect` wire contract** (design §5 / ADR 0006);
/// do not conflate the two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictReason {
    /// The edit's byte range overlaps an already-accepted edit in the stable
    /// total order (half-open `[start, end)` predicate).
    Overlap,
    /// `start_offset > end_offset` — the range is inverted/malformed. The
    /// mruby blob decoder already drops these, but the public
    /// [`apply_edits`]/[`apply_edits_logged`] API accepts `Edit` directly
    /// (native cops, deserialized JSON), so this is validated here too —
    /// otherwise `replace_range(start..end, …)` would panic. Checked first.
    InvalidRange,
    /// `start > source.len()` or `end > source.len()` — edit is outside the
    /// source buffer entirely.  Checked after `InvalidRange`.
    OutOfBounds,
    /// `!source.is_char_boundary(start)` or `!source.is_char_boundary(end)` —
    /// the edit cuts inside a multibyte codepoint.  Checked after bounds.
    NonCharBoundary,
}

/// A dropped edit and the reason it was not applied.
///
/// **Internal debug/observability type** — exposed for `.6 --debug` only.
/// This is NOT the `Offense.autocorrect` wire contract (design §5 / ADR 0006).
///
/// `conflicts_with` is `Some(winner)` for [`ConflictReason::Overlap`] (the
/// edit that was already accepted and caused the conflict), and `None` for
/// [`ConflictReason::OutOfBounds`] / [`ConflictReason::NonCharBoundary`]
/// (pre-validation failures that have no opposing winner).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    /// The edit that was not applied.
    pub dropped: Edit,
    /// The already-accepted edit this one conflicted with, if any.
    ///
    /// `Some` for `Overlap`, `None` for `OutOfBounds` / `NonCharBoundary`.
    pub conflicts_with: Option<Edit>,
    /// Why the edit was dropped.
    pub reason: ConflictReason,
}

/// The result of [`apply_edits_logged`]: the corrected source string plus a
/// log of all edits that could not be applied and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyOutcome {
    /// The source after applying all non-conflicting edits.
    pub corrected: String,
    /// Log of edits that were dropped and why.  Order is deterministic:
    /// conflicts are pushed in stable-total-order walk order.
    pub conflicts: Vec<Conflict>,
}

/// Apply `edits` to `source`, returning the corrected string and a conflict log.
///
/// This is the real engine; [`apply_edits`] is a thin wrapper that discards
/// the conflict log.
///
/// ## Algorithm
///
/// 1. **Stable sort** all edits by `(start_offset DESC, end_offset DESC, original_index ASC)`.
///    `Vec::sort_by` is stable — the original-index tiebreak is implicit for
///    equal `(start, end)` pairs, but we make it explicit to be clear.
/// 2. **Pre-validation** per edit (before overlap check):
///    - [`ConflictReason::OutOfBounds`] if `start > len || end > len`.
///    - [`ConflictReason::NonCharBoundary`] if `!is_char_boundary(start) || !is_char_boundary(end)`.
///
///    After these checks, slicing on `start..end` is safe and produces valid UTF-8
///    when combined with a valid UTF-8 replacement (guaranteed by `.2` contract).
/// 3. **Overlap detection**: an edit overlaps an already-accepted edit if
///    `accepted.start < edit.end && edit.start < accepted.end` (half-open
///    `[start, end)` predicate).  Adjacent (`a.end == b.start`) is NOT overlap.
/// 4. **Apply winners** via `String::replace_range` in descending offset order,
///    keeping all lower offsets valid.
/// 5. **Collect losers** into `conflicts`.
pub fn apply_edits_logged(source: &str, edits: &[Edit]) -> ApplyOutcome {
    if edits.is_empty() {
        return ApplyOutcome {
            corrected: source.to_owned(),
            conflicts: vec![],
        };
    }

    let source_len = source.len();

    // Stable total order: (start DESC, end DESC, replacement ASC).
    //
    // The tiebreak is the replacement TEXT, NOT the edit's position in the
    // input slice. That is what makes `ApplyOutcome` invariant under any
    // permutation of `edits`: cop registration order, aggregation order, or
    // mruby/native interleaving cannot change which edit wins a same-range
    // conflict (ADR 0007 determinism). An original-index tiebreak would make
    // the winner input-order-dependent and silently non-deterministic across
    // cop-order changes. Two edits identical in (range, replacement) are equal
    // under this key; exactly one is applied and the other is logged as an
    // Overlap conflict — the emitted text is identical regardless of order.
    let mut ordered: Vec<&Edit> = edits.iter().collect();
    ordered.sort_by(|a, b| {
        b.range
            .start_offset
            .cmp(&a.range.start_offset)
            .then(b.range.end_offset.cmp(&a.range.end_offset))
            .then(a.replacement.cmp(&b.replacement))
    });

    // Walk the sorted edits, pre-validating and conflict-checking each one.
    // `accepted` holds edits that will be applied (in sort order).
    let mut accepted: Vec<Edit> = Vec::new();
    let mut conflicts: Vec<Conflict> = Vec::new();

    for &edit in &ordered {
        let start = edit.range.start_offset as usize;
        let end = edit.range.end_offset as usize;

        // Inverted/malformed range. The mruby decoder drops these, but a
        // native cop or a deserialized `Offense` can hand an inverted `Edit`
        // straight to this public API; without this guard the descending
        // `replace_range(start..end, …)` below would panic. Checked first
        // (a range that is inverted is malformed regardless of bounds).
        if start > end {
            conflicts.push(Conflict {
                dropped: (*edit).clone(),
                conflicts_with: None,
                reason: ConflictReason::InvalidRange,
            });
            continue;
        }

        // PIN 1: bounds check (against original source).
        if start > source_len || end > source_len {
            conflicts.push(Conflict {
                dropped: (*edit).clone(),
                conflicts_with: None,
                reason: ConflictReason::OutOfBounds,
            });
            continue;
        }

        // PIN 2: char-boundary check (against original source).
        if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
            conflicts.push(Conflict {
                dropped: (*edit).clone(),
                conflicts_with: None,
                reason: ConflictReason::NonCharBoundary,
            });
            continue;
        }

        // PIN 3: overlap check against all accepted edits.
        // Overlap predicate (half-open [start, end)): a.start < b.end && b.start < a.end.
        // Touching (a.end == b.start) is NOT overlap.
        let mut conflict_winner: Option<Edit> = None;
        for winner in &accepted {
            let w_start = winner.range.start_offset as usize;
            let w_end = winner.range.end_offset as usize;
            if w_start < end && start < w_end {
                conflict_winner = Some(winner.clone());
                break;
            }
        }

        if let Some(winner) = conflict_winner {
            conflicts.push(Conflict {
                dropped: (*edit).clone(),
                conflicts_with: Some(winner),
                reason: ConflictReason::Overlap,
            });
        } else {
            accepted.push((*edit).clone());
        }
    }

    // Apply accepted edits in descending offset order (already in sort order).
    // Using `replace_range` on a mutable String: since edits are in descending
    // order, each splice does not shift the offsets of remaining splices.
    let mut corrected = source.to_owned();
    for edit in &accepted {
        let start = edit.range.start_offset as usize;
        let end = edit.range.end_offset as usize;
        // Safety: bounds + char-boundary pre-checked above; replacement is
        // valid UTF-8 by the .2 contract.
        corrected.replace_range(start..end, &edit.replacement);
    }

    ApplyOutcome {
        corrected,
        conflicts,
    }
}

/// Apply a slice of [`Edit`] records to `source`, producing a corrected copy.
///
/// This is the **frozen public contract** (pinned in murphy-hwe.3).  The
/// signature must not change.
///
/// For access to the conflict log, use [`apply_edits_logged`] directly.
///
/// # Contract (frozen in murphy-hwe.3, implemented in murphy-hwe.4)
///
/// * Edits are applied in **descending start-offset order** so earlier byte
///   offsets remain valid as later edits are applied first.
/// * Overlapping edits are detected and logged in [`apply_edits_logged`];
///   this wrapper discards the log and returns only the corrected string.
/// * Empty-edits identity: `apply_edits(source, &[]) == source`.
pub fn apply_edits(source: &str, edits: &[Edit]) -> String {
    apply_edits_logged(source, edits).corrected
}

// ---------------------------------------------------------------------------
// Fixpoint loop (murphy-hwe.5)
// ---------------------------------------------------------------------------

/// The terminal condition of a [`run_to_fixpoint`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixpointStatus {
    /// The loop produced no new edits (or all edits were conflicts with no
    /// net change to the source).  The corrected string is stable.
    Converged,
    /// The loop performed `max_iterations` apply rounds without converging or
    /// detecting an oscillation.  The corrected string is the state after the
    /// last apply round.
    MaxIterations,
    /// The loop detected that the source revisited a previously-seen state
    /// (an oscillation / cycle ≥ 2).  The corrected string is the
    /// **re-visited state at detection** (see APIN1 note below).
    ///
    /// APIN1 (murphy-hwe.5): `corrected` = the re-visited `next` state
    /// at the moment of cycle detection, NOT the previous round's output.
    /// Rationale: re-feeding this value to [`run_to_fixpoint`] immediately
    /// re-detects the oscillation and is therefore stable ("weak idempotency").
    Oscillation,
}

/// The result of a [`run_to_fixpoint`] call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixpointOutcome {
    /// The corrected source string after the loop terminates.
    ///
    /// - [`FixpointStatus::Converged`]: the stable fixed point.
    /// - [`FixpointStatus::MaxIterations`]: the state after `max_iterations`
    ///   apply rounds.
    /// - [`FixpointStatus::Oscillation`]: the re-visited state at cycle
    ///   detection (APIN1 — the `next` value that was found in `seen`).
    pub corrected: String,
    /// Number of apply rounds actually performed (a round = one non-empty
    /// edit set run through [`apply_edits_logged`]). Counted *including* the
    /// terminal round, so an oscillation `a → b → a` reports `2` and a no-op
    /// converge (`next == state` after one apply) reports `1`.
    ///
    /// Invariants (APIN3):
    /// - `iterations <= max_iterations` always.
    /// - `iterations == 0` ⟺ the *first* `lint` returned no edits (no apply
    ///   happened) ⟹ `status == Converged` and `corrected == source`.
    /// - `corrected == source` with `iterations >= 1` is a legitimate
    ///   all-conflict / no-op converged round — it is NOT `iterations == 0`.
    /// - `max_iterations == 0`: status `MaxIterations`, `iterations == 0`,
    ///   `corrected == source`, and `lint` is never called (zero budget).
    ///   This is the one case where `iterations == 0` is not `Converged`.
    pub iterations: u32,
    /// Terminal status.
    pub status: FixpointStatus,
    /// Conflicts from the **final apply round only** (the round that produced
    /// the surfaced `corrected` value).  Earlier rounds' conflicts are not
    /// accumulated.  Empty when `status == Converged` due to an empty edit
    /// list (no apply round performed) or when all edits in the final round
    /// were applied without conflict.
    pub conflicts: Vec<Conflict>,
}

/// Run `lint` repeatedly on `source`, applying the returned [`Edit`]s via
/// [`apply_edits_logged`], until one of three terminal conditions is reached:
///
/// 1. **Converged** — `lint` returns no edits, or all edits were conflicts
///    with no net change to the source (next == state).
/// 2. **MaxIterations** — `max_iterations` apply rounds have been performed
///    without convergence or oscillation.
/// 3. **Oscillation** — the source revisited a previously-seen state (a cycle
///    of length ≥ 2 was detected via a [`HashSet`] of exact `String` values).
///
/// ## `max_iterations == 0` behaviour (APIN3)
///
/// When `max_iterations == 0` the function short-circuits immediately:
/// `lint` is **never called**, the returned outcome has
/// `status = MaxIterations`, `corrected = source`, `iterations = 0`,
/// `conflicts = []`.  This is the "zero-budget" policy: no apply rounds may
/// be performed.
///
/// ## Loop semantics (pin, DESIGN §5 step 6)
///
/// ```text
/// state = source; seen = {source}; iterations = 0
/// loop:
///   edits = lint(&state)
///   if edits.empty() → Converged (corrected=state, last_conflicts=[])
///   outcome = apply_edits_logged(&state, &edits)
///   next = outcome.corrected
///   (APIN2) if next == state → Converged (corrected=next, last_conflicts=outcome.conflicts)
///   (APIN2) if next ∈ seen  → Oscillation (corrected=next, APIN1)
///   seen.insert(next); state = next; iterations += 1
///   if iterations >= max_iterations → MaxIterations (corrected=state)
/// ```
///
/// APIN2: The `next == state` check MUST come BEFORE the `next ∈ seen` check.
/// If evaluated in the wrong order, the case where all edits conflict and
/// `next == state == source` (source ∈ seen from initialisation) would be
/// misclassified as Oscillation.
///
/// ## Conflicts surface policy
///
/// [`FixpointOutcome::conflicts`] carries conflicts from the **final apply
/// round only**.  Each round overwrites the "last conflicts" accumulator.
/// The surfaced conflicts therefore belong to the apply that produced the
/// surfaced `corrected` value.
///
/// ## Determinism (ADR 0007)
///
/// Given identical `source`, a deterministic `lint` closure, and identical
/// `max_iterations`, the [`FixpointOutcome`] is identical across runs.
/// `seen` is a [`HashSet`] but its contents only drive membership tests
/// (`.contains` / `.insert`) — we never iterate over it in a way that
/// affects output.
pub fn run_to_fixpoint<F>(source: &str, mut lint: F, max_iterations: u32) -> FixpointOutcome
where
    F: FnMut(&str) -> Vec<Edit>,
{
    // APIN3 zero-budget short-circuit: no apply rounds allowed.
    if max_iterations == 0 {
        return FixpointOutcome {
            corrected: source.to_owned(),
            iterations: 0,
            status: FixpointStatus::MaxIterations,
            conflicts: vec![],
        };
    }

    let mut state = source.to_owned();
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(state.clone());
    let mut iterations: u32 = 0;
    // Conflicts from the most-recent apply round (overwritten each round).
    let mut last_conflicts: Vec<Conflict> = vec![];

    loop {
        let edits = lint(&state);

        // Step 2: no edits → already at fixpoint.
        if edits.is_empty() {
            return FixpointOutcome {
                corrected: state,
                iterations,
                status: FixpointStatus::Converged,
                conflicts: vec![],
            };
        }

        let outcome = apply_edits_logged(&state, &edits);
        last_conflicts = outcome.conflicts;
        let next = outcome.corrected;
        // An apply round was actually performed (a non-empty edit set was run
        // through `apply_edits_logged`). Count it BEFORE the termination
        // checks so a terminal round — a no-op converge (`next == state`) or
        // the second leg of an oscillation — is included. `iterations == 0`
        // therefore means strictly "the first `lint` returned no edits, no
        // apply happened"; `corrected == source` with `iterations >= 1` is a
        // legitimate all-conflict / no-op round (NOT iterations == 0).
        iterations += 1;

        // APIN2 Step 4 (MUST come before step 5): all edits were conflicts/no-ops.
        if next == state {
            return FixpointOutcome {
                corrected: next,
                iterations,
                status: FixpointStatus::Converged,
                conflicts: last_conflicts,
            };
        }

        // APIN2 Step 5 (after step 4): cycle detection.
        if seen.contains(&next) {
            // APIN1: corrected = `next` (the re-visited state at detection).
            return FixpointOutcome {
                corrected: next,
                iterations,
                status: FixpointStatus::Oscillation,
                conflicts: last_conflicts,
            };
        }

        // Step 6: advance state.
        seen.insert(next.clone());
        state = next;

        if iterations >= max_iterations {
            return FixpointOutcome {
                corrected: state,
                iterations,
                status: FixpointStatus::MaxIterations,
                conflicts: last_conflicts,
            };
        }
    }
}
