//! `Rails/FreezeTime` — prefer `freeze_time` over `travel_to` with current time.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/FreezeTime
//! upstream_version_checked: 2.35.0
//! version_added: "2.16"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:travel_to] gating, the
//!   first-argument current-time shape (`Time.now/new/current`,
//!   `DateTime.now`, `Time.zone.now`, `::`-prefixed forms, plus
//!   `.to_time`/`.in_time_zone` conversions with zero-arg chains), the
//!   zero-arg inner gate (e.g. `DateTime.new(2022, 5, 3)` does not flag),
//!   whole-send offense range (send-only when a block is present), and
//!   block-pass-preserving autocorrect (`&example` is kept). `minimum_target_rails_version 5.2`
//!   is not gated: Murphy fires regardless of the configured Rails version.
//!   Autocorrect is unsafe upstream (`SafeAutoCorrect: false`).
//! ```
//!
//! Identifies usages of `travel_to` with an argument of the current time and
//! changes them to use `freeze_time` instead.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct FreezeTime;

#[cop(
    name = "Rails/FreezeTime",
    description = "Prefer `freeze_time` over `travel_to` with an argument of the current time.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl FreezeTime {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[travel_to]`.
    #[on_node(kind = "send", methods = ["travel_to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

const NOW_METHODS: &[&str] = &["now", "new", "current"];
const CONVERT_METHODS: &[&str] = &["to_time", "in_time_zone"];

fn check(node: NodeId, cx: &Cx<'_>) {
    // Upstream `node.first_argument` — no offense without an argument.
    let args = cx.call_arguments(node);
    let Some(&first) = args.first() else {
        return;
    };
    // First argument must be a bare `Send` (upstream destructures
    // `*first_argument.children` into receiver/method/extra-arg).
    let NodeKind::Send {
        receiver: inner_recv,
        method: inner_method,
        args: inner_args,
    } = *cx.kind(first)
    else {
        return;
    };
    // Inner call must have zero arguments (`return if time_argument`).
    if !cx.list(inner_args).is_empty() {
        return;
    }
    let Some(inner_recv) = inner_recv.get() else {
        // e.g. `travel_to(current)` — no receiver on the inner call.
        return;
    };
    let method_name = cx.symbol_str(inner_method).to_owned();
    let is_current = current_time(cx, inner_recv, &method_name)
        || current_time_with_convert(cx, inner_recv, &method_name);
    if !is_current {
        return;
    }
    let range = send_only_range(cx, node);
    cx.emit_offense(range, "Use `freeze_time` instead of `travel_to`.", None);
    // Upstream preserves a trailing block-pass (`&example`).
    let replacement = match args.last() {
        Some(&last) if matches!(*cx.kind(last), NodeKind::BlockPass(_)) => {
            format!("freeze_time({})", cx.raw_source(cx.range(last)))
        }
        _ => "freeze_time".to_owned(),
    };
    cx.emit_edit(range, &replacement);
}

/// Upstream `current_time?`: method in NOW_METHODS and the receiver is
/// either a `Time.zone` send or a `Time`/`DateTime` const.
fn current_time(cx: &Cx<'_>, recv: NodeId, method: &str) -> bool {
    if !NOW_METHODS.contains(&method) {
        return false;
    }
    if matches!(*cx.kind(recv), NodeKind::Send { .. }) {
        return is_zoned_time_now(cx, recv);
    }
    is_time_const(cx, recv)
}

/// Upstream `zoned_time_now?`: `(send (const {nil? cbase} :Time) :zone)`.
fn is_zoned_time_now(cx: &Cx<'_>, node: NodeId) -> bool {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return false;
    };
    if cx.symbol_str(method) != "zone" {
        return false;
    }
    if !cx.list(args).is_empty() {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    // Must be plain `Time` (not `DateTime`).
    let NodeKind::Const { scope, name } = *cx.kind(recv) else {
        return false;
    };
    if cx.symbol_str(name) != "Time" {
        return false;
    }
    match scope.get() {
        None => true,
        Some(s) => matches!(*cx.kind(s), NodeKind::Cbase),
    }
}

/// Upstream `time_now?`: `(const {nil? cbase} {:Time :DateTime})`.
fn is_time_const(cx: &Cx<'_>, node: NodeId) -> bool {
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return false;
    };
    let n = cx.symbol_str(name);
    if n != "Time" && n != "DateTime" {
        return false;
    }
    match scope.get() {
        None => true,
        Some(s) => matches!(*cx.kind(s), NodeKind::Cbase),
    }
}

