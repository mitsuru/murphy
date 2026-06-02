//! `Style/MapCompactWithConditionalBlock` — prefer `select`/`reject` over
//! `map { ... }.compact` or `filter_map { ... }` with a conditional block
//! that simply filters elements.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MapCompactWithConditionalBlock
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `map { |e| ... }.compact` and `filter_map { |e| ... }` where
//!   the block conditionally returns the block argument or skips via `next`/`nil`.
//!
//!   Covered patterns (all map to `select` or `reject`):
//!     - `map { |e| cond ? e : next }.compact`
//!     - `map { |e| e if cond }.compact`
//!     - `map { |e| e unless cond }.compact`
//!     - `map { |e| if cond; e; else; next; end }.compact`
//!     - `map { |e| if cond; next; else; e; end }.compact` (reject)
//!     - `map { |e| next e if cond }.compact`
//!     - `map { |e| next if cond; e }.compact` (reject guard form)
//!     - `filter_map` variants of all the above
//!
//!   Gaps:
//!     - `elsif` chains — skipped (same as RuboCop).
//!     - Numblock (`map { _1 }`) and itblock (`map { it }`) — not handled,
//!       since the pattern requires a named block argument to verify the
//!       returned value matches the parameter.
//!
//!   Autocorrect:
//!     Replaces the entire offense range with
//!     `select { |param| condition }` or `reject { |param| condition }`.
//!     This is whole-node replacement because the AST is fundamentally
//!     rearranged (block body replaced with just the condition expression).
//!     Autocorrect is marked unsafe: behavior may differ if the receiver
//!     does not respond to `select`/`reject`.
//! ```
//!
//! ## Matched shapes (select)
//!
//! ```ruby
//! # bad
//! array.map { |e| some_condition? ? e : next }.compact
//! array.map { |e| e if some_condition? }.compact
//! array.filter_map { |e| some_condition? ? e : next }
//!
//! # good
//! array.select { |e| some_condition? }
//! ```
//!
//! ## Matched shapes (reject)
//!
//! ```ruby
//! # bad
//! array.map { |e| next if some_condition?; e }.compact
//! array.map { |e| e unless some_condition? }.compact
//!
//! # good
//! array.reject { |e| some_condition? }
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct MapCompactWithConditionalBlock;

const MSG: &str = "Replace `%s` with `%s`.";

#[cop(
    name = "Style/MapCompactWithConditionalBlock",
    description = "Prefer `select` or `reject` over `map { ... }.compact` or `filter_map { ... }` with a conditional block.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
    safe_autocorrect = false,
)]
impl MapCompactWithConditionalBlock {
    /// Triggered on `compact` (handles `map { ... }.compact`) and `filter_map`.
    #[on_node(kind = "send", methods = ["compact", "filter_map"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    /// Also handle csend (e.g. `array&.map { }.compact`).
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { method, .. } = *cx.kind(node) else {
            return;
        };
        if matches!(cx.symbol_str(method), "compact" | "filter_map") {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method_name = cx.method_name(node).unwrap_or("");

    let (block_node, offense_range, current_label) = if method_name == "compact" {
        // `map { ... }.compact` — receiver of compact is the block node.
        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };
        // Must not have arguments on compact.
        if !cx.call_arguments(node).is_empty() {
            return;
        }
        // The receiver must be a map/filter_map block.
        if !is_map_block(receiver, cx) {
            return;
        }
        // Offense range: from map/filter_map selector through end of .compact call.
        let map_call = block_inner_call(receiver, cx);
        let offense_start = cx.selector(map_call).start;
        let offense_end = cx.range(node).end;
        let map_method_name = cx.method_name(map_call).unwrap_or("map");
        let label = format!("{} {{ ... }}.compact", map_method_name);
        (receiver, Range { start: offense_start, end: offense_end }, label)
    } else {
        // `filter_map { ... }` — node is the filter_map send.
        // The block wrapping it is the parent.
        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        if !is_map_block(parent, cx) {
            return;
        }
        // Ensure this is specifically filter_map.
        let map_call = block_inner_call(parent, cx);
        if cx.method_name(map_call) != Some("filter_map") {
            return;
        }
        let offense_start = cx.selector(node).start;
        let offense_end = cx.range(parent).end;
        let label = "filter_map { ... }".to_string();
        (parent, Range { start: offense_start, end: offense_end }, label)
    };

    // Analyze the block body for a conditional pattern.
    let Some((is_select, arg_name, cond_source)) = analyze_block(block_node, cx) else {
        return;
    };

