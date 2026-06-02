//! `Style/HashTransformKeys` — prefer `transform_keys` over `each_with_object`,
//! `map`, or `to_h` when transforming hash keys.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashTransformKeys
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Marked unsafe in RuboCop (Safe: false) because the receiver may not be a
//!   real Hash. Murphy does not have a Safe/SafeAutoCorrect cop-level attribute
//!   yet; the unsafe nature is documented here only.
//!
//!   Requires Ruby >= 2.5 (RuboCop minimum_target_ruby_version 2.5). Murphy
//!   does not gate on TargetRubyVersion yet; this is documented here only.
//!   The to_h block form additionally requires Ruby >= 2.6; same note applies.
//!
//!   The `each_with_object` pattern (block param `|(k, v), h|`) is NOT ported:
//!   Murphy's AST represents the destructured multi-assignment param as
//!   `NodeKind::Unknown`, making it impossible to extract k/v names from the
//!   block arguments or validate that only `k` is transformed.
//!
//!   Three viable patterns ARE ported (mirrors RuboCop's node matchers minus
//!   each_with_object):
//!     1. `receiver.map/collect { |k, v| [transform(k), v] }.to_h`
//!     2. `Hash[receiver.map/collect { |k, v| [transform(k), v] }]`
//!     3. `receiver.to_h { |k, v| [transform(k), v] }`
//!
//!   hash_receiver? allowlist: hash literal; send to {to_h, to_hash, merge,
//!   merge!, update, invert, except, tally}; block call to {group_by, to_h,
//!   tally, transform_keys, transform_keys!, transform_values, transform_values!,
//!   each_with_object(hash)}.
//!
//!   Guards (mirrors RuboCop's handle_possible_offense):
//!     - noop: key body is just `(lvar k)` unchanged — false positive, skip.
//!     - uses_both_args: key transform references the value lvar — not a pure
//!       key transform, skip.
//!     - use_transformed_argname: key transform must reference k somewhere.
//!
//!   Block params must be exactly two flat args `|k, v|` (no destructuring).
//!   The array body must be `[key_expr, (lvar v)]` — value passes through.
//!
//!   Offense range: the map/collect selector (loc.name) for patterns 1 and 2;
//!   the to_h selector (loc.name of the call node) for pattern 3.
//!
//!   Autocorrect rewrites the block to use `transform_keys` with only the key
//!   argument. For patterns 1 and 2, the wrapping call is removed. The block
//!   body becomes just the key-transforming expression.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! {a: 1, b: 2}.map { |k, v| [k.to_s, v] }.to_h
//! Hash[{a: 1, b: 2}.collect { |k, v| [foo(k), v] }]
//! {a: 1, b: 2}.to_h { |k, v| [k.to_s, v] }
//! foo.to_h.map { |k, v| [k.to_s, v] }.to_h
//!
//! # good
//! {a: 1, b: 2}.transform_keys { |k| k.to_s }
//! foo.to_h.transform_keys { |k| k.to_s }
//!
//! # won't flag — receiver is not known to be a hash
//! foo.bar.map { |k, v| [k.to_s, v] }.to_h
//! baz.map { |k, v| [k.to_s, v] }.to_h
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, Symbol, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct HashTransformKeys;

#[cop(
    name = "Style/HashTransformKeys",
    description = "Prefer `transform_keys` over `each_with_object`, `map`, or `to_h`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl HashTransformKeys {
    // Pattern 1: `receiver.map { }.to_h` — triggers on the `to_h` send node.
    // Pattern 2: `Hash[receiver.map { }]` — triggers on the `[]` send node.
    #[on_node(kind = "send", methods = ["to_h", "[]"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        match cx.method_name(node).unwrap_or("") {
            "to_h" => check_map_to_h(node, cx),
            "[]" => check_hash_brackets_map(node, cx),
            _ => {}
        }
    }

    // Pattern 3: `receiver.to_h { |k, v| [...] }` — triggers on the block node.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_to_h(node, cx);
    }
}

// ---------------------------------------------------------------------------
// hash_receiver? — matches receivers known to be hashes.
// ---------------------------------------------------------------------------

