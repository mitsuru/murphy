//! Autocorrect apply engine for Murphy (design §5, §7).
//!
//! ## Responsibilities
//!
//! [`apply_edits`] is the single function that transforms source text by
//! applying a slice of [`Edit`] records produced by cops with a `fix` block.
//!
//! ## What is pinned in murphy-hwe.3 (this task)
//!
//! * **Function signature** — the public contract `apply_edits(source, edits) -> String`
//!   is frozen here so the idempotency harness (`tests/autocorrect_idempotency.rs`)
//!   and the implementation task (murphy-hwe.4) share one agreed surface.
//! * **Empty-edits identity** — when `edits` is empty, `apply_edits` returns
//!   the source unchanged.  This is the universally-correct base case (zero work
//!   to do), distinct from the descending-offset algorithm, and makes the
//!   idempotency harness's "weakly-pinnable" assertions green without invoking
//!   the descending-offset logic.
//! * **Module structure and doc contract** — pinned so reviewers see the seam.
//!
//! ## What is `#[ignore]`'d until murphy-hwe.4
//!
//! Any assertion that calls `apply_edits(input, non_empty_edits)` is marked
//! `#[ignore = "apply lands in murphy-hwe.4 (this harness pins the property first)"]`
//! in `tests/autocorrect_idempotency.rs`.  The test *compiles* today (proving
//! API compatibility), but does not run until `.4` removes the attribute.
//!
//! * `apply_matches_expected` — `apply_edits(input, edits) == expected_corrected`.
//! * `idempotency_no_oscillation` — `apply_edits(apply_edits(input, edits), same_edits) == expected`.
//!
//! ## What is deferred to murphy-hwe.5
//!
//! True idempotency requires a **reparse-and-re-derive loop**: run all cops on
//! `apply_edits(input, edits)`, get back zero new edits, repeat until stable.
//! That requires the rerun loop (`.5`) and is NOT part of this seam.  The
//! harness encodes the weaker *same-edit-set* form as a placeholder.

use crate::offense::Edit;

/// Apply a slice of [`Edit`] records to `source`, producing a corrected copy.
///
/// # Stub status (murphy-hwe.3)
///
/// The **empty-edits base case** (`edits.is_empty()` → return source verbatim)
/// is implemented here because it is universally correct and required to make
/// the idempotency harness green for the "empty edits → no change" property.
///
/// The **non-empty descending-offset algorithm** is `unimplemented!()` — that
/// is murphy-hwe.4's acceptance criterion.  murphy-hwe.4 will fill in the body
/// without changing this signature.
///
/// # Contract (frozen in murphy-hwe.3, implemented in murphy-hwe.4)
///
/// * Edits MUST be applied in **descending start-offset order** so earlier
///   byte offsets remain valid as later edits are applied first.
/// * Overlapping edits are a conflict: murphy-hwe.4 will detect and log them
///   rather than silently producing corrupt output.
/// * Idempotency (enforced via the harness): applying edits twice to source
///   that was already corrected yields no further change.
pub fn apply_edits(source: &str, edits: &[Edit]) -> String {
    // Trivial base case: zero edits → source is already correct.
    // This branch is intentionally kept in .3 so the idempotency harness
    // can assert the identity property without the descending-offset algorithm.
    if edits.is_empty() {
        return source.to_owned();
    }

    // Non-empty descending-offset apply is murphy-hwe.4.
    unimplemented!("murphy-hwe.4: descending-offset apply")
}
