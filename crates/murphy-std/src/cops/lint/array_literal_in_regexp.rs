//! `Lint/ArrayLiteralInRegexp` — flags an array literal interpolated inside a regexp.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ArrayLiteralInRegexp
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `#{array}` interpolation whose final node is an array literal
//!   inside a regexp. Arrays of single-character literals autocorrect to a
//!   character class (`[abc]`); multi-character literal arrays autocorrect to
//!   alternation (`(?:foo|bar)`); arrays containing non-literal values are
//!   reported without autocorrect. `Regexp.escape` is reimplemented to match
//!   Ruby's escaping for the replacement payload.
//! ```
//!
//! ## Matched shapes
//! - `/#{%w[a b c]}/` — array of single-char string literals → character class
//! - `/#{%w[foo bar]}/` — array of multi-char literals → alternation
//! - `/#{[foo, bar]}/` — array of non-literal values → no autocorrect
//!
//! ## Why this shape
//!
//! Interpolating an array into a regexp inserts the array's `to_s`
//! representation (e.g. `["a", "b"]` becomes the literal text `abc` separated
//! by nothing useful), which is almost never what the author intended. The cop
//! steers toward an explicit character class or alternation.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG_CHARACTER_CLASS: &str =
    "Use a character class instead of interpolating an array in a regexp.";
const MSG_ALTERNATION: &str = "Use alternation instead of interpolating an array in a regexp.";
const MSG_UNKNOWN: &str =
    "Use alternation or a character class instead of interpolating an array in a regexp.";

#[derive(Default)]
pub struct ArrayLiteralInRegexp;

#[cop(
    name = "Lint/ArrayLiteralInRegexp",
    description = "Checks for an array literal interpolated inside a regexp.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ArrayLiteralInRegexp {
    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Regexp { parts, .. } = *cx.kind(node) else {
            return;
        };
        for &part in cx.list(parts) {
            // An interpolation `#{...}` is a `Begin` node among the regexp parts.
            let NodeKind::Begin(inner) = *cx.kind(part) else {
                continue;
            };
            let Some(&final_node) = cx.list(inner).last() else {
                continue;
            };
            let NodeKind::Array(elems) = *cx.kind(final_node) else {
                continue;
            };
            let values = cx.list(elems);

            if all_literal_values(values, cx) {
                register_array_of_literal_values(part, values, cx);
            } else {
                // Non-literal array: RuboCop offenses on the interpolation
                // `begin_node` (covering `#{...}`), no autocorrect.
                cx.emit_offense(cx.range(part), MSG_UNKNOWN, None);
            }
        }
    }
}

/// `LITERAL_TYPES = %i[str sym int float true false nil]`.
fn all_literal_values(values: &[NodeId], cx: &Cx<'_>) -> bool {
    values.iter().all(|&value| {
        matches!(
            *cx.kind(value),
            NodeKind::Str(..)
                | NodeKind::Sym(..)
                | NodeKind::Int(..)
                | NodeKind::Float(..)
                | NodeKind::True_
                | NodeKind::False_
                | NodeKind::Nil
        )
    })
}

fn register_array_of_literal_values(begin_node: NodeId, values: &[NodeId], cx: &Cx<'_>) {
    let str_values: Vec<String> = values.iter().map(|&v| literal_to_string(v, cx)).collect();

    // An empty array would naively become `[]` (an invalid empty character
    // class) or `(?:)`. RuboCop emits the former, but Murphy must never produce
    // an autocorrect that fails to parse, so we still report the offense (it is
    // an array literal in a regexp) but skip the correction.
    if str_values.is_empty() {
        cx.emit_offense(cx.range(begin_node), MSG_CHARACTER_CLASS, None);
        return;
    }

    let (message, replacement) = if str_values.iter().all(|v| v.chars().count() == 1) {
        let escaped: String = str_values.iter().map(|v| regexp_escape(v)).collect();
        (MSG_CHARACTER_CLASS, format!("[{escaped}]"))
    } else {
        let escaped: Vec<String> = str_values.iter().map(|v| regexp_escape(v)).collect();
        (MSG_ALTERNATION, format!("(?:{})", escaped.join("|")))
    };

    cx.emit_offense(cx.range(begin_node), message, None);
    cx.emit_edit(cx.range(begin_node), &replacement);
}

