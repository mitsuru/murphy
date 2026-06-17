//! RuboCop `def_node_matcher` pattern compatibility survey.
//!
//! beads issue: `murphy-707j` (under epic `murphy-xvjv`).
//!
//! ## Purpose
//!
//! Map out exactly which RuboCop NodePattern strings — the kind you'd find
//! after a `def_node_matcher :foo, '<pattern>'` in any RuboCop / RuboCop-RSpec /
//! RuboCop-Rails cop — Murphy's `compile()` can take verbatim. The goal of
//! the surrounding epic (`murphy-xvjv`) is "paste a def_node_matcher pattern
//! into `def_node_matcher!` and have it just work". This test exists to size that
//! gap and to drive a punch list of NodeKindTag extensions.
//!
//! ## How it works
//!
//! Each pattern below is fed to [`murphy_pattern::compile`]. Successes and
//! failures are counted and the first error message of each failing pattern
//! is grouped into a category (`unknown node kind`, `unexpected token`,
//! `unsupported in v1`, ...). The test always passes — it's a *survey*, not
//! an assertion — and dumps a categorized summary to stderr so a human can
//! read it with `cargo test -p murphy-pattern --test rubocop_pattern_compat
//! -- --nocapture`.
//!
//! ## Pattern source
//!
//! The patterns below come from three buckets:
//!
//! 1. **Murphy self-baseline** — patterns lifted straight from
//!    `crates/murphy-std/src/cops/**`. These should compile, by definition.
//!    Used to spot regressions if a NodeKindTag extension breaks something.
//! 2. **RuboCop canon** — patterns transcribed from major RuboCop cops.
//!    These are the real "paste-and-run" exercise.
//! 3. **Gap-targeted probes** — minimal patterns that touch the suspected
//!    missing node kinds (`for`, `lambda`, `defs`, `index`, `kwbegin`,
//!    `cbase`, `regopt`, `rational`, `not`, plus pattern-matching family).
//!    These are expected to fail today; the test makes the failure mode
//!    explicit instead of leaving it to a hand-written experiment.

use murphy_pattern::compile;

struct CompatCase {
    pattern: &'static str,
    source: &'static str,
    category: Category,
}

#[derive(Debug, Clone, Copy)]
enum Category {
    /// Patterns already known to compile (Murphy self-baseline).
    MurphyBaseline,
    /// Patterns from rubocop / rubocop-rspec / rubocop-rails cops.
    RuboCopCanon,
    /// Minimal probes touching a single suspected gap node kind.
    GapProbe,
}

