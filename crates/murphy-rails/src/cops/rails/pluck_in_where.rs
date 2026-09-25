//! `Rails/PluckInWhere` — use `select` instead of `pluck` in `where`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/PluckInWhere
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [pluck ids] (send +
//!   csend), in_where? gate (first ancestor call is where/rewhere or not
//!   over where/rewhere), conservative root-receiver const gate vs
//!   aggressive, chained-call suppression via first-ancestor check
//!   (pluck.map inside where does not flag), selector-only offense range,
//!   `pluck` -> `select` and `ids` -> `select(:id)` autocorrect.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct PluckInWhere;

#[derive(CopOptions)]
pub struct PluckInWhereOptions {
    #[option(
        name = "EnforcedStyle",
        default = "conservative",
        description = "Whether to flag only model-class pluck calls or every pluck receiver."
    )]
    pub enforced_style: PluckInWhereStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum PluckInWhereStyle {
    #[option(value = "conservative")]
    Conservative,
    #[option(value = "aggressive")]
    Aggressive,
}

#[cop(
    name = "Rails/PluckInWhere",
    description = "Use `select` instead of `pluck` in `where` query methods.",
    default_severity = "warning",
    default_enabled = false,
    options = PluckInWhereOptions,
)]
impl PluckInWhere {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if method != "pluck" && method != "ids" {
        return;
    }
    if !in_where(cx, node) {
        return;
    }
    let opts = cx.options_or_default::<PluckInWhereOptions>();
    if opts.enforced_style == PluckInWhereStyle::Conservative && !root_is_const(cx, node) {
        return;
    }
    let selector = cx.loc(node).name;
    if method == "ids" {
        cx.emit_offense(
            selector,
            "Use `select(:id)` instead of `ids` within `where` query method.",
            None,
        );
        cx.emit_edit(selector, "select(:id)");
    } else {
        cx.emit_offense(
            selector,
            "Use `select` instead of `pluck` within `where` query method.",
            None,
        );
        cx.emit_edit(selector, "select");
    }
}

fn in_where(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        if !matches!(*cx.kind(anc), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            continue;
        }
        let method = match cx.method_name(anc) {
            Some(m) => m.to_owned(),
            None => continue,
        };
        if method == "where" || method == "rewhere" {
            let recv_opt = cx.call_receiver(anc).get();
            if recv_opt != Some(node) {
                return true;
            }
            return false;
        }
        if method == "not" {
            if let Some(recv) = cx.call_receiver(anc).get()
                && matches!(*cx.kind(recv), NodeKind::Send { .. } | NodeKind::Csend { .. })
            {
                let rm = cx.method_name(recv);
                if rm == Some("where") || rm == Some("rewhere") {
                    return true;
                }
            }
            return false;
        }
        return false;
    }
    false
}

fn root_is_const(cx: &Cx<'_>, mut node: NodeId) -> bool {
    // Walk receiver chain to the ultimate root.
    loop {
        let Some(recv) = cx.call_receiver(node).get() else {
            return false;
        };
        if matches!(*cx.kind(recv), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            node = recv;
            continue;
        }
        return matches!(*cx.kind(recv), NodeKind::Const { .. });
    }
}

#[cfg(test)]
mod tests {
    use super::{PluckInWhere, PluckInWhereOptions, PluckInWhereStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn aggressive() -> PluckInWhereOptions {
        PluckInWhereOptions {
            enforced_style: PluckInWhereStyle::Aggressive,
        }
    }

    #[test]
    fn flags_pluck_in_where() {
        test::<PluckInWhere>().expect_correction(
            indoc! {r#"
                Post.where(user_id: User.active.pluck(:id))
                                                ^^^^^ Use `select` instead of `pluck` within `where` query method.
            "#},
            "Post.where(user_id: User.active.select(:id))\n",
        );
    }

    #[test]
    fn flags_ids_in_where() {
        test::<PluckInWhere>().expect_correction(
            indoc! {r#"
                Post.where(user_id: User.active.ids)
                                                ^^^ Use `select(:id)` instead of `ids` within `where` query method.
            "#},
            "Post.where(user_id: User.active.select(:id))\n",
        );
    }

    #[test]
    fn flags_pluck_in_where_not() {
        test::<PluckInWhere>().expect_correction(
            indoc! {r#"
                Post.where.not(user_id: User.active.pluck(:id))
                                                    ^^^^^ Use `select` instead of `pluck` within `where` query method.
            "#},
            "Post.where.not(user_id: User.active.select(:id))\n",
        );
    }

    #[test]
    fn flags_pluck_in_rewhere() {
        test::<PluckInWhere>().expect_correction(
            indoc! {r#"
                Post.rewhere('user_id IN (?)', User.active.pluck(:id))
                                                           ^^^^^ Use `select` instead of `pluck` within `where` query method.
            "#},
            "Post.rewhere('user_id IN (?)', User.active.select(:id))\n",
        );
    }

    #[test]
    fn allows_select_in_where() {
        test::<PluckInWhere>().expect_no_offenses("Post.where(user_id: User.active.select(:id))\n");
    }

    #[test]
    fn allows_chained_pluck_in_where() {
        test::<PluckInWhere>().expect_no_offenses("Post.where(user_id: User.pluck(:id).map(&:to_i))\n");
    }

    #[test]
    fn allows_pluck_in_order() {
        test::<PluckInWhere>().expect_no_offenses("Post.order(columns.pluck(:name))\n");
    }

    #[test]
    fn conservative_ignores_variable_receiver() {
        test::<PluckInWhere>().expect_no_offenses("Post.where(user_id: users.active.pluck(:id))\n");
    }

    #[test]
    fn aggressive_flags_variable_receiver() {
        test::<PluckInWhere>()
            .with_options(&aggressive())
            .expect_correction(
                indoc! {r#"
                Post.where(user_id: users.active.pluck(:id))
                                                 ^^^^^ Use `select` instead of `pluck` within `where` query method.
            "#},
                "Post.where(user_id: users.active.select(:id))\n",
            );
    }

    #[test]
    fn allows_pluck_as_where_receiver() {
        test::<PluckInWhere>().expect_no_offenses("Post.pluck(:id).where(id: 1..10)\n");
    }
}
murphy_plugin_api::submit_cop!(PluckInWhere);
