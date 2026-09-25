//! `Rails/RedundantActiveRecordAllMethod` — redundant `all` before query methods.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RedundantActiveRecordAllMethod
//! upstream_version_checked: 2.35.0
//! version_added: "2.21"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:all] with a
//!   `QUERYING_METHODS` parent send, enumerable-block suppression
//!   (`any? count find none? one? select sum` with a block literal or
//!   block-pass), `delete_all`/`destroy_all` const-receiver gate,
//!   `AllowedReceivers` (default `ActionMailer::Preview`,
//!   `ActiveSupport::TimeZone`) plus bare-`all`
//!   `inherit_active_record_base?` gate, offense from the `all`
//!   selector to the node end with dot-removal autocorrect.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedundantActiveRecordAllMethod;

#[derive(CopOptions)]
pub struct RedundantActiveRecordAllMethodOptions {
    #[option(
        name = "AllowedReceivers",
        default = ["ActionMailer::Preview", "ActiveSupport::TimeZone"],
        description = "Receivers ignored by the cop."
    )]
    pub allowed_receivers: Vec<String>,
}

#[cop(
    name = "Rails/RedundantActiveRecordAllMethod",
    description = "Detect redundant `all` used as a receiver for Active Record query methods.",
    default_severity = "warning",
    default_enabled = false,
    options = RedundantActiveRecordAllMethodOptions,
)]
impl RedundantActiveRecordAllMethod {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(kind = "send", methods = ["all"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if cx.method_name(node) != Some("all") {
        return;
    }
    if !cx.call_arguments(node).is_empty() {
        return;
    }
    let Some(parent) = cx.parent(node).get() else {
        return;
    };
    if !matches!(*cx.kind(parent), NodeKind::Send { .. }) {
        return;
    }
    // Parent must receive through this `all` node.
    if cx.call_receiver(parent).get() != Some(node) {
        return;
    }
    let parent_method = match cx.method_name(parent) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if !is_querying_method(&parent_method) {
        return;
    }
    if possible_enumerable_block_method(cx, parent, &parent_method) {
        return;
    }
    if sensitive_association_method(cx, node, &parent_method) {
        return;
    }
    if let Some(recv) = cx.call_receiver(node).get() {
        if allowed_receiver(cx, recv) {
            return;
        }
    } else if !inherits_active_record(cx, node) {
        return;
    }
    let offense = Range {
        start: cx.selector(node).start,
        end: cx.range(node).end,
    };
    cx.emit_offense(offense, "Redundant `all` detected.", None);
    cx.emit_edit(offense, "");
    let dot = cx.loc(parent).dot();
    if dot != Range::ZERO {
        cx.emit_edit(dot, "");
    }
}

fn is_querying_method(name: &str) -> bool {
    matches!(
        name,
        "and"
            | "annotate"
            | "any?"
            | "async_average"
            | "async_count"
            | "async_ids"
            | "async_maximum"
            | "async_minimum"
            | "async_pick"
            | "async_pluck"
            | "async_sum"
            | "average"
            | "calculate"
            | "count"
            | "create_or_find_by"
            | "create_or_find_by!"
            | "create_with"
            | "delete_all"
            | "delete_by"
            | "destroy_all"
            | "destroy_by"
            | "distinct"
            | "eager_load"
            | "except"
            | "excluding"
            | "exists?"
            | "extending"
            | "extract_associated"
            | "fifth"
            | "fifth!"
            | "find"
            | "find_by"
            | "find_by!"
            | "find_each"
            | "find_in_batches"
            | "find_or_create_by"
            | "find_or_create_by!"
            | "find_or_initialize_by"
            | "find_sole_by"
            | "first"
            | "first!"
            | "first_or_create"
            | "first_or_create!"
            | "first_or_initialize"
            | "forty_two"
            | "forty_two!"
            | "fourth"
            | "fourth!"
            | "from"
            | "group"
            | "having"
            | "ids"
            | "in_batches"
            | "in_order_of"
            | "includes"
            | "invert_where"
            | "joins"
            | "last"
            | "last!"
            | "left_joins"
            | "left_outer_joins"
            | "limit"
            | "lock"
            | "many?"
            | "maximum"
            | "merge"
            | "minimum"
            | "none"
            | "none?"
            | "offset"
            | "one?"
            | "only"
            | "optimizer_hints"
            | "or"
            | "order"
            | "pick"
            | "pluck"
            | "preload"
            | "readonly"
            | "references"
            | "regroup"
            | "reorder"
            | "reselect"
            | "rewhere"
            | "second"
            | "second!"
            | "second_to_last"
            | "second_to_last!"
            | "select"
            | "sole"
            | "strict_loading"
            | "sum"
            | "take"
            | "take!"
            | "third"
            | "third!"
            | "third_to_last"
            | "third_to_last!"
            | "touch_all"
            | "unscope"
            | "update_all"
            | "where"
            | "with"
            | "without"
    )
}

fn possible_enumerable_block_method(cx: &Cx<'_>, parent: NodeId, method: &str) -> bool {
    if !matches!(method, "any?" | "count" | "find" | "none?" | "one?" | "select" | "sum") {
        return false;
    }
    if cx.block_node(parent).get().is_some() {
        return true;
    }
    // `parent.first_argument&.block_pass_type?`
    if let Some(first) = cx.first_argument(parent).get() {
        return matches!(*cx.kind(first), NodeKind::BlockPass(_));
    }
    false
}

/// Upstream `sensitive_association_method?`: `delete_all`/`destroy_all`
/// with a non-const `.all` receiver are skipped.
fn sensitive_association_method(cx: &Cx<'_>, all_node: NodeId, parent_method: &str) -> bool {
    if !matches!(parent_method, "delete_all" | "destroy_all") {
        return false;
    }
    match cx.call_receiver(all_node).get() {
        None => true,
        Some(recv) => !matches!(*cx.kind(recv), NodeKind::Const { .. }),
    }
}

fn allowed_receiver(cx: &Cx<'_>, recv: NodeId) -> bool {
    let opts = cx.options_or_default::<RedundantActiveRecordAllMethodOptions>();
    let src = cx.raw_source(cx.range(recv)).to_owned();
    // Mirror `AllowedReceivers` mixin for chained receivers by also
    // checking the root receiver source (e.g. `a.b` → `a`).
    if opts.allowed_receivers.iter().any(|a| a == &src) {
        return true;
    }
    // Walk down send chains to the base (upstream `receiver_name` recursion).
    let mut cur = recv;
    loop {
        let next = match *cx.kind(cur) {
            NodeKind::Send { receiver, .. } => receiver.get(),
            NodeKind::Csend { receiver, .. } => Some(receiver),
            _ => None,
        };
        let Some(n) = next else {
            break;
        };
        cur = n;
    }
    if cur != recv {
        let base = cx.raw_source(cx.range(cur)).to_owned();
        return opts.allowed_receivers.iter().any(|a| a == &base);
    }
    false
}

/// Upstream `inherit_active_record_base?`.
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
    use super::{RedundantActiveRecordAllMethod, RedundantActiveRecordAllMethodOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_all_where() {
        test::<RedundantActiveRecordAllMethod>().expect_offense(indoc! {r#"
            User.all.where(id: ids)
                 ^^^ Redundant `all` detected.
        "#});
    }

    #[test]
    fn corrects_all_where() {
        test::<RedundantActiveRecordAllMethod>().expect_correction(
            indoc! {r#"
                User.all.where(id: ids)
                     ^^^ Redundant `all` detected.
            "#},
            "User.where(id: ids)\n",
        );
    }

    #[test]
    fn flags_bare_all_in_model() {
        test::<RedundantActiveRecordAllMethod>().expect_offense(indoc! {r#"
            class User < ApplicationRecord
              all.where(id: ids)
              ^^^ Redundant `all` detected.
            end
        "#});
    }

    #[test]
    fn allows_bare_all_outside_model() {
        test::<RedundantActiveRecordAllMethod>()
            .expect_no_offenses("all.where(id: ids)\n");
    }

    #[test]
    fn allows_enumerable_block() {
        test::<RedundantActiveRecordAllMethod>()
            .expect_no_offenses("User.all.select { |u| u.active? }\n");
    }

    #[test]
    fn allows_delete_all_on_association() {
        test::<RedundantActiveRecordAllMethod>()
            .expect_no_offenses("user.articles.all.delete_all\n");
    }

    #[test]
    fn flags_delete_all_on_model() {
        test::<RedundantActiveRecordAllMethod>().expect_offense(indoc! {r#"
            User.all.delete_all
                 ^^^ Redundant `all` detected.
        "#});
    }

    #[test]
    fn allows_allowed_receiver() {
        test::<RedundantActiveRecordAllMethod>()
            .expect_no_offenses("ActionMailer::Preview.all.first\n");
    }

    #[test]
    fn custom_allowed_receiver() {
        test::<RedundantActiveRecordAllMethod>()
            .with_options(&RedundantActiveRecordAllMethodOptions {
                allowed_receivers: vec!["User".to_owned()],
            })
            .expect_no_offenses("User.all.where(id: 1)\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantActiveRecordAllMethod);
