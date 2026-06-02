//! `Style/HashTransformValues` — prefer `transform_values` over verbose hash transformation patterns.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashTransformValues
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covers three of the four patterns from RuboCop's HashTransformValues:
//!     1. `hash.map { |k, v| [k, expr] }.to_h`
//!     2. `hash.to_h { |k, v| [k, expr] }` (Ruby 2.6+)
//!     3. `Hash[hash.map { |k, v| [k, expr] }]`
//!
//!   The receiver (`hash`) must satisfy `hash_receiver?` — either a hash literal
//!   `{...}`, or a call whose method is one of: `to_h`, `to_hash`, `merge`,
//!   `merge!`, `update`, `invert`, `except`, `tally`, `transform_keys`,
//!   `transform_keys!`, `transform_values`, `transform_values!`, `group_by`.
//!
//!   Guards (matching RuboCop):
//!     - Noop: body is just `lvar(val_arg)` unchanged → no offense.
//!     - Uses both args: value body references the key arg → no offense.
//!     - Does not use val arg: value body never references val arg → no offense.
//!
//!   Gap: `each_with_object({}) { |(k, v), h| h[k] = ... }` is not detected
//!   because the destructured mlhs block parameter `(k, v)` is an opaque
//!   `Unknown` node in Murphy's AST; the key/value arg names cannot be
//!   recovered from it without a parser ABI extension.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! {a: 1}.map { |k, v| [k, foo(v)] }.to_h
//! {a: 1}.to_h { |k, v| [k, v * v] }
//! Hash[{a: 1}.map { |k, v| [k, foo(v)] }]
//! foo.to_h.map { |k, v| [k, v.upcase] }.to_h
//!
//! # good
//! {a: 1}.transform_values { |v| foo(v) }
//! {a: 1}.transform_values { |v| v * v }
//! ```
//!
//! ## Autocorrect
//!
//! Whole-node replacement (structural rearrangement, not surgical edits):
//! replaces the entire offense node with `receiver.transform_values { |val| body }`.
//! The val arg and body expressions pass through byte-for-byte from the source.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, Symbol, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct HashTransformValues;

/// Methods whose receivers are known to produce a hash.
const HASH_METHODS: &[&str] = &[
    "to_h", "to_hash", "merge", "merge!", "update", "invert", "except", "tally",
    "transform_keys", "transform_keys!", "transform_values", "transform_values!", "group_by",
];

/// Map/collect method names (both are aliases for the same thing).
const MAP_METHODS: &[&str] = &["map", "collect"];

#[cop(
    name = "Style/HashTransformValues",
    description = "Prefer `transform_values` over verbose hash transformation patterns.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
    safe_autocorrect = false,
)]
impl HashTransformValues {
    /// Pattern 1: `hash.map { |k, v| [k, expr] }.to_h`
    /// Pattern 3: `Hash[hash.map { |k, v| [k, expr] }]`
    ///
    /// Both are triggered when we see a `to_h` or `Hash[...]` send whose inner
    /// argument is a block over `map`/`collect`.
    #[on_node(kind = "send", methods = ["to_h", "[]"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let method = cx.method_name(node).unwrap_or("");
        match method {
            "to_h" => check_map_to_h(node, cx),
            "[]" => check_hash_brackets_map(node, cx),
            _ => {}
        }
    }

    /// Pattern 2: `hash.to_h { |k, v| [k, expr] }` — a block directly over a `to_h` call.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_to_h_block(node, cx);
    }
}

// ---------------------------------------------------------------------------
// Pattern 1: map { |k, v| [k, expr] }.to_h
// ---------------------------------------------------------------------------

/// Check `block.to_h` where block is `receiver.map { |k, v| [k, expr] }`.
fn check_map_to_h(to_h_node: NodeId, cx: &Cx<'_>) {
    // to_h must have no arguments and no block.
    if !cx.call_arguments(to_h_node).is_empty() {
        return;
    }
    if cx.block_node(to_h_node).get().is_some() {
        return;
    }
    // Receiver of to_h must be a block over map/collect.
    let recv = match cx.call_receiver(to_h_node).get() {
        Some(r) => r,
        None => return,
    };
    let block_node = recv;
    let NodeKind::Block { call, .. } = *cx.kind(block_node) else {
        return;
    };
    if !matches!(cx.method_name(call), Some(m) if MAP_METHODS.contains(&m)) {
        return;
    }
    // The map call's receiver must be a hash receiver.
    let map_recv = match cx.call_receiver(call).get() {
        Some(r) => r,
        None => return,
    };
    if !is_hash_receiver(map_recv, cx) {
        return;
    }
    // Extract |k, v| → [k, expr] block captures.
    let (key_sym, val_sym, val_body) = match extract_kv_array_body(block_node, cx) {
        Some(x) => x,
        None => return,
    };
    // Apply guards.
    if !passes_guards(key_sym, val_sym, val_body, cx) {
        return;
    }
    // Offense: the entire `to_h_node` (from receiver.map{...}.to_h).
    let offense_range = cx.range(to_h_node);
    let msg = format!(
        "Prefer `transform_values` over `{}`.",
        "map {...}.to_h"
    );
    cx.emit_offense(offense_range, &msg, None);
    // Autocorrect: receiver.transform_values { |val| body }
    emit_transform_values_correction(map_recv, val_sym, val_body, offense_range, cx);
}

