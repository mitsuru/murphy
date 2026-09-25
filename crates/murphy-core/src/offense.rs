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

    /// No-location sentinel for filepath-only offenses (murphy-e7bz.41.2).
    ///
    /// Mirrors `murphy_ast::Range::NO_LOCATION` (`u32::MAX`/`u32::MAX`).
    /// A real source range can never hold this value: `parse()` rejects
    /// sources longer than `u32::MAX` bytes. Distinct from `{0, 0}` so a
    /// locationless finding is never rendered as line 1 / column 1.
    pub const NO_LOCATION: Range = Range {
        start_offset: u32::MAX,
        end_offset: u32::MAX,
    };

    /// `true` iff this is the [`Range::NO_LOCATION`] sentinel.
    ///
    /// Takes `&self` (not `self`) so it can be used directly as a serde
    /// `skip_serializing_if` predicate.
    pub fn is_no_location(&self) -> bool {
        *self == Self::NO_LOCATION
    }

    /// [`Range::NO_LOCATION`], as a `fn` so it can be used as a serde
    /// `default` for a missing `range` key.
    pub fn no_location() -> Range {
        Self::NO_LOCATION
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

/// A single text edit that autocorrect applies to the source (design §5).
///
/// `Edit` is the wire-contract type for the `autocorrect.edits` array in an
/// offense JSON. It is separate from `mruby::sdk::FixEdit` (a crate-private
/// synthetic placeholder); marshalling from mruby `fix` blocks into `Edit`
/// values is Phase 4 Task 2's responsibility (ADR 0013).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    /// The source span to replace (byte offsets).
    pub range: Range,
    /// The text to substitute in place of `range`.
    pub replacement: String,
}

/// The autocorrect payload carried by an [`Offense`] that has a fix (design §5).
///
/// Serialises as `{"edits": [...]}`. When `None` on the enclosing `Offense`,
/// the `"autocorrect"` key is **absent** from JSON (not present with `null`),
/// preserved by `#[serde(skip_serializing_if = "Option::is_none")]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Autocorrect {
    /// Ordered list of edits to apply to produce the corrected source.
    pub edits: Vec<Edit>,
}

/// A single rule violation reported by a cop (design §5).
///
/// The five fields `file`, `cop_name`, `range`, `severity`, `message` are the
/// frozen ADR 0006/0007 contract, unchanged since Phase 1.
/// Phase 4 (ADR 0013) adds the optional `autocorrect` field; it is absent from
/// JSON when `None`, preserving byte-identity for existing snapshots.
/// B4 (murphy-fmw.2.4) adds optional `documentation_url` / `rationale` /
/// `fix_example` (extend-only, same `skip_serializing_if` pattern); they are
/// absent when `None` and enriched lint output sets them to `Some`.
///
/// `#[non_exhaustive]` (ADR 0013): the stable Rust surface is [`Offense::new`]
/// plus [`Offense::with_autocorrect`], NOT struct-literal construction. This
/// mechanically enforces the ADR claim that adding contract fields (Phase 4+)
/// is not a source-breaking change for out-of-crate callers — they cannot
/// build `Offense` by literal, so a new field cannot break them. In-crate
/// literals (cops, tests) are unaffected by `#[non_exhaustive]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Offense {
    /// Path of the file the offense was found in.
    pub file: String,
    /// Fully-qualified cop name, e.g. `Lint/Debugger`.
    pub cop_name: String,
    /// The offending source span (byte offsets), or [`Range::NO_LOCATION`]
    /// for a filepath-only offense with no source location
    /// (murphy-e7bz.41.2). Absent from JSON when no-location, so no
    /// fabricated range is ever serialized.
    #[serde(
        skip_serializing_if = "Range::is_no_location",
        default = "Range::no_location"
    )]
    pub range: Range,
    /// Severity of the offense.
    pub severity: Severity,
    /// Human-readable explanation of the offense.
    pub message: String,
    /// Optional autocorrect payload (Phase 4, ADR 0013).
    ///
    /// `None` (the common case) is **omitted from JSON** via
    /// `skip_serializing_if`; the key is absent, not `null`. `default`
    /// ensures forward-compatible deserialization of older JSON that lacks
    /// the key — it deserializes as `None` without error.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub autocorrect: Option<Autocorrect>,
    /// Canonical docs URL for the cop (B4, murphy-fmw.2.4).
    ///
    /// Deterministic per-cop (`https://murphy.dev/docs/cops/<CopName>`),
    /// built only from the registry-controlled cop name. `None` omits the
    /// key from JSON (ADR 0006 extend-only); enriched lint output sets it
    /// to `Some`. `default` keeps old JSON (without the key) readable.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub documentation_url: Option<String>,
    /// Fixed-template rationale (B4, murphy-fmw.2.4).
    ///
    /// Prompt-injection safe by construction: built ONLY from `cop_name`
    /// and cop-author `description` (see `crate::explain`), never from
    /// offense `message` or source text. `None` omits the key.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rationale: Option<String>,
    /// Fixed-template fix-example pointer (B4, murphy-fmw.2.4).
    ///
    /// Stable placeholder pointing at `documentation_url`; fixed text +
    /// registry names only, never source-derived. `None` omits the key.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub fix_example: Option<String>,
}

