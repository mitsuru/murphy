//! `Lint/RedundantRequireStatement` — removes already-loaded core features.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantRequireStatement
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial v1 port reports literal `require` calls for features RuboCop
//!   treats as already loaded on modern Ruby and removes whole-line calls.
//!   Murphy does not currently expose TargetRubyVersion to cops, so version-
//!   sensitive gating and modifier-form autocorrection are documented v1 gaps.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

#[derive(Default)]
pub struct RedundantRequireStatement;

#[cop(
    name = "Lint/RedundantRequireStatement",
    description = "Checks for unnecessary require statements.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantRequireStatement {
    #[on_node(kind = "send", methods = ["require"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        if receiver.get().is_some() {
            return;
        }
        let [arg] = cx.call_arguments(node) else {
            return;
        };
        let NodeKind::Str(feature) = *cx.kind(*arg) else {
            return;
        };
        if !is_redundant_feature(cx.string_str(feature)) {
            return;
        };

        cx.emit_offense(
            cx.range(node),
            "Remove unnecessary `require` statement.",
            None,
        );
        cx.emit_edit(cx.range_by_whole_lines(cx.range(node), true), "");
    }
}

fn is_redundant_feature(feature: &str) -> bool {
    matches!(
        feature,
        "enumerator"
            | "thread"
            | "rational"
            | "complex"
            | "ruby2_keywords"
            | "fiber"
    )
}

murphy_plugin_api::submit_cop!(RedundantRequireStatement);

#[cfg(test)]
mod tests {
    use super::RedundantRequireStatement;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_removes_redundant_require() {
        test::<RedundantRequireStatement>().expect_correction(
            indoc! {r#"
                require 'thread'
                ^^^^^^^^^^^^^^^^ Remove unnecessary `require` statement.
                require 'json'
            "#},
            "require 'json'\n",
        );
    }

    #[test]
    fn flags_enumerator() {
        test::<RedundantRequireStatement>().expect_offense(indoc! {r#"
            require 'enumerator'
            ^^^^^^^^^^^^^^^^^^^^ Remove unnecessary `require` statement.
        "#});
    }

    #[test]
    fn accepts_non_redundant_require() {
        test::<RedundantRequireStatement>().expect_no_offenses("require 'json'\n");
    }

    #[test]
    fn accepts_default_gems_that_are_not_preloaded() {
        test::<RedundantRequireStatement>().expect_no_offenses(indoc! {r#"
            require 'set'
            require 'pathname'
        "#});
    }
}