    let method = if is_select { "select" } else { "reject" };
    let msg = MSG
        .replacen("%s", &current_label, 1)
        .replacen("%s", method, 1);

    cx.emit_offense(offense_range, &msg, None);

    // Autocorrect: whole-node replacement (AST is rearranged).
    let replacement = format!("{} {{ |{}| {} }}", method, arg_name, cond_source);
    cx.emit_edit(offense_range, &replacement);
}

/// Returns `true` if `node` is a `Block` (not Numblock/Itblock) wrapping a
/// `map` or `filter_map` send, with exactly one named `Arg` parameter.
fn is_map_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Block { call, args, .. } = *cx.kind(node) else {
        return false;
    };
    if !matches!(cx.method_name(call), Some("map" | "filter_map")) {
        return false;
    }
    // Args must have exactly one `Arg` child (named parameter, not splat etc.).
    let args_list = block_args_list(args, cx);
    args_list.len() == 1 && matches!(cx.kind(args_list[0]), NodeKind::Arg(_))
}

/// Returns the inner call node from a Block.
fn block_inner_call(block_node: NodeId, cx: &Cx<'_>) -> NodeId {
    match *cx.kind(block_node) {
        NodeKind::Block { call, .. } => call,
        _ => block_node,
    }
}

/// Returns the list of argument nodes from an `Args` node.
fn block_args_list<'a>(args: NodeId, cx: &Cx<'a>) -> &'a [NodeId] {
    match *cx.kind(args) {
        NodeKind::Args(list) => cx.list(list),
        _ => &[],
    }
}

/// Analyzes the block body to determine if it matches a conditional pattern.
/// Returns `Some((is_select, arg_name, cond_source))` if offensive, `None` otherwise.
fn analyze_block(block_node: NodeId, cx: &Cx<'_>) -> Option<(bool, String, String)> {
    let NodeKind::Block { args, body, .. } = *cx.kind(block_node) else {
        return None;
    };
    let body = body.get()?;

    // Extract the single block arg name.
    let args_list = block_args_list(args, cx);
    let NodeKind::Arg(arg_sym) = *cx.kind(args_list[0]) else {
        return None;
    };
    let arg_name = cx.symbol_str(arg_sym).to_string();

    match *cx.kind(body) {
        // Shape A/B/C/D: direct `if`/`unless` node.
        NodeKind::If { cond, then_, else_ } => {
            // Skip elsif chains.
            if cx.is_elsif(body) {
                return None;
            }
            let cond_source = cx.raw_source(cx.range(cond)).to_string();
            analyze_if_shape(cond_source, then_, else_, &arg_name, cx)
        }
        // Shape E/F: `begin` with a guard `if` then an `lvar` (or trailing nil).
        NodeKind::Begin(list) => {
            let stmts = cx.list(list);
            if stmts.len() != 2 {
                return None;
            }
            analyze_begin_shape(stmts[0], stmts[1], &arg_name, cx)
        }
        _ => None,
    }
}

/// Analyze direct `if` node shapes.
fn analyze_if_shape(
    cond_source: String,
    then_: OptNodeId,
    else_: OptNodeId,
    arg_name: &str,
    cx: &Cx<'_>,
) -> Option<(bool, String, String)> {
    let then_node = then_.get();
    let else_node = else_.get();

    // Shape A: `if cond; lvar(arg); else; next/nil; end` → select
    if let Some(t) = then_node
        && is_lvar_of(t, arg_name, cx)
        && is_next_or_nil(else_node, cx)
    {
        return Some((true, arg_name.to_string(), cond_source));
    }

    // Shape B: `if cond; next/nil; else; lvar(arg); end` → reject
    // This covers both `if cond { nil } else { lvar }` and
    // `unless cond { lvar }` (Murphy swaps then/else for unless).
    if let Some(e) = else_node
        && is_lvar_of(e, arg_name, cx)
        && is_next_or_nil(then_node, cx)
    {
        return Some((false, arg_name.to_string(), cond_source));
    }

    // Shape C: `if cond; next(lvar); else; next/nil; end` → select
    if let Some(t) = then_node
        && let Some(inner) = extract_next_lvar(t, cx)
        && is_lvar_of(inner, arg_name, cx)
        && is_next_or_nil(else_node, cx)
    {
        return Some((true, arg_name.to_string(), cond_source));
    }

    // Shape D: `if cond; next/nil; else; next(lvar); end` → reject
    if let Some(e) = else_node
        && let Some(inner) = extract_next_lvar(e, cx)
        && is_lvar_of(inner, arg_name, cx)
        && is_next_or_nil(then_node, cx)
    {
        return Some((false, arg_name.to_string(), cond_source));
    }

    None
}

