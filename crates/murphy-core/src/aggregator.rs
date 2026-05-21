//! Offense aggregator (design §5).
//!
//! Deterministic ordering + exact-duplicate removal, with **severity-precedence
//! collision resolution** (ADR 0011, Phase 3 Task 6): when offenses collide on
//! the 4-tuple `(file, cop_name, range, message)` but differ in `severity`, the
//! **maximum-severity** offense survives (`Error > Warning`), deterministically
//! and independent of input/engine/thread order.

use crate::config::MurphyConfig;
use crate::offense::{Offense, SYNTAX_COP_NAME};
use std::collections::HashSet;

/// Aggregate a flat list of offenses into the canonical output order.
///
/// 1. **Sort** by the total order
///    `(file, start_offset, end_offset, cop_name, message, DESC severity)`.
///    The first five components cover the 4-tuple identity plus offsets, so the
///    key remains a genuine total order over all five `Offense` fields: any two
///    distinct offenses still compare unequal on it (reversing one component's
///    direction keeps a total order). `severity` is the FINAL tiebreaker, now
///    sorted **descending** (ADR 0011): when two offenses are 4-tuple-equal but
///    differ only in `severity`, the **maximum** severity (`Error > Warning`)
///    sorts FIRST, so step 2's keep-first dedupe yields the max-severity
///    survivor — deterministically, independent of input/engine/thread order.
/// 2. **Dedupe** exact duplicates keyed by the 4-tuple
///    `(file, cop_name, range, message)`, keeping the first occurrence.
///
/// Note: `severity` is **deliberately excluded from the dedupe key** (it is
/// only in the sort key). It is the collision *resolution* rule, not part of
/// offense identity (ADR 0011): two offenses identical on the 4-tuple but
/// differing only in `severity` are "the same offense" and must collapse to
/// one — and per ADR 0011 that survivor is the **maximum** severity (`Error`),
/// so a real `Error` is never masked by a duplicate `Warning` once the native
/// and mruby engines can both flag the same site. ADR 0006/0007 reserved this
/// precedence for Phase 3; this is the one deliberate, predicted behavior
/// change of Task 6 (Phase 1/2 kept the enum-min `Warning` as a placeholder).
///
/// Determinism (ADR 0007) is preserved: distinct *surviving* offenses differ
/// in the 4-tuple, so their relative order is decided by an EARLIER sort
/// component — the severity tiebreaker is reached only *within* a single
/// collision group (all four 4-tuple components equal), every member of which
/// deduped to one. Reversing the severity direction therefore changes ONLY
/// which collision-group member is kept; it cannot reorder two offenses that
/// both survive. Output ordering is bitwise-identical to before for any input
/// with no severity-only collision.
pub fn aggregate(mut offenses: Vec<Offense>) -> Vec<Offense> {
    offenses.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.range.start_offset.cmp(&b.range.start_offset))
            .then(a.range.end_offset.cmp(&b.range.end_offset))
            .then(a.cop_name.cmp(&b.cop_name))
            .then(a.message.cmp(&b.message))
            // DESC severity (ADR 0011): max-severity sorts first within a
            // 4-tuple-equal group so keep-first dedupe yields the higher
            // severity (Error > Warning). Note `b` vs `a` — intentional.
            .then(b.severity.cmp(&a.severity))
    });

    // Order-preserving dedupe on the 4-tuple (file, cop_name, range, message).
    // The DESC severity sort term above already makes the first duplicate the
    // survivor ADR 0011 wants, so the seen set only tracks offense identity.
    let mut kept: Vec<Offense> = Vec::with_capacity(offenses.len());
    let mut seen: HashSet<(String, String, u32, u32, String)> =
        HashSet::with_capacity(offenses.len());
    for o in offenses {
        let key = (
            o.file.clone(),
            o.cop_name.clone(),
            o.range.start_offset,
            o.range.end_offset,
            o.message.clone(),
        );
        if seen.insert(key) {
            kept.push(o);
        }
    }

    kept
}