/// Upstream `current_time_with_convert?`: method in CONVERT_METHODS with
/// zero args, recursing into the receiver.
fn current_time_with_convert(cx: &Cx<'_>, recv: NodeId, method: &str) -> bool {
    if !CONVERT_METHODS.contains(&method) {
        return false;
    }
    // `recv` is the receiver of the converting call, which must itself be
    // a zero-arg `Send` (upstream `return false if time_argument`).
    let NodeKind::Send {
        receiver: inner_recv,
        method: inner_method,
        args: inner_args,
    } = *cx.kind(recv)
    else {
        return false;
    };
    if !cx.list(inner_args).is_empty() {
        return false;
    }
    let Some(inner_recv) = inner_recv.get() else {
        return false;
    };
    let inner_name = cx.symbol_str(inner_method).to_owned();
    current_time(cx, inner_recv, &inner_name)
}

/// Whole-send range, excluding an attached block (`travel_to(...) do`).
/// Mirrors the send-only recomputation in `Rails/Env`.
fn send_only_range(cx: &Cx<'_>, node: NodeId) -> Range {
    let range = cx.range(node);
    if cx.block_node(node).get().is_none() {
        return range;
    }
    let mut end = range.end;
    if cx.is_parenthesized(node) {
        let sel_end = cx.selector(node).end;
        if let Some(paren_end) = cx
            .tokens_in(range)
            .iter()
            .filter(|tok| {
                tok.kind == SourceTokenKind::RightParen && tok.range.start >= sel_end
            })
            .map(|tok| tok.range.end)
            .next()
        {
            end = paren_end;
        }
    } else if let Some(last) = cx.call_arguments(node).last() {
        end = cx.range(*last).end;
    } else {
        end = cx.selector(node).end;
    }
    Range {
        start: range.start,
        end: end.min(range.end),
    }
}

#[cfg(test)]
mod tests {
    use super::FreezeTime;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_time_now() {
        test::<FreezeTime>().expect_offense(indoc! {r#"
            travel_to(Time.now)
            ^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
        "#});
    }

    #[test]
    fn flags_time_new_and_datetime_now() {
        test::<FreezeTime>().expect_offense(indoc! {r#"
            travel_to(Time.new)
            ^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
        "#});
        test::<FreezeTime>().expect_offense(indoc! {r#"
            travel_to(DateTime.now)
            ^^^^^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
        "#});
    }

    #[test]
    fn flags_time_current_and_zoned() {
        test::<FreezeTime>().expect_offense(indoc! {r#"
            travel_to(Time.current)
            ^^^^^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
        "#});
        test::<FreezeTime>().expect_offense(indoc! {r#"
            travel_to(Time.zone.now)
            ^^^^^^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
        "#});
    }

    #[test]
    fn flags_conversions() {
        test::<FreezeTime>().expect_offense(indoc! {r#"
            travel_to(Time.now.in_time_zone)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
        "#});
        test::<FreezeTime>().expect_offense(indoc! {r#"
            travel_to(Time.current.to_time)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
        "#});
    }

    #[test]
    fn flags_cbase_forms() {
        test::<FreezeTime>().expect_offense(indoc! {r#"
            travel_to(::Time.now)
            ^^^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
        "#});
        test::<FreezeTime>().expect_offense(indoc! {r#"
            travel_to(::Time.zone.now)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
        "#});
    }

    #[test]
    fn corrects_to_freeze_time() {
        test::<FreezeTime>().expect_correction(
            indoc! {r#"
                travel_to(Time.now)
                ^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
            "#},
            "freeze_time\n",
        );
    }

    #[test]
    fn corrects_block_form() {
        test::<FreezeTime>().expect_correction(
            indoc! {r#"
                travel_to(Time.now) do
                ^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
                  do_something
                end
            "#},
            "freeze_time do\n  do_something\nend\n",
        );
    }

    #[test]
    fn corrects_block_pass() {
        test::<FreezeTime>().expect_correction(
            indoc! {r#"
                travel_to(Time.current, &example)
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `freeze_time` instead of `travel_to`.
            "#},
            "freeze_time(&example)\n",
        );
    }

    #[test]
    fn does_not_flag_non_current() {
        test::<FreezeTime>().expect_no_offenses("travel_to(Time.current.yesterday)\n");
        test::<FreezeTime>().expect_no_offenses("travel_to(DateTime.next_day)\n");
    }

    #[test]
    fn does_not_flag_bare_current() {
        test::<FreezeTime>().expect_no_offenses("travel_to(current)\n");
    }

    #[test]
    fn does_not_flag_with_args() {
        test::<FreezeTime>().expect_no_offenses("travel_to(DateTime.new(2022, 5, 3, 12, 0, 0))\n");
        test::<FreezeTime>().expect_no_offenses("travel_to(Time.new(2019, 4, 3, 12, 30).in_time_zone)\n");
    }

    #[test]
    fn does_not_flag_without_argument() {
        test::<FreezeTime>().expect_no_offenses("travel_to\n");
    }

    #[test]
    fn does_not_flag_freeze_time() {
        test::<FreezeTime>().expect_no_offenses("freeze_time\n");
    }
}
murphy_plugin_api::submit_cop!(FreezeTime);
