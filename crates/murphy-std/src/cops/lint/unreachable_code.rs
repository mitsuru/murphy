//! `Lint/UnreachableCode` — flags sibling statements that follow a
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnreachableCode
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-py6a
//! notes: >
//!   Known gaps remain around RuboCop terminator coverage, branch flow, and message parity.
//! ```
//!
//! flow-terminator inside the *same* `Begin(NodeList)` container.
//!
//! A statement is unreachable when its **direct** sibling earlier in the
//! same `Begin` body is one of:
//!
//! - `Return(_)` — `return …`
//! - `Break(_)` — `break …`
//! - `Next(_)`  — `next …`
//! - `Send { receiver: None, method: "raise", … }` — receiver-less
//!   `raise …`. `raise` is a Kernel method in Ruby, so in the arena AST
//!   it appears as a `Send` rather than a dedicated node kind.
//!
//! "Direct sibling" is load-bearing: a `return` *inside* an `if` /
//! `case` / nested `Begin` does not make code after the surrounding
//! construct unreachable, because the terminator does not always fire.
//! Walking only `Begin`'s direct children gets this for free — the cop
//! never recurses into nested containers.
//!
//! No autocorrect: deleting unreachable code is not always the user's
//! intent (the dead branch might be a stub waiting for implementation),
//! and the cop is `warning` severity so the lint run does not fail on
//! it. This matches RuboCop's `Lint/UnreachableCode` policy.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct UnreachableCode;

#[cop(
    name = "Lint/UnreachableCode",
    description = "Flag statements following a terminator (return / break / next / raise) in the same begin block.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UnreachableCode {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Begin(_) = *cx.kind(node) else {
            return;
        };
        let children = cx.children(node);
        let mut terminated = false;
        for child in children {
            if terminated {
                // Emit one offense per dead sibling so the user sees
                // every statement that won't run. RuboCop reports the
                // first only; we follow the more informative shape.
                cx.emit_offense(
                    cx.range(child),
                    "Unreachable code (preceding statement always exits)",
                    None,
                );
                continue;
            }
            if is_terminator(cx.kind(child), cx) {
                terminated = true;
            }
        }
    }
}

/// True when `kind` is a control-flow exit that prevents the next
/// sibling from running. `raise` is detected via [`NodeKind::Send`]
/// because Ruby's `raise` is a Kernel method, not a syntactic form.
fn is_terminator(kind: &NodeKind, cx: &Cx<'_>) -> bool {
    match kind {
        NodeKind::Return(_) | NodeKind::Break(_) | NodeKind::Next(_) => true,
        NodeKind::Send {
            receiver, method, ..
        } => *receiver == OptNodeId::NONE && cx.symbol_str(*method) == "raise",
        _ => false,
    }
}
