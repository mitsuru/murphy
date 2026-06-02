//! `Style/OptionalArguments` — flags optional arguments that do not appear at
//! the end of the argument list.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/OptionalArguments
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detects method definitions (`def`/`defs`) where optional positional
//!   arguments (`optarg`) appear before required positional arguments (`arg`).
//!   Only `Arg` and `Optarg` nodes are considered; `kwarg`, `kwoptarg`,
//!   `restarg`, `kwrestarg`, and `blockarg` are ignored (matching RuboCop's
//!   `arg_type?`/`optarg_type?` checks). No autocorrect (RuboCop upstream does
//!   not include one). No configurable options.
//! ```
//!
//! ## Matched shapes
//!
//! Method definitions (`def`/`def self.x`) where at least one `optarg` appears
//! before the last `arg` in the parameter list:
//!
//! ```ruby
//! # bad
//! def foo(a = 1, b, c); end
//!
//! # good
//! def baz(a, b, c = 1); end
//! def foobar(a = 1, b = 2, c = 3); end
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Optional arguments should appear at the end of the argument list.";

/// Stateless unit struct.
#[derive(Default)]
pub struct OptionalArguments;

#[cop(
    name = "Style/OptionalArguments",
    description = "Optional arguments should appear at the end of the argument list.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl OptionalArguments {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_def(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check_def(node, cx);
    }
}

fn check_def(node: NodeId, cx: &Cx<'_>) {
    let Some(args_id) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(list) = *cx.kind(args_id) else {
        return;
    };
    let args = cx.list(list);

    // Collect positions of optarg and arg nodes only (ignore kwarg, restarg, etc.)
    let mut optarg_positions: Vec<usize> = Vec::new();
    let mut arg_positions: Vec<usize> = Vec::new();
    for (i, &arg_id) in args.iter().enumerate() {
        match cx.kind(arg_id) {
            NodeKind::Optarg { .. } => optarg_positions.push(i),
            NodeKind::Arg(_) => arg_positions.push(i),
            _ => {}
        }
    }

    if optarg_positions.is_empty() || arg_positions.is_empty() {
        return;
    }

    let max_arg_pos = *arg_positions.iter().max().unwrap();

    for &optarg_pos in &optarg_positions {
        // There can only be one group of optional arguments; break once we
        // pass the last required arg position (mirroring RuboCop's break).
        if optarg_pos > max_arg_pos {
            break;
        }
        cx.emit_offense(cx.range(args[optarg_pos]), MSG, None);
    }
}

#[cfg(test)]
mod tests {
    use super::OptionalArguments;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Bad cases -----

    #[test]
    fn flags_optional_before_required() {
        test::<OptionalArguments>().expect_offense(indoc! {"
            def foo(a = 1, b, c)
                    ^^^^^ Optional arguments should appear at the end of the argument list.
            end
        "});
    }

    #[test]
    fn flags_multiple_optionals_before_required() {
        test::<OptionalArguments>().expect_offense(indoc! {"
            def foo(a = 1, b = 2, c)
                    ^^^^^ Optional arguments should appear at the end of the argument list.
                           ^^^^^ Optional arguments should appear at the end of the argument list.
            end
        "});
    }

    #[test]
    fn flags_optional_at_start_before_two_required() {
        test::<OptionalArguments>().expect_offense(indoc! {"
            def foo(a = 1, b, c, d)
                    ^^^^^ Optional arguments should appear at the end of the argument list.
            end
        "});
    }

    #[test]
    fn flags_singleton_def_optional_before_required() {
        test::<OptionalArguments>().expect_offense(indoc! {"
            def self.foo(a = 1, b)
                         ^^^^^ Optional arguments should appear at the end of the argument list.
            end
        "});
    }

    // ----- Good cases -----

    #[test]
    fn accepts_optional_at_end() {
        test::<OptionalArguments>().expect_no_offenses(indoc! {"
            def baz(a, b, c = 1)
            end
        "});
    }

    #[test]
    fn accepts_all_optional() {
        test::<OptionalArguments>().expect_no_offenses(indoc! {"
            def foobar(a = 1, b = 2, c = 3)
            end
        "});
    }

    #[test]
    fn accepts_no_arguments() {
        test::<OptionalArguments>().expect_no_offenses(indoc! {"
            def foo
            end
        "});
    }

    #[test]
    fn accepts_only_required() {
        test::<OptionalArguments>().expect_no_offenses(indoc! {"
            def foo(a, b, c)
            end
        "});
    }

    #[test]
    fn accepts_required_then_optional() {
        test::<OptionalArguments>().expect_no_offenses(indoc! {"
            def foo(a, b = 1)
            end
        "});
    }

    #[test]
    fn ignores_kwargs_not_as_required_positional() {
        // kwoptarg is not counted as a required positional arg, so
        // an optarg after required positionals followed by only kwargs
        // does not create a false offense.
        test::<OptionalArguments>().expect_no_offenses(indoc! {"
            def foo(a, b = 1, c: 2)
            end
        "});
    }

    #[test]
    fn ignores_restarg_mixed_with_optional() {
        // restarg should be ignored in position tracking; optarg after
        // restarg is fine since there's no trailing required positional.
        test::<OptionalArguments>().expect_no_offenses(indoc! {"
            def foo(*rest, a = 1)
            end
        "});
    }

    #[test]
    fn flags_optional_before_required_ignoring_kwoptarg() {
        // kwoptarg after does not prevent flagging the optarg before arg.
        test::<OptionalArguments>().expect_offense(indoc! {"
            def foo(a = 1, b, c: 2)
                    ^^^^^ Optional arguments should appear at the end of the argument list.
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(OptionalArguments);
