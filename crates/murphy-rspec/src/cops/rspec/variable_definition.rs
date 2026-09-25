//! `RSpec/VariableDefinition` — memoized helper names are symbols or strings.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/VariableDefinition
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` gated by `InsideExampleGroup` with
//!   `variable_definition?` (`(send nil? {subject, subject!, let, let!}
//!   $({sym str dsym dstr} ...) ...)`): bare-receiver memoized helpers
//!   whose first arg is a `Sym` / `Str` / `Dsym` / `Dstr`. Under
//!   `EnforcedStyle: symbols` (default) a `Str` first arg flags per
//!   `add_offense(variable)` with `Use symbols for variable names.`;
//!   under `EnforcedStyle: strings` a `Sym` / `Dsym` first arg flags with
//!   `Use strings for variable names.` Interpolated `Dstr` never flags
//!   under `symbols`, and plain `Str` never flags under `strings`.
//!   `inside_example_group?` is simplified to "any ancestor `Block` is a
//!   spec group (example or shared, bare or `RSpec` receiver)" — the same
//!   simplification `RSpec/EmptyLineAfterSubject` carries. Detection is at
//!   parity (verified vs 3.7.0, including the multiline-string-clean and
//!   top-level-clean cases); autocorrect (replace with the preferred
//!   spelling) is not ported in this batch — same convention as
//!   `RSpec/BeNil` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["let", "let!", "subject",
//! "subject!"]` (bare receiver only) inside an example group. Default
//! style `symbols`:
//!
//! - `let('user_name') { }` — flagged (the string arg).
//! - `let(:user_name) { }` — symbol, clean.
//! - `let("user-#{id}") { }` — interpolated string, clean.
//! - Top-level `let('user_name') { }` — no group ancestor, clean.
//!
//! Alternate style `strings` (`EnforcedStyle: strings`):
//!
//! - `let(:user_name) { }` — flagged.
//! - `let(:"user-#{id}") { }` — interpolated symbol, flagged.
//! - `let('user_name') { }` — string, clean.
//!
//! ## No autocorrect
//!
//! Upstream replaces with the preferred spelling. This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::is_spec_group_call;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct VariableDefinition;

#[derive(CopOptions)]
pub struct VariableDefinitionOptions {
    #[option(
        name = "EnforcedStyle",
        default = "symbols",
        description = "Whether memoized helper names use symbols or strings."
    )]
    pub enforced_style: VariableDefinitionStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum VariableDefinitionStyle {
    #[option(value = "symbols")]
    Symbols,
    #[option(value = "strings")]
    Strings,
}

#[cop(
    name = "RSpec/VariableDefinition",
    description = "Checks that memoized helpers names are symbols or strings.",
    default_severity = "warning",
    default_enabled = true,
    options = VariableDefinitionOptions,
)]
impl VariableDefinition {
    #[on_node(kind = "send", methods = ["let", "let!", "subject", "subject!"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, args, ..
        } = *cx.kind(node)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if !is_inside_example_group(cx, node) {
            return;
        }
        let Some(&first) = cx.list(args).first() else {
            return;
        };
        let opts = cx.options_or_default::<VariableDefinitionOptions>();
        match opts.enforced_style {
            VariableDefinitionStyle::Symbols => {
                // `string?`: `str_type?` only — `Dstr` stays clean.
                if !matches!(*cx.kind(first), NodeKind::Str(_)) {
                    return;
                }
                cx.emit_offense(
                    cx.range(first),
                    "Use symbols for variable names.",
                    None,
                );
            }
            VariableDefinitionStyle::Strings => {
                // `symbol?`: `sym` or `dsym`.
                if !matches!(
                    *cx.kind(first),
                    NodeKind::Sym(_) | NodeKind::Dsym(_)
                ) {
                    return;
                }
                cx.emit_offense(
                    cx.range(first),
                    "Use strings for variable names.",
                    None,
                );
            }
        }
    }
}

/// Simplified `inside_example_group?`: `true` when any ancestor `Block`
/// is a spec group (example or shared) with an RSpec-or-bare receiver.
fn is_inside_example_group(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        if let NodeKind::Block { call, .. } = *cx.kind(anc)
            && is_spec_group_call(cx, call)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{VariableDefinition, VariableDefinitionOptions, VariableDefinitionStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn strings_style() -> VariableDefinitionOptions {
        VariableDefinitionOptions {
            enforced_style: VariableDefinitionStyle::Strings,
        }
    }

    #[test]
    fn flags_string_in_symbols_style() {
        test::<VariableDefinition>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  let('user_name') { 'Adam' }
                      ^^^^^^^^^^^ Use symbols for variable names.
                end
            "#});
    }

    #[test]
    fn ignores_symbol_in_symbols_style() {
        test::<VariableDefinition>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  let(:user_name) { 'Adam' }
                end
            "#});
    }

    #[test]
    fn ignores_interpolated_string_in_symbols_style() {
        test::<VariableDefinition>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  let("user-#{id}") { 'Adam' }
                end
            "#});
    }

    #[test]
    fn ignores_top_level_in_symbols_style() {
        test::<VariableDefinition>().expect_no_offenses(indoc! {r#"
                let('user_name') { 'Adam' }
            "#});
    }

    #[test]
    fn flags_symbol_in_strings_style() {
        test::<VariableDefinition>()
            .with_options(&strings_style())
            .expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  let(:user_name) { 'Adam' }
                      ^^^^^^^^^^ Use strings for variable names.
                end
            "#});
    }

    #[test]
    fn flags_dsym_in_strings_style() {
        test::<VariableDefinition>()
            .with_options(&strings_style())
            .expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  let(:"user-#{id}") { 'Adam' }
                      ^^^^^^^^^^^^^ Use strings for variable names.
                end
            "#});
    }

    #[test]
    fn ignores_string_in_strings_style() {
        test::<VariableDefinition>()
            .with_options(&strings_style())
            .expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  let('user_name') { 'Adam' }
                end
            "#});
    }

    #[test]
    fn ignores_top_level_in_strings_style() {
        test::<VariableDefinition>()
            .with_options(&strings_style())
            .expect_no_offenses(indoc! {r#"
                let(:user_name) { 'Adam' }
            "#});
    }

    #[test]
    fn flags_subject_string_in_symbols_style() {
        test::<VariableDefinition>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  subject('user') { create_user }
                          ^^^^^^ Use symbols for variable names.
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(VariableDefinition);
