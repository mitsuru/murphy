//! `Style/CharacterLiteral` — avoid character literals (`?x`); use string
//! literals instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CharacterLiteral
//! upstream_version_checked: 1.81.6
//! version_added: "0.9"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full parity with RuboCop. Character literals of source length 2-3 chars
//!   are flagged. Control/meta escapes (length >= 4) are not flagged.
//!   Autocorrects to single-quoted string for plain chars, double-quoted for
//!   escape sequences and single-quote characters. Str fragments inside dstr
//!   (interpolated strings) and regexp are skipped (same logic as RuboCop's
//!   StringHelp#on_str guard and on_regexp ignore).
//! ```
//!
//! ## Matched shapes
//!
//! Any `str` node whose raw source starts with `?` and whose source character
//! count is 2 or 3. This matches:
//!
//! - `?x` (single character — source length 2) → `'x'`
//! - `?\n` (simple escape sequence — source length 3) → `"\n"`
//! - `?'` (single-quote char — source length 2) → `"'"`
//!
//! Control/meta escapes like `?\C-a` (source length > 3 chars) are **not**
//! flagged — they have no concise alternative.
//!
//! ## Why this shape
//!
//! RuboCop's `character_literal?` predicate checks `loc(:begin)` present
//! (the `?` token). Murphy identifies character literals by raw source: the
//! node is a `str` dispatched by the standard `on_str` mechanism, and the
//! source starts with `?`. We additionally guard against `str` fragments
//! inside `dstr` (interpolated string) or `regexp` nodes, which can also
//! have source starting with `?` (e.g. `"a#{b}?c"` → `str "?c"`).
//!
//! ## Autocorrect
//!
//! Mirrors RuboCop's `autocorrect` method exactly:
//!
//! - `?'` (body is `'`) → `"'"` (must use double quotes)
//! - `?\n` (body length 2 chars = an escape) → `"\n"` (double quotes)
//! - `?x` (body length 1 char = plain) → `'x'` (single quotes)

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Do not use the character literal - use string literal instead.";

#[derive(Default)]
pub struct CharacterLiteral;

#[cop(
    name = "Style/CharacterLiteral",
    description = "Do not use character literals; use string literals instead.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl CharacterLiteral {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip `str` fragments that are parts of a `dstr` (interpolated string)
        // or a `regexp`. These can have source starting with `?` (e.g. the
        // fragment `"?c"` inside `"a#{b}?c"`), but they are not character
        // literals — they are raw source fragments without a `?` opener token.
        if let Some(parent_id) = cx.parent(node).get() {
            match cx.kind(parent_id) {
                NodeKind::Dstr(_) | NodeKind::Regexp { .. } => return,
                _ => {}
            }
        }

        let range = cx.range(node);
        let src = cx.raw_source(range);

        // A character literal's source starts with `?`.
        if !src.starts_with('?') {
            return;
        }

        // Source char count must be 2 or 3 — matches RuboCop's
        // `node.source.size.between?(2, 3)`. This excludes control/meta
        // escapes like `?\C-\M-d` (source length > 3 chars).
        let char_count = src.chars().count();
        if !(2..=3).contains(&char_count) {
            return;
        }

        cx.emit_offense(range, MSG, None);

        // Autocorrect: mirror RuboCop's `autocorrect` method.
        // `body` = source after the leading `?`.
        let body = &src[1..];
        let replacement = if body == "'" {
            // `?'` → must use double quotes because the body is a single-quote.
            r#""'""#.to_string()
        } else if body.chars().count() == 2 {
            // Escape sequence like `?\n`, `?\t`, `?\"` → double quotes.
            format!("\"{}\"", body)
        } else {
            // Plain single character → single quotes.
            format!("'{}'", body)
        };

        cx.emit_edit(range, &replacement);
    }
}

#[cfg(test)]
mod tests {
    use super::CharacterLiteral;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- positive cases (offense + autocorrect) --------------------------------

