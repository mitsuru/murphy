//! `Rails/ExpandedDateRange` — collapse `beginning_of_X..end_of_X` to `all_X`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ExpandedDateRange
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_irange` (inclusive `..` only;
//!   `...` never flags): same-receiver-source gating, the
//!   beginning/end method-pair table, and the argument rules —
//!   `beginning_of_week`/`end_of_week` with one identical argument each
//!   rewrite to `all_week(arg)`, any other argument combination returns
//!   without flagging. Receiver-less endpoints never flag. Gated on
//!   `TargetRailsVersion >= 5.1` (unset means newest). Upstream ships
//!   `Enabled: pending`; Murphy maps that to `default_enabled = false`.
//! ```
//!
//! ## Matched shape (inclusive range)
//!
//! `date.beginning_of_day..date.end_of_day` → `date.all_day` (and the
//! week/month/quarter/year pairs analogously).
//! `date.beginning_of_week(:monday)..date.end_of_week(:monday)` →
//! `date.all_week(:monday)`. Mismatched receivers, mismatched pairs,
//! exclusive `...` ranges, and stray arguments do not flag.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ExpandedDateRange;

/// (beginning method, mapped end method, preferred `all_*` method).
const PAIRS: &[(&str, &str, &str)] = &[
    ("beginning_of_day", "end_of_day", "all_day"),
    ("beginning_of_week", "end_of_week", "all_week"),
    ("beginning_of_month", "end_of_month", "all_month"),
    ("beginning_of_quarter", "end_of_quarter", "all_quarter"),
    ("beginning_of_year", "end_of_year", "all_year"),
];

#[cop(
    name = "Rails/ExpandedDateRange",
    description = "Checks for expanded date range.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl ExpandedDateRange {
    // Mirrors upstream `on_irange` — exclusive (`...`) ranges never flag.
    #[on_node(kind = "range")]
    fn check_range(&self, node: NodeId, cx: &Cx<'_>) {
        // Upstream `minimum_target_rails_version 5.1` — unset means newest.
        if !cx.rails_version_at_least(5, 1) {
            return;
        }
        let NodeKind::RangeExpr {
            begin_,
            end_,
            exclusive,
        } = *cx.kind(node)
        else {
            return;
        };
        if exclusive {
            return;
        }
        let (Some(begin), Some(end)) = (begin_.get(), end_.get()) else {
            return;
        };
        // Upstream `receiver_source`: both endpoints must be calls with a
        // receiver (`Send` only — `&.` is not `send_type?` upstream).
        let NodeKind::Send {
            receiver: begin_receiver,
            method: begin_method,
            ..
        } = *cx.kind(begin)
        else {
            return;
        };
        let NodeKind::Send {
            receiver: end_receiver,
            method: end_method,
            ..
        } = *cx.kind(end)
        else {
            return;
        };
        let (Some(begin_recv), Some(end_recv)) = (begin_receiver.get(), end_receiver.get())
        else {
            return;
        };
        let begin_name = cx.symbol_str(begin_method);
        let end_name = cx.symbol_str(end_method);
        // Upstream `allow?`: mismatched receivers or a non-mapped method
        // pair never flag.
        let Some((_, mapped_end, all_method)) =
            PAIRS.iter().find(|(begin, _, _)| *begin == begin_name)
        else {
            return;
        };
        if *mapped_end != end_name {
            return;
        }
        let recv_src = cx.raw_source(cx.range(begin_recv));
        if recv_src != cx.raw_source(cx.range(end_recv)) {
            return;
        }
        let begin_args = cx.call_arguments(begin);
        let end_args = cx.call_arguments(end);
        // Upstream: `beginning_of_week` with one argument on each side and
        // identical argument source keeps the argument; any other argument
        // combination returns without flagging.
        let replacement = if begin_name == "beginning_of_week"
            && begin_args.len() == 1
            && end_args.len() == 1
        {
            let begin_arg_src = cx.raw_source(cx.range(begin_args[0]));
            if begin_arg_src != cx.raw_source(cx.range(end_args[0])) {
                return;
            }
            format!("{recv_src}.{all_method}({begin_arg_src})")
        } else {
            if !begin_args.is_empty() || !end_args.is_empty() {
                return;
            }
            format!("{recv_src}.{all_method}")
        };
        cx.emit_offense(
            cx.range(node),
            &format!("Use `{replacement}` instead."),
            None,
        );
        cx.emit_edit(cx.range(node), &replacement);
    }
}

