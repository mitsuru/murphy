//! `RSpec/DescribeMethod` — the second argument to `describe` should name a method.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/DescribeMethod
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `second_string_literal_argument`
//!   (`(block (send #rspec? :describe _first ${str dstr} ...) ...)`)
//!   plus `method_name?` (`str` starting with `.` / `#`, or `dstr`
//!   whose first part is such a `str`) inside `on_top_level_group`.
//!   Receiver gating (bare plus `RSpec`/`::RSpec`) mirrors `#rspec?`;
//!   top-level gating (ancestors up to the root are only
//!   `Begin`/`Class`/`Module`) mirrors `TopLevelGroup`; the offense range
//!   is the second-arg node per `add_offense(argument)`. Non-string second
//!   args (`Int`, `Sym`, …) never match the literal capture, so they are
//!   clean. No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is an RSpec-or-bare `describe` with at
//! least two positional args and a `Str` / `Dstr` second arg, in a top-level
//! group:
//!
//! - `describe MyClass, 'do something' do ... end` — flagged.
//! - `RSpec.describe MyClass, 'bad' do ... end` — flagged.
//! - `describe MyClass, '#foo' do ... end` — method name, not flagged.
//! - `describe MyClass, '.foo' do ... end` — method name, not flagged.
//! - `describe MyClass, "#foo#{bar}" do ... end` — leading method prefix,
//!   not flagged.
//! - `describe MyClass, "#{bar}foo" do ... end` — interpolation first,
//!   flagged (mirrors upstream's leading-`str` match).
//! - `describe MyClass, 123 do ... end` — non-string, not flagged.
//! - `describe 'only one arg' do ... end` — no second arg, not flagged.
//! - `Other.describe MyClass, 'bad' do ... end` — non-RSpec receiver,
//!   not flagged.
//! - Nested `describe Other, 'bad'` inside another group — not top-level,
//!   not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; naming the tested method needs human
//! judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{is_rspec_or_bare_receiver, is_top_level_block};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DescribeMethod;

#[cop(
    name = "RSpec/DescribeMethod",
    description = "Checks that the second argument to `describe` specifies a method.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DescribeMethod {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_top_level_block(cx, node) {
            return;
        }
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
        if cx.symbol_str(method) != "describe" {
            return;
        }
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        let arg_ids = cx.list(args);
        if arg_ids.len() < 2 {
            return;
        }
        let second = arg_ids[1];
        if !matches!(
            *cx.kind(second),
            NodeKind::Str(_) | NodeKind::Dstr(_)
        ) {
            return;
        }
        if is_method_name_arg(cx, second) {
            return;
        }
        cx.emit_offense(
            cx.range(second),
            "The second argument to describe should be the method being tested. \'#instance\' or \'.class\'.",
            None,
        );
    }
}

/// `true` when `arg` names a method: a `Str` starting with `.` or `#`, or
/// a `Dstr` whose first part is such a `Str`.
///
/// Mirrors upstream `method_name?`
/// (`{(str #method_name_prefix?) (dstr (str #method_name_prefix?) ...)}`),
/// where the `dstr` arm requires the *leading* part to carry the prefix.
fn is_method_name_arg(cx: &Cx<'_>, arg: NodeId) -> bool {
    match *cx.kind(arg) {
        NodeKind::Str(id) => starts_with_method_prefix(cx.string_str(id)),
        NodeKind::Dstr(parts) => {
            let part_ids = cx.list(parts);
            let Some(&first_part) = part_ids.first() else {
                return false;
            };
            match *cx.kind(first_part) {
                NodeKind::Str(id) => starts_with_method_prefix(cx.string_str(id)),
                _ => false,
            }
        }
        _ => false,
    }
}

fn starts_with_method_prefix(s: &str) -> bool {
    s.starts_with('.') || s.starts_with('#')
}

#[cfg(test)]
mod tests {
    use super::DescribeMethod;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_plain_string_second_arg() {
        test::<DescribeMethod>().expect_offense(indoc! {r#"
                describe MyClass, 'do something' do
                                  ^^^^^^^^^^^^^^ The second argument to describe should be the method being tested. '#instance' or '.class'.
                end
            "#});
    }

    #[test]
    fn flags_rspec_describe_plain_string() {
        test::<DescribeMethod>().expect_offense(indoc! {r#"
                RSpec.describe MyClass, 'do something' do
                                        ^^^^^^^^^^^^^^ The second argument to describe should be the method being tested. '#instance' or '.class'.
                end
            "#});
    }

    #[test]
    fn does_not_flag_instance_method() {
        test::<DescribeMethod>().expect_no_offenses(indoc! {r#"
                describe MyClass, '#my_instance_method' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_class_method() {
        test::<DescribeMethod>().expect_no_offenses(indoc! {r#"
                describe MyClass, '.my_class_method' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_leading_interpolated_method() {
        test::<DescribeMethod>().expect_no_offenses(indoc! {r##"
                describe MyClass, "#foo#{bar}" do
                end
            "##});
    }

    #[test]
    fn flags_interpolation_first_second_arg() {
        // Upstream's `dstr` arm requires the *leading* part to be the
        // method-prefix `str`; interpolation-first never matches.
        test::<DescribeMethod>().expect_offense(indoc! {r##"
                describe MyClass, "#{bar}foo" do
                                  ^^^^^^^^^^^ The second argument to describe should be the method being tested. '#instance' or '.class'.
                end
            "##});
    }

    #[test]
    fn does_not_flag_non_string_second_arg() {
        // The upstream capture is `${str dstr}`; other node kinds never match.
        test::<DescribeMethod>().expect_no_offenses(indoc! {r#"
                describe MyClass, 123 do
                end
            "#});
    }

    #[test]
    fn does_not_flag_single_arg() {
        test::<DescribeMethod>().expect_no_offenses(indoc! {r#"
                describe 'only one arg' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_non_rspec_receiver() {
        test::<DescribeMethod>().expect_no_offenses(indoc! {r#"
                Other.describe MyClass, 'bad' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_nested_group() {
        // `on_top_level_group` only fires for top-level groups.
        test::<DescribeMethod>().expect_no_offenses(indoc! {r#"
                describe MyClass do
                  describe Other, 'bad' do
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(DescribeMethod);
