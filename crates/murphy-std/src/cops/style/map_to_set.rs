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
//!   Verbatim port of the call head `(call _ :to_set ...)`
//!   (murphy-s1yc.20): `call` = `{send csend}` covers safe-navigation
//!   (`something.map { }&.to_set`), mirroring RuboCop
//!   `alias on_csend on_send` plus `RESTRICT_ON_SEND [to_set]`; the wildcard
//!   receiver binds an absent or present receiver per murphy-if9y;
//!   trailing `...` absorbs any argument list. The receiver plus inner
//!   `map`/`collect` shape guards below apply separately (upstream outer is
//!   `(call ... :to_set)` in `map_to_set?`).
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

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop, def_node_matcher};

// Verbatim port of the call head (murphy-s1yc.20):
// `(call _ :to_set ...)` — `call` = `{send csend}` covers safe-navigation
// (`something.map { }&.to_set`), mirroring RuboCop `alias on_csend on_send`
// plus `RESTRICT_ON_SEND [to_set]`. The `_` receiver binds an absent or
// present receiver per murphy-if9y; trailing `...` absorbs any argument
// list, so the receiver plus inner-`map` shape guards below apply
// separately (upstream outer is `(call ... :to_set)` in `map_to_set?`).
def_node_matcher!(map_to_set_call, "(call _ :to_set ...)");

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
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
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
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {
            let name = cx.method_name(receiver_id)?;
            if !matches!(name, "map" | "collect") {
                return None;
            }
            // Must have exactly one argument that is a BlockPass wrapping a Sym.
            let arg_list = cx.call_arguments(receiver_id);
            if arg_list.len() != 1 {
                return None;
            }
            let arg = arg_list[0];
            if let NodeKind::BlockPass(inner) = *cx.kind(arg)
                && inner
                    .get()
                    .map(|n| matches!(cx.kind(n), NodeKind::Sym(_)))
                    .unwrap_or(false)
                {
                    return Some(receiver_id);
                }
            None
        }
        _ => None,
    }
}

fn check(to_set_node: NodeId, cx: &Cx<'_>) {
    // Verbatim `(call _ :to_set ...)` head: filters to `to_set` calls on
    // either send or csend (safe-navigation), with any receiver
    // (absent or present). Without this, an unrelated call on a map
    // receiver (e.g. `something.map(&:foo).to_s`) would run check
    // on every call node instead of being rejected by the method set
    // up front.
    if !map_to_set_call(to_set_node, cx) {
        return;
    }
    // Must have a receiver. Mirrors the pre-port hand-rolled guard;
    // `_` binds an absent receiver, so bare `to_set` is accepted here.
    // Guard: skip if to_set already has a block attached.
    // In Murphy's AST, when `to_set { }` has a block, the parent is a Block
    // node whose `call` field == to_set_node.
    if let Some(parent) = cx.parent(to_set_node).get()
        && let NodeKind::Block { call, .. } = *cx.kind(parent)
            && call == to_set_node {
                return;
            }

    let Some(map_node) = match_map_to_set(to_set_node, cx) else {
        return;
    };

    let map_selector = cx.node(map_node).loc.name;
    let method_name = cx.method_name(map_node).unwrap_or("map");
    let message = format!("Pass a block to `to_set` instead of calling `{method_name}.to_set`.");

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

    // --- Characterization (murphy-s1yc.20): pin the exact node set the
    // dual send (methods = ["to_set"]) + manual-csend-filter dispatch matches,
    // so the verbatim `(call _ :to_set ...)` port can be proven byte-identical.
    // `call` covers safe-navigation (mirroring upstream `alias on_csend
    // on_send` plus `RESTRICT_ON_SEND [to_set]`); trailing `...` absorbs any
    // argument list, so the receiver plus inner-`map` shape guards below
    // apply separately.

    #[test]
    fn s1yc20_flags_csend_block_pass_corrects() {
        // Safe navigation on outer `to_set`: `call` covers `csend` per
        // murphy-if9y, mirroring upstream `alias on_csend on_send`.
        // (Pre-port the `csend` handler filters `to_set` manually because
        // `methods = [...]` is only valid for `kind = "send"`; the verbatim
        // head collapses the workaround.)
        test::<MapToSet>().expect_correction(
            indoc! {r#"
                something.map(&:foo)&.to_set
                          ^^^ Pass a block to `to_set` instead of calling `map.to_set`.
            "#},
            "something.to_set(&:foo)\n",
        );
    }

    #[test]
    fn s1yc20_accepts_bare_to_set() {
        // Bare receiver: `_` binds an absent receiver per murphy-if9y, so
        // the head matches and the complementary receiver guard accepts.
        test::<MapToSet>().expect_no_offenses("to_set\n");
    }

    #[test]
    fn s1yc20_accepts_no_arg_inner_map() {
        // Inner `map` without arguments: trailing `...` absorbs the outer
        // argument list so the head matches, and the complementary
        // one-`block_pass`-arg guard accepts (upstream inner requires
        // exactly `(block_pass sym)`).
        test::<MapToSet>().expect_no_offenses("something.map.to_set\n");
    }

    #[test]
    fn s1yc20_accepts_unrelated_method() {
        // `to_s` is outside the verbatim method set, so the head rejects.
        test::<MapToSet>().expect_no_offenses("something.map(&:foo).to_s\n");
    }
}
murphy_plugin_api::submit_cop!(MapToSet);
