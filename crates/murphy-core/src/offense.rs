//! The offense contract type (design §5).
//!
//! An [`Offense`] is the unit a cop emits when source violates a rule.
//! Its serialized JSON shape is a stable contract consumed by downstream
//! tooling, so the field names here are load-bearing.

use serde::{Deserialize, Serialize};

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

/// How serious an offense is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A non-fatal style/correctness concern.
    Warning,
    /// A serious problem.
    Error,
}

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
