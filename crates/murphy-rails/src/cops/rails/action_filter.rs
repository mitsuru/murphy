//! `Rails/ActionFilter` — enforce consistent action filter methods.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ActionFilter
//! upstream_version_checked: 2.35.0
//! version_added: "0.19"
//! version_changed: "2.22"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 with `EnforcedStyle: action` (default) /
//!   `filter`. Action style flags the 13 `*_filter` methods; filter style
//!   flags the 13 `*_action` methods. Bare calls only (receiver must be
//!   None); block forms (`before_action { }`) flag the send node. Offense
//!   on the selector; autocorrect swaps the `_filter`/`_action` suffix
//!   positionally. File scope (`Include: controllers/mailers`) is enforced
//!   via the murphy-rails pack default.yml. Upstream is deprecated and
//!   `Enabled: false`.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ActionFilter;

#[derive(CopOptions)]
pub struct ActionFilterOptions {
    #[option(
        name = "EnforcedStyle",
        default = "action",
        description = "Whether to enforce `action` or `filter` method names."
    )]
    pub enforced_style: ActionFilterStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ActionFilterStyle {
    #[option(value = "action")]
    Action,
    #[option(value = "filter")]
    Filter,
}

const FILTER_METHODS: &[&str] = &[
    "after_filter",
    "append_after_filter",
    "append_around_filter",
    "append_before_filter",
    "around_filter",
    "before_filter",
    "prepend_after_filter",
    "prepend_around_filter",
    "prepend_before_filter",
    "skip_after_filter",
    "skip_around_filter",
    "skip_before_filter",
    "skip_filter",
];

const ACTION_METHODS: &[&str] = &[
    "after_action",
    "append_after_action",
    "append_around_action",
    "append_before_action",
    "around_action",
    "before_action",
    "prepend_after_action",
    "prepend_around_action",
    "prepend_before_action",
    "skip_after_action",
    "skip_around_action",
    "skip_before_action",
    "skip_action_callback",
];

#[cop(
    name = "Rails/ActionFilter",
    description = "Enforces consistent use of action filter methods.",
    default_severity = "warning",
    default_enabled = false,
    options = ActionFilterOptions,
)]
impl ActionFilter {
    #[on_node(
        kind = "send",
        methods = [
            "after_filter",
            "append_after_filter",
            "append_around_filter",
            "append_before_filter",
            "around_filter",
            "before_filter",
            "prepend_after_filter",
            "prepend_around_filter",
            "prepend_before_filter",
            "skip_after_filter",
            "skip_around_filter",
            "skip_before_filter",
            "skip_filter",
            "after_action",
            "append_after_action",
            "append_around_action",
            "append_before_action",
            "around_action",
            "before_action",
            "prepend_after_action",
            "prepend_around_action",
            "prepend_before_action",
            "skip_after_action",
            "skip_around_action",
            "skip_before_action",
            "skip_action_callback"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_send_node(node, cx);
    }

    // NOTE: block/numblock/itblock forms (`before_filter { }`) are covered by
    // the `send` handler above (the block's inner send still dispatches
    // `on_send`). A separate block handler would double-flag the same
    // selector, so none is registered (mirrors upstream single offense).
}

fn check_send_node(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    // Upstream `check_method_node` is called with the send node; `on_send`
    // skips when a receiver is present.
    if cx.call_receiver(node).get().is_some() {
        return;
    }
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    let opts = cx.options_or_default::<ActionFilterOptions>();
    let (bad, good_list) = match opts.enforced_style {
        ActionFilterStyle::Action => (FILTER_METHODS, ACTION_METHODS),
        ActionFilterStyle::Filter => (ACTION_METHODS, FILTER_METHODS),
    };
    let Some(pos) = bad.iter().position(|&m| m == method) else {
        return;
    };
    let prefer = good_list[pos];
    let msg = format!("Prefer `{prefer}` over `{method}`.");
    cx.emit_offense(cx.selector(node), &msg, None);
    cx.emit_edit(cx.selector(node), prefer);
}

#[cfg(test)]
mod tests {
    use super::{ActionFilter, ActionFilterOptions, ActionFilterStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_after_filter_default() {
        test::<ActionFilter>().expect_correction(
            indoc! {r#"
                after_filter :do_stuff
                ^^^^^^^^^^^^ Prefer `after_action` over `after_filter`.
            "#},
            "after_action :do_stuff\n",
        );
    }

    #[test]
    fn flags_before_filter_block() {
        test::<ActionFilter>().expect_correction(
            indoc! {r#"
                before_filter { do_stuff }
                ^^^^^^^^^^^^^ Prefer `before_action` over `before_filter`.
            "#},
            "before_action { do_stuff }\n",
        );
    }

    #[test]
    fn flags_skip_filter() {
        test::<ActionFilter>().expect_correction(
            indoc! {r#"
                skip_filter :do_stuff
                ^^^^^^^^^^^ Prefer `skip_action_callback` over `skip_filter`.
            "#},
            "skip_action_callback :do_stuff\n",
        );
    }

    #[test]
    fn allows_action_default() {
        test::<ActionFilter>().expect_no_offenses("after_action :do_stuff\n");
    }

    #[test]
    fn allows_receiver() {
        test::<ActionFilter>().expect_no_offenses("foo.before_filter :do_stuff\n");
    }

    #[test]
    fn filter_style_flags_action() {
        test::<ActionFilter>()
            .with_options(&ActionFilterOptions {
                enforced_style: ActionFilterStyle::Filter,
            })
            .expect_correction(
                indoc! {r#"
                    after_action :do_stuff
                    ^^^^^^^^^^^^ Prefer `after_filter` over `after_action`.
                "#},
                "after_filter :do_stuff\n",
            );
    }

    #[test]
    fn filter_style_allows_filter() {
        test::<ActionFilter>()
            .with_options(&ActionFilterOptions {
                enforced_style: ActionFilterStyle::Filter,
            })
            .expect_no_offenses("after_filter :do_stuff\n");
    }
}

murphy_plugin_api::submit_cop!(ActionFilter);
