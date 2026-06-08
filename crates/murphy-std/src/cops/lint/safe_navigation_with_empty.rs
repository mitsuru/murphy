//! `Lint/SafeNavigationWithEmpty` — avoid `&.empty?` in conditionals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/SafeNavigationWithEmpty
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial port covers non-chained `receiver&.empty?` as an `if`/`unless`
//!   condition. Autocorrect is emitted only for side-effect-free receiver
//!   shapes that can be safely evaluated twice.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

#[derive(Default)]
pub struct SafeNavigationWithEmpty;

#[cop(
    name = "Lint/SafeNavigationWithEmpty",
    description = "Avoid `&.empty?` in conditionals.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SafeNavigationWithEmpty {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::If { cond, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Csend {
            receiver, method, ..
        } = *cx.kind(cond)
        else {
            return;
        };
        if cx.symbol_str(method) != "empty?" || matches!(cx.kind(receiver), NodeKind::Csend { .. })
        {
            return;
        }
        cx.emit_offense(
            cx.range(cond),
            "Avoid calling `empty?` with the safe navigation operator in conditionals.",
            None,
        );
        if is_safe_to_duplicate(receiver, cx) {
            let receiver_source = cx.raw_source(cx.range(receiver));
            let replacement = format!("{receiver_source} && {receiver_source}.empty?");
            cx.emit_edit(cx.range(cond), &replacement);
        }
    }
}

fn is_safe_to_duplicate(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::SelfExpr | NodeKind::Const { .. }) || cx.is_variable(node)
}

murphy_plugin_api::submit_cop!(SafeNavigationWithEmpty);

#[cfg(test)]
mod tests {
    use super::SafeNavigationWithEmpty;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_safe_navigation_empty_in_condition() {
        test::<SafeNavigationWithEmpty>().expect_correction(
            indoc! {r#"
                foo = []
                return unless foo&.empty?
                              ^^^^^^^^^^^ Avoid calling `empty?` with the safe navigation operator in conditionals.
            "#},
            "foo = []\nreturn unless foo && foo.empty?\n",
        );
    }

    #[test]
    fn accepts_safe_navigation_empty_outside_condition() {
        test::<SafeNavigationWithEmpty>().expect_no_offenses("empty = foo&.empty?\n");
    }

    #[test]
    fn does_not_correct_method_receiver() {
        test::<SafeNavigationWithEmpty>().expect_no_corrections("return if next_item&.empty?\n");
    }
}
