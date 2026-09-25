//! `RSpec/SpecFilePathSuffix` — spec files must end with `_spec.rb`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/SpecFilePathSuffix
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_top_level_example_group` (`TopLevelGroup`: each
//!   top-level `spec_group?` block; the `example_group?` guard skips
//!   shared groups) with `correct_path?`
//!   (`expanded_file_path.end_with?('_spec.rb')`). A file holding a
//!   top-level example group whose path does not end with `_spec.rb`
//!   reports one file-level offense per `add_global_offense` with `Spec
//!   path should end with `_spec.rb`.` (verified vs 3.7.0, including the
//!   shared-examples-clean case). Upstream ships no autocorrect and none
//!   is added here.
//! ```
//!
//! ## Matched shapes
//!
//! File-scoped (`#[on_new_investigation]`): any top-level example-group
//! `Block` in the file triggers the suffix check.
//!
//! - `describe User` in `user.rb` — flagged (file-level offense).
//! - `describe User` in `user_spec.rb` — clean.
//! - `shared_examples_for 'foo'` in `user.rb` — shared group, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; renaming the file needs human
//! judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeKind, cop};

use crate::cops::rspec_helpers::{is_example_group_call, is_top_level_block};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpecFilePathSuffix;

#[cop(
    name = "RSpec/SpecFilePathSuffix",
    description = "Checks that spec file paths suffix are consistent and well-formed.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SpecFilePathSuffix {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let root = cx.root();
        let has_top_level_group = core::iter::once(root)
            .chain(cx.descendants(root))
            .any(|id| {
                let NodeKind::Block { call, .. } = *cx.kind(id) else {
                    return false;
                };
                // Upstream `on_top_level_example_group` fires per top-level
                // spec group but only `example_group?` nodes report, so
                // shared-only files stay clean.
                is_example_group_call(cx, call) && is_top_level_block(cx, id)
            });
        if !has_top_level_group {
            return;
        }
        if cx.file_path().ends_with("_spec.rb") {
            return;
        }
        cx.emit_file_offense("Spec path should end with `_spec.rb`.", None);
    }
}

#[cfg(test)]
mod tests {
    use super::SpecFilePathSuffix;
    use murphy_plugin_api::test_support::{indoc, run_cop, test};

    #[test]
    fn flags_example_group_in_non_spec_file() {
        // The default test path (`t.rb`) does not end with `_spec.rb`,
        // so a top-level group reports a file-level offense. File-level
        // offenses carry no caret range, so assert via `run_cop`.
        let offenses = run_cop::<SpecFilePathSuffix>(indoc! {r#"
                describe User do
                end
            "#});
        assert_eq!(offenses.len(), 1);
        assert_eq!(
            offenses[0].message,
            "Spec path should end with `_spec.rb`."
        );
        assert!(!offenses[0].has_location());
    }

    #[test]
    fn ignores_example_group_in_spec_file() {
        test::<SpecFilePathSuffix>()
            .with_file_path("spec/models/user_spec.rb")
            .expect_no_offenses(indoc! {r#"
                describe User do
                end
            "#});
    }

    #[test]
    fn ignores_shared_examples_in_non_spec_file() {
        test::<SpecFilePathSuffix>()
            .with_file_path("spec/models/user.rb")
            .expect_no_offenses(indoc! {r#"
                shared_examples_for 'foo' do
                end
            "#});
    }

    #[test]
    fn ignores_file_without_groups() {
        test::<SpecFilePathSuffix>()
            .with_file_path("spec/models/user.rb")
            .expect_no_offenses(indoc! {r#"
                puts 'hello'
            "#});
    }
}

murphy_plugin_api::submit_cop!(SpecFilePathSuffix);
