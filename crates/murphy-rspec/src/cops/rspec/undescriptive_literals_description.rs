//! `RSpec/UndescriptiveLiteralsDescription` — descriptions must be descriptive.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/UndescriptiveLiteralsDescription
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example_groups_or_example?`: a `Block`
//!   whose call is an example-group / example selector with an
//!   RSpec-or-bare receiver, capturing the first arg). The arg flags per
//!   `add_offense(arg)` with `Description should be descriptive.` when it
//!   is an `Xstr` (backticks), `Int`, or `Regexp` node. `Float`, `Str`
//!   (including interpolated `Dstr`), and `Const` descriptions stay
//!   clean. Detection is at parity (verified vs 3.7.0, including the
//!   float-clean and `RSpec.describe`-receiver cases); upstream ships no
//!   autocorrect and none is added here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block`:
//!
//! - `` describe `time` do; end `` — flagged (the backtick arg).
//! - `context /when foo/ do; end` — flagged (the regexp arg).
//! - `it 10000 do; end` — flagged (the int arg).
//! - `it 3.14 do; end` — float, clean.
//! - `describe Foo do; end` — const, clean.
//! - `it 'does something' do; end` — string, clean.
//! - `context "when #{foo} is bar" do; end` — interpolated, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; writing a description needs human
//! judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{
    is_example_group_name, is_example_name, is_rspec_or_bare_receiver,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct UndescriptiveLiteralsDescription;

#[cop(
    name = "RSpec/UndescriptiveLiteralsDescription",
    description = "Description should be descriptive.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UndescriptiveLiteralsDescription {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(call)
        else {
            return;
        };
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        let name = cx.symbol_str(method);
        if !is_example_group_name(name) && !is_example_name(name) {
            return;
        }
        let Some(&first) = cx.list(args).first() else {
            return;
        };
        // Upstream `offense?`: `%i[xstr int regexp]`.
        if !matches!(
            *cx.kind(first),
            NodeKind::Xstr(_) | NodeKind::Int(_) | NodeKind::Regexp { .. }
        ) {
            return;
        }
        cx.emit_offense(
            cx.range(first),
            "Description should be descriptive.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::UndescriptiveLiteralsDescription;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_backtick_description() {
        test::<UndescriptiveLiteralsDescription>().expect_offense(indoc! {r#"
                describe `time` do
                         ^^^^^^ Description should be descriptive.
                end
            "#});
    }

    #[test]
    fn flags_regexp_description() {
        test::<UndescriptiveLiteralsDescription>().expect_offense(indoc! {r#"
                context /when foo/ do
                        ^^^^^^^^^^ Description should be descriptive.
                end
            "#});
    }

    #[test]
    fn flags_int_description() {
        test::<UndescriptiveLiteralsDescription>().expect_offense(indoc! {r#"
                it 10000 do
                   ^^^^^ Description should be descriptive.
                end
            "#});
    }

    #[test]
    fn ignores_float_description() {
        test::<UndescriptiveLiteralsDescription>().expect_no_offenses(indoc! {r#"
                it 3.14 do
                end
            "#});
    }

    #[test]
    fn ignores_const_description() {
        test::<UndescriptiveLiteralsDescription>().expect_no_offenses(indoc! {r#"
                describe Foo do
                end
            "#});
    }

    #[test]
    fn ignores_string_description() {
        test::<UndescriptiveLiteralsDescription>().expect_no_offenses(indoc! {r#"
                it 'does something' do
                end
            "#});
    }

    #[test]
    fn ignores_interpolated_description() {
        test::<UndescriptiveLiteralsDescription>().expect_no_offenses(indoc! {r#"
                context "when #{foo} is bar" do
                end
            "#});
    }

    #[test]
    fn flags_rspec_receiver() {
        test::<UndescriptiveLiteralsDescription>().expect_offense(indoc! {r#"
                RSpec.describe `time` do
                               ^^^^^^ Description should be descriptive.
                end
            "#});
    }

    #[test]
    fn ignores_explicit_receiver() {
        test::<UndescriptiveLiteralsDescription>().expect_no_offenses(indoc! {r#"
                Other.describe `time` do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(UndescriptiveLiteralsDescription);
