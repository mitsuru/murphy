//! `Rails/PluckId` — use `ids` instead of `pluck(:id)`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/PluckId
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:pluck] (send +
//!   csend), single-arg gate (`(sym :id)` or bare `primary_key` send),
//!   in_where? suppression (first ancestor call is where/rewhere with a
//!   different receiver, or `not` over where/rewhere). Offense is
//!   selector-start to call-end; autocorrect replaces it with `ids`.
//!   Upstream carries no per-file `Include`/`Exclude` for this cop
//!   (verified vs rubocop-rails 2.38.0 default.yml); Murphy runs in all
//!   files, matching upstream.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct PluckId;

#[cop(
    name = "Rails/PluckId",
    description = "Use `ids` instead of `pluck(:id)` or `pluck(primary_key)`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl PluckId {
    #[on_node(kind = "send", methods = ["pluck"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if cx.method_name(node) != Some("pluck") {
        return;
    }
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }
    if !is_id_arg(cx, args[0]) {
        return;
    }
    if in_where(cx, node) {
        return;
    }
    let offense = murphy_plugin_api::Range {
        start: cx.loc(node).name.start,
        end: cx.range(node).end,
    };
    let msg = format!("Use `ids` instead of `{}`.", cx.raw_source(offense));
    cx.emit_offense(offense, &msg, None);
    cx.emit_edit(offense, "ids");
}

fn is_id_arg(cx: &Cx<'_>, arg: NodeId) -> bool {
    if let NodeKind::Sym(s) = *cx.kind(arg) {
        return cx.symbol_str(s) == "id";
    }
    // `(send nil? :primary_key)` — bare primary_key only
    if !matches!(*cx.kind(arg), NodeKind::Send { .. }) {
        return false;
    }
    if cx.method_name(arg) != Some("primary_key") {
        return false;
    }
    if cx.call_receiver(arg).get().is_some() {
        return false;
    }
    cx.call_arguments(arg).is_empty()
}

fn in_where(cx: &Cx<'_>, node: NodeId) -> bool {
    // Mirrors ActiveRecordHelper#in_where?: first ancestor call.
    for anc in cx.ancestors(node) {
        if !matches!(*cx.kind(anc), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            continue;
        }
        let method = match cx.method_name(anc) {
            Some(m) => m.to_owned(),
            None => continue,
        };
        let is_where = method == "where" || method == "rewhere";
        if is_where {
            // `send_node.receiver != node`
            let recv_opt = cx.call_receiver(anc).get();
            if recv_opt != Some(node) {
                return true;
            }
            return false;
        }
        // `send_node.method?(:not) && WHERE_METHODS.include?(receiver.method_name)`
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

#[cfg(test)]
mod tests {
    use super::PluckId;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_pluck_id() {
        test::<PluckId>().expect_correction(
            indoc! {r#"
                User.pluck(:id)
                     ^^^^^^^^^^ Use `ids` instead of `pluck(:id)`.
            "#},
            "User.ids\n",
        );
    }

    #[test]
    fn flags_pluck_id_csend() {
        test::<PluckId>().expect_correction(
            indoc! {r#"
                User&.pluck(:id)
                      ^^^^^^^^^^ Use `ids` instead of `pluck(:id)`.
            "#},
            "User&.ids\n",
        );
    }

    #[test]
    fn flags_pluck_primary_key() {
        test::<PluckId>().expect_correction(
            indoc! {r#"
                def self.user_ids
                  pluck(primary_key)
                  ^^^^^^^^^^^^^^^^^^ Use `ids` instead of `pluck(primary_key)`.
                end
            "#},
            "def self.user_ids\n  ids\nend\n",
        );
    }

    #[test]
    fn allows_non_id() {
        test::<PluckId>().expect_no_offenses("user.posts.pluck(:votes)\n");
    }

    #[test]
    fn allows_multi_column() {
        test::<PluckId>().expect_no_offenses("user.posts.pluck(:id, :votes)\n");
    }

    #[test]
    fn allows_inside_where() {
        test::<PluckId>().expect_no_offenses("Post.where(user_id: User.pluck(:id))\n");
    }

    #[test]
    fn flags_pluck_as_where_receiver() {
        test::<PluckId>().expect_correction(
            indoc! {r#"
                Post.pluck(:id).where(id: 1..10)
                     ^^^^^^^^^^ Use `ids` instead of `pluck(:id)`.
            "#},
            "Post.ids.where(id: 1..10)\n",
        );
    }
}
murphy_plugin_api::submit_cop!(PluckId);
