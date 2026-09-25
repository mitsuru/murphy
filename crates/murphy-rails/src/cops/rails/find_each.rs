//! `Rails/FindEach` — prefer `find_each` over `all.each`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/FindEach
//! upstream_version_checked: 2.35.0
//! version_added: "0.30"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: Send-only `each` dispatch (csend never
//!   flags), SCOPE_METHODS receiver gate (Send only), bare-scope calls
//!   require ActiveRecord inheritance (ApplicationRecord /
//!   ActiveRecord::Base class ancestor), `errors.where` exclusion, and
//!   AllowedMethods (default order/limit/select/lock) plus AllowedPatterns
//!   chain gating. Offense is the `each` selector; autocorrect renames it.
//! ```
//!
//! Identifies usages of `all.each` and changes them to `all.find_each`.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct FindEach;

#[derive(CopOptions)]
pub struct FindEachOptions {
    #[option(
        name = "AllowedMethods",
        default = ["order", "limit", "select", "lock"],
        description = "Methods in the chain that suppress the offense (e.g. order does not work well with find_each)."
    )]
    pub allowed_methods: Vec<String>,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regex patterns for method names in the chain that suppress the offense."
    )]
    pub allowed_patterns: Vec<String>,
}

const SCOPE_METHODS: &[&str] = &[
    "all",
    "eager_load",
    "includes",
    "joins",
    "left_joins",
    "left_outer_joins",
    "not",
    "or",
    "preload",
    "references",
    "unscoped",
    "where",
];

#[cop(
    name = "Rails/FindEach",
    description = "Prefer all.find_each over all.each.",
    default_severity = "warning",
    default_enabled = true,
    options = FindEachOptions,
)]
impl FindEach {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("each") {
        return;
    }
    // Upstream `node.receiver&.send_type?` — receiver must be a Send.
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    if !matches!(*cx.kind(recv), NodeKind::Send { .. }) {
        return;
    }
    let recv_method = match cx.method_name(recv) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if !SCOPE_METHODS.contains(&recv_method.as_str()) {
        return;
    }
    // Bare scope call (e.g. `all.each`) only flags inside ActiveRecord models.
    if cx.call_receiver(recv).get().is_none() && !inherits_active_record(cx, node) {
        return;
    }
    if ignored(cx, node) {
        return;
    }
    cx.emit_offense(cx.selector(node), "Use `find_each` instead of `each`.", None);
    cx.emit_edit(cx.selector(node), "find_each");
}

/// Upstream `ignored?`: active-model-errors `where` plus AllowedMethods /
/// AllowedPatterns applied to every `send` in the chain.
fn ignored(cx: &Cx<'_>, node: NodeId) -> bool {
    // `user.errors.where(...).each` — errors receiver on the scope call.
    if let Some(recv) = cx.call_receiver(node).get()
        && cx.method_name(recv) == Some("where")
        && let Some(rr) = cx.call_receiver(recv).get()
        && matches!(*cx.kind(rr), NodeKind::Send { .. })
        && cx.method_name(rr) == Some("errors")
    {
        return true;
    }
    let opts = cx.options_or_default::<FindEachOptions>();
    for name in chain_method_names(cx, node) {
        if opts.allowed_methods.iter().any(|m| m == &name) {
            return true;
        }
        if cx.matches_any_pattern(&name, &opts.allowed_patterns) {
            return true;
        }
    }
    false
}

/// Every `Send` method name in the receiver chain starting at `node`
/// (mirrors upstream `node.each_node(:send)` over the subtree).
fn chain_method_names(cx: &Cx<'_>, node: NodeId) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = Some(node);
    while let Some(id) = current {
        if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
            break;
        }
        if let Some(m) = cx.method_name(id) {
            names.push(m.to_owned());
        }
        current = cx.call_receiver(id).get();
    }
    names
}

/// Upstream `inherit_active_record_base?`: any `class` ancestor whose
/// superclass is `ApplicationRecord` or `ActiveRecord::Base`.
fn inherits_active_record(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        let NodeKind::Class { superclass, .. } = *cx.kind(anc) else {
            continue;
        };
        let Some(super_id) = superclass.get() else {
            continue;
        };
        let name = cx.const_name(super_id).unwrap_or_default();
        if name == "ApplicationRecord" || name == "ActiveRecord::Base" {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{FindEach, FindEachOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_all_each() {
        test::<FindEach>().expect_offense(indoc! {r#"
            User.all.each
                     ^^^^ Use `find_each` instead of `each`.
        "#});
    }

    #[test]
    fn autocorrects_all_each() {
        test::<FindEach>().expect_correction(
            indoc! {r#"
                User.all.each
                         ^^^^ Use `find_each` instead of `each`.
            "#},
            "User.all.find_each\n",
        );
    }

    #[test]
    fn flags_where_each() {
        test::<FindEach>().expect_offense(indoc! {r#"
            User.where(name: 'x').each
                                  ^^^^ Use `find_each` instead of `each`.
        "#});
    }

    #[test]
    fn does_not_flag_each_without_scope() {
        test::<FindEach>().expect_no_offenses("User.each\n");
    }

    #[test]
    fn does_not_flag_bare_all_each_outside_model() {
        test::<FindEach>().expect_no_offenses("all.each\n");
    }

    #[test]
    fn flags_bare_all_each_inside_model() {
        test::<FindEach>().expect_offense(indoc! {r#"
            class User < ApplicationRecord
              all.each
                  ^^^^ Use `find_each` instead of `each`.
            end
        "#});
    }

    #[test]
    fn does_not_flag_order_chain() {
        test::<FindEach>().expect_no_offenses("User.order(:foo).each\n");
    }

    #[test]
    fn allowed_methods_option_suppresses() {
        let opts = FindEachOptions {
            allowed_methods: vec!["where".to_owned()],
            allowed_patterns: vec![],
        };
        test::<FindEach>()
            .with_options(&opts)
            .expect_no_offenses("User.where(name: 'x').each\n");
    }

    #[test]
    fn allowed_patterns_option_suppresses() {
        let opts = FindEachOptions {
            allowed_methods: vec![],
            allowed_patterns: vec!["^ord".to_owned()],
        };
        test::<FindEach>()
            .with_options(&opts)
            .expect_no_offenses("User.order(:foo).each\n");
    }

    #[test]
    fn does_not_flag_errors_where_each() {
        test::<FindEach>().expect_no_offenses("user.errors.where(name: 'x').each\n");
    }

    #[test]
    fn does_not_flag_safe_navigation_each() {
        test::<FindEach>().expect_no_offenses("User.all&.each\n");
    }
}
murphy_plugin_api::submit_cop!(FindEach);
