//! `Lint/IdentityComparison` — prefers identity comparison over comparing `object_id` values.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/IdentityComparison
//! upstream_version_checked: master
//! status: verified
//! gap_issues: []
//! notes: >
//!   Matches RuboCop's `==` / `!=` object_id comparison detection, receiverless
//!   `object_id` guards, offense messages, and autocorrection to `equal?` /
//!   `!equal?`.
//! ```

use murphy_plugin_api::{cop, def_node_matcher, Cx, NoOptions, NodeId};

// RuboCop parity: mirrors `Lint/IdentityComparison`'s `object_id_comparison`
// node matcher, `(send (send _lhs :object_id) ${:== :!=} (send _rhs :object_id))`.
// We capture the two receivers (for the `equal?` autocorrect) rather than the
// operator symbol — the operator is recovered from `cx.method_name(node)`.
// `$_` does not bind an absent receiver, so a receiverless `object_id` is not
// matched, preserving the prior hand-rolled `receiver.get()?` behaviour.
def_node_matcher!(
    object_id_comparison,
    "(send (send $_ :object_id) {:== :!=} (send $_ :object_id))"
);

#[derive(Default)]
pub struct IdentityComparison;

#[cop(
    name = "Lint/IdentityComparison",
    description = "Prefer `equal?` over `==` when comparing `object_id`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl IdentityComparison {
    #[on_node(kind = "send", methods = ["==", "!="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let Some((receiver, argument)) = object_id_comparison(node, cx) else {
            return;
        };

        let is_inequality = cx.method_name(node) == Some("!=");
        let comparison_method = if is_inequality { "!=" } else { "==" };
        let bang = if is_inequality { "!" } else { "" };
        let message = format!(
            "Use `{bang}equal?` instead of `{comparison_method}` when comparing `object_id`."
        );
        cx.emit_offense(cx.range(node), &message, None);

        let replacement = format!(
            "{bang}{}.equal?({})",
            cx.raw_source(cx.range(receiver)),
            cx.raw_source(cx.range(argument))
        );
        cx.emit_edit(cx.range(node), &replacement);
    }
}

murphy_plugin_api::submit_cop!(IdentityComparison);

#[cfg(test)]
mod tests {
    use super::IdentityComparison;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_object_id_equality() {
        test::<IdentityComparison>().expect_correction(
            indoc! {r#"
                foo.object_id == bar.object_id
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `equal?` instead of `==` when comparing `object_id`.
            "#},
            "foo.equal?(bar)\n",
        );
    }

    #[test]
    fn flags_and_corrects_object_id_inequality() {
        test::<IdentityComparison>().expect_correction(
            indoc! {r#"
                foo.object_id != bar.object_id
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!equal?` instead of `!=` when comparing `object_id`.
            "#},
            "!foo.equal?(bar)\n",
        );
    }

    #[test]
    fn accepts_non_object_id_comparisons_and_bare_object_id() {
        test::<IdentityComparison>()
            .expect_no_offenses("foo.object_id == bar.do_something\n")
            .expect_no_offenses("foo.do_something != bar.object_id\n")
            .expect_no_offenses("object_id == bar.object_id\n")
            .expect_no_offenses("foo.object_id != object_id\n")
            .expect_no_offenses("foo.equal?(bar)\n")
            .expect_no_offenses("!foo.equal?(bar)\n");
    }

    // --- Boundary characterization (murphy-vn3o): pin the exact node set the
    // hand-rolled predicate matches, so the def_node_matcher! refactor can be
    // proven equivalent (not just "didn't break the happy-path pins").

    #[test]
    fn boundary_inner_object_id_with_arg_not_flagged() {
        // `object_id` must be argument-less. `object_id(x)` is a different call;
        // RuboCop's `(send _ :object_id)` has no arg slot, so it rejects it too.
        test::<IdentityComparison>()
            .expect_no_offenses("foo.object_id(x) == bar.object_id\n")
            .expect_no_offenses("foo.object_id == bar.object_id(y)\n");
    }

    #[test]
    fn boundary_safe_navigation_object_id_not_flagged() {
        // `&.object_id` lowers to a csend, not a send; RuboCop's `(send ...)`
        // pattern does not match csend, and neither does the hand-rolled check.
        test::<IdentityComparison>()
            .expect_no_offenses("foo&.object_id == bar.object_id\n")
            .expect_no_offenses("foo.object_id == bar&.object_id\n");
    }

    #[test]
    fn boundary_extra_argument_on_comparison_not_flagged() {
        // The comparison send must have exactly one argument.
        test::<IdentityComparison>().expect_no_offenses("foo.object_id.==(bar.object_id, baz)\n");
    }
}
