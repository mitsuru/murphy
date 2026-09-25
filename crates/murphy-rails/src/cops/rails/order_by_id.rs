//! `Rails/OrderById` — do not order by `id` column.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/OrderById
//! upstream_version_checked: 2.35.0
//! version_added: "2.8"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:order] (send only,
//!   no csend), single-arg gate with four shapes (sym :id, hash with
//!   single pair sym :id, bare/qualified primary_key send, hash with
//!   single pair primary_key send). Multi-pair hashes and multi-arg
//!   calls do not match. Offense is selector-start to call-end; no
//!   autocorrect. Upstream Include gating absent.
//! ```
//!
//! Checks for places where ordering by `id` column is used.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct OrderById;

#[cop(
    name = "Rails/OrderById",
    description = "Do not use the `id` column for ordering. Use a timestamp column to order chronologically.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl OrderById {
    #[on_node(kind = "send", methods = ["order"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("order") {
        return;
    }
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }
    if !is_order_by_id_arg(cx, args[0]) {
        return;
    }
    let offense = murphy_plugin_api::Range {
        start: cx.loc(node).name.start,
        end: cx.range(node).end,
    };
    cx.emit_offense(
        offense,
        "Do not use the `id` column for ordering. Use a timestamp column to order chronologically.",
        None,
    );
}

fn is_order_by_id_arg(cx: &Cx<'_>, arg: NodeId) -> bool {
    match *cx.kind(arg) {
        NodeKind::Sym(s) => cx.symbol_str(s) == "id",
        NodeKind::Hash(list) => {
            let pairs = cx.list(list);
            if pairs.len() != 1 {
                return false;
            }
            let NodeKind::Pair { key, .. } = *cx.kind(pairs[0]) else {
                return false;
            };
            is_id_or_primary_key(cx, key)
        }
        _ => is_primary_key_send(cx, arg),
    }
}

fn is_id_or_primary_key(cx: &Cx<'_>, id: NodeId) -> bool {
    if let NodeKind::Sym(s) = *cx.kind(id)
        && cx.symbol_str(s) == "id"
    {
        return true;
    }
    is_primary_key_send(cx, id)
}

fn is_primary_key_send(cx: &Cx<'_>, id: NodeId) -> bool {
    if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
        return false;
    }
    if cx.method_name(id) != Some("primary_key") {
        return false;
    }
    cx.call_arguments(id).is_empty()
}

#[cfg(test)]
mod tests {
    use super::OrderById;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_sym_id() {
        test::<OrderById>().expect_offense(indoc! {r#"
            User.order(:id)
                 ^^^^^^^^^^ Do not use the `id` column for ordering. Use a timestamp column to order chronologically.
        "#});
    }

    #[test]
    fn flags_hash_id() {
        test::<OrderById>().expect_offense(indoc! {r#"
            User.order(id: :asc)
                 ^^^^^^^^^^^^^^^ Do not use the `id` column for ordering. Use a timestamp column to order chronologically.
        "#});
    }

    #[test]
    fn flags_primary_key() {
        test::<OrderById>().expect_offense(indoc! {r#"
            scope :chronological, -> { order(primary_key) }
                                       ^^^^^^^^^^^^^^^^^^ Do not use the `id` column for ordering. Use a timestamp column to order chronologically.
        "#});
    }

    #[test]
    fn flags_primary_key_hash() {
        test::<OrderById>().expect_offense(indoc! {r#"
            scope :chronological, -> { order(primary_key => :asc) }
                                       ^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use the `id` column for ordering. Use a timestamp column to order chronologically.
        "#});
    }

    #[test]
    fn allows_non_id_column() {
        test::<OrderById>().expect_no_offenses("User.order(:created_at)\n");
    }

    #[test]
    fn allows_multi_column_including_id() {
        test::<OrderById>().expect_no_offenses("User.order(id: :asc, created_at: :desc)\n");
    }
}
murphy_plugin_api::submit_cop!(OrderById);