/// RuboCop maps each value via `value.respond_to?(:value) ? value.value : value.source`.
/// For str/sym/int/float the parsed value is used; for true/false/nil the
/// source text (`"true"`, etc.) is used.
fn literal_to_string(node: NodeId, cx: &Cx<'_>) -> String {
    match *cx.kind(node) {
        NodeKind::Str(id) => cx.string_str(id).to_string(),
        NodeKind::Sym(id) => cx.symbol_str(id).to_string(),
        // For int/float the parsed `.value` stringifies to the same text as
        // the source for the literals this cop handles; using source avoids
        // float-formatting drift (e.g. `1.0` vs `1`).
        // true/false/nil have no `.value`; RuboCop also falls back to `.source`.
        _ => cx.raw_source(cx.range(node)).to_string(),
    }
}

/// Reimplements Ruby's `Regexp.escape`: escapes regexp metacharacters and
/// control characters in the same way as the stdlib method.
fn regexp_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            // Metacharacters escaped with a backslash.
            '.' | '\\' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|'
            | '#' | '-' => {
                out.push('\\');
                out.push(c);
            }
            ' ' => out.push_str("\\ "),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0C}' => out.push_str("\\f"),
            '\u{0B}' => out.push_str("\\v"),
            _ => out.push(c),
        }
    }
    out
}

murphy_plugin_api::submit_cop!(ArrayLiteralInRegexp);

