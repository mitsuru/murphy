//! `Rails/PluralizationGrammar` — use singular/plural duration methods matching the number.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/PluralizationGrammar
//! upstream_version_checked: 2.35.0
//! version_added: "0.35"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND over all singular and
//!   plural duration methods (send only, no csend), literal Int/Float
//!   receiver gate, singular (`== 1`) vs plural mismatch detection.
//!   Offense is the whole call node; autocorrect replaces the selector
//!   with the correctly inflected method name. Upstream Include gating
//!   absent.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct PluralizationGrammar;

/// Singular duration method → plural form (upstream `SINGULAR_METHODS`).
fn pluralize(name: &str) -> Option<&'static str> {
    Some(match name {
        "second" => "seconds",
        "minute" => "minutes",
        "hour" => "hours",
        "day" => "days",
        "week" => "weeks",
        "fortnight" => "fortnights",
        "month" => "months",
        "year" => "years",
        "byte" => "bytes",
        "kilobyte" => "kilobytes",
        "megabyte" => "megabytes",
        "gigabyte" => "gigabytes",
        "terabyte" => "terabytes",
        "petabyte" => "petabytes",
        "exabyte" => "exabytes",
        "zettabyte" => "zettabytes",
        _ => return None,
    })
}

/// Plural duration method → singular form (upstream `PLURAL_METHODS`).
fn singularize(name: &str) -> Option<&'static str> {
    Some(match name {
        "seconds" => "second",
        "minutes" => "minute",
        "hours" => "hour",
        "days" => "day",
        "weeks" => "week",
        "fortnights" => "fortnight",
        "months" => "month",
        "years" => "year",
        "bytes" => "byte",
        "kilobytes" => "kilobyte",
        "megabytes" => "megabyte",
        "gigabytes" => "gigabyte",
        "terabytes" => "terabyte",
        "petabytes" => "petabyte",
        "exabytes" => "exabyte",
        "zettabytes" => "zettabyte",
        _ => return None,
    })
}

#[cop(
    name = "Rails/PluralizationGrammar",
    description = "Checks for correct grammar when using ActiveSupport's core extensions to the numeric classes.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl PluralizationGrammar {
    // Mirrors upstream `RESTRICT_ON_SEND` (all singular + plural names).
    #[on_node(
        kind = "send",
        methods = [
            "second", "seconds", "minute", "minutes", "hour", "hours",
            "day", "days", "week", "weeks", "fortnight", "fortnights",
            "month", "months", "year", "years", "byte", "bytes",
            "kilobyte", "kilobytes", "megabyte", "megabytes",
            "gigabyte", "gigabytes", "terabyte", "terabytes",
            "petabyte", "petabytes", "exabyte", "exabytes",
            "zettabyte", "zettabytes"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    // Literal number receiver only (upstream `literal_number?`: int/float).
    let receiver = match cx.call_receiver(node).get() {
        Some(r) => r,
        None => return,
    };
    let is_one = match *cx.kind(receiver) {
        NodeKind::Int(n) => n.abs() == 1,
        NodeKind::Float(f) => f.abs() == 1.0,
        _ => return,
    };
    let is_plural = method.ends_with('s');
    // Offense when singularity of receiver and method disagree.
    let correct = if is_one && is_plural {
        singularize(&method)
    } else if !is_one && !is_plural {
        pluralize(&method)
    } else {
        return;
    };
    let Some(correct) = correct else {
        return;
    };
    let number_src = cx.raw_source(cx.range(receiver));
    cx.emit_offense(
        cx.range(node),
        &format!("Prefer `{number_src}.{correct}`."),
        None,
    );
    cx.emit_edit(cx.selector(node), correct);
}

#[cfg(test)]
mod tests {
    use super::PluralizationGrammar;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_singular_number_with_plural_method() {
        test::<PluralizationGrammar>().expect_correction(
            indoc! {r#"
                3.day.ago
                ^^^^^ Prefer `3.days`.
            "#},
            "3.days.ago\n",
        );
    }

    #[test]
    fn flags_one_with_plural_method() {
        test::<PluralizationGrammar>().expect_correction(
            indoc! {r#"
                1.months.ago
                ^^^^^^^^ Prefer `1.month`.
            "#},
            "1.month.ago\n",
        );
    }

    #[test]
    fn flags_byte_mismatch() {
        test::<PluralizationGrammar>().expect_correction(
            indoc! {r#"
                5.megabyte
                ^^^^^^^^^^ Prefer `5.megabytes`.
            "#},
            "5.megabytes\n",
        );
    }

    #[test]
    fn flags_one_gigabyte_plural() {
        test::<PluralizationGrammar>().expect_correction(
            indoc! {r#"
                1.gigabytes
                ^^^^^^^^^^^ Prefer `1.gigabyte`.
            "#},
            "1.gigabyte\n",
        );
    }

    #[test]
    fn allows_matching_plural() {
        test::<PluralizationGrammar>().expect_no_offenses("3.days.ago\n");
    }

    #[test]
    fn allows_matching_singular() {
        test::<PluralizationGrammar>().expect_no_offenses("1.month.ago\n");
    }

    #[test]
    fn allows_non_literal_receiver() {
        test::<PluralizationGrammar>().expect_no_offenses("n.days.ago\n");
    }

    #[test]
    fn allows_non_duration_method() {
        test::<PluralizationGrammar>().expect_no_offenses("3.items\n");
    }
}
murphy_plugin_api::submit_cop!(PluralizationGrammar);
