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

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

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
        let Some((is_inequality, receiver, argument)) = object_id_comparison(node, cx) else {
            return;
        };

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

fn object_id_comparison(node: NodeId, cx: &Cx<'_>) -> Option<(bool, NodeId, NodeId)> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return None;
    };
    let comparison_method = cx.symbol_str(method);
    if comparison_method != "==" && comparison_method != "!=" {
        return None;
    }
    let lhs = object_id_receiver(receiver.get()?, cx)?;
    let args = cx.list(args);
    let [rhs_node] = args else {
        return None;
    };
    let rhs = object_id_receiver(*rhs_node, cx)?;
    Some((comparison_method == "!=", lhs, rhs))
}

fn object_id_receiver(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return None;
    };
    if cx.symbol_str(method) == "object_id" && cx.list(args).is_empty() {
        receiver.get()
    } else {
        None
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
}
