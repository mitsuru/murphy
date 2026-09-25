//! `Rails/WhereNotWithMultipleConditions` — flag `where.not` with multiple conditions.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/WhereNotWithMultipleConditions
//! upstream_version_checked: 2.35.0
//! version_added: "2.17"
//! version_changed: "2.18"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send` with RESTRICT_ON_SEND [:not]:
//!   `where.not(hash)` where the hash has >= 2 pairs, or a single pair
//!   whose value is a Hash that recursively has multiple conditions.
//!   Offense range is the `where` selector through the `not` call end.
//!   No autocorrect. Disabled upstream by default (`Enabled: pending`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct WhereNotWithMultipleConditions;

const MSG: &str = "Use a SQL statement instead of `where.not` with multiple conditions.";

#[cop(
    name = "Rails/WhereNotWithMultipleConditions",
    description = "Do not use `where.not(...)` with multiple conditions.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl WhereNotWithMultipleConditions {
    #[on_node(kind = "send", methods = ["not"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("not") {
        return;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    if !matches!(*cx.kind(recv), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    if cx.method_name(recv) != Some("where") {
        return;
    }
    let args = cx.call_arguments(node);
    let Some(&first) = args.first() else {
        return;
    };
    if !matches!(*cx.kind(first), NodeKind::Hash(_)) {
        return;
    }
    if !multiple_conditions(cx, first) {
        return;
    }
    let range = Range {
        start: cx.selector(recv).start,
        end: cx.range(node).end,
    };
    cx.emit_offense(range, MSG, None);
}

fn multiple_conditions(cx: &Cx<'_>, hash: NodeId) -> bool {
    let pairs = cx.hash_pairs(hash);
    if pairs.len() >= 2 {
        return true;
    }
    if pairs.len() != 1 {
        return false;
    }
    let pair = pairs[0];
    if !matches!(*cx.kind(pair), NodeKind::Pair { .. }) {
        return false;
    }
    let NodeKind::Pair { value, .. } = *cx.kind(pair) else {
        return false;
    };
    if !matches!(*cx.kind(value), NodeKind::Hash(_)) {
        return false;
    }
    multiple_conditions(cx, value)
}

#[cfg(test)]
mod tests {
    use super::WhereNotWithMultipleConditions;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_two_conditions() {
        test::<WhereNotWithMultipleConditions>().expect_offense(indoc! {r#"
            User.where.not(trashed: true, role: 'admin')
                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use a SQL statement instead of `where.not` with multiple conditions.
        "#});
    }

    #[test]
    fn flags_array_value_two_conditions() {
        test::<WhereNotWithMultipleConditions>().expect_offense(indoc! {r#"
            User.where.not(trashed: true, role: ['moderator', 'admin'])
                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use a SQL statement instead of `where.not` with multiple conditions.
        "#});
    }

    #[test]
    fn flags_nested_multiple() {
        test::<WhereNotWithMultipleConditions>().expect_offense(indoc! {r#"
            User.joins(:posts).where.not(posts: { trashed: true, title: 'Rails' })
                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use a SQL statement instead of `where.not` with multiple conditions.
        "#});
    }

    #[test]
    fn allows_single_condition() {
        test::<WhereNotWithMultipleConditions>()
            .expect_no_offenses("User.where.not(trashed: true)\n");
    }

    #[test]
    fn allows_single_nested_single() {
        test::<WhereNotWithMultipleConditions>()
            .expect_no_offenses("User.where.not(posts: { trashed: true })\n");
    }

    #[test]
    fn allows_chained_single() {
        test::<WhereNotWithMultipleConditions>().expect_no_offenses(
            "User.where.not(trashed: true).where.not(role: 'admin')\n",
        );
    }

    #[test]
    fn allows_sql_string() {
        test::<WhereNotWithMultipleConditions>().expect_no_offenses(
            "User.where.not('trashed = ? OR role = ?', true, 'admin')\n",
        );
    }

    #[test]
    fn no_offense_bare_not() {
        test::<WhereNotWithMultipleConditions>().expect_no_offenses("users.not(name: 'x', age: 1)\n");
    }
}

murphy_plugin_api::submit_cop!(WhereNotWithMultipleConditions);
