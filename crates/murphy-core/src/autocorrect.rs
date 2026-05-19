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
    /// `start > source.len()` or `end > source.len()` — edit is outside the
    /// source buffer entirely.  Checked before overlap detection.
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

    // Attach original indices so we can use them as tiebreaks.
    // Sort: (start DESC, end DESC, original_index ASC).
    let mut indexed: Vec<(usize, &Edit)> = edits.iter().enumerate().collect();
    indexed.sort_by(|(ia, a), (ib, b)| {
        // Primary: start_offset descending
        b.range
            .start_offset
            .cmp(&a.range.start_offset)
            // Secondary: end_offset descending
            .then(b.range.end_offset.cmp(&a.range.end_offset))
            // Tiebreak: original index ascending (stable, unambiguous)
            .then(ia.cmp(ib))
    });

    // Walk the sorted edits, pre-validating and conflict-checking each one.
    // `accepted` holds edits that will be applied (in sort order).
    let mut accepted: Vec<Edit> = Vec::new();
    let mut conflicts: Vec<Conflict> = Vec::new();

    for (_orig_idx, edit) in &indexed {
        let start = edit.range.start_offset as usize;
        let end = edit.range.end_offset as usize;

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
