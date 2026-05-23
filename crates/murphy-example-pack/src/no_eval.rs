//! `Example/NoEval` — flags `eval` calls. Demo cop for the
//! `murphy-example-pack` distribution.
//!
//! Matches the following receiver shapes:
//! - `eval(...)`            — bare call (no receiver)
//! - `Kernel.eval(...)`     — top-level `Kernel` constant via `Const { scope: None, name: "Kernel" }`
//! - `Kernel::eval(...)`    — `::` followed by a lowercase method name is a
//!   method call (same `Send` shape as `Kernel.eval(...)`); the receiver
//!   is still `Const { scope: None, name: "Kernel" }`.
//! - `self.eval(...)`       — explicit `SelfExpr` receiver.
//!
//! Other receivers (`obj.eval(...)`, `Foo::Bar.eval(...)`, etc.) are
//! intentionally skipped — they are typically domain methods named
//! `eval` (e.g. ActiveRecord finders) and not the dynamic-code-execution
//! built-in this cop targets.
//!
//! No autocorrect — mechanically replacing `eval` is unsafe.

use murphy_plugin_api::{
    Cop, Cx, NoOptions, NodeCop, NodeId, NodeKind, NodeKindTag, OptNodeId, Severity,
};

/// `NodeKind::Send` tag — declaration order is frozen by ADR 0037; see
/// `murphy_ast::NodeKind::tag` where `Send { .. }` is `17`.
const SEND_TAG: NodeKindTag = NodeKindTag(17);

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct NoEval;

impl Cop for NoEval {
    type Options = NoOptions;
    const NAME: &'static str = "Example/NoEval";
    const DESCRIPTION: &'static str = "Flag `eval` calls — dynamic code execution is dangerous.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    const DEFAULT_ENABLED: Option<bool> = Some(true);
}

impl NodeCop for NoEval {
    const KINDS: &'static [NodeKindTag] = &[SEND_TAG];

    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        // Pattern-match defends against a future kind-aliasing accident
        // (see `Murphy/NoReceiverPuts` for the same pattern).
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(node)
        else {
            return;
        };
        if cx.symbol_str(method) != "eval" {
            return;
        }
        if !receiver_is_eval_target(cx, receiver) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "eval is dangerous — avoid dynamic code execution",
            None,
        );
    }
}

/// `true` if the receiver is one we treat as the dangerous built-in
/// `eval` — `nil` (bare call), `Kernel` (top-level constant), or `self`.
fn receiver_is_eval_target(cx: &Cx<'_>, receiver: OptNodeId) -> bool {
    let Some(rid) = receiver.get() else {
        return true; // bare eval(...)
    };
    match *cx.kind(rid) {
        // `Foo` / `Bar::Baz` both encode as `Const { scope, name }`. For
        // demo purposes we treat receiver = top-level `Kernel`
        // (`scope == None`) as the matched form. `Bar::Kernel` (scope =
        // Some(...)) is intentionally skipped — it is a domain `Kernel`.
        NodeKind::Const { scope, name } => {
            scope == OptNodeId::NONE && cx.symbol_str(name) == "Kernel"
        }
        NodeKind::SelfExpr => true,
        _ => false,
    }
}