    #[test]
    fn flags_simple_char_literal() {
        test::<CharacterLiteral>().expect_offense(indoc! {r#"
            x = ?x
                ^^ Do not use the character literal - use string literal instead.
        "#});
    }

    #[test]
    fn autocorrects_simple_char_to_single_quoted() {
        test::<CharacterLiteral>().expect_correction(
            indoc! {r#"
                x = ?x
                    ^^ Do not use the character literal - use string literal instead.
            "#},
            "x = 'x'\n",
        );
    }

    #[test]
    fn flags_newline_escape() {
        test::<CharacterLiteral>().expect_offense(indoc! {r#"
            x = ?\n
                ^^^ Do not use the character literal - use string literal instead.
        "#});
    }

    #[test]
    fn autocorrects_newline_escape_to_double_quoted() {
        test::<CharacterLiteral>().expect_correction(
            indoc! {r#"
                x = ?\n
                    ^^^ Do not use the character literal - use string literal instead.
            "#},
            "x = \"\\n\"\n",
        );
    }

    #[test]
    fn flags_tab_escape() {
        test::<CharacterLiteral>().expect_offense(indoc! {r#"
            x = ?\t
                ^^^ Do not use the character literal - use string literal instead.
        "#});
    }

    #[test]
    fn autocorrects_tab_escape_to_double_quoted() {
        test::<CharacterLiteral>().expect_correction(
            indoc! {r#"
                x = ?\t
                    ^^^ Do not use the character literal - use string literal instead.
            "#},
            "x = \"\\t\"\n",
        );
    }

    #[test]
    fn flags_single_quote_char() {
        // `?'` must correct to `"'"` (double-quoted), not `'''`.
        test::<CharacterLiteral>().expect_offense(indoc! {"
            x = ?'
                ^^ Do not use the character literal - use string literal instead.
        "});
    }

    #[test]
    fn autocorrects_single_quote_char_to_double_quoted() {
        test::<CharacterLiteral>().expect_correction(
            indoc! {"
                x = ?'
                    ^^ Do not use the character literal - use string literal instead.
            "},
            "x = \"'\"\n",
        );
    }

    // --- negative cases (no offense) -------------------------------------------

    #[test]
    fn accepts_control_meta_escape() {
        // `?\C-\M-d` has source length > 3 chars — not flagged.
        test::<CharacterLiteral>().expect_no_offenses("x = ?\\C-\\M-d\n");
    }

    #[test]
    fn accepts_plain_single_quoted_string() {
        test::<CharacterLiteral>().expect_no_offenses("x = 'hello'\n");
    }

    #[test]
    fn accepts_plain_double_quoted_string() {
        test::<CharacterLiteral>().expect_no_offenses("x = \"hello\"\n");
    }

    #[test]
    fn accepts_question_mark_fragment_in_interpolated_string() {
        // `"a#{b}?c"` produces a `str "?c"` fragment inside a `dstr` — must NOT flag.
        test::<CharacterLiteral>().expect_no_offenses("x = \"a#{b}?c\"\n");
    }

    #[test]
    fn accepts_question_mark_in_regexp() {
        // `/a?c/` produces a `str "a?c"` inside a `regexp` — must NOT flag.
        test::<CharacterLiteral>().expect_no_offenses("x = /a?c/\n");
    }

    // --- additional char variants ----------------------------------------------

    #[test]
    fn flags_digit_char() {
        test::<CharacterLiteral>().expect_offense(indoc! {r#"
            x = ?0
                ^^ Do not use the character literal - use string literal instead.
        "#});
    }

    #[test]
    fn autocorrects_digit_char_to_single_quoted() {
        test::<CharacterLiteral>().expect_correction(
            indoc! {r#"
                x = ?0
                    ^^ Do not use the character literal - use string literal instead.
            "#},
            "x = '0'\n",
        );
    }
}

murphy_plugin_api::submit_cop!(CharacterLiteral);
