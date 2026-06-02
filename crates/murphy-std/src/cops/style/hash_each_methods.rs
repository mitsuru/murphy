//! `Style/HashEachMethods` — use `Hash#each_key` and `Hash#each_value`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashEachMethods
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Enabled by default (Safe: false — cop cannot guarantee receiver is a Hash).
//!
//!   Three detection patterns:
//!     1. kv_each block form: `hash.keys.each { }` → `hash.each_key { }`
//!        Offense range: from `keys` selector start to `each` selector end.
//!        Autocorrect: replace that range with `each_key`.
//!        Also handles `values.each` → `each_value`.
//!     2. kv_each block-pass form: `hash.keys.each(&:foo)` → `hash.each_key(&:foo)`.
//!        Same range/edit.
//!     3. each_arguments unused-arg form: `hash.each { |k, _v| p k }` → `each_key`.
//!        Only triggered when exactly one of the two block args is unused in the body.
//!        Offense range: whole block node.
//!        Autocorrect: rename `each` selector + remove unused arg range.
//!
//!   Guards:
//!     - AllowedReceivers: if the source of the receiver of `keys`/`values` matches
//!       any entry in AllowedReceivers, the offense is suppressed.
//!     - Array converter methods preceding the pattern: if the immediate receiver of
//!       `keys`/`values` (or `each` in pattern 3) is a call to {assoc, chunk, flatten,
//!       rassoc, sort, sort_by, to_a}, suppress the offense.
//!     - Root receiver must exist (the receiver chain must have a root).
//!     - Root receiver must not be a non-hash literal (e.g., integer, string literal —
//!       but hash literals are fine).
//!     - hash_mutated? (looking for `[]=` on root receiver): not implemented; gap documented.
//!
//!   Limitations / gaps:
//!     - Destructured block params (`|(k, v)|`) are mapped to `Unknown` in Prism/Murphy;
//!       the each_arguments unused-arg detection skips any Args node where either arg
//!       is not a plain `Arg` node (same limitation as map_to_hash destructuring).
//!     - `numblock` and `itblock` forms are not handled (Murphy does not expose these).
//!     - `hash_mutated?` guard is not implemented.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! hash.keys.each { |k| p k }
//! hash.values.each { |v| p v }
//! hash.keys.each(&:to_s)
//! hash.each { |k, _unused| p k }
//! hash.each { |_unused, v| p v }
//!
//! # good
//! hash.each_key { |k| p k }
//! hash.each_value { |v| p v }
//! hash.each_key(&:to_s)
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

const ARRAY_CONVERTER_METHODS: &[&str] =
    &["assoc", "chunk", "flatten", "rassoc", "sort", "sort_by", "to_a"];

/// Stateless unit struct.
#[derive(Default)]
pub struct HashEachMethods;

#[derive(CopOptions)]
pub struct HashEachMethodsOptions {
    #[option(
        name = "AllowedReceivers",
        default = [],
        description = "Receiver method call source strings that are whitelisted."
    )]
    pub allowed_receivers: Vec<String>,
}

#[cop(
    name = "Style/HashEachMethods",
    description = "Use Hash#each_key and Hash#each_value.",
    default_severity = "warning",
    default_enabled = true,
    options = HashEachMethodsOptions,
)]
impl HashEachMethods {
    // Pattern 1 & 2: triggered when we see a Block with a `keys.each` or
    // `values.each` call chain.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        // Must be `.each` call.
        if cx.method_name(call) != Some("each") {
            return;
        }
        let Some(kv_recv_id) = cx.call_receiver(call).get() else {
            return;
        };
        if let Some(kv_method) = keys_or_values_method(kv_recv_id, cx) {
            check_kv_each(node, call, kv_recv_id, kv_method, cx);
        } else {
            // Pattern 3: hash.each { |k, v| ... } with unused arg.
            check_each_arguments(node, call, cx);
        }
    }

    // Pattern 2: block-pass form: `hash.keys.each(&:foo)` — the send has no block.
    #[on_node(kind = "send", methods = ["each"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Only handle when the parent is NOT a Block (to avoid double-firing).
        if let Some(parent) = cx.parent(node).get() {
            if matches!(cx.kind(parent), NodeKind::Block { call, .. } if *call == node) {
                return;
            }
        }
        // Must have exactly one block-pass arg.
        let arg_list = cx.call_arguments(node);
        if arg_list.len() != 1 {
            return;
        }
        if !matches!(cx.kind(arg_list[0]), NodeKind::BlockPass(_)) {
            return;
        }
        let Some(kv_recv_id) = cx.call_receiver(node).get() else {
            return;
        };
        if let Some(kv_method) = keys_or_values_method(kv_recv_id, cx) {
            check_kv_each_with_block_pass(node, kv_recv_id, kv_method, cx);
        }
    }
}

