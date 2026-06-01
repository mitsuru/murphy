//! `Murphy/NoReceiverPuts` — flags `puts` / `print` / `p` calls with no
//! ## Murphy catalog
//!
//! ```murphy-parity
//! cop: Murphy/NoReceiverPuts
//! status: custom
//! notes: >
//!   Murphy-specific bootstrap cop; no RuboCop upstream target.
//! ```
//!
//! explicit receiver. Ruby's bare debug-output methods almost always
//! belong on a logger; an explicit receiver (`logger.info "x"`,
//! `obj.puts "x"`) is fine and is filtered out by Gate 1 below.
//!
//! The offense `range` is the full `Send` node range. The pre-arena
//! implementation used prism's `message_loc()` (just the `puts` selector
//! token); the arena AST does not retain a selector-only sub-range and
//! re-deriving one would require source scanning. The selector-only
//! narrowing was not part of the offense JSON contract (ADR 0006 — only
//! `cop_name` / `message` / `range` / `severity` are frozen) and the
//! coarser `Send` range is unambiguously the same offense site, so this
//! is the intended trade-off.
//!
//! Authored against `murphy-plugin-api` only (single-surface ABI, ADR
//! 0038): both the AST types and `register_cops!` are reached through
//! the plugin-api re-export surface.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct NoReceiverPuts;

#[cop(
    name = "Murphy/NoReceiverPuts",
    description = "Flag receiver-less `puts`/`print`/`p` calls; use a logger instead.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl NoReceiverPuts {
    #[on_node(kind = "send", methods = ["puts", "print", "p"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Pattern-match defends against the dispatcher feeding us a non-Send
        // node (it should not — `KINDS = [Send]` — but the `let-else` is
        // free, and a future kind aliasing accident here would silently
        // misreport without it).
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        // Gate 1 (ADR 0001): an explicit receiver is intentional output.
        if receiver != OptNodeId::NONE {
            return;
        }
        cx.emit_offense(cx.range(node), "Use a logger instead of puts", None);
    }
}
murphy_plugin_api::submit_cop!(NoReceiverPuts);