#[cfg(test)]
mod tests {
    use super::ExpandedDateRange;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_beginning_of_day() {
        test::<ExpandedDateRange>().expect_offense(indoc! {r#"
            date.beginning_of_day..date.end_of_day
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `date.all_day` instead.
        "#});
    }

    #[test]
    fn flags_beginning_of_week() {
        test::<ExpandedDateRange>().expect_offense(indoc! {r#"
            date.beginning_of_week..date.end_of_week
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `date.all_week` instead.
        "#});
    }

    #[test]
    fn flags_beginning_of_month() {
        test::<ExpandedDateRange>().expect_offense(indoc! {r#"
            date.beginning_of_month..date.end_of_month
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `date.all_month` instead.
        "#});
    }

    #[test]
    fn flags_beginning_of_quarter() {
        test::<ExpandedDateRange>().expect_offense(indoc! {r#"
            date.beginning_of_quarter..date.end_of_quarter
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `date.all_quarter` instead.
        "#});
    }

    #[test]
    fn flags_beginning_of_year() {
        test::<ExpandedDateRange>().expect_offense(indoc! {r#"
            date.beginning_of_year..date.end_of_year
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `date.all_year` instead.
        "#});
    }

    #[test]
    fn flags_week_with_matching_argument() {
        test::<ExpandedDateRange>().expect_offense(indoc! {r#"
            date.beginning_of_week(:monday)..date.end_of_week(:monday)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `date.all_week(:monday)` instead.
        "#});
    }

    #[test]
    fn flags_chained_receiver() {
        test::<ExpandedDateRange>().expect_offense(indoc! {r#"
            Date.today.beginning_of_day..Date.today.end_of_day
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Date.today.all_day` instead.
        "#});
    }

    #[test]
    fn does_not_flag_exclusive_range() {
        test::<ExpandedDateRange>()
            .expect_no_offenses("date.beginning_of_day...date.end_of_day\n");
    }

    #[test]
    fn does_not_flag_mismatched_receivers() {
        test::<ExpandedDateRange>()
            .expect_no_offenses("a.beginning_of_day..b.end_of_day\n");
    }

    #[test]
    fn does_not_flag_mismatched_pair() {
        test::<ExpandedDateRange>()
            .expect_no_offenses("date.beginning_of_day..date.end_of_week\n");
    }

    #[test]
    fn does_not_flag_week_with_different_arguments() {
        test::<ExpandedDateRange>().expect_no_offenses(
            "date.beginning_of_week(:monday)..date.end_of_week(:sunday)\n",
        );
    }

    #[test]
    fn does_not_flag_stray_argument() {
        test::<ExpandedDateRange>()
            .expect_no_offenses("date.beginning_of_day(x)..date.end_of_day\n");
    }

    #[test]
    fn does_not_flag_receiver_less_endpoints() {
        test::<ExpandedDateRange>().expect_no_offenses("beginning_of_day..end_of_day\n");
    }

    #[test]
    fn does_not_flag_below_rails_51() {
        test::<ExpandedDateRange>()
            .with_target_rails_version(5, 0)
            .expect_no_offenses("date.beginning_of_day..date.end_of_day\n");
    }

    #[test]
    fn fires_at_rails_51() {
        test::<ExpandedDateRange>()
            .with_target_rails_version(5, 1)
            .expect_offense(indoc! {r#"
                date.beginning_of_day..date.end_of_day
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `date.all_day` instead.
            "#});
    }

    #[test]
    fn corrects_expanded_range() {
        test::<ExpandedDateRange>()
            .expect_correction(
                indoc! {r#"
                    date.beginning_of_day..date.end_of_day
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `date.all_day` instead.
                "#},
                "date.all_day\n",
            )
            .expect_no_offenses("date.all_day\n");
    }

    #[test]
    fn corrects_week_with_argument() {
        test::<ExpandedDateRange>()
            .expect_correction(
                indoc! {r#"
                    date.beginning_of_week(:monday)..date.end_of_week(:monday)
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `date.all_week(:monday)` instead.
                "#},
                "date.all_week(:monday)\n",
            )
            .expect_no_offenses("date.all_week(:monday)\n");
    }
}
murphy_plugin_api::submit_cop!(ExpandedDateRange);