/// Returns `Some("keys")` or `Some("values")` if `node` is a `keys` or `values` call,
/// otherwise `None`.
fn keys_or_values_method<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'static str> {
    match cx.method_name(node)? {
        "keys" => Some("keys"),
        "values" => Some("values"),
        _ => None,
    }
}

/// Check pattern 1: block form `hash.keys.each { }` → `hash.each_key { }`.
fn check_kv_each(
    block_node: NodeId,
    each_call: NodeId,
    kv_recv: NodeId,
    kv_method: &str,
    cx: &Cx<'_>,
) {
    // Guard: root receiver must exist and be valid.
    let Some(parent_receiver) = cx.call_receiver(kv_recv).get() else {
        return;
    };

    if !is_handleable(kv_recv, cx) {
        return;
    }

    let _ = block_node;

    if is_allowed_receiver(parent_receiver, cx) {
        return;
    }

    // Offense range: from the `keys`/`values` selector start to `each` selector end.
    let kv_selector = cx.node(kv_recv).loc.name;
    let each_selector = cx.node(each_call).loc.name;
    let offense_range = Range {
        start: kv_selector.start,
        end: each_selector.end,
    };

    let prefer = if kv_method == "keys" { "each_key" } else { "each_value" };
    let current = cx.raw_source(offense_range);
    let message = format!("Use `{prefer}` instead of `{current}`.");

    cx.emit_offense(offense_range, &message, None);
    cx.emit_edit(offense_range, prefer);
}

/// Check pattern 2: block-pass form `hash.keys.each(&:foo)` → `hash.each_key(&:foo)`.
fn check_kv_each_with_block_pass(
    each_send: NodeId,
    kv_recv: NodeId,
    kv_method: &str,
    cx: &Cx<'_>,
) {
    // Guard: find the receiver of the kv call (e.g., `hash`).
    let Some(parent_receiver) = cx.call_receiver(kv_recv).get() else {
        return;
    };

    if !is_handleable(kv_recv, cx) {
        return;
    }

    if is_allowed_receiver(parent_receiver, cx) {
        return;
    }

    // Offense range: from `keys`/`values` selector start to `each` selector end.
    let kv_selector = cx.node(kv_recv).loc.name;
    let each_selector = cx.node(each_send).loc.name;
    let offense_range = Range {
        start: kv_selector.start,
        end: each_selector.end,
    };

    let prefer = if kv_method == "keys" { "each_key" } else { "each_value" };
    let current = cx.raw_source(offense_range);
    let message = format!("Use `{prefer}` instead of `{current}`.");

    cx.emit_offense(offense_range, &message, None);
    cx.emit_edit(offense_range, prefer);
}

/// Check pattern 3: `hash.each { |k, unused_v| body }` → `each_key` (or `each_value`).
fn check_each_arguments(block_node: NodeId, each_call: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { args, body, .. } = *cx.kind(block_node) else {
        return;
    };

    // Must have a body.
    if body.get().is_none() {
        return;
    }
    let body_id = body.get().unwrap();

    // Must have exactly 2 plain Arg params.
    let NodeKind::Args(args_list) = *cx.kind(args) else {
        return;
    };
    let params = cx.list(args_list);
    if params.len() != 2 {
        return;
    }
    let key_id = params[0];
    let val_id = params[1];

    // Both must be plain Arg nodes (no destructuring).
    let NodeKind::Arg(key_sym) = *cx.kind(key_id) else {
        return;
    };
    let NodeKind::Arg(val_sym) = *cx.kind(val_id) else {
        return;
    };

    let key_name = cx.symbol_str(key_sym);
    let val_name = cx.symbol_str(val_sym);

    // Check which args are used in the body.
    let value_used = is_lvar_used(body_id, val_name, cx);
    let key_used = is_lvar_used(body_id, key_name, cx);

    // Skip if both used or both unused.
    if value_used == key_used {
        return;
    }

    // Guard: root receiver must exist.
    let Some(recv_id) = cx.call_receiver(each_call).get() else {
        return;
    };

    if !is_handleable(each_call, cx) {
        return;
    }

    // AllowedReceivers check: root receiver of the `each` call.
    let root = root_receiver(recv_id, cx);
    if is_allowed_receiver(root, cx) {
        return;
    }

    let prefer;
    let unused_name;
    let unused_range;

    if !value_used {
        // Value is unused → use each_key, remove the `, val` range.
        prefer = "each_key";
        unused_name = val_name;
        // Remove from end of key_id range to end of val_id range: `, v`.
        unused_range = Range {
            start: cx.range(key_id).end,
            end: cx.range(val_id).end,
        };
    } else {
        // Key is unused → use each_value, remove `key, ` range.
        prefer = "each_value";
        unused_name = key_name;
        // Remove from start of key_id range to start of val_id range: `k, `.
        unused_range = Range {
            start: cx.range(key_id).start,
            end: cx.range(val_id).start,
        };
    }

    let current = cx.method_name(each_call).unwrap_or("each");
    let message = format!(
        "Use `{prefer}` instead of `{current}` and remove the unused `{unused_name}` block argument."
    );

    cx.emit_offense(cx.range(block_node), &message, None);
    // Edit 1: rename `each` selector.
    cx.emit_edit(cx.node(each_call).loc.name, prefer);
    // Edit 2: remove unused arg.
    cx.emit_edit(unused_range, "");
}