fn is_hash_receiver(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Hash { .. } => true,
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {
            let method = cx.method_name(node).unwrap_or("");
            matches!(
                method,
                "to_h" | "to_hash" | "merge" | "merge!" | "update" | "invert" | "except"
                    | "tally"
            )
        }
        NodeKind::Block { call, .. } => {
            let method = cx.method_name(*call).unwrap_or("");
            if matches!(
                method,
                "group_by"
                    | "to_h"
                    | "tally"
                    | "transform_keys"
                    | "transform_keys!"
                    | "transform_values"
                    | "transform_values!"
            ) {
                return true;
            }
            // each_with_object with a hash literal argument
            if method == "each_with_object" {
                let args = cx.call_arguments(*call);
                if args.len() == 1 && matches!(cx.kind(args[0]), NodeKind::Hash { .. }) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Shared match helper
// ---------------------------------------------------------------------------

struct BlockMatch {
    block_node: NodeId,
    /// The inner send node (map/collect for patterns 1/2; to_h for pattern 3).
    call_node: NodeId,
    key_sym: Symbol,
    val_sym: Symbol,
    /// The key-transforming expression.
    key_expr: NodeId,
    match_desc: &'static str,
    /// Offense annotation range.
    offense_range: Range,
}

/// Try to match a block against the hash-transform-keys shape.
///
/// `valid_methods`: slice of method names to accept on the call node
/// (e.g. `["map", "collect"]` or `["to_h"]`).
fn match_transform_keys_block(
    block_node: NodeId,
    valid_methods: &[&str],
    cx: &Cx<'_>,
) -> Option<BlockMatch> {
    let NodeKind::Block { call, args, body } = *cx.kind(block_node) else {
        return None;
    };

    // Call method must be in the valid set.
    let method = cx.method_name(call)?;
    if !valid_methods.contains(&method) {
        return None;
    }

    // Receiver of the call must be a known hash receiver.
    let receiver = cx.call_receiver(call).get()?;
    if !is_hash_receiver(receiver, cx) {
        return None;
    }

    // Block args: `(args (arg k) (arg v))` — exactly two flat Arg nodes.
    let NodeKind::Args(args_list) = *cx.kind(args) else {
        return None;
    };
    let arg_nodes = cx.list(args_list);
    if arg_nodes.len() != 2 {
        return None;
    }
    let (NodeKind::Arg(key_sym), NodeKind::Arg(val_sym)) =
        (cx.kind(arg_nodes[0]), cx.kind(arg_nodes[1]))
    else {
        return None;
    };
    let key_sym = *key_sym;
    let val_sym = *val_sym;

    // Body: `(array key_expr (lvar v))` — value passes through unchanged.
    let body_id = body.get()?;
    let NodeKind::Array(elems) = *cx.kind(body_id) else {
        return None;
    };
    let elem_list = cx.list(elems);
    if elem_list.len() != 2 {
        return None;
    }
    let key_expr = elem_list[0];
    let val_elem = elem_list[1];

    // Second element must be `(lvar v)`.
    if let NodeKind::Lvar(sym) = cx.kind(val_elem) {
        if *sym != val_sym {
            return None;
        }
    } else {
        return None;
    }

    let offense_range = cx.node(call).loc.name;
    let match_desc = if matches!(method, "map" | "collect") {
        "map {...}.to_h"
    } else {
        "to_h {...}"
    };

    Some(BlockMatch {
        block_node,
        call_node: call,
        key_sym,
        val_sym,
        key_expr,
        match_desc,
        offense_range,
    })
}

// ---------------------------------------------------------------------------
// Guards
// ---------------------------------------------------------------------------

fn is_noop(key_expr: NodeId, key_sym: Symbol, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(key_expr), NodeKind::Lvar(s) if *s == key_sym)
}

fn transformation_uses_both_args(key_expr: NodeId, val_sym: Symbol, cx: &Cx<'_>) -> bool {
    if matches!(cx.kind(key_expr), NodeKind::Lvar(s) if *s == val_sym) {
        return true;
    }
    cx.descendants(key_expr)
        .iter()
        .any(|&d| matches!(cx.kind(d), NodeKind::Lvar(s) if *s == val_sym))
}

fn uses_key_argname(key_expr: NodeId, key_sym: Symbol, cx: &Cx<'_>) -> bool {
    if matches!(cx.kind(key_expr), NodeKind::Lvar(s) if *s == key_sym) {
        return true;
    }
    cx.descendants(key_expr)
        .iter()
        .any(|&d| matches!(cx.kind(d), NodeKind::Lvar(s) if *s == key_sym))
}

fn apply_guards(m: &BlockMatch, cx: &Cx<'_>) -> bool {
    !is_noop(m.key_expr, m.key_sym, cx)
        && !transformation_uses_both_args(m.key_expr, m.val_sym, cx)
        && uses_key_argname(m.key_expr, m.key_sym, cx)
}

// ---------------------------------------------------------------------------
// Autocorrect
// ---------------------------------------------------------------------------

fn emit_transform_keys_correction(m: &BlockMatch, cx: &Cx<'_>) {
    let key_name = cx.symbol_str(m.key_sym);
    let key_body_src = cx.raw_source(cx.range(m.key_expr));
    let new_args = format!("|{key_name}|");
    let new_body = if matches!(cx.kind(m.key_expr), NodeKind::Hash { .. }) {
        format!("{{ {key_body_src} }}")
    } else {
        key_body_src.to_string()
    };

    let NodeKind::Block { body: body_opt, .. } = *cx.kind(m.block_node) else {
        return;
    };
    let Some(body_id) = body_opt.get() else {
        return;
    };

    // Replace the `|k, v|` pipes range with `|k|`.
    if let Some(pipes_range) = block_args_pipes_range(m.block_node, cx) {
        cx.emit_edit(pipes_range, &new_args);
    }

    // Replace body with just the key expr.
    cx.emit_edit(cx.range(body_id), &new_body);

    // Rename the call selector to `transform_keys`.
    cx.emit_edit(cx.node(m.call_node).loc.name, "transform_keys");
}

/// Find the range spanning the two `|` pipe tokens in the block args.
fn block_args_pipes_range(block_node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let block_range = cx.range(block_node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    let lo = toks.partition_point(|t| t.range.start < block_range.start);
    let mut pipes = toks[lo..]
        .iter()
        .take_while(|t| t.range.start < block_range.end)
        .filter(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"|"
        })
        .copied();

    let open_pipe = pipes.next()?;
    let close_pipe = pipes.next()?;

    Some(Range {
        start: open_pipe.range.start,
        end: close_pipe.range.end,
    })
}

// ---------------------------------------------------------------------------
// Pattern 1: `receiver.map { |k, v| [transform(k), v] }.to_h`
// ---------------------------------------------------------------------------

fn check_map_to_h(to_h_node: NodeId, cx: &Cx<'_>) {
    let receiver = match cx.call_receiver(to_h_node).get() {
        Some(r) => r,
        None => return,
    };
    let NodeKind::Block { .. } = cx.kind(receiver) else {
        return;
    };

    let Some(m) = match_transform_keys_block(receiver, &["map", "collect"], cx) else {
        return;
    };
    if !apply_guards(&m, cx) {
        return;
    }

    let message = format!("Prefer `transform_keys` over `{}`.", m.match_desc);
    cx.emit_offense(m.offense_range, &message, None);

    // Remove the `.to_h` suffix (receiver end → to_h end).
    let receiver_end = cx.range(receiver).end;
    let to_h_end = cx.range(to_h_node).end;
    cx.emit_edit(
        Range {
            start: receiver_end,
            end: to_h_end,
        },
        "",
    );

    emit_transform_keys_correction(&m, cx);
}

// ---------------------------------------------------------------------------
// Pattern 2: `Hash[receiver.map { |k, v| [transform(k), v] }]`
// ---------------------------------------------------------------------------

fn check_hash_brackets_map(brackets_node: NodeId, cx: &Cx<'_>) {
    // Receiver must be `Hash` constant (unqualified).
    let receiver = match cx.call_receiver(brackets_node).get() {
        Some(r) => r,
        None => return,
    };
    let NodeKind::Const { name, scope } = cx.kind(receiver) else {
        return;
    };
    if cx.symbol_str(*name) != "Hash" || scope.get().is_some() {
        return;
    }

    // Single Block argument.
    let args = cx.call_arguments(brackets_node);
    if args.len() != 1 {
        return;
    }
    let block_node = args[0];
    let NodeKind::Block { .. } = cx.kind(block_node) else {
        return;
    };

    let Some(m) = match_transform_keys_block(block_node, &["map", "collect"], cx) else {
        return;
    };
    if !apply_guards(&m, cx) {
        return;
    }

    let message = format!("Prefer `transform_keys` over `{}`.", "Hash[_.map {...}]");
    cx.emit_offense(m.offense_range, &message, None);

    // Strip `Hash[` prefix and `]` suffix.
    let outer_start = cx.range(brackets_node).start;
    let block_start = cx.range(block_node).start;
    let block_end = cx.range(block_node).end;
    let outer_end = cx.range(brackets_node).end;

    cx.emit_edit(Range { start: outer_start, end: block_start }, "");
    cx.emit_edit(Range { start: block_end, end: outer_end }, "");

    emit_transform_keys_correction(&m, cx);
}

// ---------------------------------------------------------------------------
// Pattern 3: `receiver.to_h { |k, v| [transform(k), v] }`
// ---------------------------------------------------------------------------

fn check_block_to_h(block_node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call, .. } = *cx.kind(block_node) else {
        return;
    };
    if cx.method_name(call) != Some("to_h") {
        return;
    }

    let Some(m) = match_transform_keys_block(block_node, &["to_h"], cx) else {
        return;
    };
    if !apply_guards(&m, cx) {
        return;
    }

    let message = format!("Prefer `transform_keys` over `{}`.", m.match_desc);
    cx.emit_offense(m.offense_range, &message, None);

    emit_transform_keys_correction(&m, cx);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::HashTransformKeys;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Pattern 1: map.to_h -----

    #[test]
    fn flags_map_to_h() {
        test::<HashTransformKeys>().expect_offense(indoc! {r#"
            {a: 1, b: 2}.map { |k, v| [k.to_s, v] }.to_h
                         ^^^ Prefer `transform_keys` over `map {...}.to_h`.
        "#});
    }

    #[test]
    fn flags_collect_to_h() {
        test::<HashTransformKeys>().expect_offense(indoc! {r#"
            {a: 1, b: 2}.collect { |k, v| [k.to_s, v] }.to_h
                         ^^^^^^^ Prefer `transform_keys` over `map {...}.to_h`.
        "#});
    }

    #[test]
    fn flags_map_to_h_with_send_receiver() {
        test::<HashTransformKeys>().expect_offense(indoc! {r#"
            foo.to_h.map { |k, v| [k.to_sym, v] }.to_h
                     ^^^ Prefer `transform_keys` over `map {...}.to_h`.
        "#});
    }

    #[test]
    fn flags_map_to_h_with_merge_receiver() {
        test::<HashTransformKeys>().expect_offense(indoc! {r#"
            foo.merge(bar).map { |k, v| [k.to_s, v] }.to_h
                           ^^^ Prefer `transform_keys` over `map {...}.to_h`.
        "#});
    }

    // ----- Pattern 2: Hash[map] -----

    #[test]
    fn flags_hash_brackets_map() {
        test::<HashTransformKeys>().expect_offense(indoc! {r#"
            Hash[{a: 1, b: 2}.map { |k, v| [k.to_s, v] }]
                              ^^^ Prefer `transform_keys` over `Hash[_.map {...}]`.
        "#});
    }

    #[test]
    fn flags_hash_brackets_collect() {
        test::<HashTransformKeys>().expect_offense(indoc! {r#"
            Hash[{a: 1, b: 2}.collect { |k, v| [foo(k), v] }]
                              ^^^^^^^ Prefer `transform_keys` over `Hash[_.map {...}]`.
        "#});
    }

    // ----- Pattern 3: to_h block -----

    #[test]
    fn flags_to_h_block() {
        test::<HashTransformKeys>().expect_offense(indoc! {r#"
            {a: 1, b: 2}.to_h { |k, v| [k.to_s, v] }
                         ^^^^ Prefer `transform_keys` over `to_h {...}`.
        "#});
    }

    #[test]
    fn flags_to_h_block_with_send_receiver() {
        test::<HashTransformKeys>().expect_offense(indoc! {r#"
            foo.to_h.to_h { |k, v| [k.to_sym, v] }
                     ^^^^ Prefer `transform_keys` over `to_h {...}`.
        "#});
    }

    // ----- Guard: noop key -----

    #[test]
    fn no_offense_noop_key() {
        test::<HashTransformKeys>().expect_no_offenses(indoc! {r#"
            {a: 1}.map { |k, v| [k, v] }.to_h
        "#});
    }

    // ----- Guard: key transform uses val -----

    #[test]
    fn no_offense_key_uses_val() {
        test::<HashTransformKeys>().expect_no_offenses(indoc! {r#"
            {a: 1}.map { |k, v| [v.to_s, v] }.to_h
        "#});
    }

    // ----- Guard: receiver not a known hash -----

    #[test]
    fn no_offense_unknown_receiver() {
        test::<HashTransformKeys>().expect_no_offenses(indoc! {r#"
            foo.bar.map { |k, v| [k.to_s, v] }.to_h
        "#});
    }

    #[test]
    fn no_offense_plain_variable_receiver() {
        test::<HashTransformKeys>().expect_no_offenses(indoc! {r#"
            baz.map { |k, v| [k.to_s, v] }.to_h
        "#});
    }

    // ----- Guard: wrong array shape -----

    #[test]
    fn no_offense_three_element_array() {
        test::<HashTransformKeys>().expect_no_offenses(indoc! {r#"
            {a: 1}.map { |k, v| [k.to_s, v, 1] }.to_h
        "#});
    }

    #[test]
    fn no_offense_value_not_passthrough() {
        test::<HashTransformKeys>().expect_no_offenses(indoc! {r#"
            {a: 1}.map { |k, v| [k.to_s, v.to_i] }.to_h
        "#});
    }

    // ----- Guard: single arg block -----

    #[test]
    fn no_offense_single_arg_block() {
        test::<HashTransformKeys>().expect_no_offenses(indoc! {r#"
            {a: 1}.map { |k| [k.to_s, 1] }.to_h
        "#});
    }

    // ----- Autocorrect: pattern 1 -----

    #[test]
    fn autocorrects_map_to_h() {
        test::<HashTransformKeys>().expect_correction(
            indoc! {r#"
                {a: 1, b: 2}.map { |k, v| [k.to_s, v] }.to_h
                             ^^^ Prefer `transform_keys` over `map {...}.to_h`.
            "#},
            "{a: 1, b: 2}.transform_keys { |k| k.to_s }\n",
        );
    }

    #[test]
    fn autocorrects_collect_to_h() {
        test::<HashTransformKeys>().expect_correction(
            indoc! {r#"
                {a: 1, b: 2}.collect { |k, v| [k.to_s, v] }.to_h
                             ^^^^^^^ Prefer `transform_keys` over `map {...}.to_h`.
            "#},
            "{a: 1, b: 2}.transform_keys { |k| k.to_s }\n",
        );
    }

    // ----- Autocorrect: pattern 2 -----

    #[test]
    fn autocorrects_hash_brackets_map() {
        test::<HashTransformKeys>().expect_correction(
            indoc! {r#"
                Hash[{a: 1, b: 2}.map { |k, v| [k.to_s, v] }]
                                  ^^^ Prefer `transform_keys` over `Hash[_.map {...}]`.
            "#},
            "{a: 1, b: 2}.transform_keys { |k| k.to_s }\n",
        );
    }

    // ----- Autocorrect: pattern 3 -----

    #[test]
    fn autocorrects_to_h_block() {
        test::<HashTransformKeys>().expect_correction(
            indoc! {r#"
                {a: 1, b: 2}.to_h { |k, v| [k.to_s, v] }
                             ^^^^ Prefer `transform_keys` over `to_h {...}`.
            "#},
            "{a: 1, b: 2}.transform_keys { |k| k.to_s }\n",
        );
    }
}

murphy_plugin_api::submit_cop!(HashTransformKeys);
