//! `Rails/RefuteMethods` — prefer one Minitest negative-assertion spelling.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RefuteMethods
//! upstream_version_checked: 2.35.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Send-dispatch port of RuboCop's on_send with EnforcedStyle
//!   (assert_not default, refute alternative). Bare-call gating
//!   (receiver None), selector-only offense range, and selector
//!   replacement autocorrect all mirror upstream. The 14-entry
//!   CORRECTIONS table (including refute_match to assert_no_match)
//!   is complete. Upstream Include (`**/test/**/*`) is enforced via the
//!   murphy-rails pack default.yml (engine `cop_applies_to_file` gate,
//!   verified vs rubocop-rails 2.38.0 default.yml, murphy-4gd.1.15;
//!   same coverage as Rails/AssertNot, tracked in murphy-juee).
//! ```
//!
//! Minitest ships two spellings for every negative assertion:
//! `refute*` and `assert_not*` (plus the `refute_match` /
//! `assert_no_match` pair). RuboCop defaults to the `assert_not`
//! spelling but lets projects opt into `refute` via EnforcedStyle.
//!
//! ## Matched shape (Send node)
//!
//! `Send(receiver=None, method in BAD, args=[...])` — a bare call
//! (implicit self), any arguments (upstream `...`).
//!
//! - **Default style `assert_not`**: BAD is the 14 `refute*` keys.
//! - **Style `refute`**: BAD is the 14 `assert_not*` values
//!   (including `assert_no_match`).
//!
//! Explicit receivers (`obj.refute`, `self.refute`) never match —
//! upstream's pattern requires `nil?` receiver. `Csend` (`&.refute`)
//! never matches either — upstream only defines `on_send`.
//! Comments and string literals never produce Send nodes, so they
//! are ignored by construction.
//!
//! ## Offense range and message
//!
//! Selector only (`node.loc.selector`), message
//! `Prefer \`good\` over \`bad\`.` — e.g.
//! `Prefer \`assert_not\` over \`refute\`.`
//!
//! ## Autocorrect
//!
//! Replace the selector with the preferred spelling. Arguments pass
//! through untouched.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Upstream CORRECTIONS table (rubocop-rails 2.35.0), `refute*` to
/// `assert_not*`. Note the irregular pair: `refute_match` maps to
/// `assert_no_match` (not `assert_not_match`).
const CORRECTIONS: &[(&str, &str)] = &[
    ("refute", "assert_not"),
    ("refute_empty", "assert_not_empty"),
    ("refute_equal", "assert_not_equal"),
    ("refute_in_delta", "assert_not_in_delta"),
    ("refute_in_epsilon", "assert_not_in_epsilon"),
    ("refute_includes", "assert_not_includes"),
    ("refute_instance_of", "assert_not_instance_of"),
    ("refute_kind_of", "assert_not_kind_of"),
    ("refute_nil", "assert_not_nil"),
    ("refute_operator", "assert_not_operator"),
    ("refute_predicate", "assert_not_predicate"),
    ("refute_respond_to", "assert_not_respond_to"),
    ("refute_same", "assert_not_same"),
    ("refute_match", "assert_no_match"),
];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RefuteMethods;

#[derive(CopOptions)]
pub struct RefuteMethodsOptions {
    #[option(
        name = "EnforcedStyle",
        default = "assert_not",
        description = "Which negative-assertion spelling to enforce."
    )]
    pub enforced_style: RefuteMethodsStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum RefuteMethodsStyle {
    #[option(value = "assert_not")]
    AssertNot,
    #[option(value = "refute")]
    Refute,
}

#[cop(
    name = "Rails/RefuteMethods",
    description = "Use `assert_not` methods instead of `refute` methods.",
    default_severity = "warning",
    default_enabled = true,
    options = RefuteMethodsOptions,
)]
impl RefuteMethods {
    // `methods = [...]` mirrors upstream `RESTRICT_ON_SEND` (all 28
    // spellings) — dispatch only on candidate selectors. The body
    // still gates on style-specific bad sets and bare receiver.
    #[on_node(kind = "send", methods = [
        "refute",
        "refute_empty",
        "refute_equal",
        "refute_in_delta",
        "refute_in_epsilon",
        "refute_includes",
        "refute_instance_of",
        "refute_kind_of",
        "refute_nil",
        "refute_operator",
        "refute_predicate",
        "refute_respond_to",
        "refute_same",
        "refute_match",
        "assert_not",
        "assert_not_empty",
        "assert_not_equal",
        "assert_not_in_delta",
        "assert_not_in_epsilon",
        "assert_not_includes",
        "assert_not_instance_of",
        "assert_not_kind_of",
        "assert_not_nil",
        "assert_not_operator",
        "assert_not_predicate",
        "assert_not_respond_to",
        "assert_not_same",
        "assert_no_match",
    ])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
            return;
        };
        // Bare call only — `(send nil? ...)` upstream. Explicit
        // receivers (`obj.refute`, `self.refute`) do not match.
        if receiver.get().is_some() {
            return;
        }
        let method_str = cx.symbol_str(method);
        let opts = cx.options_or_default::<RefuteMethodsOptions>();
        let Some(good) = good_method(method_str, opts.enforced_style) else {
            return;
        };
        cx.emit_offense(
            cx.loc(node).name,
            &format!("Prefer `{good}` over `{method_str}`."),
            None,
        );
        cx.emit_edit(cx.loc(node).name, good);
    }
}