/// Returns true when `name` appears as an lvar in any descendant of `body`.
fn is_lvar_used(body: NodeId, name: &str, cx: &Cx<'_>) -> bool {
    if let NodeKind::Lvar(sym) = *cx.kind(body) {
        if cx.symbol_str(sym) == name {
            return true;
        }
    }
    cx.descendants(body).iter().any(|&d| {
        if let NodeKind::Lvar(sym) = *cx.kind(d) {
            cx.symbol_str(sym) == name
        } else {
            false
        }
    })
}

/// Guards shared by kv_each and each_arguments patterns.
fn is_handleable(kv_or_each_recv: NodeId, cx: &Cx<'_>) -> bool {
    // Guard: array converter method as preceding.
    if has_array_converter_preceding(kv_or_each_recv, cx) {
        return false;
    }
    // Guard: root receiver must exist.
    let Some(recv_id) = cx.call_receiver(kv_or_each_recv).get() else {
        return false;
    };
    let root = root_receiver(recv_id, cx);
    // Guard: root receiver must not be a non-hash literal.
    if is_non_hash_literal(root, cx) {
        return false;
    }
    true
}

/// Returns true when the immediate receiver of `node` is a call to an
/// array converter method.
fn has_array_converter_preceding(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(recv_id) = cx.call_receiver(node).get() else {
        return false;
    };
    match cx.method_name(recv_id) {
        Some(m) => ARRAY_CONVERTER_METHODS.contains(&m),
        None => false,
    }
}

/// Walk to the root (deepest) receiver in a send chain.
fn root_receiver(node: NodeId, cx: &Cx<'_>) -> NodeId {
    let mut current = node;
    loop {
        match cx.call_receiver(current).get() {
            Some(recv) => current = recv,
            None => return current,
        }
    }
}

/// True when `node` is a literal that is NOT a hash literal.
/// Hash literals are fine (they really are hashes); string/int/etc. are not.
fn is_non_hash_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Hash { .. } => false, // hash literal is ok
        NodeKind::Int(_)
        | NodeKind::Float(_)
        | NodeKind::Rational { .. }
        | NodeKind::Complex { .. }
        | NodeKind::Str(_)
        | NodeKind::Sym(_)
        | NodeKind::Array { .. }
        | NodeKind::Nil
        | NodeKind::True_
        | NodeKind::False_ => true,
        _ => false,
    }
}

