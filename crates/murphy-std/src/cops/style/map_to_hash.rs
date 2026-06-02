//! `Style/MapToHash` — prefer `to_h` with a block over `map.to_h` /
//! `collect.to_h`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MapToHash
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Marked unsafe in RuboCop (Safe: false) because the receiver may not be
//!   an Enumerable. Murphy does not have a Safe/SafeAutoCorrect cop-level
//!   attribute yet; the unsafe nature is documented here only.
//!
//!   Requires Ruby >= 2.6 (upstream minimum_target_ruby_version 2.6). Murphy
//!   does not gate on TargetRubyVersion yet; this is documented here only.
//!
//!   Handled patterns (mirrors RuboCop's node matcher):
//!     1. Block form:      `something.map { |v| [v, v * 2] }.to_h`
//!     2. Block-pass form: `something.map(&:foo).to_h`
//!   Both `map` and `collect` are detected.
//!   Both `send` and `csend` are handled for the outer `to_h` call.
//!   Both `Send` and `Csend` are accepted for the inner map/collect call.
//!
//!   Guard: if `to_h` already has a block attached (its parent is a Block
//!   whose `call` is the `to_h` node), the offense is suppressed.
//!
//!   Offense range: the map/collect selector only (loc.name).
//!
//!   Message includes the dot source (`.` or `&.`) so for csend-outer the
//!   message reads `map&.to_h`, matching RuboCop's `%<dot>s` format.
//!
//!   Autocorrect:
//!     - Removes the `<dot>to_h` suffix (from receiver end to to_h node end).
//!     - If the inner map call has a dot (`.` or `&.`), replaces it with the
//!       outer to_h dot source (transfers dot ownership, matching RuboCop).
//!     - Renames `map`/`collect` selector to `to_h`.
//!
//!   Destructuring (`|(k, v)|` block params): Murphy's AST represents the
//!   inner destructured param as `NodeKind::Unknown` (Prism
//!   `RequiredDestructuredParameterNode` is not yet fully mapped). The
//!   paren-stripping that RuboCop performs (`|(k, v)| → |k, v|`) is therefore
//!   skipped. `to_h { |(k, v)| ... }` is valid, idempotent Ruby; this is a
//!   minor formatting gap only.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! something.map { |v| [v, v * 2] }.to_h
//! {foo: bar}.collect { |k, v| [k.to_s, v] }.to_h
//! something.map(&:foo).to_h
//!
//! # good
//! something.to_h { |v| [v, v * 2] }
//! {foo: bar}.to_h { |k, v| [k.to_s, v] }
//! something.to_h(&:foo)
//! ```
//!
//! ## Autocorrect
//!
//! Two or three surgical edits:
//! 1. Delete `<dot>to_h` suffix (from receiver end to to_h node end).
//! 2. (If map has a dot) Replace that dot with the to_h dot source.
//! 3. Rename `map`/`collect` selector to `to_h`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct MapToHash;

#[cop(
    name = "Style/MapToHash",
    description = "Prefer `to_h` with a block over `map.to_h` or `collect.to_h`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl MapToHash {
    #[on_node(kind = "send", methods = ["to_h"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) == Some("to_h") {
            check(node, cx);
        }
    }
}

