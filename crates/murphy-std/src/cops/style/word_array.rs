//! `Style/WordArray` — enforces `%w[]` notation for arrays of plain words.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/WordArray
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: only "percent" (default) is implemented; "brackets" style
//!   (flag %w[] arrays) is not yet implemented.
//!   MinSize: implemented (default 2).
//!   WordRegex: not configurable; Murphy uses alphanumeric + underscore chars
//!   which is a subset of RuboCop's default [\p{Word}\n\t]+. Arrays with
//!   tab/newline in element values are not flagged (acceptable conservative gap).
//!   Already-%w arrays are skipped (idempotency guard via raw_source prefix).
//!   Autocorrect: single whole-node emit_edit (structural rewrite, not surgical).
//! ```
//!
//! Subscribes to `NodeKind::Array` nodes. When all children are plain `Str`
//! nodes whose values contain only word characters (alphanumeric + underscore),
//! no backslash escapes, and the array has at least `MinSize` elements
//! (default 2), the cop recommends switching to `%w[word1 word2]` notation.
//!
//! ## Guard
//!
//! Arrays already written as `%w[...]` or `%W[...]` parse as `Array(Str*)`
//! nodes too — the guard reads the raw source and skips if it starts with
//! `%w` or `%W`.
//!
//! ## Autocorrect
//!
//! Single whole-node `emit_edit` replacing the bracket array with
//! `%w[word1 word2 ...]`. This is a structural rewrite (bracket array →
//! percent literal), so whole-node replacement is the appropriate form per
//! `.claude/rules/autocorrect-pattern.md`.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct WordArray;

/// Cop options for [`WordArray`].
#[derive(CopOptions)]
pub struct WordArrayOptions {
    #[option(
        name = "MinSize",
        default = 2,
        description = "Minimum number of elements to trigger the cop."
    )]
    pub min_size: i64,
}

#[cop(
    name = "Style/WordArray",
    description = "Use `%w` or `%W` for an array of words.",
    default_severity = "warning",
    default_enabled = true,
    options = WordArrayOptions
)]
impl WordArray {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<WordArrayOptions>();

        let children = cx.array_elements(node);

        // Minimum size guard.
        if children.len() < opts.min_size as usize {
            return;
        }

        // Already a percent-word array? Skip (idempotency guard).
        let array_src = cx.raw_source(cx.range(node));
        if array_src.starts_with("%w") || array_src.starts_with("%W") {
            return;
        }

        // All children must be plain Str (no Dstr, Int, Sym, etc.).
        // Collect string values while checking.
        let mut words: Vec<&str> = Vec::with_capacity(children.len());
        for &child in children {
            let NodeKind::Str(string_id) = *cx.kind(child) else {
                return;
            };
            let value = cx.string_str(string_id);

            // Word-safe: only word characters (alphanumeric + underscore).
            if !is_word_safe(value) {
                return;
            }

            // No backslash escapes in raw source (conservative check).
            let child_src = cx.raw_source(cx.range(child));
            if child_src.contains('\\') {
                return;
            }

            words.push(value);
        }

        // Emit offense on the entire array range.
        cx.emit_offense(
            cx.range(node),
            "Use `%w` or `%W` for an array of words.",
            None,
        );

        // Autocorrect: replace with %w[word1 word2 ...].
        let replacement = build_percent_word(&words);
        cx.emit_edit(cx.range(node), &replacement);
    }
}