/// Map a bad-method name to its preferred spelling for the given
/// style, or `None` when the method is not offensive under it.
fn good_method(bad: &str, style: RefuteMethodsStyle) -> Option<&'static str> {
    match style {
        RefuteMethodsStyle::AssertNot => CORRECTIONS
            .iter()
            .find(|(refute, _)| *refute == bad)
            .map(|(_, good)| *good),
        RefuteMethodsStyle::Refute => CORRECTIONS
            .iter()
            .find(|(_, assert_not)| *assert_not == bad)
            .map(|(refute, _)| *refute),
    }
}

#[cfg(test)]
mod tests {
    use super::{RefuteMethods, RefuteMethodsOptions, RefuteMethodsStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn refute_style() -> RefuteMethodsOptions {
        RefuteMethodsOptions {
            enforced_style: RefuteMethodsStyle::Refute,
        }
    }

    // === default style (assert_not): hit cases ===

    #[test]
    fn flags_bare_refute_by_default() {
        test::<RefuteMethods>().expect_offense(indoc! {r#"
                refute false
                ^^^^^^ Prefer `assert_not` over `refute`.
            "#});
    }

    #[test]
    fn corrects_bare_refute_by_default() {
        test::<RefuteMethods>().expect_correction(
            indoc! {r#"
                refute false
                ^^^^^^ Prefer `assert_not` over `refute`.
            "#},
            "assert_not false\n",
        );
    }

    #[test]
    fn flags_refute_equal_by_default() {
        test::<RefuteMethods>().expect_offense(indoc! {r#"
                refute_equal true, false
                ^^^^^^^^^^^^ Prefer `assert_not_equal` over `refute_equal`.
            "#});
    }

    #[test]
    fn corrects_refute_equal_by_default() {
        test::<RefuteMethods>().expect_correction(
            indoc! {r#"
                refute_equal true, false
                ^^^^^^^^^^^^ Prefer `assert_not_equal` over `refute_equal`.
            "#},
            "assert_not_equal true, false\n",
        );
    }

    #[test]
    fn flags_refute_match_to_assert_no_match() {
        // Irregular pair: refute_match -> assert_no_match.
        test::<RefuteMethods>().expect_offense(indoc! {r#"
                refute_match(pattern, string)
                ^^^^^^^^^^^^ Prefer `assert_no_match` over `refute_match`.
            "#});
    }

    #[test]
    fn corrects_refute_match_to_assert_no_match() {
        test::<RefuteMethods>().expect_correction(
            indoc! {r#"
                refute_match(pattern, string)
                ^^^^^^^^^^^^ Prefer `assert_no_match` over `refute_match`.
            "#},
            "assert_no_match(pattern, string)\n",
        );
    }

    #[test]
    fn flags_refute_empty_parens() {
        test::<RefuteMethods>().expect_correction(
            indoc! {r#"
                refute_empty([1, 2, 3])
                ^^^^^^^^^^^^ Prefer `assert_not_empty` over `refute_empty`.
            "#},
            "assert_not_empty([1, 2, 3])\n",
        );
    }

    #[test]
    fn flags_refute_nil_with_message_arg() {
        // Extra args (failure message) pass through; any arity matches.
        test::<RefuteMethods>().expect_correction(
            indoc! {r#"
                refute_nil(obj, 'must be nil')
                ^^^^^^^^^^ Prefer `assert_not_nil` over `refute_nil`.
            "#},
            "assert_not_nil(obj, 'must be nil')\n",
        );
    }

    #[test]
    fn flags_all_refute_variants_by_default() {
        // Exhaust the 14-entry CORRECTIONS table in one test so a
        // future typo in the table fails loudly.
        let pairs = [
            ("refute", "assert_not"),
            ("refute_empty", "assert_not_empty"),
            ("refute_equal", "assert_not_equal"),
            ("refute_in_delta", "assert_not_in_delta"),
            ("refute_in_epsilon", "assert_not_in_epsilon"),
            ("refute_includes", "assert_not_includes"),
            ("refute_instance_of", "assert_not_instance_of"),
            ("refute_kind_of", "assert_not_kind_of"),
            ("refute_nil", "assert_not_nil"),
            ("refute_operator", "assert_not_operator"),
            ("refute_predicate", "assert_not_predicate"),
            ("refute_respond_to", "assert_not_respond_to"),
            ("refute_same", "assert_not_same"),
            ("refute_match", "assert_no_match"),
        ];
        for (bad, good) in pairs {
            let src = format!("{bad} x\n");
            let msg = format!("Prefer `{good}` over `{bad}`.");
            let carets = "^".repeat(bad.len());
            let annotated = format!("{bad} x\n{carets} {msg}\n");
            test::<RefuteMethods>().expect_offense(&annotated);
            // Autocorrect reaches the good spelling, which is then clean.
            let corrected = format!("{good} x\n");
            test::<RefuteMethods>().expect_correction(&annotated, &corrected);
            let _ = src;
        }
    }

    // === default style: no-hit cases ===

    #[test]
    fn does_not_flag_assert_not_by_default() {
        test::<RefuteMethods>().expect_no_offenses("assert_not false\n");
    }

    #[test]
    fn does_not_flag_assert_no_match_by_default() {
        test::<RefuteMethods>().expect_no_offenses("assert_no_match(pattern, string)\n");
    }

    #[test]
    fn does_not_flag_receiver_refute_by_default() {
        // Explicit receiver — upstream requires nil? receiver.
        test::<RefuteMethods>().expect_no_offenses("obj.refute(false)\n");
    }

    #[test]
    fn does_not_flag_self_refute_by_default() {
        test::<RefuteMethods>().expect_no_offenses("self.refute(false)\n");
    }

    #[test]
    fn does_not_flag_refute_in_comment_by_default() {
        test::<RefuteMethods>().expect_no_offenses("# refute false\nassert_not x\n");
    }

    #[test]
    fn does_not_flag_refute_in_string_by_default() {
        test::<RefuteMethods>().expect_no_offenses("puts \"refute false\"\n");
    }

    #[test]
    fn does_not_flag_similar_prefix_by_default() {
        // Dispatch is on exact method names; longer names do not match.
        test::<RefuteMethods>().expect_no_offenses("refute_equality(a, b)\n");
    }

    // === refute style: hit cases ===

    #[test]
    fn refute_style_flags_assert_not() {
        test::<RefuteMethods>()
            .with_options(&refute_style())
            .expect_offense(indoc! {r#"
                assert_not false
                ^^^^^^^^^^ Prefer `refute` over `assert_not`.
            "#});
    }

    #[test]
    fn refute_style_corrects_assert_not() {
        test::<RefuteMethods>()
            .with_options(&refute_style())
            .expect_correction(
                indoc! {r#"
                    assert_not false
                    ^^^^^^^^^^ Prefer `refute` over `assert_not`.
                "#},
                "refute false\n",
            );
    }

    #[test]
    fn refute_style_flags_assert_no_match_to_refute_match() {
        // Inverse irregular pair: assert_no_match -> refute_match.
        test::<RefuteMethods>()
            .with_options(&refute_style())
            .expect_correction(
                indoc! {r#"
                    assert_no_match(pattern, string)
                    ^^^^^^^^^^^^^^^ Prefer `refute_match` over `assert_no_match`.
                "#},
                "refute_match(pattern, string)\n",
            );
    }

    #[test]
    fn refute_style_flags_all_assert_not_variants() {
        let pairs = [
            ("assert_not", "refute"),
            ("assert_not_empty", "refute_empty"),
            ("assert_not_equal", "refute_equal"),
            ("assert_not_in_delta", "refute_in_delta"),
            ("assert_not_in_epsilon", "refute_in_epsilon"),
            ("assert_not_includes", "refute_includes"),
            ("assert_not_instance_of", "refute_instance_of"),
            ("assert_not_kind_of", "refute_kind_of"),
            ("assert_not_nil", "refute_nil"),
            ("assert_not_operator", "refute_operator"),
            ("assert_not_predicate", "refute_predicate"),
            ("assert_not_respond_to", "refute_respond_to"),
            ("assert_not_same", "refute_same"),
            ("assert_no_match", "refute_match"),
        ];
        for (bad, good) in pairs {
            let carets = "^".repeat(bad.len());
            let annotated = format!("{bad} x\n{carets} Prefer `{good}` over `{bad}`.\n");
            test::<RefuteMethods>()
                .with_options(&refute_style())
                .expect_offense(&annotated);
        }
    }

    // === refute style: no-hit cases ===

    #[test]
    fn refute_style_does_not_flag_refute() {
        test::<RefuteMethods>()
            .with_options(&refute_style())
            .expect_no_offenses("refute false\n");
    }

    #[test]
    fn refute_style_does_not_flag_receiver_assert_not() {
        test::<RefuteMethods>()
            .with_options(&refute_style())
            .expect_no_offenses("obj.assert_not(false)\n");
    }

    #[test]
    fn refute_style_does_not_flag_comment_or_string() {
        test::<RefuteMethods>()
            .with_options(&refute_style())
            .expect_no_offenses("# assert_not false\nrefute x\n");
        test::<RefuteMethods>()
            .with_options(&refute_style())
            .expect_no_offenses("puts \"assert_not false\"\n");
    }

    // === autocorrect fixpoint ===

    #[test]
    fn correction_reaches_fixpoint_by_default() {
        // Corrected output uses the preferred spelling and is clean.
        test::<RefuteMethods>().expect_correction(
            indoc! {r#"
                refute_equal(a, b)
                ^^^^^^^^^^^^ Prefer `assert_not_equal` over `refute_equal`.
            "#},
            "assert_not_equal(a, b)\n",
        );
        test::<RefuteMethods>().expect_no_offenses("assert_not_equal(a, b)\n");
    }
}
murphy_plugin_api::submit_cop!(RefuteMethods);
