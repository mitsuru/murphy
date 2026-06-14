//! `Lint/SafeNavigationConsistency` - keep safe navigation consistent in conditions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/SafeNavigationConsistency
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial v1 port covers simple two-operand `&&` and `||` conditions where
//!   both operands are dot/safe-navigation calls on the same receiver. Operands
//!   are matched by their *immediate* receiver source, mirroring RuboCop's
//!   `receiver_name_as_key` (`method.receiver.source`): a multi-level chain like
//!   `@resource&.account&.suspended?` keys on `@resource&.account`, so it is not
//!   grouped with `@resource.local?` (which keys on `@resource`) and no offense
//!   fires (murphy-wcdv). It autocorrects only explicit `.`/`&.` operator
//!   ranges. RuboCop's recursive operand collection, grouped conditions,
//!   operator calls, assignment calls, configured AllowedMethods, and full
//!   nil-method handling are documented v1 gaps.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

const USE_DOT_MSG: &str = "Use `.` instead of unnecessary `&.`.";
const USE_SAFE_NAVIGATION_MSG: &str = "Use `&.` for consistency with safe navigation.";

const NIL_SAFE_METHODS: &[&str] = &["nil?", "blank?", "present?", "try", "try!", "presence"];

#[derive(Default)]
pub struct SafeNavigationConsistency;

#[cop(
    name = "Lint/SafeNavigationConsistency",
    description = "Keep safe navigation consistent in `&&` and `||` conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SafeNavigationConsistency {
    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::And { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        check_pair(lhs, rhs, LogicalOp::And, cx);
    }

    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Or { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        check_pair(lhs, rhs, LogicalOp::Or, cx);
    }
}

#[derive(Clone, Copy)]
enum LogicalOp {
    And,
    Or,
}

struct CallInfo<'a> {
    safe_navigation: bool,
    /// Source of this call's *immediate* receiver — RuboCop groups operands by
    /// `method.receiver.source` (`receiver_name_as_key`), so the comparison must
    /// use the outermost call's direct receiver, not a recursively-resolved base
    /// (murphy-wcdv).
    receiver_source: &'a str,
    operator: Range,
}

fn check_pair(lhs: NodeId, rhs: NodeId, op: LogicalOp, cx: &Cx<'_>) {
    let Some(lhs_call) = call_info(lhs, cx) else {
        return;
    };
    let Some(rhs_call) = call_info(rhs, cx) else {
        return;
    };
    if lhs_call.receiver_source != rhs_call.receiver_source {
        return;
    }

    match op {
        LogicalOp::And if rhs_call.safe_navigation => {
            emit_operator_offense(rhs_call.operator, ".", USE_DOT_MSG, cx)
        }
        LogicalOp::Or if lhs_call.safe_navigation && !rhs_call.safe_navigation => {
            emit_operator_offense(rhs_call.operator, "&.", USE_SAFE_NAVIGATION_MSG, cx);
        }
        LogicalOp::Or if !lhs_call.safe_navigation && rhs_call.safe_navigation => {
            emit_operator_offense(rhs_call.operator, ".", USE_DOT_MSG, cx);
        }
        _ => {}
    }
}

fn emit_operator_offense(range: Range, replacement: &str, message: &str, cx: &Cx<'_>) {
    if range == Range::ZERO {
        return;
    }
    cx.emit_offense(range, message, None);
    cx.emit_edit(range, replacement);
}

fn call_info<'a>(node: NodeId, cx: &Cx<'a>) -> Option<CallInfo<'a>> {
    if cx.method_name(node).is_some_and(is_nil_safe_method) {
        return None;
    }
    let operator = cx.loc(node).dot();
    if operator == Range::ZERO {
        return None;
    }
    let receiver = cx.call_receiver(node).get()?;
    Some(CallInfo {
        safe_navigation: matches!(cx.kind(node), NodeKind::Csend { .. }),
        // RuboCop's `method.receiver.source`: the outermost call's *immediate*
        // receiver, verbatim. `@resource&.account&.suspended?` keys on
        // `@resource&.account`, not `@resource`, so it is not grouped with
        // `@resource.local?` (murphy-wcdv).
        receiver_source: cx.raw_source(cx.range(receiver)),
        operator,
    })
}

fn is_nil_safe_method(method: &str) -> bool {
    NIL_SAFE_METHODS.contains(&method)
}

murphy_plugin_api::submit_cop!(SafeNavigationConsistency);

#[cfg(test)]
mod tests {
    use super::SafeNavigationConsistency;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_safe_navigation_on_right_of_and() {
        test::<SafeNavigationConsistency>().expect_correction(
            indoc! {r#"
                foo.bar && foo&.baz
                              ^^ Use `.` instead of unnecessary `&.`.
            "#},
            "foo.bar && foo.baz\n",
        );
    }

    #[test]
    fn flags_and_corrects_ordinary_call_on_right_of_or() {
        test::<SafeNavigationConsistency>().expect_correction(
            indoc! {r#"
                foo&.bar || foo.baz
                               ^ Use `&.` for consistency with safe navigation.
            "#},
            "foo&.bar || foo&.baz\n",
        );
    }

    #[test]
    fn flags_and_corrects_redundant_safe_navigation_on_right_of_or() {
        test::<SafeNavigationConsistency>().expect_correction(
            indoc! {r#"
                foo.bar || foo&.baz
                              ^^ Use `.` instead of unnecessary `&.`.
            "#},
            "foo.bar || foo.baz\n",
        );
    }

    #[test]
    fn accepts_safe_navigation_on_left_of_and() {
        test::<SafeNavigationConsistency>().expect_no_offenses("foo&.bar && foo.baz\n");
    }

    #[test]
    fn accepts_different_receivers() {
        test::<SafeNavigationConsistency>().expect_no_offenses("foo&.bar || other.baz\n");
    }

    /// Regression (murphy-wcdv): the right operand's outermost call
    /// (`suspended?`) has receiver `@resource&.account`, not `@resource`, so it
    /// is a different group from `@resource.local?` and no offense fires.
    /// Previously the receiver was resolved recursively to `@resource`, wrongly
    /// matching the left operand. RuboCop 1.87 reports no offense.
    #[test]
    fn accepts_multi_level_safe_nav_with_distinct_immediate_receiver() {
        test::<SafeNavigationConsistency>()
            .expect_no_offenses("@resource.local? || @resource&.account&.suspended?\n");
    }

    /// `try!` is nil-safe in ActiveSupport (like its sibling `try`), so a
    /// safe-navigation `try!` operand is skipped rather than flagged. Without
    /// `try!` in `NIL_SAFE_METHODS`, `foo.bar && foo&.try!` would wrongly emit
    /// "Use `.` instead of unnecessary `&.`".
    #[test]
    fn accepts_safe_navigation_try_bang() {
        test::<SafeNavigationConsistency>().expect_no_offenses("foo.bar && foo&.try!\n");
    }
}
