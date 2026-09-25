//! `RSpec/MissingExampleGroupArgument` — example groups need a first argument.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/MissingExampleGroupArgument
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example_group?`: RSpec-or-bare
//!   `ExampleGroups.all` — regular, skipped (`xdescribe` / `xcontext` /
//!   `xfeature`), and focused (`fdescribe` / `fcontext` / `ffeature`))
//!   gated on an empty `Send` argument list per
//!   `node.send_node.arguments?`. The whole block flags per
//!   `add_offense(node)` with `The first argument to `%<method>s`
//!   should not be empty.` No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is an RSpec-or-bare example-group
//! selector with no arguments:
//!
//! - `describe do; end` — flagged (whole block).
//! - `RSpec.describe do; end` — explicit receiver, flagged.
//! - `xdescribe do; end` — skipped groups count, flagged.
//! - `describe() do; end` — empty parens, flagged.
//! - `describe TestedClass do; end` — has an argument, clean.
//! - `describe "a feature" do; end` — string subject, clean.
//! - `shared_examples do; end` — not an example group, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; naming the group needs human
//! judgement about what is under test.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::is_example_group_call;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MissingExampleGroupArgument;

#[cop(
    name = "RSpec/MissingExampleGroupArgument",
    description = "Checks that the first argument to an example group is not empty.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl MissingExampleGroupArgument {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        if !is_example_group_call(cx, call) {
            return;
        }
        let NodeKind::Send { method, args, .. } = *cx.kind(call) else {
            return;
        };
        if !cx.list(args).is_empty() {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            &format!(
                "The first argument to `{}` should not be empty.",
                cx.symbol_str(method)
            ),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::MissingExampleGroupArgument;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_describe_without_argument() {
        test::<MissingExampleGroupArgument>().expect_offense(indoc! {r#"
                describe do; end
                ^^^^^^^^^^^^^^^^ The first argument to `describe` should not be empty.
            "#});
    }

    #[test]
    fn flags_explicit_rspec_describe_without_argument() {
        test::<MissingExampleGroupArgument>().expect_offense(indoc! {r#"
                RSpec.describe do; end
                ^^^^^^^^^^^^^^^^^^^^^^ The first argument to `describe` should not be empty.
            "#});
    }

    #[test]
    fn flags_skipped_group_without_argument() {
        test::<MissingExampleGroupArgument>().expect_offense(indoc! {r#"
                xdescribe do; end
                ^^^^^^^^^^^^^^^^^ The first argument to `xdescribe` should not be empty.
            "#});
    }

    #[test]
    fn flags_empty_parens_without_argument() {
        test::<MissingExampleGroupArgument>().expect_offense(indoc! {r#"
                describe() do; end
                ^^^^^^^^^^^^^^^^^^ The first argument to `describe` should not be empty.
            "#});
    }

    #[test]
    fn ignores_describe_with_class() {
        test::<MissingExampleGroupArgument>().expect_no_offenses(indoc! {r#"
                describe TestedClass do; end
            "#});
    }

    #[test]
    fn ignores_describe_with_string() {
        test::<MissingExampleGroupArgument>().expect_no_offenses(indoc! {r#"
                describe "A feature example" do; end
            "#});
    }

    #[test]
    fn ignores_shared_groups() {
        // `shared_examples` is not an `example_group?` (verified vs
        // 3.7.0).
        test::<MissingExampleGroupArgument>().expect_no_offenses(indoc! {r#"
                shared_examples do; end
            "#});
    }
}

murphy_plugin_api::submit_cop!(MissingExampleGroupArgument);
