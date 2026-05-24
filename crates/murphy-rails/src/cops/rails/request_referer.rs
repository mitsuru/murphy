//! `Rails/RequestReferer` — flag the misspelled `request.referer`
//! accessor, recommending the HTTP-standard `request.referrer` spelling
//! instead. Rails exposes both as aliases, but the historical typo
//! survives in many codebases; this cop nudges projects toward the
//! canonical name.
//!
//! ## Matched shape (Send node)
//!
//! `Send(receiver=Send(receiver=None, method="request"), method="referer", args=[])`.
//!
//! - **method == `referer`** on the outer Send.
//! - **receiver is the bare `request` send** — i.e. a Send whose
//!   receiver is `OptNodeId::NONE` (the implicit-self `request` reader
//!   you get inside controllers / views) and whose method symbol is
//!   `request`. Anything else (a constant `Request`, an instance
//!   variable `@request`, another receiver `other.referer`) is left
//!   alone — only the controller/view `request.referer` form is the
//!   misspelling we care about.
//!
//! Method chains like `request.referer.present?` still hit: the
//! dispatcher visits every Send node, and the inner `request.referer`
//! Send (the chain's receiver) matches the gates above on its own —
//! independent of the outer `.present?` call.
//!
//! ## No autocorrect
//!
//! Mechanically rewriting `referer` → `referrer` is technically safe
//! (Rails aliases both names to the same value), but ADR 0006 still
//! requires a deliberate fix block per cop, and this v1 implementation
//! is detect-only. Tracked as a follow-up if dogfood demand surfaces.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RequestReferer;

#[cop(
    name = "Rails/RequestReferer",
    description = "Use `request.referrer` instead of `request.referer`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RequestReferer {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Defensive pattern-match: the dispatcher feeds us only Send
        // nodes today (`KINDS = [send]`), but the `let-else` is free
        // insurance against a future kind-aliasing accident. Same
        // posture as `Rails/Output`.
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(node)
        else {
            return;
        };
        // Gate 1: method must be exactly `referer`.
        if cx.symbol_str(method) != "referer" {
            return;
        }
        // Gate 2: receiver must be the implicit-self `request` send,
        // i.e. `Send(receiver=None, method="request", args=[])`. A bare
        // `referer` (no receiver), a constant `Request.referer`, an
        // instance variable `@request.referer`, or any other receiver
        // shape is intentionally ignored — this cop only targets the
        // controller/view `request.referer` accessor.
        if !receiver_is_bare_request(cx, receiver) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Use `request.referrer` instead of `request.referer`.",
            None,
        );
    }
}

/// `true` when `receiver` is the bare implicit-self `request` send —
/// `Send(receiver=None, method="request", args=[])`. Any other shape
/// (None, Const, IVar, another Send) returns `false`.
fn receiver_is_bare_request(cx: &Cx<'_>, receiver: OptNodeId) -> bool {
    let Some(rid) = receiver.get() else {
        return false;
    };
    let NodeKind::Send {
        receiver: inner_recv,
        method: inner_method,
        args: inner_args,
    } = *cx.kind(rid)
    else {
        return false;
    };
    // Three gates on the inner Send: bare call (no receiver), method name
    // exactly `request`, and **zero arguments**. The last gate rules out
    // `request(foo).referer` where `request` is a user-defined helper /
    // local method taking arguments — that is not the Rails controller
    // accessor we want to flag (roborev review feedback on murphy-9v0).
    inner_recv == OptNodeId::NONE
        && cx.symbol_str(inner_method) == "request"
        && cx.list(inner_args).is_empty()
}

#[cfg(test)]
mod tests {
    use super::RequestReferer;
    use murphy_plugin_api::test_support::{expect_no_offenses, expect_offense, indoc};

    // === hit cases ===

    #[test]
    fn flags_request_referer() {
        expect_offense!(
            RequestReferer,
            indoc! {r#"
                request.referer
                ^^^^^^^^^^^^^^^ Use `request.referrer` instead of `request.referer`.
            "#}
        );
    }

    #[test]
    fn flags_request_referer_in_conditional() {
        // Two hits — one on each `request.referer` Send. The
        // dispatcher visits every Send node, so both fire
        // independently.
        expect_offense!(
            RequestReferer,
            indoc! {r#"
                if request.referer.present?
                   ^^^^^^^^^^^^^^^ Use `request.referrer` instead of `request.referer`.
                  redirect_to request.referer
                              ^^^^^^^^^^^^^^^ Use `request.referrer` instead of `request.referer`.
                end
            "#}
        );
    }

    #[test]
    fn flags_request_referer_chained() {
        // `request.referer.present?` — the inner Send `request.referer`
        // is the outer Send's receiver and still hits on its own.
        expect_offense!(
            RequestReferer,
            indoc! {r#"
                request.referer.present?
                ^^^^^^^^^^^^^^^ Use `request.referrer` instead of `request.referer`.
            "#}
        );
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_request_referrer() {
        // Correct spelling — leave alone.
        expect_no_offenses!(RequestReferer, "request.referrer\n");
    }

    #[test]
    fn does_not_flag_other_receiver_referer() {
        // Different receiver — not the controller `request` accessor.
        expect_no_offenses!(RequestReferer, "other.referer\n");
    }

    #[test]
    fn does_not_flag_bare_referer() {
        // Receiver-less call; not `request.referer`.
        expect_no_offenses!(RequestReferer, "referer\n");
    }

    #[test]
    fn does_not_flag_const_request_receiver() {
        // `Request` is a Const, not a `Send(None, "request")`; the
        // controller `request.referer` we care about is the implicit-
        // self method call, not a top-level constant accessor.
        expect_no_offenses!(RequestReferer, "Request.referer\n");
    }

    #[test]
    fn does_not_flag_ivar_request_receiver() {
        // `@request` is an IVar read, not the `request` Send.
        expect_no_offenses!(RequestReferer, "@request.referer\n");
    }

    #[test]
    fn does_not_flag_unrelated_method_on_request() {
        // `request.path` is a different accessor entirely.
        expect_no_offenses!(RequestReferer, "request.path\n");
    }

    #[test]
    fn does_not_flag_request_with_args() {
        // User-defined helper `request(foo)` returning an object with
        // `referer` accessor is **not** the Rails controller `request`.
        // roborev review (job 1122) found that without the zero-args
        // gate the cop would false-positive here.
        expect_no_offenses!(RequestReferer, "request(foo).referer\n");
    }

    #[test]
    fn does_not_flag_request_with_block() {
        // `request { ... }` is also a method call with structure beyond
        // the bare accessor — block-arg form should not be flagged.
        expect_no_offenses!(RequestReferer, "request(:get) { |r| r.referer }\n");
    }
}