/// Returns true when the source of `node` matches any entry in AllowedReceivers.
fn is_allowed_receiver(node: NodeId, cx: &Cx<'_>) -> bool {
    let opts = cx.options_or_default::<HashEachMethodsOptions>();
    if opts.allowed_receivers.is_empty() {
        return false;
    }
    let src = cx.raw_source(cx.range(node));
    opts.allowed_receivers.iter().any(|r| r == src)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Pattern 1: kv_each block form -----

    #[test]
    fn flags_keys_each_block() {
        test::<HashEachMethods>().expect_offense(indoc! {r#"
            hash.keys.each { |k| p k }
                 ^^^^^^^^^ Use `each_key` instead of `keys.each`.
        "#});
    }

    #[test]
    fn corrects_keys_each_block() {
        test::<HashEachMethods>().expect_correction(
            indoc! {r#"
                hash.keys.each { |k| p k }
                     ^^^^^^^^^ Use `each_key` instead of `keys.each`.
            "#},
            "hash.each_key { |k| p k }\n",
        );
    }

    #[test]
    fn flags_values_each_block() {
        test::<HashEachMethods>().expect_offense(indoc! {r#"
            hash.values.each { |v| p v }
                 ^^^^^^^^^^^ Use `each_value` instead of `values.each`.
        "#});
    }

    #[test]
    fn corrects_values_each_block() {
        test::<HashEachMethods>().expect_correction(
            indoc! {r#"
                hash.values.each { |v| p v }
                     ^^^^^^^^^^^ Use `each_value` instead of `values.each`.
            "#},
            "hash.each_value { |v| p v }\n",
        );
    }

    // ----- Pattern 2: block-pass form -----

    #[test]
    fn flags_keys_each_block_pass() {
        test::<HashEachMethods>().expect_offense(indoc! {r#"
            hash.keys.each(&:to_s)
                 ^^^^^^^^^ Use `each_key` instead of `keys.each`.
        "#});
    }

    #[test]
    fn corrects_keys_each_block_pass() {
        test::<HashEachMethods>().expect_correction(
            indoc! {r#"
                hash.keys.each(&:to_s)
                     ^^^^^^^^^ Use `each_key` instead of `keys.each`.
            "#},
            "hash.each_key(&:to_s)\n",
        );
    }

    // ----- Pattern 3: each_arguments unused arg -----

    #[test]
    fn flags_each_with_unused_value() {
        test::<HashEachMethods>().expect_offense(indoc! {r#"
            hash.each { |k, unused_value| p k }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `each_key` instead of `each` and remove the unused `unused_value` block argument.
        "#});
    }

    #[test]
    fn corrects_each_with_unused_value() {
        test::<HashEachMethods>().expect_correction(
            indoc! {r#"
                hash.each { |k, unused_value| p k }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `each_key` instead of `each` and remove the unused `unused_value` block argument.
            "#},
            "hash.each_key { |k| p k }\n",
        );
    }

    #[test]
    fn flags_each_with_unused_key() {
        test::<HashEachMethods>().expect_offense(indoc! {r#"
            hash.each { |unused_key, v| p v }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `each_value` instead of `each` and remove the unused `unused_key` block argument.
        "#});
    }

    #[test]
    fn corrects_each_with_unused_key() {
        test::<HashEachMethods>().expect_correction(
            indoc! {r#"
                hash.each { |unused_key, v| p v }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `each_value` instead of `each` and remove the unused `unused_key` block argument.
            "#},
            "hash.each_value { |v| p v }\n",
        );
    }

    // Both args used — no offense.
    #[test]
    fn accepts_each_with_both_args_used() {
        test::<HashEachMethods>().expect_no_offenses("hash.each { |k, v| p k; p v }\n");
    }

    // Both args unused — no offense (not specifically flagged).
    #[test]
    fn accepts_each_with_both_args_unused() {
        test::<HashEachMethods>().expect_no_offenses("hash.each { |_k, _v| puts 'x' }\n");
    }

    // ----- Array converter preceding -----

    #[test]
    fn accepts_flatten_preceding() {
        test::<HashEachMethods>().expect_no_offenses("hash.flatten.each { |v| p v }\n");
    }

    #[test]
    fn accepts_sort_preceding() {
        test::<HashEachMethods>().expect_no_offenses("hash.sort.each { |k, v| p k }\n");
    }

    // ----- AllowedReceivers -----

    #[test]
    fn accepts_allowed_receiver() {
        test::<HashEachMethods>()
            .with_options(&HashEachMethodsOptions {
                allowed_receivers: vec!["Thread.current".to_string()],
            })
            .expect_no_offenses("Thread.current.keys.each { |k| p k }\n");
    }

    #[test]
    fn flags_non_allowed_receiver() {
        test::<HashEachMethods>().expect_offense(indoc! {r#"
            Thread.current.keys.each { |k| p k }
                           ^^^^^^^^^ Use `each_key` instead of `keys.each`.
        "#});
    }

    // ----- Negative: not a block -----

    #[test]
    fn accepts_keys_each_without_block_or_block_pass() {
        test::<HashEachMethods>().expect_no_offenses("hash.keys.each\n");
    }
}

murphy_plugin_api::submit_cop!(HashEachMethods);
