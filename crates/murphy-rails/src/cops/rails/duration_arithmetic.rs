//! `Rails/DurationArithmetic` — flag `Time.current +/- duration`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/DurationArithmetic
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:+, :-] gating,
//!   `time_current?` (`Time.current` / `Time.zone.now`, `::Time` folded)
//!   receiver with `duration?` single argument (`int`/`float`/bare-call
//!   receiver + duration method, zero args). Whole-send offense range;
//!   autocorrect to `duration.ago` / `duration.from_now`.
//!   `created_at - 1.minute` and `3.days - 1.hour` do not flag.
//! ```
//!
//! ## Matched shapes
//!
//! - `Time.current - 1.minute` → `1.minute.ago`.
//! - `Time.current + 2.days` → `2.days.from_now`.
//! - `::Time.zone.now + 1.hour` → `1.hour.from_now`.
//!
//! `created_at - 1.minute` and `Date.yesterday + 3.days` do not flag.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Do not add or subtract duration.";

const DURATIONS: &[&str] = &[
    "second",
    "seconds",
    "minute",
    "minutes",
    "hour",
    "hours",
    "day",
    "days",
    "week",
    "weeks",
    "fortnight",
    "fortnights",
    "month",
    "months",
    "year",
    "years",
];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DurationArithmetic;

#[cop(
    name = "Rails/DurationArithmetic",
    description = "Do not use duration as arithmetic operand with `Time.current`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl DurationArithmetic {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[+ -]`.
    #[on_node(kind = "send", methods = ["+", "-"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };
        let op = cx.symbol_str(method);
        if op != "+" && op != "-" {
            return;
        }
        let Some(recv) = receiver.get() else {
            return;
        };
        if !is_time_current(cx, recv) {
            return;
        }
        let args = cx.call_arguments(node);
        if args.len() != 1 {
            return;
        }
        let dur = args[0];
        if !is_duration(cx, dur) {
            return;
        }
        cx.emit_offense(cx.range(node), MSG, None);
        let dur_src = cx.raw_source(cx.range(dur));
        let replacement = if op == "-" {
            format!("{dur_src}.ago")
        } else {
            format!("{dur_src}.from_now")
        };
        cx.emit_edit(cx.range(node), &replacement);
    }
}

/// Upstream `time_current?`: `Time.current` or `Time.zone.now`
/// (`::Time` folds to `Time` via `const_name`).
fn is_time_current(cx: &Cx<'_>, node: NodeId) -> bool {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return false;
    };
    let name = cx.symbol_str(method);
    if !cx.call_arguments(node).is_empty() {
        return false;
    }
    if name == "current" {
        let Some(recv) = receiver.get() else {
            return false;
        };
        return cx.const_name(recv).as_deref() == Some("Time");
    }
    if name == "now" {
        let Some(zone_id) = receiver.get() else {
            return false;
        };
        let NodeKind::Send {
            receiver: zone_recv,
            method: zone_method,
            ..
        } = *cx.kind(zone_id)
        else {
            return false;
        };
        if cx.symbol_str(zone_method) != "zone" {
            return false;
        }
        if !cx.call_arguments(zone_id).is_empty() {
            return false;
        }
        let Some(time_id) = zone_recv.get() else {
            return false;
        };
        return cx.const_name(time_id).as_deref() == Some("Time");
    }
    false
}

/// Upstream `duration?`: `(send {int float (send nil _)} DURATIONS)`
/// with zero args.
fn is_duration(cx: &Cx<'_>, node: NodeId) -> bool {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return false;
    };
    if !DURATIONS.contains(&cx.symbol_str(method)) {
        return false;
    }
    if !cx.call_arguments(node).is_empty() {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    match *cx.kind(recv) {
        NodeKind::Int(_) | NodeKind::Float(_) => true,
        NodeKind::Send {
            receiver: inner_recv,
            ..
        } => {
            // `(send nil _)` — bare zero-arg call (e.g. `foo` in `foo.hour`).
            inner_recv.get().is_none() && cx.call_arguments(recv).is_empty()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::DurationArithmetic;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_subtraction() {
        test::<DurationArithmetic>().expect_offense(indoc! {r#"
            Time.current - 1.minute
            ^^^^^^^^^^^^^^^^^^^^^^^ Do not add or subtract duration.
        "#});
    }

    #[test]
    fn flags_addition() {
        test::<DurationArithmetic>().expect_offense(indoc! {r#"
            Time.current + 2.days
            ^^^^^^^^^^^^^^^^^^^^^ Do not add or subtract duration.
        "#});
    }

    #[test]
    fn flags_zone_now() {
        test::<DurationArithmetic>().expect_offense(indoc! {r#"
            Time.zone.now + 1.hour
            ^^^^^^^^^^^^^^^^^^^^^^ Do not add or subtract duration.
        "#});
    }

    #[test]
    fn flags_cbase_time() {
        test::<DurationArithmetic>().expect_offense(indoc! {r#"
            ::Time.current - 1.minute
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not add or subtract duration.
        "#});
    }

    #[test]
    fn does_not_flag_created_at() {
        test::<DurationArithmetic>().expect_no_offenses("created_at - 1.minute\n");
    }

    #[test]
    fn does_not_flag_duration_minus_duration() {
        test::<DurationArithmetic>().expect_no_offenses("3.days - 1.hour\n");
    }

    #[test]
    fn does_not_flag_other_receiver() {
        test::<DurationArithmetic>().expect_no_offenses("Date.yesterday + 3.days\n");
    }

    #[test]
    fn does_not_flag_ago() {
        test::<DurationArithmetic>().expect_no_offenses("1.minute.ago\n");
    }

    #[test]
    fn corrects_subtraction() {
        test::<DurationArithmetic>()
            .expect_correction(
                indoc! {r#"
                    Time.current - 1.minute
                    ^^^^^^^^^^^^^^^^^^^^^^^ Do not add or subtract duration.
                "#},
                "1.minute.ago\n",
            )
            .expect_no_offenses("1.minute.ago\n");
    }

    #[test]
    fn corrects_addition() {
        test::<DurationArithmetic>()
            .expect_correction(
                indoc! {r#"
                    Time.current + 2.days
                    ^^^^^^^^^^^^^^^^^^^^^ Do not add or subtract duration.
                "#},
                "2.days.from_now\n",
            )
            .expect_no_offenses("2.days.from_now\n");
    }
}
murphy_plugin_api::submit_cop!(DurationArithmetic);
