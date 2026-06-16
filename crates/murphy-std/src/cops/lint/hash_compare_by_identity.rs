//! `Lint/HashCompareByIdentity` — flag `object_id` values used as hash keys.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/HashCompareByIdentity
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Verbatim port of RuboCop's matcher
//!   `(call _ {:key? :has_key? :fetch :[] :[]=} (send _ :object_id) ...)`
//!   (murphy-s1yc). `call` = `{send csend}` covers safe-navigation on the hash
//!   (`hash&.key?(...)`); the `_` receivers bind an absent or present receiver
//!   (so a receiverless `key?(x.object_id)` / `object_id` is matched, per
//!   if9y); `...` allows trailing args (`fetch(x.object_id, default)`). The
//!   inner `(send _ :object_id)` is **send-only** — a csend `foo&.object_id`
//!   does NOT match, exactly as RuboCop. No options or autocorrect.

use murphy_plugin_api::{cop, def_node_matcher, Cx, NoOptions, NodeId};

// `(call _ {…} (send _ :object_id) ...)` — the hash receiver is unconstrained,
// the selector is one of the key-lookup methods, the first argument is an
// `object_id` send (send-only, send/csend hash via the `call` head), and `...`
// absorbs any trailing arguments (e.g. `fetch`'s default).
def_node_matcher!(
    id_as_hash_key,
    "(call _ {:key? :has_key? :fetch :[] :[]=} (send _ :object_id) ...)"
);

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
    // `methods` mirrors RuboCop's `RESTRICT_ON_SEND` — a cheap dispatch-level
    // pre-filter; the matcher re-checks the selector for correctness.
    #[on_node(kind = "send", methods = ["key?", "has_key?", "fetch", "[]", "[]="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if id_as_hash_key(node, cx) {
        cx.emit_offense(
            cx.range(node),
            "Use `Hash#compare_by_identity` instead of using `object_id` for keys.",
            None,
        );
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
    fn flags_fetch_with_trailing_default_arg() {
        // `...` in the matcher absorbs the trailing `default` argument; the
        // first argument is still the `object_id` send.
        test::<HashCompareByIdentity>().expect_offense(indoc! {r#"
            hash.fetch(foo.object_id, :default)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Hash#compare_by_identity` instead of using `object_id` for keys.
        "#});
    }

    #[test]
    fn flags_receiverless_object_id() {
        // `(send _ :object_id)` matches a receiverless `object_id` too (the `_`
        // binds the nil-filled receiver slot, per murphy-if9y).
        test::<HashCompareByIdentity>().expect_offense(indoc! {r#"
            hash.key?(object_id)
            ^^^^^^^^^^^^^^^^^^^^ Use `Hash#compare_by_identity` instead of using `object_id` for keys.
        "#});
    }

    #[test]
    fn accepts_non_object_id_keys() {
        test::<HashCompareByIdentity>().expect_no_offenses("hash.key?(foo)\n");
    }

    #[test]
    fn accepts_object_id_not_first_arg() {
        // `object_id` reached through a further call (`.to_s`) is not the
        // direct first argument, so it does not match.
        test::<HashCompareByIdentity>().expect_no_offenses("hash.key?(foo.object_id.to_s)\n");
    }

    #[test]
    fn accepts_csend_object_id_key() {
        // Parity fix (murphy-s1yc): RuboCop's inner `(send _ :object_id)` is
        // send-only, so a safe-navigation `foo&.object_id` key does NOT match —
        // the previous hand-rolled implementation flagged it (a false positive).
        test::<HashCompareByIdentity>().expect_no_offenses("hash.key?(foo&.object_id)\n");
    }
}
