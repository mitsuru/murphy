//! Offense aggregator (design §5).
//!
//! Phase 1 scope: deterministic ordering + exact-duplicate removal only.
//! Severity-precedence / cross-engine (native + mruby) priority resolution is
//! explicitly Phase 3 and is intentionally NOT done here.

use crate::offense::Offense;

/// Aggregate a flat list of offenses into the canonical output order.
///
/// 1. **Stable sort** by `(file, range.start_offset)`. Rust's slice sort is
///    stable, so offenses equal under this key keep their input order — this
///    determinism is relied on by the dedupe below and by snapshot tests.
/// 2. **Dedupe** exact duplicates keyed by the 4-tuple
///    `(file, cop_name, range, message)`, keeping the first occurrence.
///
/// Note: `severity` is **deliberately excluded** from the dedupe key. Two
/// offenses identical on the 4-tuple but differing only in `severity` collapse
/// to the first; severity/priority resolution across native + mruby engines is
/// owned by Phase 3 and intentionally not implemented here.
pub fn aggregate(mut offenses: Vec<Offense>) -> Vec<Offense> {
    offenses.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.range.start_offset.cmp(&b.range.start_offset))
    });

    // Order-preserving dedupe on the 4-tuple (file, cop_name, range, message).
    // The stable sort groups by (file, start_offset); two 4-tuple-equal
    // offenses are only adjacent if they also share start_offset, so an
    // explicit seen-list (not just `dedup_by` on neighbours) makes the dedupe
    // robust regardless of adjacency while preserving first-occurrence order.
    // A `Vec` seen-list (`Range` is `Eq` but not `Hash`, and Task 6 must not
    // touch the offense contract type) keeps this minimal for Phase 1.
    let mut kept: Vec<Offense> = Vec::with_capacity(offenses.len());
    for o in offenses {
        let is_dup = kept.iter().any(|k| {
            k.file == o.file
                && k.cop_name == o.cop_name
                && k.range == o.range
                && k.message == o.message
        });
        if !is_dup {
            kept.push(o);
        }
    }

    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offense::{Range, Severity};

    fn off(file: &str, cop: &str, start: u32, end: u32, sev: Severity, msg: &str) -> Offense {
        Offense::new(
            file,
            cop,
            Range {
                start_offset: start,
                end_offset: end,
            },
            sev,
            msg,
        )
    }

    #[test]
    fn aggregate_sorts_by_file_then_offset_and_dedupes_exact_4_tuple() {
        // Input is out of order across files/offsets and contains:
        //  - an exact 4-tuple duplicate (b.rb/CopX@10 "dup") -> deduped
        //  - a near-dup differing only in message (a.rb/CopY@5) -> NOT deduped
        //  - a STABILITY pair: two distinct offenses tying on (file,start_offset)
        //    (a.rb/CopY@5 "first sev" then a.rb/CopY@5 "other msg"). They share
        //    the sort key but are not 4-tuple-equal, so neither is dropped and
        //    their relative input order MUST be preserved by the stable sort.
        let input = vec![
            off("b.rb", "CopX", 10, 12, Severity::Warning, "dup"),
            off("a.rb", "CopY", 5, 7, Severity::Warning, "first sev"),
            off("a.rb", "CopY", 5, 7, Severity::Warning, "other msg"), // ties on key; kept after
            off("b.rb", "CopX", 10, 12, Severity::Warning, "dup"),     // exact 4-tuple dup
            off("a.rb", "CopA", 0, 3, Severity::Warning, "early"),
            off("b.rb", "CopX", 2, 4, Severity::Warning, "before dup"),
        ];

        let got = aggregate(input);

        let expected = vec![
            off("a.rb", "CopA", 0, 3, Severity::Warning, "early"),
            // Stability: these two tie on (a.rb,5); input order is preserved.
            off("a.rb", "CopY", 5, 7, Severity::Warning, "first sev"),
            off("a.rb", "CopY", 5, 7, Severity::Warning, "other msg"), // message differs -> kept
            off("b.rb", "CopX", 2, 4, Severity::Warning, "before dup"),
            off("b.rb", "CopX", 10, 12, Severity::Warning, "dup"), // single, deduped
        ];

        assert_eq!(
            got, expected,
            "aggregate must stable-sort by (file,start_offset) and dedupe the \
             4-tuple (file,cop_name,range,message) keeping first; \nGOT:  {got:#?}\nWANT: {expected:#?}"
        );
    }

    /// Phase 1 behavior: same 4-tuple `(file,cop_name,range,message)` differing
    /// ONLY in `severity` collapses to the FIRST occurrence (severity is
    /// deliberately excluded from the dedupe key). Phase 3 (severity/priority
    /// resolution across native + mruby engines, design §5) will change this to
    /// severity-resolved — flipping THIS test is correct evolution, not a
    /// regression. This case is intentionally isolated from the main 4-tuple
    /// contract test so a Phase 3 dev sees the change here, not a scary failure
    /// in a test named like a load-bearing contract guarantee.
    #[test]
    fn severity_only_dup_collapses_to_first_phase1_behavior() {
        let input = vec![
            off("a.rb", "CopY", 5, 7, Severity::Warning, "same msg"),
            off("a.rb", "CopY", 5, 7, Severity::Error, "same msg"), // severity-only dup
        ];

        let got = aggregate(input);

        // Phase 1: the FIRST (Warning) wins; the Error near-dup is dropped.
        let expected = vec![off("a.rb", "CopY", 5, 7, Severity::Warning, "same msg")];

        assert_eq!(
            got, expected,
            "Phase 1: severity-only near-dup must collapse to the FIRST \
             occurrence (severity excluded from dedupe key)"
        );
    }

    #[test]
    fn aggregate_empty_input_yields_empty() {
        assert_eq!(aggregate(Vec::<Offense>::new()), Vec::<Offense>::new());
    }

    #[test]
    fn aggregate_single_element_unchanged() {
        let one = off("a.rb", "CopA", 0, 3, Severity::Warning, "only");
        assert_eq!(aggregate(vec![one.clone()]), vec![one]);
    }

    #[test]
    fn aggregate_all_identical_4_tuple_collapses_to_one() {
        let o = off("a.rb", "CopA", 0, 3, Severity::Warning, "same");
        let got = aggregate(vec![o.clone(), o.clone(), o.clone()]);
        assert_eq!(got, vec![o]);
    }
}
