//! `Style/SymbolArray` — prefer `%i[]` for arrays of plain symbols.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SymbolArray
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-y7ax
//! notes: >
//!   v1 implements `EnforcedStyle: percent` (the default) — flag bracket-style
//!   symbol arrays and suggest `%i[]`.  The `brackets` style (flag percent
//!   literals, prefer bracket form) is deferred to murphy-y7ax.  MinSize is
//!   implemented (default 2, matching RuboCop).  Complex symbols (spaces,
//!   delimiter chars, dynamic `dsym` elements) are skipped correctly.
//!   Autocorrect replaces the whole array with `%i[sym1 sym2 …]`.
//! ```
//!
//! Dispatches on `NodeKind::Array`.  Flags arrays where every element is a
//! plain `NodeKind::Sym` with a simple identifier name and whose length meets
//! `MinSize`, when the source form uses the bracket syntax `[:a, :b]`.
//!
//! ## Checks
//!
//! An array node is flagged when **all** of the following hold:
//!
//! 1. The source text does **not** already start with `%i` or `%I`
//!    (percent-literal guard — avoids flagging what we'd produce).
//! 2. Every child is `NodeKind::Sym` (no dynamic `dsym` elements).
//! 3. Every symbol's name is a *simple identifier*: starts with `[a-zA-Z_]`,
//!    followed by `[a-zA-Z0-9_]`, optionally ending with `!` or `?`.
//!    Symbols with embedded spaces, quotes, or delimiter chars are skipped.
//! 4. The number of elements ≥ `MinSize` (default 2).
//!
//! ## Autocorrect
//!
//! Whole-node interpolation: collect each symbol's name via `cx.symbol_str`,
//! format as `%i[name1 name2 …]`, and replace the full array range.
//!
//! Per `.claude/rules/autocorrect-pattern.md`: whole-node replacement is the
//! correct form here because the rewrite fundamentally reshapes the AST
//! (strips colons, commas, brackets → percent literal).
//!
//! ## MinSize option
//!
//! Arrays shorter than `MinSize` are not flagged.  Default is 2 (same as
//! RuboCop), meaning single-element arrays `[:a]` are never flagged.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SymbolArray;

/// Cop options for [`SymbolArray`].
#[derive(CopOptions)]
pub struct SymbolArrayOptions {
    #[option(
        name = "MinSize",
        default = 2,
        description = "Minimum array size to trigger the cop."
    )]
    pub min_size: i64,
}

#[cop(
    name = "Style/SymbolArray",
    description = "Use `%i` or `%I` for an array of symbols.",
    default_severity = "warning",
    default_enabled = true,
    options = SymbolArrayOptions,
)]
impl SymbolArray {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        let elements = cx.array_elements(node);

        // Cheap early-exit: empty arrays and arrays whose first element is not
        // a symbol are the common case — skip before loading options.
        if elements.is_empty() {
            return;
        }
        if !matches!(cx.kind(elements[0]), NodeKind::Sym(_)) {
            return;
        }

        let opts = cx.options_or_default::<SymbolArrayOptions>();

        // MinSize guard.
        if elements.len() < opts.min_size as usize {
            return;
        }

        // Percent-literal guard: already `%i[…]` or `%I[…]`.
        let array_src = cx.raw_source(cx.range(node));
        if array_src.starts_with("%i") || array_src.starts_with("%I") {
            return;
        }

        // All elements must be plain Sym nodes with simple identifier names.
        // Non-allocating check first; only build the replacement string if we
        // will actually emit an offense.
        let all_simple = elements.iter().all(|&elem| {
            if let NodeKind::Sym(sym) = *cx.kind(elem) {
                is_simple_identifier(cx.symbol_str(sym))
            } else {
                false
            }
        });
        if !all_simple {
            return;
        }

        let range = cx.range(node);
        cx.emit_offense(range, "Use `%i` or `%I` for an array of symbols.", None);

        // Autocorrect: whole-node replacement with percent literal.
        let mut replacement = String::from("%i[");
        for (i, &elem) in elements.iter().enumerate() {
            if i > 0 {
                replacement.push(' ');
            }
            let NodeKind::Sym(sym) = *cx.kind(elem) else {
                unreachable!("checked above")
            };
            replacement.push_str(cx.symbol_str(sym));
        }
        replacement.push(']');
        cx.emit_edit(range, &replacement);
    }
}

