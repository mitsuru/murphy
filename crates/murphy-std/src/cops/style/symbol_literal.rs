//! `Style/SymbolLiteral` — use plain symbols instead of string symbols when
//! possible.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SymbolLiteral
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags symbols whose source form uses unnecessary string quotes:
//!   `:"word"` or `:'word'` where the name is a plain word-like identifier
//!   matching `[A-Za-z_]\w*`. Symbols with trailing `!` or `?` (like
//!   `:"foo?"`) are not flagged because the regex used by RuboCop only
//!   matches `\w*` characters before the closing quote. Autocorrects by
//!   stripping the quotes: `:"symbol"` → `:symbol`.
//! ```
//!
//! ## Detection
//!
//! A `Sym` node is flagged when its raw source text matches the pattern
//! `/\A:["'][A-Za-z_]\w*["']\z/` — a colon, then a quote (single or double),
//! then an initial letter or underscore, then zero or more `\w` characters,
//! then the matching (or any) closing quote.
//!
//! Note: RuboCop's regex allows a mismatched quote pair (e.g. `:"foo'`) but
//! this is unusual in practice. We match the same permissive pattern.
//!
//! ## Autocorrect
//!
//! Surgical: the quoted form `:["']name["']` becomes `:name` by deleting the
//! two quote characters (position 1 and last-1 of the source text).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Do not use strings for word-like symbol literals.";

#[derive(Default)]
pub struct SymbolLiteral;

#[cop(
    name = "Style/SymbolLiteral",
    description = "Use plain symbols instead of string symbols when possible.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SymbolLiteral {
    #[on_node(kind = "sym")]
    fn check_sym(&self, node: NodeId, cx: &Cx<'_>) {
        // Only flag `Sym` (plain symbol), not `Dsym` (interpolated symbol).
        if !matches!(cx.kind(node), NodeKind::Sym(_)) {
            return;
        }

        let src = cx.raw_source(cx.range(node));
        let bytes = src.as_bytes();

        // Must start with `:"` or `:'` and be at least 4 bytes: `:`, quote,
        // one word char, closing quote.
        if bytes.len() < 4 {
            return;
        }
        if bytes[0] != b':' {
            return;
        }
        let quote = bytes[1];
        if quote != b'"' && quote != b'\'' {
            return;
        }
        // Must end with a quote character.
        let last = bytes[bytes.len() - 1];
        if last != b'"' && last != b'\'' {
            return;
        }
        // The inner portion (between quotes) must match `[A-Za-z_]\w*`.
        let inner = &bytes[2..bytes.len() - 1];
        if !is_word_like(inner) {
            return;
        }

        let range = cx.range(node);
        cx.emit_offense(range, MSG, None);

        // Autocorrect: delete the opening quote (byte at offset 1)
        // and the closing quote (byte at offset len-1).
        // Two surgical non-overlapping edits.
        let open_quote = Range {
            start: range.start + 1,
            end: range.start + 2,
        };
        let close_quote = Range {
            start: range.end - 1,
            end: range.end,
        };
        cx.emit_edit(open_quote, "");
        cx.emit_edit(close_quote, "");
    }
}

/// Returns `true` when `bytes` matches `[A-Za-z_]\w*` — the inner content of
/// a quoted symbol that can be expressed as a plain symbol literal.
///
/// Note: unlike `SymbolArray`'s `is_simple_identifier`, this does **not**
/// permit trailing `!` or `?` because RuboCop's regex (`\w*`) excludes them.
fn is_word_like(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let first = bytes[0];
    if !(first == b'_' || first.is_ascii_alphabetic()) {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|&b| b == b'_' || b.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::SymbolLiteral;
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- detection -----------------------------------------------------------

    #[test]
    fn flags_double_quoted_symbol() {
        test::<SymbolLiteral>().expect_offense(indoc! {r#"
            x = :"symbol"
                ^^^^^^^^^ Do not use strings for word-like symbol literals.
        "#});
    }

    #[test]
    fn flags_single_quoted_symbol() {
        test::<SymbolLiteral>().expect_offense(indoc! {"
            x = :'symbol'
                ^^^^^^^^^ Do not use strings for word-like symbol literals.
        "});
    }

    #[test]
    fn accepts_plain_symbol() {
        test::<SymbolLiteral>().expect_no_offenses(":symbol\n");
    }

    #[test]
    fn accepts_symbol_with_bang() {
        // :"foo!" is not word-like (trailing `!` is not `\w`).
        test::<SymbolLiteral>().expect_no_offenses(":\"foo!\"\n");
    }

    #[test]
    fn accepts_symbol_with_question_mark() {
        // :"foo?" is not word-like (trailing `?` is not `\w`).
        test::<SymbolLiteral>().expect_no_offenses(":\"foo?\"\n");
    }

    #[test]
    fn accepts_symbol_with_spaces() {
        test::<SymbolLiteral>().expect_no_offenses(":\"foo bar\"\n");
    }

    #[test]
    fn accepts_symbol_starting_with_digit() {
        test::<SymbolLiteral>().expect_no_offenses(":\"1foo\"\n");
    }

    #[test]
    fn flags_symbol_with_underscores() {
        test::<SymbolLiteral>().expect_offense(indoc! {r#"
            x = :"foo_bar"
                ^^^^^^^^^^ Do not use strings for word-like symbol literals.
        "#});
    }

    #[test]
    fn flags_symbol_starting_with_underscore() {
        test::<SymbolLiteral>().expect_offense(indoc! {r#"
            x = :"_private"
                ^^^^^^^^^^^ Do not use strings for word-like symbol literals.
        "#});
    }

    // ---- autocorrect --------------------------------------------------------

    #[test]
    fn autocorrects_double_quoted_symbol() {
        test::<SymbolLiteral>().expect_correction(
            indoc! {r#"
                x = :"symbol"
                    ^^^^^^^^^ Do not use strings for word-like symbol literals.
            "#},
            "x = :symbol\n",
        );
    }

    #[test]
    fn autocorrects_single_quoted_symbol() {
        test::<SymbolLiteral>().expect_correction(
            indoc! {"
                x = :'symbol'
                    ^^^^^^^^^ Do not use strings for word-like symbol literals.
            "},
            "x = :symbol\n",
        );
    }

    #[test]
    fn autocorrect_is_idempotent() {
        test::<SymbolLiteral>().expect_no_offenses(":symbol\n");
    }

    // ---- word-like predicate ------------------------------------------------

    #[test]
    fn word_like_accepts_plain_words() {
        use super::is_word_like;
        assert!(is_word_like(b"foo"));
        assert!(is_word_like(b"foo_bar"));
        assert!(is_word_like(b"_private"));
        assert!(is_word_like(b"FooBar"));
        assert!(is_word_like(b"a1b2"));
    }

    #[test]
    fn word_like_rejects_special_names() {
        use super::is_word_like;
        assert!(!is_word_like(b""));
        assert!(!is_word_like(b"foo bar"));
        assert!(!is_word_like(b"1foo"));
        assert!(!is_word_like(b"foo!"));
        assert!(!is_word_like(b"foo?"));
        assert!(!is_word_like(b"foo-bar"));
    }
}

murphy_plugin_api::submit_cop!(SymbolLiteral);