/// Returns the map/collect send node if the pattern matches, otherwise None.
///
/// Two forms:
/// 1. Block form: `receiver = Block { call: map_send, ... }`
/// 2. Block-pass form: `receiver = Send/Csend { method: map/collect, args: [BlockPass(Sym)] }`
fn match_map_to_hash(to_h_node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let receiver_id = cx.call_receiver(to_h_node).get()?;

    match *cx.kind(receiver_id) {
        // Form 1: `something.map { ... }.to_h`
        NodeKind::Block { call, .. } => {
            let method = cx.method_name(call)?;
            if matches!(method, "map" | "collect") {
                Some(call)
            } else {
                None
            }
        }
        // Form 2: `something.map(&:foo).to_h`
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

fn check(to_h_node: NodeId, cx: &Cx<'_>) {
    // Guard: skip if to_h already has a block attached.
    if let Some(parent) = cx.parent(to_h_node).get() {
        if let NodeKind::Block { call, .. } = *cx.kind(parent) {
            if call == to_h_node {
                return;
            }
        }
    }

    let Some(map_node) = match_map_to_hash(to_h_node, cx) else {
        return;
    };

    let map_selector = cx.node(map_node).loc.name;
    let method_name = cx.method_name(map_node).unwrap_or("map");

    // Get the to_h dot source for the message (`.` or `&.`).
    let to_h_dot_src = cx
        .call_operator_loc(to_h_node)
        .map(|r| cx.raw_source(r))
        .unwrap_or(".");

    let message = format!(
        "Pass a block to `to_h` instead of calling `{method_name}{to_h_dot_src}to_h`."
    );

    cx.emit_offense(map_selector, &message, None);

    // Autocorrect:
    // Edit 1: remove `<dot>to_h` suffix — from receiver end to to_h node end.
    let receiver_id = cx.call_receiver(to_h_node).get().unwrap();
    let receiver_end = cx.range(receiver_id).end;
    let to_h_end = cx.range(to_h_node).end;
    let removal_range = Range {
        start: receiver_end,
        end: to_h_end,
    };
    cx.emit_edit(removal_range, "");

    // Edit 2 (optional): if the inner map call has an explicit dot, replace
    // it with the outer to_h dot source. This transfers dot ownership so that
    // `x&.map { }.to_h` becomes `x&.to_h { }` and `x.map { }&.to_h` becomes
    // `x&.to_h { }`.
    if let Some(map_dot_range) = cx.call_operator_loc(map_node) {
        if let Some(to_h_dot_range) = cx.call_operator_loc(to_h_node) {
            let to_h_dot = cx.raw_source(to_h_dot_range);
            cx.emit_edit(map_dot_range, to_h_dot);
        }
    }

    // Edit 3: rename the map/collect selector to `to_h`.
    cx.emit_edit(map_selector, "to_h");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::MapToHash;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Block form: flagged cases -----

    #[test]
    fn flags_map_block_to_h() {
        test::<MapToHash>().expect_offense(indoc! {r#"
            something.map { |v| [v, v * 2] }.to_h
                      ^^^ Pass a block to `to_h` instead of calling `map.to_h`.
        "#});
    }

    #[test]
    fn flags_collect_block_to_h() {
        test::<MapToHash>().expect_offense(indoc! {r#"
            {foo: 1}.collect { |k, v| [k.to_s, v] }.to_h
                     ^^^^^^^ Pass a block to `to_h` instead of calling `collect.to_h`.
        "#});
    }

    // ----- Block-pass form: flagged cases -----

    #[test]
    fn flags_map_block_pass_to_h() {
        test::<MapToHash>().expect_offense(indoc! {r#"
            something.map(&:foo).to_h
                      ^^^ Pass a block to `to_h` instead of calling `map.to_h`.
        "#});
    }

    // ----- Guard: to_h already has a block -----

    #[test]
    fn accepts_to_h_with_block() {
        test::<MapToHash>().expect_no_offenses("something.to_h { |v| [v, v] }\n");
    }

    #[test]
    fn accepts_map_block_to_h_with_block() {
        test::<MapToHash>().expect_no_offenses("something.map { |v| v }.to_h { |v| v }\n");
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_map_without_to_h() {
        test::<MapToHash>().expect_no_offenses("something.map { |v| [v, v * 2] }\n");
    }

    #[test]
    fn accepts_to_h_without_map() {
        test::<MapToHash>().expect_no_offenses("[[:a, 1]].to_h\n");
    }

    #[test]
    fn accepts_flat_map_to_h() {
        test::<MapToHash>().expect_no_offenses("something.flat_map { |v| v }.to_h\n");
    }

    // ----- csend outer: message uses &. -----

    #[test]
    fn flags_csend_outer_to_h_message() {
        test::<MapToHash>().expect_offense(indoc! {r#"
            something.map { |v| [v, v] }&.to_h
                      ^^^ Pass a block to `to_h` instead of calling `map&.to_h`.
        "#});
    }

    // ----- Autocorrect -----

    #[test]
    fn autocorrects_map_block_to_h() {
        test::<MapToHash>().expect_correction(
            indoc! {r#"
                something.map { |v| [v, v * 2] }.to_h
                          ^^^ Pass a block to `to_h` instead of calling `map.to_h`.
            "#},
            "something.to_h { |v| [v, v * 2] }\n",
        );
    }

    #[test]
    fn autocorrects_collect_block_to_h() {
        test::<MapToHash>().expect_correction(
            indoc! {r#"
                {foo: 1}.collect { |k, v| [k.to_s, v] }.to_h
                         ^^^^^^^ Pass a block to `to_h` instead of calling `collect.to_h`.
            "#},
            "{foo: 1}.to_h { |k, v| [k.to_s, v] }\n",
        );
    }

    #[test]
    fn autocorrects_map_block_pass_to_h() {
        test::<MapToHash>().expect_correction(
            indoc! {r#"
                something.map(&:foo).to_h
                          ^^^ Pass a block to `to_h` instead of calling `map.to_h`.
            "#},
            "something.to_h(&:foo)\n",
        );
    }

    #[test]
    fn autocorrects_csend_outer_transfers_dot() {
        // `something.map { }&.to_h` → `something&.to_h { }` — dot is transferred.
        test::<MapToHash>().expect_correction(
            indoc! {r#"
                something.map { |v| [v, v] }&.to_h
                          ^^^ Pass a block to `to_h` instead of calling `map&.to_h`.
            "#},
            "something&.to_h { |v| [v, v] }\n",
        );
    }
}
murphy_plugin_api::submit_cop!(MapToHash);
