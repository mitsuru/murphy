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
//! ## Implementation
//!
//! The two-level Send check is expressed declaratively with
//! [`node_pattern!`] (RuboCop NodePattern grammar). `nil?` in DSL means
//! "receiver is `None`" (no AST node), distinct from `nil` which would
//! be the Ruby `nil` literal. The pattern omits a trailing argument
//! placeholder on each Send so both have to take **zero** arguments —
//! that excludes shapes like `request(foo).referer` where `request` is
//! a user-defined helper (a fix originally driven by murphy-9v0
//! roborev review job 1122; retained here by the DSL's strict-arity
//! semantics).
//!
//! ## No autocorrect
//!
//! Mechanically rewriting `referer` → `referrer` is technically safe
//! (Rails aliases both names to the same value), but ADR 0006 still
//! requires a deliberate fix block per cop, and this v1 implementation
//! is detect-only. Tracked as a follow-up if dogfood demand surfaces.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, node_pattern};

// RuboCop NodePattern equivalent: `(send (send nil? :request) :referer)`.
// See module docs for the `nil?` vs `nil` distinction and why zero-arg
// strictness is load-bearing.
node_pattern!(is_request_referer, "(send (send nil? :request) :referer)");

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
        if !is_request_referer(node, cx) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Use `request.referrer` instead of `request.referer`.",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::RequestReferer;
    use murphy_plugin_api::test_support::{indoc, test};

    // === hit cases ===

    #[test]
    fn flags_request_referer() {
        test::<RequestReferer>().expect_offense(indoc! {r#"
                request.referer
                ^^^^^^^^^^^^^^^ Use `request.referrer` instead of `request.referer`.
            "#});
    }

    #[test]
    fn flags_request_referer_in_conditional() {
        // Two hits — one on each `request.referer` Send. The
        // dispatcher visits every Send node, so both fire
        // independently.
        test::<RequestReferer>().expect_offense(indoc! {r#"
                if request.referer.present?
                   ^^^^^^^^^^^^^^^ Use `request.referrer` instead of `request.referer`.
                  redirect_to request.referer
                              ^^^^^^^^^^^^^^^ Use `request.referrer` instead of `request.referer`.
                end
            "#});
    }

    #[test]
    fn flags_request_referer_chained() {
        // `request.referer.present?` — the inner Send `request.referer`
        // is the outer Send's receiver and still hits on its own.
        test::<RequestReferer>().expect_offense(indoc! {r#"
                request.referer.present?
                ^^^^^^^^^^^^^^^ Use `request.referrer` instead of `request.referer`.
            "#});
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_request_referrer() {
        // Correct spelling — leave alone.
        test::<RequestReferer>().expect_no_offenses("request.referrer\n");
    }

    #[test]
    fn does_not_flag_other_receiver_referer() {
        // Different receiver — not the controller `request` accessor.
        test::<RequestReferer>().expect_no_offenses("other.referer\n");
    }

    #[test]
    fn does_not_flag_bare_referer() {
        // Receiver-less call; not `request.referer`.
        test::<RequestReferer>().expect_no_offenses("referer\n");
    }

    #[test]
    fn does_not_flag_const_request_receiver() {
        // `Request` is a Const, not a `Send(None, "request")`; the
        // controller `request.referer` we care about is the implicit-
        // self method call, not a top-level constant accessor.
        test::<RequestReferer>().expect_no_offenses("Request.referer\n");
    }

    #[test]
    fn does_not_flag_ivar_request_receiver() {
        // `@request` is an IVar read, not the `request` Send.
        test::<RequestReferer>().expect_no_offenses("@request.referer\n");
    }

    #[test]
    fn does_not_flag_unrelated_method_on_request() {
        // `request.path` is a different accessor entirely.
        test::<RequestReferer>().expect_no_offenses("request.path\n");
    }

    #[test]
    fn does_not_flag_request_with_args() {
        // User-defined helper `request(foo)` returning an object with
        // `referer` accessor is **not** the Rails controller `request`.
        // roborev review (job 1122) found that without the zero-args
        // gate the cop would false-positive here; the DSL's strict
        // arity preserves the guarantee.
        test::<RequestReferer>().expect_no_offenses("request(foo).referer\n");
    }

    #[test]
    fn does_not_flag_request_with_block() {
        // `request { ... }` is also a method call with structure beyond
        // the bare accessor — block-arg form should not be flagged.
        test::<RequestReferer>().expect_no_offenses("request(:get) { |r| r.referer }\n");
    }
}
