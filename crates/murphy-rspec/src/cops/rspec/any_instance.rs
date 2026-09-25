//! `RSpec/AnyInstance` — avoid stubbing any instance globally.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/AnyInstance
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` with
//!   `RESTRICT_ON_SEND = [:any_instance, :allow_any_instance_of,
//!   :expect_any_instance_of]`. Upstream matches any receiver and checks
//!   neither arguments nor surrounding scope, so this port flags unconditionally
//!   on dispatch. The offense range is the whole node per `add_offense(node)`.
//!   No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["any_instance",
//! "allow_any_instance_of", "expect_any_instance_of"]` (any receiver):
//!
//! - `allow_any_instance_of(MyClass).to receive(:foo)` — flagged.
//! - `expect_any_instance_of(MyClass).to receive(:foo)` — flagged.
//! - `obj.any_instance` — flagged (upstream flags any receiver).
//! - `allow(MyClass).to receive(:foo)` — different selector, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; replacing with instance doubles needs human
//! judgement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct AnyInstance;

#[cop(
    name = "RSpec/AnyInstance",
    description = "Check that instances are not being stubbed globally.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl AnyInstance {
    #[on_node(
        kind = "send",
        methods = ["any_instance", "allow_any_instance_of", "expect_any_instance_of"]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { method, .. } = *cx.kind(node) else {
            return;
        };
        let method_name = cx.symbol_str(method);
        cx.emit_offense(
            cx.range(node),
            &format!("Avoid stubbing using `{method_name}`."),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::AnyInstance;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_allow_any_instance_of() {
        test::<AnyInstance>().expect_offense(indoc! {r#"
                allow_any_instance_of(MyClass).to receive(:foo)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid stubbing using `allow_any_instance_of`.
            "#});
    }

    #[test]
    fn flags_expect_any_instance_of() {
        test::<AnyInstance>().expect_offense(indoc! {r#"
                expect_any_instance_of(MyClass).to receive(:foo)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid stubbing using `expect_any_instance_of`.
            "#});
    }

    #[test]
    fn flags_any_instance() {
        test::<AnyInstance>().expect_offense(indoc! {r#"
                obj.any_instance
                ^^^^^^^^^^^^^^^^ Avoid stubbing using `any_instance`.
            "#});
    }

    #[test]
    fn does_not_flag_plain_allow() {
        test::<AnyInstance>().expect_no_offenses(indoc! {r#"
                allow(MyClass).to receive(:foo)
            "#});
    }
}

murphy_plugin_api::submit_cop!(AnyInstance);