// ---------------------------------------------------------------------------
// Pattern 2: to_h { |k, v| [k, expr] }
// ---------------------------------------------------------------------------

/// Check block node for `receiver.to_h { |k, v| [k, expr] }`.
fn check_to_h_block(block_node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call, .. } = *cx.kind(block_node) else {
        return;
    };
    // Must be a `to_h` call.
    if cx.method_name(call) != Some("to_h") {
        return;
    }
    // The call must have no positional arguments.
    if !cx.call_arguments(call).is_empty() {
        return;
    }
    // Receiver must be a hash receiver.
    let recv = match cx.call_receiver(call).get() {
        Some(r) => r,
        None => return,
    };
    if !is_hash_receiver(recv, cx) {
        return;
    }
    // Extract |k, v| → [k, expr] block captures.
    let (key_sym, val_sym, val_body) = match extract_kv_array_body(block_node, cx) {
        Some(x) => x,
        None => return,
    };
    // Apply guards.
    if !passes_guards(key_sym, val_sym, val_body, cx) {
        return;
    }
    // Offense: the entire block node.
    let offense_range = cx.range(block_node);
    let msg = format!(
        "Prefer `transform_values` over `{}`.",
        "to_h {...}"
    );
    cx.emit_offense(offense_range, &msg, None);
    emit_transform_values_correction(recv, val_sym, val_body, offense_range, cx);
}

// ---------------------------------------------------------------------------
// Pattern 3: Hash[receiver.map { |k, v| [k, expr] }]
// ---------------------------------------------------------------------------

/// Check `Hash[receiver.map { |k, v| [k, expr] }]`.
fn check_hash_brackets_map(send_node: NodeId, cx: &Cx<'_>) {
    // Must be `Hash[...]`.
    let recv = match cx.call_receiver(send_node).get() {
        Some(r) => r,
        None => return,
    };
    if !is_const_hash(recv, cx) {
        return;
    }
    // Must have exactly one argument: the block.
    let args = cx.call_arguments(send_node);
    if args.len() != 1 {
        return;
    }
    let block_node = args[0];
    let NodeKind::Block { call, .. } = *cx.kind(block_node) else {
        return;
    };
    if !matches!(cx.method_name(call), Some(m) if MAP_METHODS.contains(&m)) {
        return;
    }
    // The map call's receiver must be a hash receiver.
    let map_recv = match cx.call_receiver(call).get() {
        Some(r) => r,
        None => return,
    };
    if !is_hash_receiver(map_recv, cx) {
        return;
    }
    // Extract |k, v| → [k, expr] block captures.
    let (key_sym, val_sym, val_body) = match extract_kv_array_body(block_node, cx) {
        Some(x) => x,
        None => return,
    };
    // Apply guards.
    if !passes_guards(key_sym, val_sym, val_body, cx) {
        return;
    }
    // Offense: the entire send node.
    let offense_range = cx.range(send_node);
    let msg = format!(
        "Prefer `transform_values` over `{}`.",
        "Hash[_.map {...}]"
    );
    cx.emit_offense(offense_range, &msg, None);
    emit_transform_values_correction(map_recv, val_sym, val_body, offense_range, cx);
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Check if a node is `Hash` (const without scope).
fn is_const_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    if let NodeKind::Const { scope, name } = *cx.kind(node) {
        return scope.get().is_none() && cx.symbol_str(name) == "Hash";
    }
    false
}

/// Check if a node satisfies the `hash_receiver?` condition:
/// - A hash literal `{...}`
/// - A send whose method is in HASH_METHODS (e.g. `to_h`, `merge`, etc.)
/// - A block over one of the hash-returning block methods
fn is_hash_receiver(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Hash(_) => true,
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {
            matches!(cx.method_name(node), Some(m) if HASH_METHODS.contains(&m))
        }
        NodeKind::Block { call, .. } => {
            // Block over group_by / to_h / transform_keys / etc.
            // Also: block over each_with_object with hash argument.
            matches!(
                cx.method_name(call),
                Some(
                    "group_by"
                        | "to_h"
                        | "tally"
                        | "transform_keys"
                        | "transform_keys!"
                        | "transform_values"
                        | "transform_values!"
                )
            ) || is_each_with_object_hash(call, cx)
        }
        _ => false,
    }
}