impl Offense {
    /// Construct an [`Offense`], taking `&str` and doing the owned-string
    /// conversions internally so a cop emits one in a single call instead of
    /// a 5-field literal with repeated `.into()`s.
    ///
    /// `autocorrect` is initialised to `None`. Use [`Offense::with_autocorrect`]
    /// to attach a fix after construction.
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
            autocorrect: None,
            documentation_url: None,
            rationale: None,
            fix_example: None,
        }
    }

    /// Builder: attach an [`Autocorrect`] payload and return `self`.
    ///
    /// Allows fluent construction: `Offense::new(...).with_autocorrect(ac)`.
    #[must_use]
    pub fn with_autocorrect(mut self, ac: Autocorrect) -> Offense {
        self.autocorrect = Some(ac);
        self
    }

    /// Construct a filepath-only [`Offense`] with no source location
    /// (murphy-e7bz.41.2).
    ///
    /// The `range` field holds [`Range::NO_LOCATION`]; JSON omits the key
    /// and human output renders without `line:column`, so no fabricated
    /// `1:1` ever appears. A file offense never carries autocorrect edits.
    pub fn new_without_location(
        file: &str,
        cop_name: &str,
        severity: Severity,
        message: &str,
    ) -> Offense {
        Offense {
            file: file.into(),
            cop_name: cop_name.into(),
            range: Range::NO_LOCATION,
            severity,
            message: message.into(),
            autocorrect: None,
            documentation_url: None,
            rationale: None,
            fix_example: None,
        }
    }

    /// `true` iff this is a filepath-only offense with no source location.
    pub fn has_location(&self) -> bool {
        !self.range.is_no_location()
    }

    /// The source span when located, or `None` for a filepath-only offense.
    pub fn location(&self) -> Option<Range> {
        if self.range.is_no_location() {
            None
        } else {
            Some(self.range)
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
            cop_name: "Lint/Debugger".into(),
            range: Range {
                start_offset: 0,
                end_offset: 8,
            },
            severity: Severity::Warning,
            message: "Remove debugger entry point `debugger`.".into(),
            autocorrect: None,
            documentation_url: None,
            rationale: None,
            fix_example: None,
        };
        let j: serde_json::Value = serde_json::to_value(&o).unwrap();
        assert_eq!(j["range"]["start_offset"], 0);
        assert_eq!(j["range"]["end_offset"], 8);
        assert_eq!(j["cop_name"], "Lint/Debugger");
        assert_eq!(j["severity"], "warning");

        let round_tripped: Offense = serde_json::from_value(j.clone()).unwrap();
        assert_eq!(round_tripped, o);
    }

    #[test]
    fn offense_with_autocorrect_serializes_to_phase4_shape() {
        let o = Offense::new(
            "b.rb",
            "Murphy/FooBar",
            Range {
                start_offset: 10,
                end_offset: 20,
            },
            Severity::Warning,
            "use foo instead",
        )
        .with_autocorrect(Autocorrect {
            edits: vec![Edit {
                range: Range {
                    start_offset: 10,
                    end_offset: 20,
                },
                replacement: "foo".into(),
            }],
        });

        let j: serde_json::Value = serde_json::to_value(&o).unwrap();

        // §5 wire shape: autocorrect.edits[0].range.start_offset etc.
        assert_eq!(j["autocorrect"]["edits"][0]["range"]["start_offset"], 10);
        assert_eq!(j["autocorrect"]["edits"][0]["range"]["end_offset"], 20);
        assert_eq!(j["autocorrect"]["edits"][0]["replacement"], "foo");

        // round-trip
        let round_tripped: Offense = serde_json::from_value(j.clone()).unwrap();
        assert_eq!(round_tripped, o);
    }

    #[test]
    fn offense_without_fix_has_autocorrect_absent_not_null() {
        let o = Offense::new(
            "c.rb",
            "Murphy/NoPuts",
            Range {
                start_offset: 0,
                end_offset: 4,
            },
            Severity::Warning,
            "no puts",
        );

        let j: serde_json::Value = serde_json::to_value(&o).unwrap();

        // Key must be ABSENT (not present with null value).
        // serde skip_serializing_if = "Option::is_none" guarantees this.
        assert!(
            j.as_object().unwrap().get("autocorrect").is_none(),
            "\"autocorrect\" key must be absent from JSON when there is no fix"
        );
    }

    #[test]
    fn no_location_offense_omits_range_from_json() {
        let o = Offense::new_without_location(
            "whitelist.rb",
            "Naming/InclusiveLanguage",
            Severity::Warning,
            "Consider replacing 'whitelist' in file path.",
        );
        assert!(!o.has_location());
        assert_eq!(o.location(), None);

        let j: serde_json::Value = serde_json::to_value(&o).unwrap();
        assert!(
            j.as_object().unwrap().get("range").is_none(),
            "\"range\" key must be absent from JSON for a no-location offense"
        );

        let round_tripped: Offense = serde_json::from_value(j.clone()).unwrap();
        assert_eq!(round_tripped, o);
        assert!(!round_tripped.has_location());
    }

    #[test]
    fn located_offense_keeps_range_in_json() {
        let o = Offense::new(
            "a.rb",
            "Lint/Debugger",
            Range {
                start_offset: 0,
                end_offset: 8,
            },
            Severity::Warning,
            "Remove debugger entry point `debugger`.",
        );
        assert!(o.has_location());
        assert_eq!(
            o.location(),
            Some(Range {
                start_offset: 0,
                end_offset: 8,
            })
        );

        let j: serde_json::Value = serde_json::to_value(&o).unwrap();
        assert_eq!(j["range"]["start_offset"], 0);
        assert_eq!(j["range"]["end_offset"], 8);
    }

    #[test]
    fn missing_range_key_deserializes_to_no_location() {
        let j = serde_json::json!({
            "file": "whitelist.rb",
            "cop_name": "Naming/InclusiveLanguage",
            "severity": "warning",
            "message": "Consider replacing 'whitelist' in file path."
        });
        let o: Offense = serde_json::from_value(j).unwrap();
        assert_eq!(o.range, Range::NO_LOCATION);
        assert!(!o.has_location());
    }

    #[test]
    fn b4_explain_fields_absent_when_none_extend_only() {
        // ADR 0006 extend-only: fresh offenses carry no explain keys until
        // enriched, so pre-B4 snapshots stay byte-identical.
        let o = Offense::new(
            "a.rb",
            "Lint/Debugger",
            Range {
                start_offset: 0,
                end_offset: 8,
            },
            Severity::Warning,
            "Remove debugger entry point `debugger`.",
        );
        let j: serde_json::Value = serde_json::to_value(&o).unwrap();
        let obj = j.as_object().unwrap();
        assert!(obj.get("documentation_url").is_none());
        assert!(obj.get("rationale").is_none());
        assert!(obj.get("fix_example").is_none());
        // Frozen keys still present.
        assert_eq!(j["cop_name"], "Lint/Debugger");
        assert_eq!(j["severity"], "warning");
    }

    #[test]
    fn b4_explain_fields_present_when_enriched_and_round_trip() {
        let mut o = Offense::new(
            "a.rb",
            "Lint/Debugger",
            Range {
                start_offset: 0,
                end_offset: 8,
            },
            Severity::Warning,
            "Remove debugger entry point `debugger`.",
        );
        crate::explain::enrich_offense(&mut o, "Flag debugger calls.");
        let j: serde_json::Value = serde_json::to_value(&o).unwrap();
        assert_eq!(
            j["documentation_url"],
            "https://murphy.dev/docs/cops/Lint/Debugger"
        );
        assert!(j["rationale"].as_str().unwrap().contains("Lint/Debugger"));
        assert!(j["fix_example"].as_str().unwrap().contains("Lint/Debugger"));
        let round_tripped: Offense = serde_json::from_value(j).unwrap();
        assert_eq!(round_tripped, o);
    }

    #[test]
    fn b4_old_json_without_explain_keys_deserializes_to_none() {
        // Forward compat: pre-B4 JSON (no new keys) reads as None.
        let j = serde_json::json!({
            "file": "a.rb",
            "cop_name": "Lint/Debugger",
            "range": {"start_offset": 0, "end_offset": 8},
            "severity": "warning",
            "message": "Remove debugger entry point `debugger`."
        });
        let o: Offense = serde_json::from_value(j).unwrap();
        assert_eq!(o.documentation_url, None);
        assert_eq!(o.rationale, None);
        assert_eq!(o.fix_example, None);
    }
}
