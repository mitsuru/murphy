//! `Style/MapToSet` — prefer `to_set` with a block over `map.to_set` /
//! `collect.to_set`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MapToSet
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Marked unsafe in RuboCop (Safe: false) because the receiver may not be
//!   an Enumerable. Murphy does not have a Safe/SafeAutoCorrect cop-level
//!   attribute yet; the unsafe nature is documented here only.
//!
//!   Handled patterns (mirrors RuboCop's node matcher):
//!     1. Block form:      `something.map { |i| ... }.to_set`
//!     2. Block-pass form: `something.map(&:method).to_set`
//!   Both `map` and `collect` are detected.
//!   Both `send` and `csend` are handled for the outer `to_set` call.
//!   Both `Send` and `Csend` are accepted for the inner map/collect call.
//!
//!   Guard: if `to_set` already has a block attached (its parent is a Block
//!   whose `call` is the `to_set` node), the offense is suppressed — matches
//!   RuboCop's `return if to_set_node.block_literal?`.
//!
//!   Offense range: the map/collect selector only (loc.name), not the full
//!   chain — mirrors RuboCop's `add_offense(map_node.loc.selector, ...)`.
//!
//!   Autocorrect:
//!     - Removes the `.to_set` suffix (from the end of the receiver block/send
//!       to the end of the to_set node).
//!     - Renames `map`/`collect` selector to `to_set` (loc.name surgical edit).
//!   No dot transfer — RuboCop also does not transfer the dot for MapToSet.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! something.map { |i| i * 2 }.to_set
//! [1, 2, 3].collect { |i| i.to_s }.to_set
//! something.map(&:method).to_set
//!
//! # good
//! something.to_set { |i| i * 2 }
//! [1, 2, 3].to_set { |i| i.to_s }
//! something.to_set(&:method)
//! ```
//!
//! ## Autocorrect
//!
//! Two surgical edits:
//! 1. Delete `.to_set` suffix (from receiver end to to_set node end).
//! 2. Rename `map`/`collect` selector to `to_set`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Pass a block to `to_set` instead of calling `%method%.to_set`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct MapToSet;

#[cop(
    name = "Style/MapToSet",
    description = "Prefer `to_set` with a block over `map.to_set` or `collect.to_set`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl MapToSet {
    #[on_node(kind = "send", methods = ["to_set"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) == Some("to_set") {
            check(node, cx);
        }
    }
}

/// Returns the map/collect send node if the pattern matches, otherwise None.
///
/// Two forms:
/// 1. Block form: `receiver = Block { call: map_send, ... }`
/// 2. Block-pass form: `receiver = Send/Csend { method: map/collect, args: [BlockPass(Sym)] }`
fn match_map_to_set(to_set_node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let receiver_id = cx.call_receiver(to_set_node).get()?;

    match *cx.kind(receiver_id) {
        // Form 1: `something.map { ... }.to_set`
        NodeKind::Block { call, .. } => {
            let method = cx.method_name(call)?;
            if matches!(method, "map" | "collect") {
                Some(call)
            } else {
                None
            }
        }
        // Form 2: `something.map(&:method).to_set`
        NodeKind::Send { method, args, .. } | NodeKind::Csend { method, args, .. } => {
            let name = cx.symbol_str(method);
            if !matches!(name, "map" | "collect") {
                return None;
            }
            // Must have exactly one argument that is a BlockPass wrapping a Sym.
            let arg_list = cx.list(args);
            if arg_list.len() != 1 {
                return None;
            }
            let arg = arg_list[0];
            if let NodeKind::BlockPass(inner) = *cx.kind(arg) {
                if inner
                    .get()
                    .map(|n| matches!(cx.kind(n), NodeKind::Sym(_)))
                    .unwrap_or(false)
                {
                    return Some(receiver_id);
                }
            }
            None
        }
        _ => None,
    }
}

