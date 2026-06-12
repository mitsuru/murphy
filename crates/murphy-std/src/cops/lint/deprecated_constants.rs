//! `Lint/DeprecatedConstants` — flag usage of deprecated constants and
//! autocorrect to their preferred alternatives.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DeprecatedConstants
//! upstream_version_checked: 1.86.2
//! version_added: "1.8"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ships RuboCop's default `DeprecatedConstants` map (NIL, TRUE, FALSE,
//!   Net::HTTPServerException, Random::DEFAULT, Struct::Group, Struct::Passwd).
//!   Two inherent v1 ABI limitations, documented rather than tracked as open
//!   work: (1) the `DeprecatedConstants` config map is not user-overridable —
//!   the v1 derive has no support for a nested
//!   constant→{Alternative,DeprecatedVersion} hash; (2) `target_ruby_version`
//!   gating is not exposed by the plugin ABI, so the `DeprecatedVersion` is
//!   rendered in the message but never used to suppress an offense (RuboCop
//!   skips a constant when the target Ruby predates its deprecation). On a
//!   modern target Ruby — the common case — this is observationally identical
//!   to RuboCop.
//! ```
//!
//! ## Matched shapes
//!
//! Every `const` node whose normalized source (leading `::` stripped) is a key
//! in the deprecated-constants map. The whole const node is the offense range
//! and the autocorrect replacement target.
//!
//! ## Why this shape
//!
//! RuboCop subscribes to `on_const` and matches `node.source.delete_prefix('::')`
//! against the configured map. Murphy mirrors this with `kind = "const"` and
//! `cx.const_name`, which already produces the `Net::HTTPServerException` form
//! without a cbase prefix.
//!
//! ## Autocorrect
//!
//! Replaces the whole const node with the configured `Alternative`. Safe
//! because the alternatives are constant/literal source the user is expected to
//! adopt verbatim.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop};

#[derive(Default)]
pub struct DeprecatedConstants;

/// RuboCop's default `DeprecatedConstants` map, as
/// `(constant, Some(alternative_or_none), deprecated_version)` triples.
/// `alternative == None` renders the "Do not use" message variant.
const DEPRECATED_CONSTANTS: &[(&str, Option<&str>, &str)] = &[
    ("NIL", Some("nil"), "2.4"),
    ("TRUE", Some("true"), "2.4"),
    ("FALSE", Some("false"), "2.4"),
    (
        "Net::HTTPServerException",
        Some("Net::HTTPClientException"),
        "2.6",
    ),
    ("Random::DEFAULT", Some("Random.new"), "3.0"),
    ("Struct::Group", Some("Etc::Group"), "3.0"),
    ("Struct::Passwd", Some("Etc::Passwd"), "3.0"),
];

#[cop(
    name = "Lint/DeprecatedConstants",
    description = "Flag deprecated constants and suggest alternatives.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DeprecatedConstants {
    #[on_node(kind = "const")]
    fn check_const(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(constant) = cx.const_name(node) else {
            return;
        };

        let Some(&(_, alternative, version)) =
            DEPRECATED_CONSTANTS.iter().find(|(name, ..)| *name == constant)
        else {
            return;
        };

        let bad = cx.raw_source(cx.range(node));
        let message = message(alternative, bad, version);
        cx.emit_offense(cx.range(node), &message, None);

        if let Some(good) = alternative {
            cx.emit_edit(cx.range(node), good);
        }
    }
}

fn message(good: Option<&str>, bad: &str, deprecated_version: &str) -> String {
    if let Some(good) = good {
        format!(
            "Use `{good}` instead of `{bad}`, deprecated since Ruby {deprecated_version}."
        )
    } else {
        format!("Do not use `{bad}`, deprecated since Ruby {deprecated_version}.")
    }
}

#[cfg(test)]
mod tests {
    use super::DeprecatedConstants;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_nil_true_false() {
        test::<DeprecatedConstants>().expect_offense(indoc! {r#"
            x = NIL
                ^^^ Use `nil` instead of `NIL`, deprecated since Ruby 2.4.
            y = TRUE
                ^^^^ Use `true` instead of `TRUE`, deprecated since Ruby 2.4.
            z = FALSE
                ^^^^^ Use `false` instead of `FALSE`, deprecated since Ruby 2.4.
        "#});
    }

    #[test]
    fn flags_namespaced_constants() {
        test::<DeprecatedConstants>().expect_offense(indoc! {r#"
            Net::HTTPServerException
            ^^^^^^^^^^^^^^^^^^^^^^^^ Use `Net::HTTPClientException` instead of `Net::HTTPServerException`, deprecated since Ruby 2.6.
            Random::DEFAULT
            ^^^^^^^^^^^^^^^ Use `Random.new` instead of `Random::DEFAULT`, deprecated since Ruby 3.0.
        "#});
    }

    #[test]
    fn flags_cbase_prefixed_constant() {
        // `::NIL` normalizes to `NIL`; the offense range still covers the
        // whole `::NIL` source.
        test::<DeprecatedConstants>().expect_offense(indoc! {r#"
            x = ::NIL
                ^^^^^ Use `nil` instead of `::NIL`, deprecated since Ruby 2.4.
        "#});
    }

    #[test]
    fn does_not_flag_live_constants() {
        test::<DeprecatedConstants>().expect_no_offenses(indoc! {r#"
            nil
            Foo::Bar
            Random.new
            Net::HTTPClientException
        "#});
    }

    #[test]
    fn autocorrects_to_alternative() {
        test::<DeprecatedConstants>().expect_correction(
            indoc! {r#"
                x = NIL
                    ^^^ Use `nil` instead of `NIL`, deprecated since Ruby 2.4.
            "#},
            "x = nil\n",
        );
    }

    #[test]
    fn autocorrects_namespaced() {
        test::<DeprecatedConstants>().expect_correction(
            indoc! {r#"
                Random::DEFAULT
                ^^^^^^^^^^^^^^^ Use `Random.new` instead of `Random::DEFAULT`, deprecated since Ruby 3.0.
            "#},
            "Random.new\n",
        );
    }
}

murphy_plugin_api::submit_cop!(DeprecatedConstants);