fn cases() -> Vec<CompatCase> {
    use Category::*;
    vec![
        // ── Bucket 1: Murphy self-baseline (should all compile) ────────────
        CompatCase {
            pattern: "(send (const nil? :ENV) {:clone :dup :freeze})",
            source: "murphy-std Lint/DeprecatedClassMethods",
            category: MurphyBaseline,
        },
        CompatCase {
            pattern: "(send (const nil? {:File :Dir}) :exists? _)",
            source: "murphy-std Lint/DeprecatedClassMethods",
            category: MurphyBaseline,
        },
        CompatCase {
            pattern: "(send nil? :iterator?)",
            source: "murphy-std Lint/DeprecatedClassMethods",
            category: MurphyBaseline,
        },
        CompatCase {
            pattern: "(send nil? :attr _ {true false})",
            source: "murphy-std Lint/DeprecatedClassMethods",
            category: MurphyBaseline,
        },
        // ── Bucket 2: RuboCop canon ────────────────────────────────────────
        CompatCase {
            pattern: "(send _ :===)",
            source: "rubocop Lint/CaseEquality",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(return)",
            source: "rubocop Style/RedundantReturn (no-arg form)",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(return _)",
            source: "rubocop Style/RedundantReturn",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(send (const nil? :Proc) :new)",
            source: "rubocop Style/Lambda",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(block (send nil? :lambda) _ _)",
            source: "rubocop Style/Lambda",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(send nil? {:proc :lambda})",
            source: "rubocop Style/Lambda variants",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(lvasgn _ _)",
            source: "rubocop Lint/UselessAssignment",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(if _ _ {nil _})",
            source: "rubocop Style/IfUnlessModifier",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(rescue _ (resbody nil nil nil) nil)",
            source: "rubocop Style/RescueModifier",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(send _ :__ENCODING__)",
            source: "rubocop Style/Encoding",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(send _ :== nil)",
            source: "rubocop Style/NilComparison",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(send (send _ :nil?) :!)",
            source: "rubocop Style/NonNilCheck",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(and (send _ :nil?) _)",
            source: "rubocop Style/SafeNavigation",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(csend _ _ ...)",
            source: "rubocop Style/SafeNavigation",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(send _ :+ _)",
            source: "rubocop Style/StringConcatenation",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(send (array ...) :join _)",
            source: "rubocop Style/StringConcatenation variants",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(case _ (when _ _) ...)",
            source: "rubocop Style/CaseLikeIf",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(send _ :each_with_object _)",
            source: "rubocop Style/EachWithObject",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(def _ (args) _)",
            source: "rubocop Style/EmptyMethod (empty body)",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(class _ _ _)",
            source: "rubocop Style/ClassAndModuleChildren",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(module _ _)",
            source: "rubocop Style/ClassAndModuleChildren",
            category: RuboCopCanon,
        },
        // ── Bucket 3: Gap-targeted probes (expected fails) ────────────────
        CompatCase {
            pattern: "(for _ _ _)",
            source: "gap probe: `for` loop",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(lambda)",
            source: "gap probe: short `->` lambda",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(defs self _ _ _)",
            source: "gap probe: singleton def (`def self.foo`)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(index _ _)",
            source: "gap probe: modern bracket call",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(indexasgn _ _ _)",
            source: "gap probe: bracket assignment (`foo[i] = x`)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(kwbegin _)",
            source: "gap probe: `begin..end` keyword form",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(cbase)",
            source: "gap probe: top-level const root (`::Foo`)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(regopt :i :m)",
            source: "gap probe: regex options",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(rational _)",
            source: "gap probe: rational literal (`1r`)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(complex _)",
            source: "gap probe: complex literal (`1i`)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(not _)",
            source: "gap probe: `not` keyword",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(case_match _ (in_pattern _ _) ...)",
            source: "gap probe: Ruby 3 pattern matching (`case ... in ...`)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(array_pattern _ ...)",
            source: "gap probe: array pattern (`in [a, b, c]`)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(hash_pattern _ ...)",
            source: "gap probe: hash pattern (`in {a:, b:}`)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(match_var _)",
            source: "gap probe: pattern match variable binding",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(numblock _ _ _)",
            source: "gap probe: numbered-param block (`{ _1 + _2 }`)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(itblock _ _ _)",
            source: "gap probe: `it` block (Ruby 3.4)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(back_ref :$~)",
            source: "gap probe: back-reference (`$~`, `$&`, ...)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(nth_ref 1)",
            source: "gap probe: numbered match ref (`$1`, `$2`, ...)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(alias _ _)",
            source: "gap probe: `alias`",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(undef _)",
            source: "gap probe: `undef`",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(preexe _)",
            source: "gap probe: `BEGIN { ... }`",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(postexe _)",
            source: "gap probe: `END { ... }`",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(forward_args)",
            source: "gap probe: `...` arg forwarding",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(forwarded_args)",
            source: "gap probe: `...` arg forwarding (receive)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(procarg0 _)",
            source: "gap probe: single-arg proc",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(shadowarg _)",
            source: "gap probe: lambda shadow arg (`; x`)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(kwnilarg)",
            source: "gap probe: `**nil` (kwarg suppression)",
            category: GapProbe,
        },
        CompatCase {
            pattern: "(blocknilarg)",
            source: "gap probe: `&nil` (block suppression)",
            category: GapProbe,
        },
        CompatCase {
            // `retry` now lowers to NodeKind::Retry (a real node), so this is a
            // supported keyword-node pattern, not a gap. Mirrors `(return)`.
            pattern: "(retry)",
            source: "retry keyword node",
            category: RuboCopCanon,
        },
        CompatCase {
            pattern: "(redo)",
            source: "gap probe: `redo` keyword",
            category: GapProbe,
        },
    ]
}