fn check(to_set_node: NodeId, cx: &Cx<'_>) {
    // Guard: skip if to_set already has a block attached.
    // In Murphy's AST, when `to_set { }` has a block, the parent is a Block
    // node whose `call` field == to_set_node.
    if let Some(parent) = cx.parent(to_set_node).get() {
        if let NodeKind::Block { call, .. } = *cx.kind(parent) {
            if call == to_set_node {
                return;
            }
        }
    }

    let Some(map_node) = match_map_to_set(to_set_node, cx) else {
        return;
    };

    let map_selector = cx.node(map_node).loc.name;
    let method_name = cx.method_name(map_node).unwrap_or("map");
    let message = MSG.replace("%method%", method_name);

    cx.emit_offense(map_selector, &message, None);

    // Autocorrect:
    // Edit 1: remove `.to_set` suffix — from receiver end to to_set node end.
    let receiver_id = cx.call_receiver(to_set_node).get().unwrap();
    let receiver_end = cx.range(receiver_id).end;
    let to_set_end = cx.range(to_set_node).end;
    let removal_range = Range {
        start: receiver_end,
        end: to_set_end,
    };
    cx.emit_edit(removal_range, "");

    // Edit 2: rename the map/collect selector to `to_set`.
    cx.emit_edit(map_selector, "to_set");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::MapToSet;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Block form: flagged cases -----

    #[test]
    fn flags_map_block_to_set() {
        test::<MapToSet>().expect_offense(indoc! {r#"
            something.map { |i| i * 2 }.to_set
                      ^^^ Pass a block to `to_set` instead of calling `map.to_set`.
        "#});
    }

    #[test]
    fn flags_collect_block_to_set() {
        test::<MapToSet>().expect_offense(indoc! {r#"
            [1, 2, 3].collect { |i| i.to_s }.to_set
                      ^^^^^^^ Pass a block to `to_set` instead of calling `collect.to_set`.
        "#});
    }

    // ----- Block-pass form: flagged cases -----

    #[test]
    fn flags_map_block_pass_to_set() {
        test::<MapToSet>().expect_offense(indoc! {r#"
            something.map(&:method).to_set
                      ^^^ Pass a block to `to_set` instead of calling `map.to_set`.
        "#});
    }

    // ----- Guard: to_set already has a block -----

    #[test]
    fn accepts_to_set_with_block() {
        test::<MapToSet>().expect_no_offenses("something.to_set { |i| i * 2 }\n");
    }

    #[test]
    fn accepts_map_block_to_set_with_block() {
        // `something.map { }.to_set { }` — to_set has its own block, skip
        test::<MapToSet>().expect_no_offenses("something.map { |i| i }.to_set { |i| i }\n");
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_map_without_to_set() {
        test::<MapToSet>().expect_no_offenses("something.map { |i| i * 2 }\n");
    }

    #[test]
    fn accepts_to_set_without_map() {
        test::<MapToSet>().expect_no_offenses("[1, 2, 3].to_set\n");
    }

    #[test]
    fn accepts_flat_map_to_set() {
        test::<MapToSet>().expect_no_offenses("something.flat_map { |i| i }.to_set\n");
    }

    // ----- csend outer -----

    #[test]
    fn flags_csend_outer_to_set() {
        test::<MapToSet>().expect_offense(indoc! {r#"
            something.map { |i| i * 2 }&.to_set
                      ^^^ Pass a block to `to_set` instead of calling `map.to_set`.
        "#});
    }

    // ----- Autocorrect -----

    #[test]
    fn autocorrects_map_block_to_set() {
        test::<MapToSet>().expect_correction(
            indoc! {r#"
                something.map { |i| i * 2 }.to_set
                          ^^^ Pass a block to `to_set` instead of calling `map.to_set`.
            "#},
            "something.to_set { |i| i * 2 }\n",
        );
    }

    #[test]
    fn autocorrects_collect_block_to_set() {
        test::<MapToSet>().expect_correction(
            indoc! {r#"
                [1, 2, 3].collect { |i| i.to_s }.to_set
                          ^^^^^^^ Pass a block to `to_set` instead of calling `collect.to_set`.
            "#},
            "[1, 2, 3].to_set { |i| i.to_s }\n",
        );
    }

    #[test]
    fn autocorrects_map_block_pass_to_set() {
        test::<MapToSet>().expect_correction(
            indoc! {r#"
                something.map(&:method).to_set
                          ^^^ Pass a block to `to_set` instead of calling `map.to_set`.
            "#},
            "something.to_set(&:method)\n",
        );
    }
}
murphy_plugin_api::submit_cop!(MapToSet);
