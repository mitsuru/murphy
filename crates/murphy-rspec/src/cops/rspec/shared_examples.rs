//! `RSpec/SharedExamples` — consistent shared-example name style (`string` vs `symbol`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/SharedExamples
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` (`shared_examples` matcher: a
//!   `SharedGroups.all` call with a bare or `RSpec` receiver, or an
//!   `Includes.all` call with a bare receiver, capturing the first
//!   arg) with `EnforcedStyle` (`string` default, `symbol` alternate).
//!   `string` style flags a leading `Sym` with `Prefer '<spaced>' over
//!   `:sym` to titleize shared examples.`; `symbol` style flags a
//!   leading `Str` with `Prefer :snaked over `"str"` to symbolize
//!   shared examples.`. The offense range is the name node per
//!   `add_offense(ast_node)`. Detection is at parity (verified vs
//!   3.7.0, including `RSpec.`-receiver and multi-arg shapes and the
//!   `Module` / `Class` clean cases); autocorrect (replace with the
//!   preferred spelling) is not ported in this batch — same convention
//!   as `RSpec/BeEql` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` naming shared examples. Default style `string`:
//!
//! - `it_behaves_like :foo_bar_baz` — flagged (prefer `'foo bar baz'`).
//! - `shared_examples :foo_bar_baz` — flagged.
//! - `RSpec.shared_examples :foo_bar_baz` — flagged.
//! - `it_behaves_like 'foo bar baz'` — preferred spelling, clean.
//! - `it_behaves_like FooBarBaz` — module title, clean.
//!
//! Alternate style `symbol` (`EnforcedStyle: symbol`):
//!
//! - `it_behaves_like 'foo bar baz'` — flagged (prefer `:foo_bar_baz`).
//! - `it_behaves_like :foo_bar_baz` — preferred spelling, clean.
//!
//! ## No autocorrect
//!
//! Upstream replaces with the preferred spelling. This batch reports
//! only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::is_rspec_or_bare_receiver;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SharedExamples;

#[derive(CopOptions)]
pub struct SharedExamplesOptions {
    #[option(
        name = "EnforcedStyle",
        default = "string",
        description = "Whether shared example names use string or symbol style."
    )]
    pub enforced_style: SharedExamplesStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum SharedExamplesStyle {
    #[option(value = "string")]
    String,
    #[option(value = "symbol")]
    Symbol,
}

#[cop(
    name = "RSpec/SharedExamples",
    description = "Checks for consistent style for shared example names.",
    default_severity = "warning",
    default_enabled = true,
    options = SharedExamplesOptions,
)]
impl SharedExamples {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
            return;
        };
        let name = cx.symbol_str(method);
        let is_shared_group = is_shared_group_name(name);
        let is_include = is_include_name(name);
        if is_shared_group {
            if !is_rspec_or_bare_receiver(cx, receiver) {
                return;
            }
        } else if is_include {
            if receiver != OptNodeId::NONE {
                return;
            }
        } else {
            return;
        }
        let Some(&first) = cx.list(args).first() else {
            return;
        };
        let opts = cx.options_or_default::<SharedExamplesOptions>();
        match opts.enforced_style {
            SharedExamplesStyle::String => {
                let NodeKind::Sym(sym) = *cx.kind(first) else {
                    return;
                };
                let value = cx.symbol_str(sym).to_owned();
                let prefer = format!("'{}'", value.replace('_', " "));
                let current = format!(":{value}");
                cx.emit_offense(
                    cx.range(first),
                    &format!("Prefer {prefer} over `{current}` to titleize shared examples."),
                    None,
                );
            }
            SharedExamplesStyle::Symbol => {
                let NodeKind::Str(str_id) = *cx.kind(first) else {
                    return;
                };
                let value = cx.string_str(str_id).to_owned();
                let prefer = format!(":{}", value.to_lowercase().replace(' ', "_"));
                let current = format!("\"{value}\"");
                cx.emit_offense(
                    cx.range(first),
                    &format!("Prefer {prefer} over `{current}` to symbolize shared examples."),
                    None,
                );
            }
        }
    }
}

/// `SharedGroups.all`: `shared_examples` / `shared_examples_for` /
/// `shared_context`.
fn is_shared_group_name(name: &str) -> bool {
    matches!(
        name,
        "shared_examples" | "shared_examples_for" | "shared_context"
    )
}

/// `Includes.all`: `it_behaves_like` / `it_should_behave_like` /
/// `include_examples` / `include_context`.
fn is_include_name(name: &str) -> bool {
    matches!(
        name,
        "it_behaves_like" | "it_should_behave_like" | "include_examples" | "include_context"
    )
}

#[cfg(test)]
mod tests {
    use super::{SharedExamples, SharedExamplesOptions, SharedExamplesStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn symbol_style() -> SharedExamplesOptions {
        SharedExamplesOptions {
            enforced_style: SharedExamplesStyle::Symbol,
        }
    }

    #[test]
    fn flags_symbol_title_default_style() {
        test::<SharedExamples>().expect_offense(indoc! {r#"
                it_behaves_like :foo_bar_baz
                                ^^^^^^^^^^^^ Prefer 'foo bar baz' over `:foo_bar_baz` to titleize shared examples.
            "#});
    }

    #[test]
    fn flags_shared_examples_symbol() {
        test::<SharedExamples>().expect_offense(indoc! {r#"
                shared_examples :foo_bar_baz
                                ^^^^^^^^^^^^ Prefer 'foo bar baz' over `:foo_bar_baz` to titleize shared examples.
            "#});
    }

    #[test]
    fn flags_rspec_receiver_symbol() {
        test::<SharedExamples>().expect_offense(indoc! {r#"
                RSpec.shared_examples :foo_bar_baz
                                      ^^^^^^^^^^^^ Prefer 'foo bar baz' over `:foo_bar_baz` to titleize shared examples.
            "#});
    }

    #[test]
    fn ignores_string_title_default_style() {
        test::<SharedExamples>().expect_no_offenses(indoc! {r#"
                it_behaves_like 'foo bar baz'
            "#});
    }

    #[test]
    fn ignores_module_title_default_style() {
        test::<SharedExamples>().expect_no_offenses(indoc! {r#"
                it_behaves_like FooBarBaz
            "#});
    }

    #[test]
    fn flags_string_title_symbol_style() {
        test::<SharedExamples>()
            .with_options(&symbol_style())
            .expect_offense(indoc! {r#"
                it_behaves_like 'foo bar baz'
                                ^^^^^^^^^^^^^ Prefer :foo_bar_baz over `"foo bar baz"` to symbolize shared examples.
            "#});
    }

    #[test]
    fn ignores_symbol_title_symbol_style() {
        test::<SharedExamples>()
            .with_options(&symbol_style())
            .expect_no_offenses(indoc! {r#"
                it_behaves_like :foo_bar_baz
            "#});
    }

    #[test]
    fn ignores_non_rspec_receiver() {
        test::<SharedExamples>().expect_no_offenses(indoc! {r#"
                Other.shared_examples :foo_bar_baz
            "#});
    }
}

murphy_plugin_api::submit_cop!(SharedExamples);
