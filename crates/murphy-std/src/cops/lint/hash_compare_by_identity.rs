//! `Lint/HashCompareByIdentity` — flag `object_id` values used as hash keys.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/HashCompareByIdentity
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's send/csend matcher for key?, has_key?, fetch, [], and []=
//!   when the first argument is an object_id call. No options or autocorrect.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct HashCompareByIdentity;

#[cop(
    name = "Lint/HashCompareByIdentity",
    description = "Prefer Hash#compare_by_identity over object_id hash keys.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl HashCompareByIdentity {
    #[on_node(kind = "send", methods = ["key?", "has_key?", "fetch", "[]", "[]="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if first_arg_is_object_id_call(node, cx) {
            cx.emit_offense(cx.range(node), "Use `Hash#compare_by_identity` instead of using `object_id` for keys.", None);
        }
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { method, .. } = *cx.kind(node) else { return; };
        if matches!(cx.symbol_str(method), "key?" | "has_key?" | "fetch" | "[]" | "[]=")
            && first_arg_is_object_id_call(node, cx)
        {
            cx.emit_offense(cx.range(node), "Use `Hash#compare_by_identity` instead of using `object_id` for keys.", None);
        }
    }
}

fn first_arg_is_object_id_call(node: NodeId, cx: &Cx<'_>) -> bool {
    let args = match *cx.kind(node) {
        NodeKind::Send { args, .. } | NodeKind::Csend { args, .. } => cx.list(args),
        _ => return false,
    };
    let Some(&first) = args.first() else { return false; };
    matches_object_id_call(first, cx)
}

fn matches_object_id_call(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Send { method, args, .. } => cx.symbol_str(method) == "object_id" && cx.list(args).is_empty(),
        NodeKind::Csend { method, args, .. } => cx.symbol_str(method) == "object_id" && cx.list(args).is_empty(),
        _ => false,
    }
}

murphy_plugin_api::submit_cop!(HashCompareByIdentity);

#[cfg(test)]
mod tests {
    use super::HashCompareByIdentity;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_object_id_key_lookups() {
        test::<HashCompareByIdentity>()
            .expect_offense(indoc! {r#"
                hash.key?(foo.object_id)
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `Hash#compare_by_identity` instead of using `object_id` for keys.
            "#})
            .expect_offense(indoc! {r#"
                hash[foo.object_id] = :bar
                ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Hash#compare_by_identity` instead of using `object_id` for keys.
            "#})
            .expect_offense(indoc! {r#"
                hash&.key?(foo.object_id)
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Hash#compare_by_identity` instead of using `object_id` for keys.
            "#});
    }

    #[test]
    fn accepts_non_object_id_keys() {
        test::<HashCompareByIdentity>().expect_no_offenses("hash.key?(foo)\n");
    }
}
