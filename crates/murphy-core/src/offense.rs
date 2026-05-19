//! The offense contract type (design §5).
//!
//! An [`Offense`] is the unit a cop emits when source violates a rule.
//! Its serialized JSON shape is a stable contract consumed by downstream
//! tooling, so the field names here are load-bearing.

use serde::{Deserialize, Serialize};

/// Stable cop_name for the synthetic parser-level syntax-error offense
/// (design §6); consumer-facing contract — snapshot-stable.
pub const SYNTAX_COP_NAME: &str = "Murphy/Syntax";

/// A source span, expressed as **byte offsets** into the original source.
///
/// ADR 0001: these are byte offsets (`u32`), never char indices. All cop
/// and autocorrect logic operates on bytes; reflect that when constructing
/// or consuming a `Range`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// Inclusive start byte offset into the source.
    pub start_offset: u32,
    /// Exclusive end byte offset into the source.
    pub end_offset: u32,
}

impl Range {
    /// Build a byte-offset [`Range`] from a prism `Location`.
    ///
    /// **This is the single audited site for the `usize -> u32` narrowing
    /// of prism offsets (ADR 0001).** `parse()` rejects any source longer
    /// than `u32::MAX` bytes up front, so every offset into a successfully
    /// parsed source provably fits in `u32`; the `as u32` cast is therefore
    /// sound here without a re-guard. Both [`crate::parse`] and every cop
    /// MUST go through this function rather than re-deriving the narrowing,
    /// so the soundness argument and the `#[allow]` live in exactly one place.
    #[allow(clippy::cast_possible_truncation)]
    pub fn from_prism_location(loc: &ruby_prism::Location<'_>) -> Range {
        Range {
            start_offset: loc.start_offset() as u32,
            end_offset: loc.end_offset() as u32,
        }
    }
}

/// How serious an offense is.
///
/// `Ord`/`PartialOrd` on `Severity` are LOAD-BEARING (ADR 0011):
/// `aggregator::aggregate` resolves a `(file, cop_name, range, message)`
/// collision to the MAXIMUM-severity offense via a *descending* severity sort
/// term. That yields "max by real severity" ONLY because the variants are
/// declared least-severe → most-severe, so derive `Ord` == ascending severity.
/// ANY new variant MUST be inserted in its true severity position (an `Info`
/// BEFORE `Warning`; a `Fatal` AFTER `Error`) — adding it in the wrong position
/// silently inverts collision precedence with NO failing test. Serde output is
/// unaffected by variant order (`#[serde(rename_all = "lowercase")]` controls
/// the wire form).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A non-fatal style/correctness concern.
    Warning,
    /// A serious problem.
    Error,
}

// ADR 0011 compile-time anchor: the descending-severity tiebreaker in
// `aggregator::aggregate` only yields "max by real severity" while derive `Ord`
// equals ascending severity, i.e. variants declared least-severe → most-severe.
// This fails to BUILD (not at runtime) if `Warning`/`Error` are reordered,
// forcing a conscious decision when a `Severity` variant is added/moved.
const _: () = assert!(
    (Severity::Warning as u8) < (Severity::Error as u8),
    "ADR 0011: Severity variants MUST be declared least-severe -> most-severe; \
     aggregate's descending tiebreaker depends on derive Ord == ascending severity"
);

/// A single rule violation reported by a cop (design §5).
///
/// Phase 1 subset: no `autocorrect` field yet (deferred to Phase 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offense {
    /// Path of the file the offense was found in.
    pub file: String,
    /// Fully-qualified cop name, e.g. `Murphy/NoReceiverPuts`.
    pub cop_name: String,
    /// The offending source span (byte offsets).
    pub range: Range,
    /// Severity of the offense.
    pub severity: Severity,
    /// Human-readable explanation of the offense.
    pub message: String,
}

impl Offense {
    /// Construct an [`Offense`], taking `&str` and doing the owned-string
    /// conversions internally so a cop emits one in a single call instead of
    /// a 5-field literal with repeated `.into()`s.
    pub fn new(
        file: &str,
        cop_name: &str,
        range: Range,
        severity: Severity,
        message: &str,
    ) -> Offense {
        Offense {
            file: file.into(),
            cop_name: cop_name.into(),
            range,
            severity,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offense_serializes_to_contract() {
        let o = Offense {
            file: "a.rb".into(),
            cop_name: "Murphy/NoReceiverPuts".into(),
            range: Range {
                start_offset: 0,
                end_offset: 4,
            },
            severity: Severity::Warning,
            message: "Use a logger instead of puts".into(),
        };
        let j: serde_json::Value = serde_json::to_value(&o).unwrap();
        assert_eq!(j["range"]["start_offset"], 0);
        assert_eq!(j["range"]["end_offset"], 4);
        assert_eq!(j["cop_name"], "Murphy/NoReceiverPuts");
        assert_eq!(j["severity"], "warning");

        let round_tripped: Offense = serde_json::from_value(j.clone()).unwrap();
        assert_eq!(round_tripped, o);
    }
}
