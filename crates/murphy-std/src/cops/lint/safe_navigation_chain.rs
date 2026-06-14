//! `Lint/SafeNavigationChain` - avoid ordinary calls after safe navigation.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/SafeNavigationChain
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial v1 port covers ordinary dot calls directly chained after a safe
//!   navigation call, including later ordinary calls in an otherwise safe
//!   navigation chain. It skips nil-safe predicate/coercion methods. RuboCop's
//!   broader operator/index assignment, block-wrapper, logical-condition, and
//!   ternary autocorrection shapes are documented v1 gaps.
//!
//!   `NIL_SAFE_METHODS` approximates RuboCop's `nil_methods` (the `NilMethods`
//!   mixin: `nil.methods` in an ActiveSupport-loaded runtime, plus the cop's
//!   `AllowedMethods`). It includes ActiveSupport's nil-safe `blank?` /
//!   `present?` / `presence` / `try` / `try!`; `presence_in` is *not* nil-safe
//!   and is correctly still flagged (murphy-wcdv). The list is hardcoded — a
//!   user-configured `AllowedMethods` is a documented gap.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

const MSG: &str = "Do not chain ordinary method call after safe navigation operator.";

const NIL_SAFE_METHODS: &[&str] = &[
    "nil?",
    "blank?",
    "present?",
    "presence",
    "try",
    "try!",
    "to_a",
    "to_c",
    "to_d",
    "to_f",
    "to_h",
    "to_i",
    "to_r",
    "to_s",
    "to_sym",
    "inspect",
    "frozen?",
    "object_id",
    "class",
    "is_a?",
    "kind_of?",
    "respond_to?",
    "instance_of?",
    "freeze",
    "dup",
    "clone",
    "hash",
    "equal?",
    "itself",
];

#[derive(Default)]
pub struct SafeNavigationChain;

#[cop(
    name = "Lint/SafeNavigationChain",
    description = "Avoid ordinary method calls after safe navigation.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SafeNavigationChain {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        let Some(receiver) = receiver.get() else {
            return;
        };
        if !matches!(cx.kind(receiver), NodeKind::Csend { .. }) {
            return;
        }
        if cx.method_name(node).is_some_and(is_nil_safe_method) {
            return;
        }

        let dot = cx.loc(node).dot();
        if dot == Range::ZERO {
            return;
        }

        let offense_range = Range {
            start: dot.start,
            end: cx.range(node).end,
        };
        cx.emit_offense(offense_range, MSG, None);
        cx.emit_edit(
            Range {
                start: dot.start,
                end: dot.start,
            },
            "&",
        );
    }
}

fn is_nil_safe_method(method: &str) -> bool {
    NIL_SAFE_METHODS.contains(&method)
}

murphy_plugin_api::submit_cop!(SafeNavigationChain);

#[cfg(test)]
mod tests {
    use super::SafeNavigationChain;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_ordinary_method_after_safe_navigation() {
        test::<SafeNavigationChain>().expect_correction(
            indoc! {r#"
                x&.foo.bar
                      ^^^^ Do not chain ordinary method call after safe navigation operator.
            "#},
            "x&.foo&.bar\n",
        );
    }

    #[test]
    fn flags_and_corrects_later_ordinary_method_in_chain() {
        test::<SafeNavigationChain>().expect_correction(
            indoc! {r#"
                x&.foo&.bar.baz
                           ^^^^ Do not chain ordinary method call after safe navigation operator.
            "#},
            "x&.foo&.bar&.baz\n",
        );
    }

    #[test]
    fn accepts_safe_navigation_only_chain() {
        test::<SafeNavigationChain>().expect_no_offenses("x&.foo&.bar\n");
    }

    #[test]
    fn accepts_nil_predicate_after_safe_navigation() {
        test::<SafeNavigationChain>().expect_no_offenses("x&.foo.nil?\n");
    }

    /// Regression (murphy-wcdv): ActiveSupport's nil-safe `presence` / `try!`
    /// after a safe-navigation call must not be flagged — RuboCop 1.87 allows
    /// them via `nil_methods`. All 6 Mastodon false positives were `.presence`.
    #[test]
    fn accepts_presence_after_safe_navigation() {
        test::<SafeNavigationChain>().expect_no_offenses("x&.foo.presence\n");
    }

    #[test]
    fn accepts_try_bang_after_safe_navigation() {
        test::<SafeNavigationChain>().expect_no_offenses("x&.foo.try!(:z)\n");
    }

    /// Discriminator (murphy-wcdv): `presence_in` is *not* nil-safe, so it must
    /// still be flagged — adding `presence` must not over-broaden to its
    /// look-alikes. RuboCop 1.87 flags this.
    #[test]
    fn flags_presence_in_after_safe_navigation() {
        test::<SafeNavigationChain>().expect_correction(
            indoc! {r#"
                x&.foo.presence_in([1])
                      ^^^^^^^^^^^^^^^^^ Do not chain ordinary method call after safe navigation operator.
            "#},
            "x&.foo&.presence_in([1])\n",
        );
    }
}
