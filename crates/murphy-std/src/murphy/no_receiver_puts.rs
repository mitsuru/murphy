//! `Murphy/NoReceiverPuts` — flags `puts` / `print` / `p` calls with no
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

use murphy_plugin_api::{
    Cop, Cx, NoOptions, NodeCop, NodeId, NodeKind, NodeKindTag, OptNodeId, Severity,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct NoReceiverPuts;

impl Cop for NoReceiverPuts {
    type Options = NoOptions;
    const NAME: &'static str = "Murphy/NoReceiverPuts";
    const DESCRIPTION: &'static str =
        "Flag receiver-less `puts`/`print`/`p` calls; use a logger instead.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    const DEFAULT_ENABLED: Option<bool> = Some(true);
}

/// `NodeKind::Send` tag — declaration order is frozen by ADR 0037; see
/// `murphy_ast::KIND_PATTERN_NAMES` where `"send"` is bound to `17`.
const SEND_TAG: NodeKindTag = NodeKindTag(17);

impl NodeCop for NoReceiverPuts {
    const KINDS: &'static [NodeKindTag] = &[SEND_TAG];

    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        // Pattern-match defends against the dispatcher feeding us a non-Send
        // node (it should not — `KINDS = [Send]` — but the `let-else` is
        // free, and a future kind aliasing accident here would silently
        // misreport without it).
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(node)
        else {
            return;
        };
        // Gate 1 (ADR 0001): an explicit receiver is intentional output.
        if receiver != OptNodeId::NONE {
            return;
        }
        // Gate 2: only the three bare-output method names.
        if !matches!(cx.symbol_str(method), "puts" | "print" | "p") {
            return;
        }
        cx.emit_offense(cx.range(node), "Use a logger instead of puts", None);
    }
}
