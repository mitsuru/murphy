//! `Style/PerlBackrefs` — avoid Perl-style regex back references.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/PerlBackrefs
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags Perl-style regex back references ($1, $2, ..., $&, $`, $', $+,
//!   and their English equivalents $MATCH, $PREMATCH, $POSTMATCH,
//!   $LAST_PAREN_MATCH) and suggests using Regexp.last_match instead.
//!
//!   Handled:
//!     nth_ref ($1, $2, ...) → Regexp.last_match(N)
//!     back_ref ($&, $`, $', $+) → Regexp.last_match(0/.pre_match/.post_match/-1)
//!     gvar ($MATCH, $PREMATCH, $POSTMATCH, $LAST_PAREN_MATCH) → same as above
//!
//!   Gaps vs RuboCop:
//!     - constant_prefix: RuboCop prefixes `::` when inside a class/module body
//!       (requires ancestor walk through class/module nodes to detect). Murphy
//!       omits the `::` prefix; the autocorrect replacement is always unqualified.
//!     - derived_from_braceless_interpolation?: RuboCop wraps the replacement in
//!       `{}` when the node's parent is dstr/regexp/xstr (braceless interpolation).
//!       Murphy does not detect the parent context, so the replacement may produce
//!       invalid Ruby in rare braceless-interpolation cases. Both gaps require
//!       parent/ancestor context that is not exposed via the ABI boundary for
//!       autocorrect generation.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG_FORMAT: &str = "Prefer `{preferred}` over `{original}`.";

#[derive(Default)]
pub struct PerlBackrefs;

#[cop(
    name = "Style/PerlBackrefs",
    description = "Avoid Perl-style regex back references.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl PerlBackrefs {
    #[on_node(kind = "nth_ref")]
    fn check_nth_ref(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::NthRef(n) = *cx.kind(node) else {
            return;
        };
        let original = format!("${n}");
        let preferred = format!("Regexp.last_match({n})");
        let msg = MSG_FORMAT
            .replace("{preferred}", &preferred)
            .replace("{original}", &original);
        cx.emit_offense(cx.range(node), &msg, None);
        cx.emit_edit(cx.range(node), &preferred);
    }

    #[on_node(kind = "back_ref")]
    fn check_back_ref(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::BackRef(sym) = *cx.kind(node) else {
            return;
        };
        let var_name = cx.symbol_str(sym);
        if let Some(preferred) = preferred_for_var(var_name) {
            let msg = MSG_FORMAT
                .replace("{preferred}", preferred)
                .replace("{original}", var_name);
            cx.emit_offense(cx.range(node), &msg, None);
            cx.emit_edit(cx.range(node), preferred);
        }
    }

    #[on_node(kind = "gvar")]
    fn check_gvar(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Gvar(sym) = *cx.kind(node) else {
            return;
        };
        let var_name = cx.symbol_str(sym);
        if let Some(preferred) = preferred_for_var(var_name) {
            let msg = MSG_FORMAT
                .replace("{preferred}", preferred)
                .replace("{original}", var_name);
            cx.emit_offense(cx.range(node), &msg, None);
            cx.emit_edit(cx.range(node), preferred);
        }
    }
}