/// Check if a call is `each_with_object({})` (hash arg).
fn is_each_with_object_hash(call: NodeId, cx: &Cx<'_>) -> bool {
    if cx.method_name(call) != Some("each_with_object") {
        return false;
    }
    let args = cx.call_arguments(call);
    if args.len() != 1 {
        return false;
    }
    matches!(cx.kind(args[0]), NodeKind::Hash(_))
}

/// Extract `(key_sym, val_sym, val_body_node)` from a block `{ |k, v| [k, expr] }`.
///
/// Returns `None` if the block doesn't match the expected pattern.
fn extract_kv_array_body(
    block_node: NodeId,
    cx: &Cx<'_>,
) -> Option<(Symbol, Symbol, NodeId)> {
    let NodeKind::Block { args: args_node, body: body_opt, .. } = *cx.kind(block_node) else {
        return None;
    };
    // Must have a body.
    let body = body_opt.get()?;

    // Block args: exactly two plain args.
    let NodeKind::Args(args_list) = *cx.kind(args_node) else {
        return None;
    };
    let args = cx.list(args_list);
    if args.len() != 2 {
        return None;
    }
    let (NodeKind::Arg(key_sym), NodeKind::Arg(val_sym)) =
        (*cx.kind(args[0]), *cx.kind(args[1]))
    else {
        return None;
    };

    // Body must be a two-element array `[key_lvar, expr]`.
    let NodeKind::Array(elems_list) = *cx.kind(body) else {
        return None;
    };
    let elems = cx.list(elems_list);
    if elems.len() != 2 {
        return None;
    }

    // First element must be `lvar(key_sym)`.
    if !matches!(cx.kind(elems[0]), NodeKind::Lvar(s) if *s == key_sym) {
        return None;
    }

    Some((key_sym, val_sym, elems[1]))
}

/// Returns `true` if the transformation passes all three guards.
fn passes_guards(key_sym: Symbol, val_sym: Symbol, val_body: NodeId, cx: &Cx<'_>) -> bool {
    // Guard 1: noop — body is just `lvar(val_sym)` unchanged.
    if matches!(cx.kind(val_body), NodeKind::Lvar(s) if *s == val_sym) {
        return false;
    }
    // Guard 2: transformation uses both args — body references key_sym.
    if references_lvar(val_body, key_sym, cx) {
        return false;
    }
    // Guard 3: must use the val arg at least once.
    if !references_lvar(val_body, val_sym, cx) {
        return false;
    }
    true
}

/// Recursively checks if the subtree rooted at `node` contains an `lvar(sym)`.
fn references_lvar(node: NodeId, sym: Symbol, cx: &Cx<'_>) -> bool {
    if matches!(cx.kind(node), NodeKind::Lvar(s) if *s == sym) {
        return true;
    }
    cx.descendants(node).iter().any(|&child| matches!(cx.kind(child), NodeKind::Lvar(s) if *s == sym))
}