/// Analyze `begin` shapes (guard form, two statements).
fn analyze_begin_shape(
    first: NodeId,
    second: NodeId,
    arg_name: &str,
    cx: &Cx<'_>,
) -> Option<(bool, String, String)> {
    // Shape E: `if(cond, next, nil)` then `lvar(arg)` — guard form.
    if is_lvar_of(second, arg_name, cx)
        && let NodeKind::If { cond, then_, else_ } = *cx.kind(first)
        && !cx.is_elsif(first)
    {
        let cond_source = cx.raw_source(cx.range(cond)).to_string();
        // E1: `if cond; next; end; lvar` → reject
        if is_next_no_arg(then_.get(), cx) && is_nil_or_absent(else_.get(), cx) {
            return Some((false, arg_name.to_string(), cond_source));
        }
        // E2: `unless cond; next; end; lvar` = `if cond; nil; else; next; end; lvar` → select
        if is_nil_or_absent(then_.get(), cx) && is_next_no_arg(else_.get(), cx) {
            return Some((true, arg_name.to_string(), cond_source));
        }
    }

    // Shape F: `if(cond, next(lvar), nil)` then `nil` — `next e if cond` form.
    if matches!(cx.kind(second), NodeKind::Nil)
        && let NodeKind::If { cond, then_, else_ } = *cx.kind(first)
        && !cx.is_elsif(first)
    {
            let cond_source = cx.raw_source(cx.range(cond)).to_string();
            // F1: `if cond; next(lvar); end` then nil → select
            if let Some(t) = then_.get()
                && let Some(inner) = extract_next_lvar(t, cx)
                && is_lvar_of(inner, arg_name, cx)
                && is_nil_or_absent(else_.get(), cx)
            {
                return Some((true, arg_name.to_string(), cond_source));
            }
            // F2: `unless cond; next(lvar); end` then nil → reject
            if let Some(e) = else_.get()
                && let Some(inner) = extract_next_lvar(e, cx)
                && is_lvar_of(inner, arg_name, cx)
                && is_nil_or_absent(then_.get(), cx)
            {
                return Some((false, arg_name.to_string(), cond_source));
            }
    }

    None
}

// ---------------------------------------------------------------------------
// Predicate helpers
// ---------------------------------------------------------------------------

/// Returns `true` if `node` is `lvar(arg_name)`.
fn is_lvar_of(node: NodeId, arg_name: &str, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Lvar(sym) => cx.symbol_str(sym) == arg_name,
        _ => false,
    }
}

/// Returns `true` if the optional node is absent, `nil`, bare `next`, or `next nil`.
fn is_next_or_nil(node: Option<NodeId>, cx: &Cx<'_>) -> bool {
    match node {
        None => true,
        Some(n) => match *cx.kind(n) {
            NodeKind::Nil => true,
            NodeKind::Next(inner) => match inner.get() {
                None => true,
                Some(v) => matches!(cx.kind(v), NodeKind::Nil),
            },
            _ => false,
        },
    }
}

/// Returns `true` if the optional node is absent or `nil`.
fn is_nil_or_absent(node: Option<NodeId>, cx: &Cx<'_>) -> bool {
    match node {
        None => true,
        Some(n) => matches!(cx.kind(n), NodeKind::Nil),
    }
}

/// Returns `true` if `node` is a bare `next` (no argument) or `next nil`.
fn is_next_no_arg(node: Option<NodeId>, cx: &Cx<'_>) -> bool {
    let Some(n) = node else { return false; };
    match *cx.kind(n) {
        NodeKind::Next(inner) => match inner.get() {
            None => true,
            Some(v) => matches!(cx.kind(v), NodeKind::Nil),
        },
        _ => false,
    }
}

