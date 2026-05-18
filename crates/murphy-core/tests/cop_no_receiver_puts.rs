//! Table-driven integration test for the `Murphy/NoReceiverPuts` cop.
//!
//! This is an *integration* test crate: it exercises only the public API
//! (`murphy_core::{parse, run_cops, NoReceiverPuts, ...}`), the same surface
//! a downstream consumer sees.
//!
//! ADR 0001: offense ranges are **byte** offsets into the source. Each
//! positive row pins the *exact* selector-token range hand-computed from the
//! source (e.g. `puts "x"` → selector `puts` at bytes [0, 4)).

use murphy_core::{Cop, NoReceiverPuts, Offense, Range, Severity, parse, run_cops};

/// One table row.
struct Case {
    /// Human-readable row label, surfaced in every assertion message so a
    /// failure points at the exact row (not just the source string).
    name: &'static str,
    /// Ruby source to lint.
    src: &'static str,
    /// The exact expected selector ranges (byte offsets), **in emission
    /// order**, one per expected offense. Empty for negative (0-offense)
    /// rows; the offense count is `expected_ranges.len()`.
    expected_ranges: &'static [Range],
}

fn run(src: &str) -> Vec<Offense> {
    let ast = parse(src).expect("test fixtures are valid Ruby");
    let cops: Vec<Box<dyn Cop>> = vec![Box::new(NoReceiverPuts)];
    let mut sink = Vec::new();
    run_cops(&ast, "t.rb", &cops, &mut sink);
    sink
}

#[test]
fn no_receiver_puts_table() {
    const PUTS_0_4: Range = Range {
        start_offset: 0,
        end_offset: 4,
    };
    const PRINT_0_5: Range = Range {
        start_offset: 0,
        end_offset: 5,
    };
    const P_0_1: Range = Range {
        start_offset: 0,
        end_offset: 1,
    };
    // Second statement in "puts 1\nputs 2\n": byte 0..6 is `puts 1`, byte 6
    // is `\n`, so the 2nd `puts` selector is bytes [7, 11).
    const PUTS_7_11: Range = Range {
        start_offset: 7,
        end_offset: 11,
    };

    let cases = [
        // ---- positives: receiver-less puts/print/p → 1 offense on selector ----
        Case {
            name: "puts with string arg",
            src: "puts \"x\"\n",
            // selector `puts` = bytes [0, 4)
            expected_ranges: &[PUTS_0_4],
        },
        Case {
            name: "print with int arg",
            src: "print 1\n",
            // selector `print` = bytes [0, 5)
            expected_ranges: &[PRINT_0_5],
        },
        Case {
            name: "p with ident arg",
            src: "p obj\n",
            // selector `p` = bytes [0, 1)
            expected_ranges: &[P_0_1],
        },
        // bare receiver-less call with no args still offends (name gate only).
        Case {
            name: "bare puts, no args",
            src: "puts\n",
            // selector `puts` = bytes [0, 4)
            expected_ranges: &[PUTS_0_4],
        },
        // multi-offense: two bare `puts` statements → two offenses, each on
        // its own DISTINCT selector range (template for multi-offense rows).
        Case {
            name: "two puts statements -> two distinct ranges",
            src: "puts 1\nputs 2\n",
            expected_ranges: &[PUTS_0_4, PUTS_7_11],
        },
        // ---- negatives: 0 offenses ----
        // explicit receiver → not a bare puts.
        Case {
            name: "explicit receiver obj.puts",
            src: "obj.puts\n",
            expected_ranges: &[],
        },
        // name is `info`, not in {puts,print,p}.
        Case {
            name: "logger.info (name not flagged)",
            src: "logger.info \"x\"\n",
            expected_ranges: &[],
        },
        // local-variable assignment is not a CallNode at all.
        Case {
            name: "local assignment is not a call",
            src: "x = 1\n",
            expected_ranges: &[],
        },
        // similarly-spelled but distinct receiver-less method.
        Case {
            name: "puts_thing (distinct method name)",
            src: "puts_thing\n",
            expected_ranges: &[],
        },
    ];

    for case in &cases {
        let offenses = run(case.src);
        assert_eq!(
            offenses.len(),
            case.expected_ranges.len(),
            "offense count mismatch for case {:?} (src {:?})",
            case.name,
            case.src
        );

        for (i, expected_range) in case.expected_ranges.iter().enumerate() {
            let o = &offenses[i];
            assert_eq!(
                o.range, *expected_range,
                "selector range mismatch for case {:?} offense #{} (src {:?})",
                case.name, i, case.src
            );
            assert_eq!(
                o.cop_name, "Murphy/NoReceiverPuts",
                "cop_name mismatch for case {:?} offense #{} (src {:?})",
                case.name, i, case.src
            );
            assert_eq!(
                o.message, "Use a logger instead of puts",
                "message mismatch for case {:?} offense #{} (src {:?})",
                case.name, i, case.src
            );
            assert_eq!(
                o.severity,
                Severity::Warning,
                "severity mismatch for case {:?} offense #{} (src {:?})",
                case.name,
                i,
                case.src
            );
            assert_eq!(
                o.file, "t.rb",
                "file mismatch for case {:?} offense #{} (src {:?})",
                case.name, i, case.src
            );
        }
    }
}