#[cfg(test)]
mod tests {
    use super::ArrayLiteralInRegexp;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_single_char_string_array_as_character_class() {
        test::<ArrayLiteralInRegexp>()
            .expect_offense(indoc! {r#"
                /#{%w[a b c]}/
                 ^^^^^^^^^^^^ Use a character class instead of interpolating an array in a regexp.
            "#})
            .expect_correction(
                indoc! {r#"
                    /#{%w[a b c]}/
                     ^^^^^^^^^^^^ Use a character class instead of interpolating an array in a regexp.
                "#},
                "/[abc]/\n",
            );
    }

    #[test]
    fn flags_multi_char_string_array_as_alternation() {
        test::<ArrayLiteralInRegexp>()
            .expect_offense(indoc! {r#"
                /#{%w[foo bar]}/
                 ^^^^^^^^^^^^^^ Use alternation instead of interpolating an array in a regexp.
            "#})
            .expect_correction(
                indoc! {r#"
                    /#{%w[foo bar]}/
                     ^^^^^^^^^^^^^^ Use alternation instead of interpolating an array in a regexp.
                "#},
                "/(?:foo|bar)/\n",
            );
    }

    #[test]
    fn flags_non_literal_array_without_correction() {
        test::<ArrayLiteralInRegexp>().expect_offense(indoc! {r#"
            /#{[foo, bar]}/
             ^^^^^^^^^^^^^ Use alternation or a character class instead of interpolating an array in a regexp.
        "#});
    }

    #[test]
    fn flags_symbol_array_as_character_class() {
        test::<ArrayLiteralInRegexp>()
            .expect_offense(indoc! {r#"
                /#{%i[a b c]}/
                 ^^^^^^^^^^^^ Use a character class instead of interpolating an array in a regexp.
            "#})
            .expect_correction(
                indoc! {r#"
                    /#{%i[a b c]}/
                     ^^^^^^^^^^^^ Use a character class instead of interpolating an array in a regexp.
                "#},
                "/[abc]/\n",
            );
    }

    #[test]
    fn flags_int_array_as_character_class() {
        test::<ArrayLiteralInRegexp>()
            .expect_offense(indoc! {r#"
                /#{[1, 2, 3]}/
                 ^^^^^^^^^^^^ Use a character class instead of interpolating an array in a regexp.
            "#})
            .expect_correction(
                indoc! {r#"
                    /#{[1, 2, 3]}/
                     ^^^^^^^^^^^^ Use a character class instead of interpolating an array in a regexp.
                "#},
                "/[123]/\n",
            );
    }

    #[test]
    fn escapes_metacharacters_in_character_class() {
        test::<ArrayLiteralInRegexp>()
            .expect_offense(indoc! {r#"
                /#{%w[^ - $ |]}/
                 ^^^^^^^^^^^^^^ Use a character class instead of interpolating an array in a regexp.
            "#})
            .expect_correction(
                indoc! {r#"
                    /#{%w[^ - $ |]}/
                     ^^^^^^^^^^^^^^ Use a character class instead of interpolating an array in a regexp.
                "#},
                "/[\\^\\-\\$\\|]/\n",
            );
    }

    #[test]
    fn flags_float_array_as_alternation_with_escaped_dots() {
        test::<ArrayLiteralInRegexp>()
            .expect_offense(indoc! {r#"
                /#{[1.0, 2.5, 4.7]}/
                 ^^^^^^^^^^^^^^^^^^ Use alternation instead of interpolating an array in a regexp.
            "#})
            .expect_correction(
                indoc! {r#"
                    /#{[1.0, 2.5, 4.7]}/
                     ^^^^^^^^^^^^^^^^^^ Use alternation instead of interpolating an array in a regexp.
                "#},
                "/(?:1\\.0|2\\.5|4\\.7)/\n",
            );
    }

    #[test]
    fn flags_boolean_nil_array_as_alternation() {
        test::<ArrayLiteralInRegexp>()
            .expect_offense(indoc! {r#"
                /#{[true, false, nil]}/
                 ^^^^^^^^^^^^^^^^^^^^^ Use alternation instead of interpolating an array in a regexp.
            "#})
            .expect_correction(
                indoc! {r#"
                    /#{[true, false, nil]}/
                     ^^^^^^^^^^^^^^^^^^^^^ Use alternation instead of interpolating an array in a regexp.
                "#},
                "/(?:true|false|nil)/\n",
            );
    }

    #[test]
    fn flags_range_element_array_without_correction() {
        // A range element is not a literal type → no autocorrect.
        test::<ArrayLiteralInRegexp>().expect_offense(indoc! {r#"
            /#{[1..2]}/
             ^^^^^^^^^ Use alternation or a character class instead of interpolating an array in a regexp.
        "#});
    }

    #[test]
    fn flags_nested_regexp_element_array_without_correction() {
        test::<ArrayLiteralInRegexp>().expect_offense(indoc! {r#"
            /#{[/abc/]}/
             ^^^^^^^^^^ Use alternation or a character class instead of interpolating an array in a regexp.
        "#});
    }

    #[test]
    fn flags_empty_array_without_correction() {
        // `[]` would become `/[]/` (invalid empty character class); report the
        // offense but skip the unsafe autocorrect.
        test::<ArrayLiteralInRegexp>().expect_offense(indoc! {r#"
            /#{[]}/
             ^^^^^ Use a character class instead of interpolating an array in a regexp.
        "#});
    }

    #[test]
    fn flags_empty_percent_w_array_without_correction() {
        test::<ArrayLiteralInRegexp>().expect_offense(indoc! {r#"
            /#{%w[]}/
             ^^^^^^^ Use a character class instead of interpolating an array in a regexp.
        "#});
    }

    #[test]
    fn accepts_regexp_without_array_interpolation() {
        test::<ArrayLiteralInRegexp>().expect_no_offenses("/#{foo}/\n");
    }

    #[test]
    fn accepts_plain_regexp() {
        test::<ArrayLiteralInRegexp>().expect_no_offenses("/abc/\n");
    }

    #[test]
    fn accepts_array_outside_regexp() {
        test::<ArrayLiteralInRegexp>().expect_no_offenses("x = %w[a b c]\n");
    }
}
