//! `Rails/IgnoredColumnsAssignment` — use `+=` for `ignored_columns`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/IgnoredColumnsAssignment
//! upstream_version_checked: 2.35.0
//! version_added: "2.17"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:ignored_columns=]
//!   gating, offense on the assignment operator (`=`), autocorrect to `+=`.
//!   Any receiver matches (upstream does not gate on receiver; the idiom is
//!   `self.ignored_columns =`). Upstream Include path gating absent in Murphy.
//! ```
//!
//! Looks for assignments of `ignored_columns` that may override previous
//! assignments. Overwriting is usually a mistake; append with `+=` instead.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct IgnoredColumnsAssignment;

#[cop(
    name = "Rails/IgnoredColumnsAssignment",
    description = "Use `+=` instead of `=` for `ignored_columns`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl IgnoredColumnsAssignment {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[ignored_columns=]`.
    #[on_node(kind = "send", methods = ["ignored_columns="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { method, .. } = *cx.kind(node) else {
            return;
        };
        if cx.symbol_str(method) != "ignored_columns=" {
            return;
        }
        let op = cx.assignment_operator_loc(node);
        if op == murphy_plugin_api::Range::ZERO {
            return;
        }
        cx.emit_offense(op, "Use `+=` instead of `=`.", None);
        cx.emit_edit(op, "+=");
    }
}

#[cfg(test)]
mod tests {
    use super::IgnoredColumnsAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_single_assignment() {
        test::<IgnoredColumnsAssignment>().expect_offense(indoc! {r#"
            class User < ActiveRecord::Base
              self.ignored_columns = [:one]
                                   ^ Use `+=` instead of `=`.
            end
        "#});
    }

    #[test]
    fn corrects_single_assignment() {
        test::<IgnoredColumnsAssignment>().expect_correction(
            indoc! {r#"
                class User < ActiveRecord::Base
                  self.ignored_columns = [:one]
                                       ^ Use `+=` instead of `=`.
                end
            "#},
            "class User < ActiveRecord::Base\n  self.ignored_columns += [:one]\nend\n",
        );
    }

    #[test]
    fn flags_twice() {
        test::<IgnoredColumnsAssignment>().expect_offense(indoc! {r#"
            class User < ActiveRecord::Base
              self.ignored_columns = [:one]
                                   ^ Use `+=` instead of `=`.
              self.ignored_columns = [:two]
                                   ^ Use `+=` instead of `=`.
            end
        "#});
    }

    #[test]
    fn flags_after_append() {
        test::<IgnoredColumnsAssignment>().expect_offense(indoc! {r#"
            class User < ActiveRecord::Base
              self.ignored_columns += [:one]
              self.ignored_columns = [:two]
                                   ^ Use `+=` instead of `=`.
            end
        "#});
    }

    #[test]
    fn does_not_flag_append() {
        test::<IgnoredColumnsAssignment>().expect_no_offenses(indoc! {r#"
            class User < ActiveRecord::Base
              self.ignored_columns += [:one]
              self.ignored_columns += [:two]
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(IgnoredColumnsAssignment);