/// Categorise a `compile()` error message into a short bucket so we can
/// summarise without dumping the whole message for every line.
fn bucket_error(msg: &str) -> &'static str {
    let m = msg.to_ascii_lowercase();
    if m.contains("unknown node type") || m.contains("unknown node kind") {
        "unknown-node-kind"
    } else if m.contains("unexpected token") {
        "unexpected-token"
    } else if m.contains("unexpected character") {
        "unexpected-character"
    } else if m.contains("not supported in v1") || m.contains("v1") {
        "v1-scope-out"
    } else if m.contains("dangling") || m.contains("expected") {
        "shape-error"
    } else {
        "other"
    }
}

#[test]
fn def_node_matcher_compat_survey() {
    let cases = cases();
    let total = cases.len();
    let mut ok_count = 0;
    let mut fail_count = 0;

    use std::collections::BTreeMap;
    let mut fail_by_bucket: BTreeMap<&'static str, Vec<(String, String)>> = BTreeMap::new();
    let mut ok_by_category: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut fail_by_category: BTreeMap<&'static str, usize> = BTreeMap::new();

    for c in &cases {
        let cat_name: &str = match c.category {
            Category::MurphyBaseline => "MurphyBaseline",
            Category::RuboCopCanon => "RuboCopCanon",
            Category::GapProbe => "GapProbe",
        };
        match compile(c.pattern) {
            Ok(_) => {
                ok_count += 1;
                *ok_by_category.entry(cat_name).or_insert(0) += 1;
            }
            Err(e) => {
                fail_count += 1;
                *fail_by_category.entry(cat_name).or_insert(0) += 1;
                let bucket = bucket_error(&e.message);
                fail_by_bucket
                    .entry(bucket)
                    .or_default()
                    .push((c.pattern.to_string(), e.message.clone()));
            }
        }
    }

    eprintln!();
    eprintln!("══════════════════════════════════════════════════════════════════");
    eprintln!(" RuboCop def_node_matcher Compat Survey  (beads: murphy-707j)");
    eprintln!("══════════════════════════════════════════════════════════════════");
    eprintln!(" total cases : {total}");
    eprintln!(" compile ok  : {ok_count}");
    eprintln!(" compile err : {fail_count}");
    eprintln!();
    eprintln!(" By category:");
    for cat in ["MurphyBaseline", "RuboCopCanon", "GapProbe"] {
        let ok = ok_by_category.get(cat).copied().unwrap_or(0);
        let fail = fail_by_category.get(cat).copied().unwrap_or(0);
        eprintln!("   {cat:<16} ok={ok:>3}   fail={fail:>3}");
    }
    eprintln!();
    eprintln!(" Failures by error bucket:");
    let pattern_to_source: std::collections::HashMap<&str, &str> =
        cases.iter().map(|c| (c.pattern, c.source)).collect();
    for (bucket, items) in &fail_by_bucket {
        eprintln!("   [{bucket}] {} cases", items.len());
        for (pat, msg) in items {
            let src = pattern_to_source.get(pat.as_str()).copied().unwrap_or("?");
            eprintln!("     - {pat:<45}  ({msg})  [from: {src}]");
        }
    }
    eprintln!("══════════════════════════════════════════════════════════════════");
    eprintln!();

    // The test is a survey; it always succeeds. The whole point is the stderr
    // dump. We still sanity-check that the MurphyBaseline bucket is 100%
    // green — if it ever isn't, a NodeKindTag extension broke production cops
    // and that *is* a regression.
    let baseline_fail = fail_by_category.get("MurphyBaseline").copied().unwrap_or(0);
    assert_eq!(
        baseline_fail, 0,
        "Murphy-baseline patterns regressed: {baseline_fail} failures (see stderr above)"
    );
}