pub fn aggregate_with_config(mut offenses: Vec<Offense>, config: &MurphyConfig) -> Vec<Offense> {
    for offense in &mut offenses {
        if offense.cop_name == SYNTAX_COP_NAME {
            continue;
        }
        if let Some(severity) = config.severity_override(&offense.cop_name) {
            offense.severity = severity;
        }
    }
    aggregate(offenses)
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

    /// Phase 3 severity precedence (ADR 0011): same 4-tuple
    /// `(file,cop_name,range,message)` differing ONLY in `severity` collapses to
    /// ONE survivor — the **maximum severity** (`Error > Warning`),
    /// deterministically and **independent of input order**.
    ///
    /// This test was Phase-1 `severity_only_dup_collapses_to_first_phase1_behavior`,
    /// which asserted the enum-MIN (`Warning`) survivor. Its Phase-1 comment
    /// **explicitly predicted this Phase-3 flip** ("Phase 3 … will redefine
    /// which severity wins — flipping THIS test is correct evolution, not a
    /// regression"). ADR 0006/0007 reserved severity precedence for Phase 3;
    /// ADR 0011 makes the call: max-severity wins so a real `Error` is never
    /// masked by a duplicate `Warning` once the native + mruby engines can both
    /// flag the same site. The dedupe KEY is still the 4-tuple (severity
    /// excluded — severity is the collision *resolution* rule, not identity);
    /// only *which* collision-equal offense survives changed. This flip is the
    /// one intended, documented behavior change of Task 6 — correct evolution
    /// per ADR 0006/0011, NOT a regression.
    #[test]
    fn severity_collision_resolves_to_higher_severity_phase3() {
        // Warning-then-Error and the reversed Error-then-Warning must collapse
        // to the IDENTICAL, input-order-independent survivor.
        let warn = off("a.rb", "CopY", 5, 7, Severity::Warning, "same msg");
        let err = off("a.rb", "CopY", 5, 7, Severity::Error, "same msg");

        let forward = aggregate(vec![warn.clone(), err.clone()]);
        let reversed = aggregate(vec![err.clone(), warn]);

        // Input-order-independent: same survivor regardless of feed order.
        assert_eq!(
            forward, reversed,
            "severity-collision survivor must NOT depend on input order"
        );

        // ADR 0011: the survivor is the MAXIMUM severity (Error > Warning),
        // NOT the Phase-1 enum-min (Warning). Severity remains excluded from
        // the dedupe key; it is the collision-resolution rule.
        let expected = vec![err];
        assert_eq!(
            forward, expected,
            "Phase 3 (ADR 0011): severity-only collision resolves to the \
             higher severity (Error), deterministically; severity is still \
             excluded from the dedupe key"
        );
    }

    /// ADR 0011 with a genuine cross-engine collision: a native cop and an
    /// mruby cop fire at the SAME `(file, cop_name, range, message)` but the
    /// native one is `Error` and the mruby one is `Warning` (the Phase-3
    /// motivating case — masking a real Error behind a duplicate Warning is
    /// wrong). `aggregate` operates on `Vec<Offense>`, so feeding two colliding
    /// `Offense` values directly is the right level (no mruby cop run needed,
    /// mirroring the other aggregator unit tests). The `Error` must survive
    /// regardless of which engine's offense appears first in the flat list.
    #[test]
    fn cross_engine_severity_collision_keeps_error_input_order_independent() {
        // Same 4-tuple; "native" emits Error, "mruby" emits Warning.
        let native_err = off("app.rb", "Murphy/Foo", 12, 18, Severity::Error, "bad");
        let mruby_warn = off("app.rb", "Murphy/Foo", 12, 18, Severity::Warning, "bad");

        let native_first = aggregate(vec![native_err.clone(), mruby_warn.clone()]);
        let mruby_first = aggregate(vec![mruby_warn, native_err.clone()]);

        assert_eq!(
            native_first, mruby_first,
            "cross-engine collision survivor must be engine/input-order independent"
        );
        assert_eq!(
            native_first,
            vec![native_err],
            "ADR 0011: cross-engine 4-tuple collision keeps the Error, not the \
             Warning — a real Error is never masked by a duplicate Warning"
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
        // These are 4-tuple-equal, so dedupe collapses them — and per ADR 0011
        // the survivor is the MAXIMUM severity (Error > Warning),
        // deterministically regardless of input order. (Phase 1 asserted the
        // enum-min Warning here; ADR 0011 flips it — same predicted flip as
        // `severity_collision_resolves_to_higher_severity_phase3`.)
        {
            let warn = off("a.rb", "Cop", 5, 7, Severity::Warning, "m");
            let err = off("a.rb", "Cop", 5, 7, Severity::Error, "m");
            let fwd = aggregate(vec![warn.clone(), err.clone()]);
            let rev = aggregate(vec![err, warn]);
            assert_eq!(fwd, rev, "(d) severity tiebreak: input-order-dependent");
            assert_eq!(fwd.len(), 1, "(d) 4-tuple-equal pair dedupes to one");
            assert_eq!(
                fwd[0].severity,
                Severity::Error,
                "(d) ADR 0011: maximum severity (Error > Warning) survives"
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
    fn aggregate_many_distinct_offenses_dedupes_without_quadratic_scan() {
        let input = (0..20_000)
            .map(|index| off("a.rb", "Cop", index, index + 1, Severity::Warning, "x"))
            .collect::<Vec<_>>();

        let started = std::time::Instant::now();
        let got = aggregate(input);

        assert_eq!(got.len(), 20_000);
        assert!(
            started.elapsed().as_millis() < 1_000,
            "aggregate must avoid O(n^2) duplicate scanning for large offense sets"
        );
    }

    #[test]
    fn aggregate_all_identical_4_tuple_collapses_to_one() {
        let o = off("a.rb", "CopA", 0, 3, Severity::Warning, "same");
        let got = aggregate(vec![o.clone(), o.clone(), o.clone()]);
        assert_eq!(got, vec![o]);
    }
}