/// If `node` is `next(lvar(x))`, returns `Some(lvar_node)`. Else `None`.
fn extract_next_lvar(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Next(inner) = *cx.kind(node) else {
        return None;
    };
    let inner_node = inner.get()?;
    if matches!(cx.kind(inner_node), NodeKind::Lvar(_)) {
        Some(inner_node)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::MapCompactWithConditionalBlock;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- map { |e| cond ? e : next }.compact → select ---

    #[test]
    fn flags_map_ternary_compact() {
        test::<MapCompactWithConditionalBlock>().expect_offense(indoc! {"
            array.map { |e| some_condition? ? e : next }.compact
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `map { ... }.compact` with `select`.
        "});
    }

    #[test]
    fn corrects_map_ternary_compact() {
        test::<MapCompactWithConditionalBlock>().expect_correction(
            indoc! {"
                array.map { |e| some_condition? ? e : next }.compact
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `map { ... }.compact` with `select`.
            "},
            "array.select { |e| some_condition? }\n",
        );
    }

    // --- map { |e| e if cond }.compact → select ---

    #[test]
    fn flags_map_modifier_if_compact() {
        test::<MapCompactWithConditionalBlock>().expect_offense(indoc! {"
            array.map { |e| e if some_condition? }.compact
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `map { ... }.compact` with `select`.
        "});
    }

    #[test]
    fn corrects_map_modifier_if_compact() {
        test::<MapCompactWithConditionalBlock>().expect_correction(
            indoc! {"
                array.map { |e| e if some_condition? }.compact
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `map { ... }.compact` with `select`.
            "},
            "array.select { |e| some_condition? }\n",
        );
    }

    // --- map { |e| e unless cond }.compact → reject ---

    #[test]
    fn flags_map_modifier_unless_compact() {
        test::<MapCompactWithConditionalBlock>().expect_offense(indoc! {"
            array.map { |e| e unless some_condition? }.compact
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `map { ... }.compact` with `reject`.
        "});
    }

    #[test]
    fn corrects_map_modifier_unless_compact() {
        test::<MapCompactWithConditionalBlock>().expect_correction(
            indoc! {"
                array.map { |e| e unless some_condition? }.compact
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `map { ... }.compact` with `reject`.
            "},
            "array.reject { |e| some_condition? }\n",
        );
    }

    // --- filter_map { |e| cond ? e : next } → select ---

    #[test]
    fn flags_filter_map_ternary() {
        test::<MapCompactWithConditionalBlock>().expect_offense(indoc! {"
            array.filter_map { |e| some_condition? ? e : next }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `filter_map { ... }` with `select`.
        "});
    }

    #[test]
    fn corrects_filter_map_ternary() {
        test::<MapCompactWithConditionalBlock>().expect_correction(
            indoc! {"
                array.filter_map { |e| some_condition? ? e : next }
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `filter_map { ... }` with `select`.
            "},
            "array.select { |e| some_condition? }\n",
        );
    }

    // --- map { |e| if cond; next; else; e; end }.compact → reject ---

    #[test]
    fn flags_map_if_next_else_lvar_compact() {
        test::<MapCompactWithConditionalBlock>().expect_offense(indoc! {"
            array.map { |e| if cond; next; else; e; end }.compact
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `map { ... }.compact` with `reject`.
        "});
    }

    #[test]
    fn corrects_map_if_next_else_lvar_compact() {
        test::<MapCompactWithConditionalBlock>().expect_correction(
            indoc! {"
                array.map { |e| if cond; next; else; e; end }.compact
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `map { ... }.compact` with `reject`.
            "},
            "array.reject { |e| cond }\n",
        );
    }

    // --- map { |e| next if cond; e }.compact → reject (guard form) ---

    #[test]
    fn flags_map_guard_next_compact() {
        test::<MapCompactWithConditionalBlock>().expect_offense(indoc! {"
            array.map { |e| next if some_condition?; e }.compact
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `map { ... }.compact` with `reject`.
        "});
    }

    #[test]
    fn corrects_map_guard_next_compact() {
        test::<MapCompactWithConditionalBlock>().expect_correction(
            indoc! {"
                array.map { |e| next if some_condition?; e }.compact
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Replace `map { ... }.compact` with `reject`.
            "},
            "array.reject { |e| some_condition? }\n",
        );
    }

    // --- no offense cases ---

    #[test]
    fn accepts_map_with_transformation() {
        test::<MapCompactWithConditionalBlock>()
            .expect_no_offenses("array.map { |e| e * 2 }.compact\n");
    }

    #[test]
    fn accepts_compact_without_map() {
        test::<MapCompactWithConditionalBlock>().expect_no_offenses("array.compact\n");
    }

    #[test]
    fn accepts_filter_map_with_transformation() {
        test::<MapCompactWithConditionalBlock>()
            .expect_no_offenses("array.filter_map { |e| e * 2 }\n");
    }

    #[test]
    fn accepts_map_compact_with_two_params() {
        test::<MapCompactWithConditionalBlock>()
            .expect_no_offenses("array.map { |k, v| v if k }.compact\n");
    }

    #[test]
    fn accepts_filter_map_different_lvar() {
        test::<MapCompactWithConditionalBlock>()
            .expect_no_offenses("array.filter_map { |e| x if some_condition? }\n");
    }
}

murphy_plugin_api::submit_cop!(MapCompactWithConditionalBlock);