/// Returns `true` when the string value contains only word characters
/// (alphanumeric + underscore). Empty strings are not word-safe
/// (they cannot be expressed as a `%w` element without ambiguity).
fn is_word_safe(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    value.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Build the `%w[word1 word2 ...]` literal replacement from a slice of word
/// values.
fn build_percent_word(words: &[&str]) -> String {
    let mut out = String::from("%w[");
    for (i, word) in words.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(word);
    }
    out.push(']');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- is_word_safe unit tests ---

    #[test]
    fn word_safe_ascii_word() {
        assert!(is_word_safe("hello"));
    }

    #[test]
    fn word_safe_with_underscore() {
        assert!(is_word_safe("foo_bar"));
    }

    #[test]
    fn word_safe_with_digits() {
        assert!(is_word_safe("foo123"));
    }

    #[test]
    fn word_not_safe_with_space() {
        assert!(!is_word_safe("foo bar"));
    }

    #[test]
    fn word_not_safe_with_hyphen() {
        assert!(!is_word_safe("foo-bar"));
    }

    #[test]
    fn word_not_safe_empty() {
        assert!(!is_word_safe(""));
    }

    // --- build_percent_word ---

    #[test]
    fn build_single_word() {
        assert_eq!(build_percent_word(&["foo"]), "%w[foo]");
    }

    #[test]
    fn build_multiple_words() {
        assert_eq!(
            build_percent_word(&["foo", "bar", "baz"]),
            "%w[foo bar baz]"
        );
    }

    // --- Offense tests ---

    #[test]
    fn flags_bracketed_word_array_default() {
        test::<WordArray>().expect_offense(indoc! {r#"
            x = ['foo', 'bar', 'baz']
                ^^^^^^^^^^^^^^^^^^^^^ Use `%w` or `%W` for an array of words.
        "#});
    }

    #[test]
    fn flags_two_element_array_at_min_size() {
        test::<WordArray>().expect_offense(indoc! {r#"
            x = ['foo', 'bar']
                ^^^^^^^^^^^^^^ Use `%w` or `%W` for an array of words.
        "#});
    }

    #[test]
    fn no_offense_single_element_below_min_size() {
        test::<WordArray>().expect_no_offenses("x = ['foo']\n");
    }

    #[test]
    fn no_offense_already_percent_w() {
        test::<WordArray>().expect_no_offenses("x = %w[foo bar baz]\n");
    }

    #[test]
    fn no_offense_already_percent_w_upper() {
        test::<WordArray>().expect_no_offenses("x = %W[foo bar baz]\n");
    }

    #[test]
    fn no_offense_with_space_in_element() {
        test::<WordArray>().expect_no_offenses("x = ['foo bar', 'baz']\n");
    }

    #[test]
    fn no_offense_with_hyphen_in_element() {
        test::<WordArray>().expect_no_offenses("x = ['foo-bar', 'baz']\n");
    }

    #[test]
    fn no_offense_with_dstr_element() {
        test::<WordArray>().expect_no_offenses("x = [\"foo#{bar}\", 'baz']\n");
    }

    #[test]
    fn no_offense_with_integer_element() {
        test::<WordArray>().expect_no_offenses("x = ['foo', 1]\n");
    }

    #[test]
    fn no_offense_with_empty_string() {
        test::<WordArray>().expect_no_offenses("x = ['', 'foo']\n");
    }

    #[test]
    fn no_offense_with_backslash_escape() {
        test::<WordArray>().expect_no_offenses("x = [\"foo\\n\", 'bar']\n");
    }

    // --- Autocorrect tests ---

    #[test]
    fn corrects_bracketed_to_percent_w() {
        test::<WordArray>().expect_correction(
            indoc! {r#"
                x = ['foo', 'bar', 'baz']
                    ^^^^^^^^^^^^^^^^^^^^^ Use `%w` or `%W` for an array of words.
            "#},
            "x = %w[foo bar baz]\n",
        );
    }

    #[test]
    fn corrects_two_element_array() {
        test::<WordArray>().expect_correction(
            indoc! {r#"
                x = ['foo', 'bar']
                    ^^^^^^^^^^^^^^ Use `%w` or `%W` for an array of words.
            "#},
            "x = %w[foo bar]\n",
        );
    }

    #[test]
    fn corrects_double_quoted_strings() {
        test::<WordArray>().expect_correction(
            indoc! {r#"
                x = ["foo", "bar", "baz"]
                    ^^^^^^^^^^^^^^^^^^^^^ Use `%w` or `%W` for an array of words.
            "#},
            "x = %w[foo bar baz]\n",
        );
    }

    // --- MinSize config tests ---

    #[test]
    fn min_size_three_does_not_flag_two_element_array() {
        test::<WordArray>()
            .with_options(&WordArrayOptions { min_size: 3 })
            .expect_no_offenses("x = ['foo', 'bar']\n");
    }

    #[test]
    fn min_size_three_flags_three_element_array() {
        test::<WordArray>()
            .with_options(&WordArrayOptions { min_size: 3 })
            .expect_offense(indoc! {r#"
                x = ['foo', 'bar', 'baz']
                    ^^^^^^^^^^^^^^^^^^^^^ Use `%w` or `%W` for an array of words.
            "#});
    }

    #[test]
    fn min_size_one_flags_single_element_array() {
        test::<WordArray>()
            .with_options(&WordArrayOptions { min_size: 1 })
            .expect_offense(indoc! {r#"
                x = ['foo']
                    ^^^^^^^ Use `%w` or `%W` for an array of words.
            "#});
    }

    // --- Config JSON tests ---

    #[test]
    fn min_size_from_config_json() {
        use murphy_plugin_api::CopOptions;
        let opts = WordArrayOptions::from_config_json(br#"{"MinSize": 3}"#).expect("valid config");
        assert_eq!(opts.min_size, 3);
    }

    #[test]
    fn min_size_default_is_two() {
        let opts = WordArrayOptions::default();
        assert_eq!(opts.min_size, 2);
    }
}
murphy_plugin_api::submit_cop!(WordArray);
