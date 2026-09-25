//! `RSpec/ContextMethod` — `context` should not describe methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ContextMethod
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `context_method` (`(block (send #rspec? :context
//!   ${(str #method_name?) (dstr (str #method_name?) ...)} ...) ...)`):
//!   RSpec-or-bare `context` whose first arg is a `Str` starting with `.`
//!   or `#`, or a `Dstr` whose first part is such a `Str`. Offense range
//!   is the first-arg node per `add_offense(context)`. Detection is at
//!   parity; autocorrect (replace `context` with `describe`) is not ported
//!   in this batch — same convention as `RSpec/Focus` (status: partial,
//!   autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block`. Flags when the call is an RSpec-or-bare
//! `context` and the first positional arg names a method:
//!
//! - `context '#foo' do ... end` — flagged.
//! - `context '.foo' do ... end` — flagged.
//! - `RSpec.context '#foo' do ... end` — flagged.
//! - `context "#foo#{bar}" do ... end` — flagged (first `Dstr` part).
//! - `context 'foo' do ... end` — plain description, not flagged.
//! - `describe '#foo' do ... end` — different selector, not flagged.
//! - `Other.context '#foo' do ... end` — non-RSpec receiver, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream replaces the `context` selector with `describe`. This batch
//! reports only; the edit is trivial but out of scope for the port batch.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::is_rspec_or_bare_receiver;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ContextMethod;

#[cop(
    name = "RSpec/ContextMethod",
    description = "Use describe for testing methods.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ContextMethod {
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
        if cx.symbol_str(method) != "context" {
            return;
        }
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        let arg_ids = cx.list(args);
        let Some(&first) = arg_ids.first() else {
            return;
        };
        if !is_method_name_arg(cx, first) {
            return;
        }
        cx.emit_offense(
            cx.range(first),
            "Use `describe` for testing methods.",
            None,
        );
    }
}

/// `true` when `arg` is a method-name literal: a `Str` starting with `.`
/// or `#`, or a `Dstr` whose first part is such a `Str`.
///
/// Mirrors upstream `method_name?` (`description.start_with?('.', '#')`)
/// applied to the `str` / leading `dstr` part.
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
    use super::ContextMethod;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_context_with_instance_method() {
        test::<ContextMethod>().expect_offense(indoc! {r#"
                context '#foo' do
                        ^^^^^^ Use `describe` for testing methods.
                end
            "#});
    }

    #[test]
    fn flags_context_with_class_method() {
        test::<ContextMethod>().expect_offense(indoc! {r#"
                context '.foo' do
                        ^^^^^^ Use `describe` for testing methods.
                end
            "#});
    }

    #[test]
    fn flags_rspec_context_with_method() {
        test::<ContextMethod>().expect_offense(indoc! {r#"
                RSpec.context '#foo' do
                              ^^^^^^ Use `describe` for testing methods.
                end
            "#});
    }

    #[test]
    fn flags_context_with_interpolated_method() {
        test::<ContextMethod>().expect_offense(indoc! {r##"
                context "#foo#{bar}" do
                        ^^^^^^^^^^^^ Use `describe` for testing methods.
                end
            "##});
    }

    #[test]
    fn does_not_flag_plain_description() {
        test::<ContextMethod>().expect_no_offenses(indoc! {r#"
                context 'foo' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_describe_with_method() {
        test::<ContextMethod>().expect_no_offenses(indoc! {r#"
                describe '#foo' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_non_rspec_receiver() {
        test::<ContextMethod>().expect_no_offenses(indoc! {r#"
                Other.context '#foo' do
                end
            "#});
    }

    #[test]
    fn does_not_flag_context_without_args() {
        test::<ContextMethod>().expect_no_offenses(indoc! {r#"
                context do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ContextMethod);
