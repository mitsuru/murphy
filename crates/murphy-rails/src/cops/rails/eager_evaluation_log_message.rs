//! `Rails/EagerEvaluationLogMessage` — flag interpolated strings passed to `Rails.logger.debug`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/EagerEvaluationLogMessage
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:debug] gating,
//!   `(send (send (const {cbase nil?} :Rails) :logger) :debug $(dstr ...))`
//!   shape with `Rails.logger` zero-arg gate and top-level `Rails`
//!   (`::Rails` folds via `const_name`). Skips when the send already has
//!   a block (`node.parent&.block_type?` via `block_node`). Offense and
//!   replacement cover selector-end to node-end (parenthesized) or
//!   selector-end+1 to node-end (bare), rewriting to ` { arg }` /
//!   `{ arg }` block form.
//! ```
//!
//! ## Matched shapes
//!
//! - `Rails.logger.debug "The time is #{Time.zone.now}."` → `Rails.logger.debug { "The time is #{Time.zone.now}." }`.
//! - `Rails.logger.debug("The time is #{x}.")` → `Rails.logger.debug { "The time is #{x}." }`.
//!
//! `Rails.logger.debug "plain"` (no interpolation), `Rails.logger.debug { "..." }`
//! (already a block), and `Rails.logger.info "x#{y}"` do not flag.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Pass a block to `Rails.logger.debug`.";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EagerEvaluationLogMessage;

#[cop(
    name = "Rails/EagerEvaluationLogMessage",
    description = "Checks that blocks are used for interpolated strings passed to `Rails.logger.debug`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl EagerEvaluationLogMessage {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[debug]`.
    #[on_node(kind = "send", methods = ["debug"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Skip `Rails.logger.debug { "..." }` — send already has a block.
    // Upstream 2.35: `return if node.parent&.block_type?`.
    if cx.block_node(node).get().is_some() {
        return;
    }
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return;
    };
    if cx.symbol_str(method) != "debug" {
        return;
    }
    let Some(recv) = receiver.get() else {
        return;
    };
    // `(send (const {cbase nil?} :Rails) :logger)` — zero-arg `Rails.logger`.
    let NodeKind::Send {
        receiver: logger_recv,
        method: logger_method,
        ..
    } = *cx.kind(recv)
    else {
        return;
    };
    if cx.symbol_str(logger_method) != "logger" {
        return;
    }
    if !cx.call_arguments(recv).is_empty() {
        return;
    }
    let Some(rails_id) = logger_recv.get() else {
        return;
    };
    if cx.const_name(rails_id).as_deref() != Some("Rails") {
        return;
    }
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }
    let arg = args[0];
    if !matches!(*cx.kind(arg), NodeKind::Dstr(_)) {
        return;
    }
    let arg_src = cx.raw_source(cx.range(arg)).to_owned();
    let sel = cx.selector(node);
    let whole = cx.range(node);
    let parenthesized = cx.is_parenthesized(node);
    let start = if parenthesized {
        sel.end
    } else {
        sel.end.saturating_add(1)
    };
    if start > whole.end {
        return;
    }
    let range = Range {
        start,
        end: whole.end,
    };
    let replacement = if parenthesized {
        format!(" {{ {arg_src} }}")
    } else {
        format!("{{ {arg_src} }}")
    };
    cx.emit_offense(range, MSG, None);
    cx.emit_edit(range, &replacement);
}

#[cfg(test)]
mod tests {
    use super::EagerEvaluationLogMessage;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_interpolated_bare() {
        test::<EagerEvaluationLogMessage>().expect_offense(indoc! {r#"
            Rails.logger.debug "The time is #{Time.zone.now}."
                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Pass a block to `Rails.logger.debug`.
        "#});
    }

    #[test]
    fn flags_interpolated_parens() {
        test::<EagerEvaluationLogMessage>().expect_offense(indoc! {r#"
            Rails.logger.debug("The time is #{x}.")
                              ^^^^^^^^^^^^^^^^^^^^^ Pass a block to `Rails.logger.debug`.
        "#});
    }

    #[test]
    fn does_not_flag_plain_string() {
        test::<EagerEvaluationLogMessage>()
            .expect_no_offenses("Rails.logger.debug \"plain\"\n");
    }

    #[test]
    fn does_not_flag_block_form() {
        test::<EagerEvaluationLogMessage>()
            .expect_no_offenses("Rails.logger.debug { \"The time is #{x}.\" }\n");
    }

    #[test]
    fn does_not_flag_info() {
        test::<EagerEvaluationLogMessage>()
            .expect_no_offenses("Rails.logger.info \"The time is #{x}.\"\n");
    }

    #[test]
    fn does_not_flag_bare_logger() {
        test::<EagerEvaluationLogMessage>()
            .expect_no_offenses("logger.debug \"The time is #{x}.\"\n");
    }

    #[test]
    fn does_not_flag_cbase_mismatch() {
        test::<EagerEvaluationLogMessage>()
            .expect_no_offenses("Foo::Rails.logger.debug \"The time is #{x}.\"\n");
    }

    #[test]
    fn flags_cbase_rails() {
        test::<EagerEvaluationLogMessage>().expect_offense(indoc! {r#"
            ::Rails.logger.debug "The time is #{x}."
                                 ^^^^^^^^^^^^^^^^^^^ Pass a block to `Rails.logger.debug`.
        "#});
    }

    #[test]
    fn corrects_bare_to_block() {
        test::<EagerEvaluationLogMessage>()
            .expect_correction(
                indoc! {r#"
                    Rails.logger.debug "The time is #{x}."
                                       ^^^^^^^^^^^^^^^^^^^ Pass a block to `Rails.logger.debug`.
                "#},
                "Rails.logger.debug { \"The time is #{x}.\" }\n",
            )
            .expect_no_offenses("Rails.logger.debug { \"The time is #{x}.\" }\n");
    }

    #[test]
    fn corrects_parens_to_block() {
        test::<EagerEvaluationLogMessage>()
            .expect_correction(
                indoc! {r#"
                    Rails.logger.debug("The time is #{x}.")
                                      ^^^^^^^^^^^^^^^^^^^^^ Pass a block to `Rails.logger.debug`.
                "#},
                "Rails.logger.debug { \"The time is #{x}.\" }\n",
            )
            .expect_no_offenses("Rails.logger.debug { \"The time is #{x}.\" }\n");
    }
}
murphy_plugin_api::submit_cop!(EagerEvaluationLogMessage);