/// Emit the autocorrect edit: replace `offense_range` with
/// `recv.transform_values { |val| body }`.
fn emit_transform_values_correction(
    recv: NodeId,
    val_sym: Symbol,
    val_body: NodeId,
    offense_range: Range,
    cx: &Cx<'_>,
) {
    let recv_src = cx.raw_source(cx.range(recv));
    let val_src = cx.symbol_str(val_sym);
    let body_src = cx.raw_source(cx.range(val_body));
    let correction = format!(
        "{}.transform_values {{ |{}| {} }}",
        recv_src, val_src, body_src
    );
    cx.emit_edit(offense_range, &correction);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::HashTransformValues;
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- Pattern 1: map { }.to_h ----

    #[test]
    fn flags_map_to_h() {
        test::<HashTransformValues>().expect_offense(indoc! {r#"
            {a: 1}.map { |k, v| [k, foo(v)] }.to_h
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `transform_values` over `map {...}.to_h`.
        "#});
    }

    #[test]
    fn corrects_map_to_h() {
        test::<HashTransformValues>().expect_correction(
            indoc! {r#"
                {a: 1}.map { |k, v| [k, foo(v)] }.to_h
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `transform_values` over `map {...}.to_h`.
            "#},
            "{a: 1}.transform_values { |v| foo(v) }\n",
        );
    }

    #[test]
    fn flags_collect_to_h() {
        test::<HashTransformValues>().expect_offense(indoc! {r#"
            {a: 1}.collect { |k, v| [k, v * 2] }.to_h
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `transform_values` over `map {...}.to_h`.
        "#});
    }

    // ---- Pattern 2: to_h { } ----

    #[test]
    fn flags_to_h_block() {
        test::<HashTransformValues>().expect_offense(indoc! {r#"
            {a: 1}.to_h { |k, v| [k, v * v] }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `transform_values` over `to_h {...}`.
        "#});
    }

    #[test]
    fn corrects_to_h_block() {
        test::<HashTransformValues>().expect_correction(
            indoc! {r#"
                {a: 1}.to_h { |k, v| [k, v * v] }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `transform_values` over `to_h {...}`.
            "#},
            "{a: 1}.transform_values { |v| v * v }\n",
        );
    }

    // ---- Pattern 3: Hash[map { }] ----

    #[test]
    fn flags_hash_brackets_map() {
        test::<HashTransformValues>().expect_offense(indoc! {r#"
            Hash[{a: 1}.map { |k, v| [k, foo(v)] }]
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `transform_values` over `Hash[_.map {...}]`.
        "#});
    }

    #[test]
    fn corrects_hash_brackets_map() {
        test::<HashTransformValues>().expect_correction(
            indoc! {r#"
                Hash[{a: 1}.map { |k, v| [k, foo(v)] }]
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `transform_values` over `Hash[_.map {...}]`.
            "#},
            "{a: 1}.transform_values { |v| foo(v) }\n",
        );
    }

    // ---- hash_receiver? guard ----

    #[test]
    fn flags_to_h_chain_map_to_h() {
        // foo.to_h is a hash receiver
        test::<HashTransformValues>().expect_offense(indoc! {r#"
            foo.to_h.map { |k, v| [k, v.upcase] }.to_h
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `transform_values` over `map {...}.to_h`.
        "#});
    }

    #[test]
    fn no_offense_unknown_receiver_map_to_h() {
        // foo.bar is not a hash receiver
        test::<HashTransformValues>().expect_no_offenses(
            "foo.bar.map { |k, v| [k, v.upcase] }.to_h\n",
        );
    }

    #[test]
    fn no_offense_unknown_receiver_to_h_block() {
        // foo.bar is not a hash receiver
        test::<HashTransformValues>().expect_no_offenses(
            "foo.bar.to_h { |k, v| [k, v.upcase] }\n",
        );
    }

    // ---- noop guard ----

    #[test]
    fn no_offense_noop_body() {
        // body is just `v` unchanged
        test::<HashTransformValues>().expect_no_offenses(
            "{a: 1}.map { |k, v| [k, v] }.to_h\n",
        );
    }

    // ---- uses-both-args guard ----

    #[test]
    fn no_offense_body_uses_key() {
        // body references `k` (key arg) — skip
        test::<HashTransformValues>().expect_no_offenses(
            "{a: 1}.map { |k, v| [k, v.to_s + k.to_s] }.to_h\n",
        );
    }

    // ---- use-val-arg guard ----

    #[test]
    fn no_offense_body_does_not_use_val() {
        // body doesn't reference `v` at all — skip
        test::<HashTransformValues>().expect_no_offenses(
            "{a: 1}.map { |k, v| [k, \"constant\"] }.to_h\n",
        );
    }

    // ---- body must be two-element array ----

    #[test]
    fn no_offense_single_element_array() {
        test::<HashTransformValues>().expect_no_offenses(
            "{a: 1}.map { |k, v| [v] }.to_h\n",
        );
    }

    #[test]
    fn no_offense_key_not_first() {
        // array is [v, k] instead of [k, v_expr]
        test::<HashTransformValues>().expect_no_offenses(
            "{a: 1}.map { |k, v| [v, k] }.to_h\n",
        );
    }

    // ---- merge receiver ----

    #[test]
    fn flags_merge_receiver() {
        test::<HashTransformValues>().expect_offense(indoc! {r#"
            foo.merge(bar).map { |k, v| [k, v.to_s] }.to_h
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `transform_values` over `map {...}.to_h`.
        "#});
    }

    #[test]
    fn corrects_merge_receiver() {
        test::<HashTransformValues>().expect_correction(
            indoc! {r#"
                foo.merge(bar).map { |k, v| [k, v.to_s] }.to_h
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `transform_values` over `map {...}.to_h`.
            "#},
            "foo.merge(bar).transform_values { |v| v.to_s }\n",
        );
    }
}

murphy_plugin_api::submit_cop!(HashTransformValues);
