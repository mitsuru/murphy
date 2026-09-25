//! `RSpec/BeNil` — consistent `nil` matching style (`be_nil` vs `be(nil)`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/BeNil
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `be_nil_matcher?` (`(send nil? :be_nil)`) and
//!   `nil_value_expectation?` (`(send nil? :be nil)`) with
//!   `RESTRICT_ON_SEND = [:be, :be_nil]` and `EnforcedStyle` (`be_nil`
//!   default, `be` alternate). `be_nil` style flags bare `be(nil)`;
//!   `be` style flags bare zero-arg `be_nil`. Other `be_*` matchers
//!   (`be_truthy`, `be(1)`) never flag. The offense range is the matcher
//!   node per `add_offense(node)`. Detection is at parity; autocorrect
//!   (replace with the preferred spelling) is not ported in this batch —
//!   same convention as `RSpec/BeEql` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["be", "be_nil"]` (bare receiver
//! only). Default style `be_nil`:
//!
//! - `expect(foo).to be(nil)` — flagged (prefer `be_nil`).
//! - `expect(foo).to be_nil` — preferred spelling, not flagged.
//! - `expect(foo).to be(true)` — non-nil arg, not flagged.
//! - `expect(foo).to be_truthy` — different matcher, not flagged.
//!
//! Alternate style `be` (`EnforcedStyle: be`):
//!
//! - `expect(foo).to be_nil` — flagged (prefer `be(nil)`).
//! - `expect(foo).to be(nil)` — preferred spelling, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream replaces with the preferred spelling. This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct BeNil;

#[derive(CopOptions)]
pub struct BeNilOptions {
    #[option(
        name = "EnforcedStyle",
        default = "be_nil",
        description = "Whether to enforce be_nil or be(nil) for nil matching."
    )]
    pub enforced_style: BeNilStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum BeNilStyle {
    #[option(value = "be")]
    Be,
    #[option(value = "be_nil")]
    BeNil,
}

#[cop(
    name = "RSpec/BeNil",
    description = "Ensures a consistent style is used when matching nil.",
    default_severity = "warning",
    default_enabled = true,
    options = BeNilOptions,
)]
impl BeNil {
    #[on_node(kind = "send", methods = ["be", "be_nil"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver,
            method,
            args,
        } = *cx.kind(node)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        let opts = cx.options_or_default::<BeNilOptions>();
        let method_name = cx.symbol_str(method);
        match opts.enforced_style {
            BeNilStyle::BeNil => {
                if method_name != "be" {
                    return;
                }
                let arg_ids = cx.list(args);
                if arg_ids.len() != 1 {
                    return;
                }
                if !matches!(*cx.kind(arg_ids[0]), NodeKind::Nil) {
                    return;
                }
                cx.emit_offense(
                    cx.range(node),
                    "Prefer `be_nil` over `be(nil)`.",
                    None,
                );
            }
            BeNilStyle::Be => {
                if method_name != "be_nil" {
                    return;
                }
                if !cx.list(args).is_empty() {
                    return;
                }
                cx.emit_offense(
                    cx.range(node),
                    "Prefer `be(nil)` over `be_nil`.",
                    None,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BeNil, BeNilOptions, BeNilStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn be_style() -> BeNilOptions {
        BeNilOptions {
            enforced_style: BeNilStyle::Be,
        }
    }

    #[test]
    fn flags_be_nil_arg_in_default_style() {
        test::<BeNil>().expect_offense(indoc! {r#"
                expect(foo).to be(nil)
                               ^^^^^^^ Prefer `be_nil` over `be(nil)`.
            "#});
    }

    #[test]
    fn does_not_flag_be_nil_in_default_style() {
        test::<BeNil>().expect_no_offenses(indoc! {r#"
                expect(foo).to be_nil
            "#});
    }

    #[test]
    fn does_not_flag_be_with_other_values() {
        test::<BeNil>().expect_no_offenses(indoc! {r#"
                expect(foo).to be(true)
            "#});
        test::<BeNil>().expect_no_offenses(indoc! {r#"
                expect(foo).to be(1)
            "#});
        test::<BeNil>().expect_no_offenses(indoc! {r#"
                expect(foo).to be("yes")
            "#});
    }

    #[test]
    fn does_not_flag_be_truthy_in_default_style() {
        test::<BeNil>().expect_no_offenses(indoc! {r#"
                expect(foo).to be_truthy
            "#});
    }

    #[test]
    fn flags_be_nil_in_be_style() {
        test::<BeNil>()
            .with_options(&be_style())
            .expect_offense(indoc! {r#"
                expect(foo).to be_nil
                               ^^^^^^ Prefer `be(nil)` over `be_nil`.
            "#});
    }

    #[test]
    fn does_not_flag_be_nil_arg_in_be_style() {
        test::<BeNil>()
            .with_options(&be_style())
            .expect_no_offenses(indoc! {r#"
                expect(foo).to be(nil)
            "#});
    }

    #[test]
    fn does_not_flag_other_be_matchers_in_be_style() {
        test::<BeNil>()
            .with_options(&be_style())
            .expect_no_offenses(indoc! {r#"
                expect(foo).to be_truthy
            "#});
    }

    #[test]
    fn does_not_flag_receiver_be() {
        // Upstream requires a bare (`nil?`) receiver.
        test::<BeNil>().expect_no_offenses(indoc! {r#"
                expect(foo).to obj.be(nil)
            "#});
    }
}

murphy_plugin_api::submit_cop!(BeNil);
