//! Offense aggregator (design §5).
//!
//! Phase 1 scope: deterministic ordering + exact-duplicate removal only.
//! Severity-precedence / cross-engine (native + mruby) priority resolution is
//! explicitly Phase 3 and is intentionally NOT done here.

use crate::offense::Offense;

/// Aggregate a flat list of offenses into the canonical output order.
///
/// 1. **Sort** by the total order
///    `(file, start_offset, end_offset, cop_name, message, severity)`. This key
///    covers ALL five `Offense` fields, so it is a genuine total order: any two
///    distinct offenses compare unequal on it. `severity` is the FINAL
///    tiebreaker — its sole purpose here is to make step 2's "first" wholly
///    deterministic when two offenses are 4-tuple-equal but differ only in
///    `severity` (otherwise input/thread order would decide the survivor; this
///    is exactly the non-determinism parallel collection would expose). Using
///    `severity` as the last tiebreaker asserts NO severity *precedence* policy
///    (Phase 3 owns that, see below); it only fixes *which* of an otherwise
///    identical pair sorts first. Because the key is now total, the stable
///    `sort_by` is genuinely belt-and-suspenders rather than load-bearing.
/// 2. **Dedupe** exact duplicates keyed by the 4-tuple
///    `(file, cop_name, range, message)`, keeping the first occurrence.
///
/// Note: `severity` is **deliberately excluded** from the dedupe *key* (it is
/// only in the sort key). Two offenses identical on the 4-tuple but differing
/// only in `severity` collapse to the first — and after step 1 that "first" is
/// the deterministic enum-min severity (`Warning < Error` by derive order),
/// independent of input order. Severity/priority *precedence* resolution across
/// native + mruby engines is owned by Phase 3 and intentionally not done here;
/// this comparator only makes the choice deterministic, it picks no policy.
pub fn aggregate(mut offenses: Vec<Offense>) -> Vec<Offense> {
    offenses.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.range.start_offset.cmp(&b.range.start_offset))
            .then(a.range.end_offset.cmp(&b.range.end_offset))
            .then(a.cop_name.cmp(&b.cop_name))
            .then(a.message.cmp(&b.message))
            .then(a.severity.cmp(&b.severity))
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
    /// ONLY in `severity` collapses to ONE survivor. The dedupe key is still the
    /// 4-tuple (severity excluded) and dedupe is still first-wins — but with
    /// `severity` as the FINAL sort tiebreaker (C1), "first" is now the
    /// DETERMINISTIC enum-min severity (`Warning < Error` by derive order),
    /// **independent of input order**. So forward and reversed input must yield
    /// the SAME survivor, and that survivor is `Warning` — now because of the
    /// deterministic severity sort order, NOT because it happened to be first in
    /// the input. This asserts no severity *precedence* policy: Phase 3
    /// (severity/priority resolution across native + mruby engines, design §5)
    /// will redefine which severity wins — flipping THIS test is correct
    /// evolution, not a regression. Isolated from the main 4-tuple contract test
    /// so a Phase 3 dev sees the change here, not a scary failure in a test
    /// named like a load-bearing contract guarantee.
    #[test]
    fn severity_only_dup_collapses_to_first_phase1_behavior() {
        // Warning-then-Error and the reversed Error-then-Warning must collapse
        // to the IDENTICAL, input-order-independent survivor.
        let warn = off("a.rb", "CopY", 5, 7, Severity::Warning, "same msg");
        let err = off("a.rb", "CopY", 5, 7, Severity::Error, "same msg");

        let forward = aggregate(vec![warn.clone(), err.clone()]);
        let reversed = aggregate(vec![err, warn.clone()]);

        // Input-order-independent: same survivor regardless of feed order.
        assert_eq!(
            forward, reversed,
            "severity-only near-dup survivor must NOT depend on input order"
        );

        // The deterministic survivor is the enum-min severity (Warning < Error
        // by derive order) — not "first by input order", but by the severity
        // sort tiebreaker.
        let expected = vec![warn];
        assert_eq!(
            forward, expected,
            "Phase 1: severity-only near-dup collapses to the deterministic \
             enum-min severity (Warning) via the severity sort tiebreaker; \
             severity is still excluded from the dedupe key"
        );
    }

    /// Total-order tie-break (Issue-2 of murphy-eu9): once two cops fire at the
    /// SAME `(file, start_offset)`, ordering must NOT depend on input order
    /// (which is cop-registration-order-dependent). The sort key is the total
    /// order `(file, start_offset, end_offset, cop_name, message, severity)`;
    /// the dedupe 4-tuple is unchanged. Each tiebreak DIMENSION is isolated
    /// here: a pair ties on every earlier component and differs ONLY in the one
    /// under test, so that single component alone decides the order. Feeding the
    /// pair forward then reversed must yield the IDENTICAL output sequence, and
    /// the lesser-keyed offense must sort first. Cases (a)..(d) are
    /// individually identifiable on failure via the assert messages.
    #[test]
    fn aggregate_total_order_is_input_independent() {
        // (a) tie on (file,start); differ end_offset ONLY.
        {
            let lo = off("a.rb", "Cop", 5, 7, Severity::Warning, "m");
            let hi = off("a.rb", "Cop", 5, 9, Severity::Warning, "m");
            let fwd = aggregate(vec![lo.clone(), hi.clone()]);
            let rev = aggregate(vec![hi, lo]);
            assert_eq!(fwd, rev, "(a) end_offset tiebreak: input-order-dependent");
            assert_eq!(fwd[0].range.end_offset, 7, "(a) lesser end_offset first");
            assert_eq!(fwd[1].range.end_offset, 9, "(a) greater end_offset second");
        }

        // (b) tie on (file,start,end); differ cop_name ONLY.
        {
            let lo = off("a.rb", "Murphy/Aaa", 5, 7, Severity::Warning, "m");
            let hi = off("a.rb", "Murphy/Bbb", 5, 7, Severity::Warning, "m");
            let fwd = aggregate(vec![lo.clone(), hi.clone()]);
            let rev = aggregate(vec![hi, lo]);
            assert_eq!(fwd, rev, "(b) cop_name tiebreak: input-order-dependent");
            assert_eq!(fwd[0].cop_name, "Murphy/Aaa", "(b) lesser cop_name first");
            assert_eq!(fwd[1].cop_name, "Murphy/Bbb", "(b) greater cop_name second");
        }

        // (c) tie on (file,start,end,cop_name); differ message ONLY.
        {
            let lo = off("a.rb", "Cop", 5, 7, Severity::Warning, "aaa");
            let hi = off("a.rb", "Cop", 5, 7, Severity::Warning, "bbb");
            let fwd = aggregate(vec![lo.clone(), hi.clone()]);
            let rev = aggregate(vec![hi, lo]);
            assert_eq!(fwd, rev, "(c) message tiebreak: input-order-dependent");
            assert_eq!(fwd[0].message, "aaa", "(c) lesser message first");
            assert_eq!(fwd[1].message, "bbb", "(c) greater message second");
        }

        // (d) tie on (file,start,end,cop_name,message); differ severity ONLY.
        // These are 4-tuple-equal, so dedupe collapses them — but the survivor
        // must be the DETERMINISTIC enum-min severity regardless of input order.
        {
            let warn = off("a.rb", "Cop", 5, 7, Severity::Warning, "m");
            let err = off("a.rb", "Cop", 5, 7, Severity::Error, "m");
            let fwd = aggregate(vec![warn.clone(), err.clone()]);
            let rev = aggregate(vec![err, warn]);
            assert_eq!(fwd, rev, "(d) severity tiebreak: input-order-dependent");
            assert_eq!(fwd.len(), 1, "(d) 4-tuple-equal pair dedupes to one");
            assert_eq!(
                fwd[0].severity,
                Severity::Warning,
                "(d) deterministic enum-min severity (Warning < Error) survives"
            );
        }
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
