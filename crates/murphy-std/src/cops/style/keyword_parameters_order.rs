//! `Style/KeywordParametersOrder` — flags optional keyword parameters that
//! appear before required keyword parameters in the argument list.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/KeywordParametersOrder
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection: flags every `kwoptarg` that appears before the last `kwarg`
//!   in the parameter list. Covers `def`/`defs`/`block` argument lists,
//!   matching RuboCop's `on_kwoptarg` which fires for both method definitions
//!   and block parameters. No autocorrect — reordering keyword parameters is
//!   a "shuffle the AST" rewrite that risks non-idempotence in Murphy's
//!   fixpoint harness; deferred to a gap issue.
//!   `Enabled: true` in default.yml.
//! ```
//!
//! ## Matched shapes
//!
//! Method definitions (`def`/`def self.x`) and blocks where at least one
//! optional keyword parameter (`kwoptarg`) appears before a required keyword
//! parameter (`kwarg`):
//!
//! ```ruby
//! # bad
//! def some_method(first: false, second:, third: 10)
//! end
//!
//! # good
//! def some_method(second:, first: false, third: 10)
//! end
//!
//! # bad
//! do_something do |first: false, second:|
//! end
//!
//! # good
//! do_something do |second:, first: false|
//! end
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Place optional keyword parameters at the end of the parameters list.";

/// Stateless unit struct.
#[derive(Default)]
pub struct KeywordParametersOrder;

#[cop(
    name = "Style/KeywordParametersOrder",
    description = "Enforces that optional keyword parameters are placed at the end of the parameters list.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl KeywordParametersOrder {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_args(cx.def_arguments(node), cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check_args(cx.def_arguments(node), cx);
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_args(cx.block_arguments(node), cx);
    }
}

fn check_args(args_opt: murphy_plugin_api::OptNodeId, cx: &Cx<'_>) {
    let Some(args_id) = args_opt.get() else {
        return;
    };

    let NodeKind::Args(list) = *cx.kind(args_id) else {
        return;
    };
    let args = cx.list(list);

    // Collect positions of kwoptarg and kwarg nodes only.
    // Ignore kwrestarg, blockarg, regular arg/optarg/restarg, etc.
    let mut kwoptarg_positions: Vec<usize> = Vec::new();
    let mut kwarg_positions: Vec<usize> = Vec::new();
    for (i, &arg_id) in args.iter().enumerate() {
        match cx.kind(arg_id) {
            NodeKind::Kwoptarg { .. } => kwoptarg_positions.push(i),
            NodeKind::Kwarg(_) => kwarg_positions.push(i),
            _ => {}
        }
    }

    if kwoptarg_positions.is_empty() || kwarg_positions.is_empty() {
        return;
    }

    let max_kwarg_pos = *kwarg_positions.iter().max().unwrap();

    for &kwoptarg_pos in &kwoptarg_positions {
        if kwoptarg_pos > max_kwarg_pos {
            break;
        }
        cx.emit_offense(cx.range(args[kwoptarg_pos]), MSG, None);
    }
}

#[cfg(test)]
mod tests {
    use super::KeywordParametersOrder;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Bad cases (def) -----

    #[test]
    fn flags_kwoptarg_before_kwarg_in_def() {
        test::<KeywordParametersOrder>().expect_offense(indoc! {"
            def some_method(first: false, second:, third: 10)
                            ^^^^^^^^^^^^ Place optional keyword parameters at the end of the parameters list.
            end
        "});
    }

    #[test]
    fn flags_multiple_kwoptargs_before_kwarg() {
        test::<KeywordParametersOrder>().expect_offense(indoc! {"
            def foo(a: 1, b: 2, c:)
                    ^^^^ Place optional keyword parameters at the end of the parameters list.
                          ^^^^ Place optional keyword parameters at the end of the parameters list.
            end
        "});
    }

    #[test]
    fn flags_kwoptarg_before_kwarg_in_defs() {
        test::<KeywordParametersOrder>().expect_offense(indoc! {"
            def self.foo(first: false, second:)
                         ^^^^^^^^^^^^ Place optional keyword parameters at the end of the parameters list.
            end
        "});
    }

    #[test]
    fn flags_kwoptarg_before_kwarg_in_block() {
        test::<KeywordParametersOrder>().expect_offense(indoc! {"
            do_something do |first: false, second:|
                             ^^^^^^^^^^^^ Place optional keyword parameters at the end of the parameters list.
            end
        "});
    }

    #[test]
    fn flags_kwoptarg_before_kwarg_in_brace_block() {
        test::<KeywordParametersOrder>().expect_offense(indoc! {"
            do_something { |first: false, second:| }
                            ^^^^^^^^^^^^ Place optional keyword parameters at the end of the parameters list.
        "});
    }

    // ----- Good cases -----

    #[test]
    fn accepts_kwoptargs_at_end() {
        test::<KeywordParametersOrder>().expect_no_offenses(indoc! {"
            def some_method(second:, first: false, third: 10)
            end
        "});
    }

    #[test]
    fn accepts_all_kwoptarg() {
        test::<KeywordParametersOrder>().expect_no_offenses(indoc! {"
            def foo(a: 1, b: 2)
            end
        "});
    }

    #[test]
    fn accepts_all_kwarg() {
        test::<KeywordParametersOrder>().expect_no_offenses(indoc! {"
            def foo(a:, b:)
            end
        "});
    }

    #[test]
    fn accepts_no_keyword_params() {
        test::<KeywordParametersOrder>().expect_no_offenses(indoc! {"
            def foo(a, b = 1)
            end
        "});
    }

    #[test]
    fn accepts_good_block_order() {
        test::<KeywordParametersOrder>().expect_no_offenses(indoc! {"
            do_something do |second:, first: false|
            end
        "});
    }

    #[test]
    fn ignores_positional_params_mixed_with_kwargs() {
        // Regular positional args don't affect the kwarg/kwoptarg ordering check.
        test::<KeywordParametersOrder>().expect_no_offenses(indoc! {"
            def foo(a, b = 1, second:, first: false)
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(KeywordParametersOrder);
