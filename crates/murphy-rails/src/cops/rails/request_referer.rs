//! `Rails/RequestReferer` — enforce one spelling for the Rails
//! `request.referer` / `request.referrer` accessors. Rails exposes both
//! as aliases; RuboCop defaults to the HTTP-standard misspelling
//! `referer`, but lets projects opt into `referrer`.
//!
//! ## Matched shape (Send node)
//!
//! `Send(receiver=Send(receiver=None, method="request"), method=<configured spelling>, args=[])`.
//!
//! - **method == the spelling opposite the configured style** on the
//!   outer Send.
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
//! ## Autocorrect
//!
//! Mechanically rewriting between `referer` and `referrer` is safe
//! because Rails aliases both names to the same value. Matching RuboCop,
//! the correction replaces the whole send node with `request.<style>`,
//! normalising odd whitespace around the dot.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, cop, node_pattern};

// RuboCop NodePattern equivalent: `(send (send nil? :request) :referer)`.
// See module docs for the `nil?` vs `nil` distinction and why zero-arg
// strictness is load-bearing.
node_pattern!(is_request_referer, "(send (send nil? :request) :referer)");
node_pattern!(is_request_referrer, "(send (send nil? :request) :referrer)");

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RequestReferer;

#[derive(CopOptions)]
pub struct RequestRefererOptions {
    #[option(
        name = "EnforcedStyle",
        default = "referer",
        description = "Which request accessor spelling to enforce."
    )]
    pub enforced_style: RequestRefererStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum RequestRefererStyle {
    #[option(value = "referer")]
    Referer,
    #[option(value = "referrer")]
    Referrer,
}

#[cop(
    name = "Rails/RequestReferer",
    description = "Enforce a consistent request referer accessor spelling.",
    default_severity = "warning",
    default_enabled = true,
    options = RequestRefererOptions,
)]
impl RequestReferer {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<RequestRefererOptions>();
        let Some((preferred, actual)) = offending_spelling(node, cx, opts.enforced_style) else {
            return;
        };
        cx.emit_offense(
            cx.range(node),
            &format!("Use `request.{preferred}` instead of `request.{actual}`."),
            None,
        );
        cx.emit_edit(cx.range(node), &format!("request.{preferred}"));
    }
}

fn offending_spelling(
    node: NodeId,
    cx: &Cx<'_>,
    style: RequestRefererStyle,
) -> Option<(&'static str, &'static str)> {
    match style {
        RequestRefererStyle::Referer if is_request_referrer(node, cx) => {
            Some(("referer", "referrer"))
        }
        RequestRefererStyle::Referrer if is_request_referer(node, cx) => {
            Some(("referrer", "referer"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{RequestReferer, RequestRefererOptions, RequestRefererStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn referrer_style() -> RequestRefererOptions {
        RequestRefererOptions {
            enforced_style: RequestRefererStyle::Referrer,
        }
    }

    // === hit cases ===

    #[test]
    fn flags_request_referrer_by_default() {
        test::<RequestReferer>().expect_offense(indoc! {r#"
                request.referrer
                ^^^^^^^^^^^^^^^^ Use `request.referer` instead of `request.referrer`.
            "#});
    }

    #[test]
    fn corrects_request_referrer_by_default() {
        test::<RequestReferer>()
            .expect_correction(
                indoc! {r#"
                    request.referrer
                    ^^^^^^^^^^^^^^^^ Use `request.referer` instead of `request.referrer`.
                "#},
                "request.referer\n",
            )
            .expect_no_offenses("request.referer\n");
    }

    #[test]
    fn corrects_whitespace_around_dot_like_rubocop() {
        test::<RequestReferer>().expect_correction(
            indoc! {r#"
                request . referrer
                ^^^^^^^^^^^^^^^^^^ Use `request.referer` instead of `request.referrer`.
            "#},
            "request.referer\n",
        );
    }

    #[test]
    fn flags_request_referrer_in_conditional_by_default() {
        // Two hits — one on each `request.referrer` Send. The
        // dispatcher visits every Send node, so both fire
        // independently.
        test::<RequestReferer>().expect_offense(indoc! {r#"
                if request.referrer.present?
                   ^^^^^^^^^^^^^^^^ Use `request.referer` instead of `request.referrer`.
                  redirect_to request.referrer
                              ^^^^^^^^^^^^^^^^ Use `request.referer` instead of `request.referrer`.
                end
            "#});
    }

    #[test]
    fn flags_request_referrer_chained_by_default() {
        // `request.referrer.present?` — the inner Send `request.referrer`
        // is the outer Send's receiver and still hits on its own.
        test::<RequestReferer>().expect_offense(indoc! {r#"
                request.referrer.present?
                ^^^^^^^^^^^^^^^^ Use `request.referer` instead of `request.referrer`.
            "#});
    }

    #[test]
    fn referrer_style_flags_request_referer() {
        test::<RequestReferer>()
            .with_options(&referrer_style())
            .expect_offense(indoc! {r#"
                request.referer
                ^^^^^^^^^^^^^^^ Use `request.referrer` instead of `request.referer`.
            "#});
    }

    #[test]
    fn referrer_style_corrects_request_referer() {
        test::<RequestReferer>()
            .with_options(&referrer_style())
            .expect_correction(
                indoc! {r#"
                    request.referer
                    ^^^^^^^^^^^^^^^ Use `request.referrer` instead of `request.referer`.
                "#},
                "request.referrer\n",
            )
            .expect_no_offenses("request.referrer\n");
    }

    #[test]
    fn referrer_style_does_not_flag_request_referrer() {
        test::<RequestReferer>()
            .with_options(&referrer_style())
            .expect_no_offenses("request.referrer\n");
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_request_referer_by_default() {
        // Default EnforcedStyle is RuboCop's `referer`.
        test::<RequestReferer>().expect_no_offenses("request.referer\n");
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