/// Returns the preferred `Regexp.last_match` expression for a given Perl-style
/// back reference variable name, or `None` if the variable is not a known
/// back reference (e.g. `$stdout`, `$~`, `$LOAD_PATH`).
fn preferred_for_var(var_name: &str) -> Option<&'static str> {
    match var_name {
        "$&" | "$MATCH" => Some("Regexp.last_match(0)"),
        "$`" | "$PREMATCH" => Some("Regexp.last_match.pre_match"),
        "$'" | "$POSTMATCH" => Some("Regexp.last_match.post_match"),
        "$+" | "$LAST_PAREN_MATCH" => Some("Regexp.last_match(-1)"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::PerlBackrefs;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- nth_ref ($1, $2, ...) ---

    #[test]
    fn flags_dollar_one() {
        test::<PerlBackrefs>().expect_offense(indoc! {"
            puts $1
                 ^^ Prefer `Regexp.last_match(1)` over `$1`.
        "});
    }

    #[test]
    fn corrects_dollar_one() {
        test::<PerlBackrefs>().expect_correction(
            indoc! {"
                puts $1
                     ^^ Prefer `Regexp.last_match(1)` over `$1`.
            "},
            "puts Regexp.last_match(1)\n",
        );
    }

    #[test]
    fn flags_dollar_two() {
        test::<PerlBackrefs>().expect_offense(indoc! {"
            puts $2
                 ^^ Prefer `Regexp.last_match(2)` over `$2`.
        "});
    }

    #[test]
    fn corrects_dollar_two() {
        test::<PerlBackrefs>().expect_correction(
            indoc! {"
                puts $2
                     ^^ Prefer `Regexp.last_match(2)` over `$2`.
            "},
            "puts Regexp.last_match(2)\n",
        );
    }

    // --- back_ref ($&, $`, $', $+) ---

    #[test]
    fn flags_dollar_ampersand() {
        test::<PerlBackrefs>().expect_offense(indoc! {"
            puts $&
                 ^^ Prefer `Regexp.last_match(0)` over `$&`.
        "});
    }

    #[test]
    fn corrects_dollar_ampersand() {
        test::<PerlBackrefs>().expect_correction(
            indoc! {"
                puts $&
                     ^^ Prefer `Regexp.last_match(0)` over `$&`.
            "},
            "puts Regexp.last_match(0)\n",
        );
    }

    #[test]
    fn flags_dollar_backtick() {
        test::<PerlBackrefs>().expect_offense(indoc! {"
            puts $`
                 ^^ Prefer `Regexp.last_match.pre_match` over `$``.
        "});
    }

    #[test]
    fn flags_dollar_single_quote() {
        test::<PerlBackrefs>().expect_offense(indoc! {r#"
            puts $'
                 ^^ Prefer `Regexp.last_match.post_match` over `$'`.
        "#});
    }

    #[test]
    fn flags_dollar_plus() {
        test::<PerlBackrefs>().expect_offense(indoc! {"
            puts $+
                 ^^ Prefer `Regexp.last_match(-1)` over `$+`.
        "});
    }

    // --- gvar English names ---

    #[test]
    fn flags_match_english() {
        test::<PerlBackrefs>().expect_offense(indoc! {"
            puts $MATCH
                 ^^^^^^ Prefer `Regexp.last_match(0)` over `$MATCH`.
        "});
    }

    #[test]
    fn corrects_match_english() {
        test::<PerlBackrefs>().expect_correction(
            indoc! {"
                puts $MATCH
                     ^^^^^^ Prefer `Regexp.last_match(0)` over `$MATCH`.
            "},
            "puts Regexp.last_match(0)\n",
        );
    }

    #[test]
    fn flags_prematch_english() {
        test::<PerlBackrefs>().expect_offense(indoc! {"
            puts $PREMATCH
                 ^^^^^^^^^ Prefer `Regexp.last_match.pre_match` over `$PREMATCH`.
        "});
    }

    #[test]
    fn flags_postmatch_english() {
        test::<PerlBackrefs>().expect_offense(indoc! {"
            puts $POSTMATCH
                 ^^^^^^^^^^ Prefer `Regexp.last_match.post_match` over `$POSTMATCH`.
        "});
    }

    #[test]
    fn flags_last_paren_match_english() {
        test::<PerlBackrefs>().expect_offense(indoc! {"
            puts $LAST_PAREN_MATCH
                 ^^^^^^^^^^^^^^^^^ Prefer `Regexp.last_match(-1)` over `$LAST_PAREN_MATCH`.
        "});
    }

    // --- no offense for unrelated gvars ---

    #[test]
    fn accepts_dollar_tilde() {
        // $~ is SpecialGlobalVars territory, not PerlBackrefs
        test::<PerlBackrefs>().expect_no_offenses("puts $~\n");
    }

    #[test]
    fn accepts_dollar_stdout() {
        test::<PerlBackrefs>().expect_no_offenses("puts $stdout\n");
    }

    #[test]
    fn accepts_dollar_load_path() {
        test::<PerlBackrefs>().expect_no_offenses("$LOAD_PATH << 'lib'\n");
    }
}
murphy_plugin_api::submit_cop!(PerlBackrefs);