/// Returns `true` when `name` is a simple symbol identifier that can be used
/// bare inside `%i[…]`.
///
/// Accepted: `[a-zA-Z_][a-zA-Z0-9_]*[!?]?`
/// Rejected: names with spaces, quotes, brackets, slashes, or other
/// delimiters that would need quoting or would break `%i` parsing.
fn is_simple_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bytes = name.as_bytes();
    let first = bytes[0];
    if !(first == b'_' || first.is_ascii_alphabetic()) {
        return false;
    }
    // Check optional trailing `!` or `?`.
    let (body, tail) = match bytes.last() {
        Some(b'!' | b'?') if bytes.len() > 1 => (&bytes[1..bytes.len() - 1], true),
        _ => (&bytes[1..], false),
    };
    let _ = tail; // informational only
    body.iter().all(|&b| b == b'_' || b.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::{SymbolArray, SymbolArrayOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- detection -----------------------------------------------------------

    #[test]
    fn flags_bracket_symbol_array() {
        test::<SymbolArray>().expect_offense(indoc! {r#"
            x = [:foo, :bar]
                ^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
        "#});
    }

    #[test]
    fn accepts_percent_literal_already() {
        test::<SymbolArray>().expect_no_offenses("x = %i[foo bar]\n");
    }

    #[test]
    fn accepts_single_element_below_min_size() {
        // Default MinSize = 2; one-element array is not flagged.
        test::<SymbolArray>().expect_no_offenses("x = [:foo]\n");
    }

    #[test]
    fn accepts_array_with_non_sym_element() {
        test::<SymbolArray>().expect_no_offenses("x = [:foo, 1]\n");
    }

    #[test]
    fn accepts_array_with_complex_symbol_name() {
        // Symbol with spaces or special chars — skip.
        test::<SymbolArray>().expect_no_offenses("x = [:\"foo bar\", :baz]\n");
    }

    #[test]
    fn flags_three_symbol_array() {
        test::<SymbolArray>().expect_offense(indoc! {r#"
            x = [:a, :b, :c]
                ^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
        "#});
    }

    #[test]
    fn accepts_array_smaller_than_custom_min_size() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions { min_size: 3 })
            .expect_no_offenses("x = [:foo, :bar]\n");
    }

    #[test]
    fn flags_array_meeting_custom_min_size() {
        test::<SymbolArray>()
            .with_options(&SymbolArrayOptions { min_size: 3 })
            .expect_offense(indoc! {r#"
                x = [:a, :b, :c]
                    ^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
            "#});
    }

    // ---- autocorrect --------------------------------------------------------

    #[test]
    fn autocorrects_bracket_array_to_percent_literal() {
        test::<SymbolArray>().expect_correction(
            indoc! {r#"
                x = [:foo, :bar]
                    ^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
            "#},
            "x = %i[foo bar]\n",
        );
    }

    #[test]
    fn autocorrects_three_symbol_array() {
        test::<SymbolArray>().expect_correction(
            indoc! {r#"
                x = [:a, :b, :c]
                    ^^^^^^^^^^^^ Use `%i` or `%I` for an array of symbols.
            "#},
            "x = %i[a b c]\n",
        );
    }

    #[test]
    fn autocorrect_is_idempotent() {
        // After correction the result should not trigger another offense.
        test::<SymbolArray>().expect_no_offenses("x = %i[foo bar]\n");
    }

    // ---- predicate functions ------------------------------------------------

    #[test]
    fn simple_identifier_accepts_plain_words() {
        use super::is_simple_identifier;
        assert!(is_simple_identifier("foo"));
        assert!(is_simple_identifier("foo_bar"));
        assert!(is_simple_identifier("_private"));
        assert!(is_simple_identifier("FooBar"));
        assert!(is_simple_identifier("foo?"));
        assert!(is_simple_identifier("foo!"));
        assert!(is_simple_identifier("foo_bar?"));
    }

    #[test]
    fn simple_identifier_rejects_special_names() {
        use super::is_simple_identifier;
        assert!(!is_simple_identifier(""));
        assert!(!is_simple_identifier("foo bar"));
        assert!(!is_simple_identifier("1foo"));
        assert!(!is_simple_identifier("foo-bar"));
    }
}
murphy_plugin_api::submit_cop!(SymbolArray);
