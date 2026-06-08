//! `Lint/RedundantSafeNavigation` — replaces redundant `&.` with `.`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantSafeNavigation
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial v1 port covers safe navigation on `self`, non-nil literals,
//!   constants, guaranteed conversion receivers, and configured nil-safe
//!   predicate methods in conditions. RuboCop's InferNonNilReceiver,
//!   AllowedMethods/AdditionalNilMethods options, `||` default-literal removal,
//!   and broader data-flow analysis are documented v1 gaps.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

const MSG: &str = "Redundant safe navigation detected, use `.` instead.";
const NIL_SAFE_METHODS: &[&str] = &[
    "instance_of?",
    "kind_of?",
    "is_a?",
    "eql?",
    "respond_to?",
    "equal?",
];
const GUARANTEED_INSTANCE_METHODS: &[&str] = &["to_s", "to_i", "to_f", "to_a", "to_h"];

#[derive(Default)]
pub struct RedundantSafeNavigation;

#[cop(
    name = "Lint/RedundantSafeNavigation",
    description = "Checks for redundant safe navigation calls.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantSafeNavigation {
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { receiver, .. } = *cx.kind(node) else {
            return;
        };
        if !is_redundant_safe_navigation(node, receiver, cx) {
            return;
        }

        let dot = cx.loc(node).dot();
        if dot == Range::ZERO {
            return;
        }
        cx.emit_offense(dot, MSG, None);
        cx.emit_edit(dot, ".");
    }
}

fn is_redundant_safe_navigation(node: NodeId, receiver: NodeId, cx: &Cx<'_>) -> bool {
    let receiver = unwrap_begin(receiver, cx);
    assume_receiver_instance_exists(receiver, cx)
        || guaranteed_instance_receiver(receiver, cx)
        || (is_nil_safe_method(cx.method_name(node)) && is_condition(node, cx))
}

fn unwrap_begin(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    loop {
        match *cx.kind(node) {
            NodeKind::Begin(list) | NodeKind::Kwbegin(list) if cx.list(list).len() == 1 => {
                node = cx.list(list)[0];
            }
            _ => return node,
        }
    }
}

fn assume_receiver_instance_exists(receiver: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(receiver) {
        NodeKind::SelfExpr => true,
        NodeKind::Const { .. } => true,
        NodeKind::Nil => false,
        _ => cx.is_literal(receiver),
    }
}

fn guaranteed_instance_receiver(receiver: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(receiver), NodeKind::Send { .. })
        && cx
            .method_name(receiver)
            .is_some_and(|method| GUARANTEED_INSTANCE_METHODS.contains(&method))
}

fn is_nil_safe_method(method: Option<&str>) -> bool {
    method.is_some_and(|method| NIL_SAFE_METHODS.contains(&method))
}

fn is_condition(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.parent(node)
        .get()
        .is_some_and(|parent| match *cx.kind(parent) {
            NodeKind::If { cond, .. }
            | NodeKind::While { cond, .. }
            | NodeKind::Until { cond, .. } => cond == node,
            _ => false,
        })
}

murphy_plugin_api::submit_cop!(RedundantSafeNavigation);

#[cfg(test)]
mod tests {
    use super::RedundantSafeNavigation;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_self_receiver() {
        test::<RedundantSafeNavigation>().expect_correction(
            indoc! {r#"
                self&.foo
                    ^^ Redundant safe navigation detected, use `.` instead.
            "#},
            "self.foo\n",
        );
    }

    #[test]
    fn flags_and_corrects_literal_receiver() {
        test::<RedundantSafeNavigation>().expect_correction(
            indoc! {r#"
                'x'&.upcase
                   ^^ Redundant safe navigation detected, use `.` instead.
            "#},
            "'x'.upcase\n",
        );
    }

    #[test]
    fn flags_nil_safe_method_in_condition() {
        test::<RedundantSafeNavigation>().expect_correction(
            indoc! {r#"
                if attrs&.respond_to?(:[])
                        ^^ Redundant safe navigation detected, use `.` instead.
                  work
                end
            "#},
            "if attrs.respond_to?(:[])\n  work\nend\n",
        );
    }

    #[test]
    fn accepts_safe_navigation_that_can_return_nil() {
        test::<RedundantSafeNavigation>().expect_no_offenses("foo&.bar\n");
    }

    #[test]
    fn accepts_conversion_after_safe_navigation() {
        test::<RedundantSafeNavigation>().expect_no_offenses("foo&.to_s&.upcase\n");
    }

    #[test]
    fn flags_parenthesized_non_nil_receivers() {
        test::<RedundantSafeNavigation>()
            .expect_correction(
                indoc! {r#"
                    (self)&.foo
                          ^^ Redundant safe navigation detected, use `.` instead.
                "#},
                "(self).foo\n",
            )
            .expect_correction(
                indoc! {r#"
                    (('x'))&.upcase
                           ^^ Redundant safe navigation detected, use `.` instead.
                "#},
                "(('x')).upcase\n",
            );
    }
}
