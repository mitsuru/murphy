//! `Style/QuotedSymbols` — use a consistent style for quoted symbols.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/QuotedSymbols
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-y3h2
//! notes: >
//!   Checks that quoted symbols use a consistent quote style. Supports three
//!   EnforcedStyle values:
//!     same_as_string_literals (default): delegates to single_quotes style
//!       (Murphy cannot read another cop's runtime config, so cross-cop
//!       inference of Style/StringLiterals' setting is not implemented).
//!     single_quotes: flag double-quoted symbols that don't need double quotes.
//!     double_quotes: flag single-quoted symbols that don't need single quotes.
//!
//!   Unquoted symbols (:foo, :bar) are never flagged — only quoted forms
//!   (:"foo", :'foo') are inspected.
//!   Interpolated symbols (:"#{str}") are always ignored (they are dsym nodes,
//!   not sym nodes).
//!
//!   Autocorrect: swaps the opening and closing quote byte. Skipped when the
//!   body contains backslashes (escapes differ between the two quote styles) or
//!   when the target quote character already appears in the body.
//!
//!   Gaps vs RuboCop:
//!     - same_as_string_literals cross-cop inference: RuboCop reads
//!       Style/StringLiterals' EnforcedStyle at runtime. Murphy treats
//!       same_as_string_literals as an alias for single_quotes.
//!     - Hash colon-key style (e.g. `"foo": 1`): requires parent context
//!       to detect. Only standalone `:""` / `:''` forms are handled.
//!     - correct_quotes escape-translation: RuboCop reverses escape-doubling
//!       introduced during String#inspect. Murphy skips autocorrect when
//!       backslashes are present, so this edge case does not produce wrong output.
//! ```
//!
//! ## Detection
//!
//! A `Sym` node is flagged when:
//! 1. Its raw source starts with `:"` or `:'` (it is a quoted symbol).
//! 2. The quote style does not match the configured `EnforcedStyle`.
//! 3. The current quote style is not required (e.g. double quotes not needed
//!    when single quotes would suffice).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

use crate::cops::style::string_literals::{
    QuoteStyle, double_quotes_required, parse_quote_form,
};

/// Stateless unit struct.
#[derive(Default)]
pub struct QuotedSymbols;

/// Quote style enforcement for quoted symbols.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Use the same style as `Style/StringLiterals` (default). Murphy treats
    /// this as `single_quotes` since cross-cop config inference is not
    /// available.
    #[default]
    #[option(value = "same_as_string_literals")]
    SameAsStringLiterals,
    #[option(value = "single_quotes")]
    SingleQuotes,
    #[option(value = "double_quotes")]
    DoubleQuotes,
}

/// Cop options for [`QuotedSymbols`].
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "same_as_string_literals",
        description = "Quote style for quoted symbols."
    )]
    pub enforced_style: EnforcedStyle,
}

const MSG_SINGLE: &str =
    "Prefer single-quoted symbols when you don't need string interpolation or special symbols.";
const MSG_DOUBLE: &str =
    "Prefer double-quoted symbols unless you need single quotes to avoid extra backslashes for escaping.";

#[cop(
    name = "Style/QuotedSymbols",
    description = "Use a consistent style for quoted symbols.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl QuotedSymbols {
    /// Only subscribe to plain `sym` nodes. Interpolated symbols (`dsym`) are
    /// always correct in double quotes and must never be flagged.
    #[on_node(kind = "sym")]
    fn check_sym(&self, node: NodeId, cx: &Cx<'_>) {
        if !matches!(cx.kind(node), NodeKind::Sym(_)) {
            return;
        }

        let range = cx.range(node);
        let src = cx.raw_source(range);
        let bytes = src.as_bytes();

        // Must start with `:` followed by a quote.
        if bytes.len() < 4 || bytes[0] != b':' {
            return;
        }
        let open_quote = bytes[1];
        if open_quote != b'"' && open_quote != b'\'' {
            return;
        }

        // Determine the actual quote style by examining the source after `:`.
        let symbol_src = &src[1..]; // strip leading `:`
        let Some((actual_style, _body)) = parse_quote_form(symbol_src) else {
            return;
        };

        let opts = cx.options_or_default::<Options>();
        // same_as_string_literals is treated as single_quotes (cross-cop
        // inference is a documented gap).
        let prefer_single = matches!(
            opts.enforced_style,
            EnforcedStyle::SameAsStringLiterals | EnforcedStyle::SingleQuotes
        );

        if prefer_single {
            // Flag double-quoted symbols that don't need double quotes.
            if actual_style != QuoteStyle::Double {
                return;
            }
            // `double_quotes_required` checks the form starting with the quote
            // character (not `:`), which is `symbol_src`.
            if double_quotes_required(symbol_src) {
                return;
            }
            cx.emit_offense(range, MSG_SINGLE, None);
            // Autocorrect: swap `"` to `'`.
            maybe_emit_quote_swap(range, bytes, b'\'', b'"', cx);
        } else {
            // Flag single-quoted symbols that don't need single quotes.
            if actual_style != QuoteStyle::Single {
                return;
            }
            if single_quotes_required_for_symbol(symbol_src) {
                return;
            }
            cx.emit_offense(range, MSG_DOUBLE, None);
            // Autocorrect: swap `'` to `"`.
            maybe_emit_quote_swap(range, bytes, b'"', b'\'', cx);
        }
    }
}

/// Emit surgical edits to swap quotes on a quoted symbol node if safe.
///
/// `target_quote` is the new quote character; `source_quote` is the old one.
/// Bytes are the raw source bytes of the full node (including leading `:`).
///
/// Skips the edit if:
/// - The body contains any backslash (escape semantics differ).
/// - The body contains the target quote character (would need re-escaping).
fn maybe_emit_quote_swap(
    range: Range,
    bytes: &[u8],
    target_quote: u8,
    source_quote: u8,
    cx: &Cx<'_>,
) {
    // Body is bytes[2..len-1] (strip `:`, opening quote, closing quote).
    let body = &bytes[2..bytes.len() - 1];
    if body.contains(&b'\\') {
        return;
    }
    if body.contains(&target_quote) {
        return;
    }
    // Also block if source quote appears in body (it would become unescaped).
    if body.contains(&source_quote) {
        return;
    }

    // Two surgical edits: swap opening quote (bytes[1]) and closing quote (bytes[len-1]).
    let open_quote_range = Range {
        start: range.start + 1,
        end: range.start + 2,
    };
    let close_quote_range = Range {
        start: range.end - 1,
        end: range.end,
    };
    let tq_bytes = [target_quote];
    let target = std::str::from_utf8(&tq_bytes).unwrap_or("'");
    cx.emit_edit(open_quote_range, target);
    cx.emit_edit(close_quote_range, target);
}

/// Checks whether single quotes are required for a symbol (when enforcing
/// double quotes). This mirrors the `invalid_double_quotes?` logic from
/// RuboCop's `Style/QuotedSymbols`.
///
/// RuboCop regex: `/" | (?<!\\)\\[aAbcdefkMnprsStuUxzZ0-7] | \#[@{$]/x`
///
/// Returns `true` when double quotes would be invalid (single quotes needed):
/// - Source contains a literal `"`.
/// - Source contains a meaningful backslash escape (escape character in
///   the set [aAbcdefkMnprsStuUxzZ0-7] not preceded by another backslash).
/// - Source contains an interpolation anchor `#@`, `#{`, `#$`.
fn single_quotes_required_for_symbol(src: &str) -> bool {
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => return true,
            b'\\' => {
                let next = if i + 1 < b.len() { b[i + 1] } else { 0 };
                // Meaningful escape: backslash followed by a character in the
                // RuboCop set [aAbcdefkMnprsStuUxzZ0-7].
                if matches!(
                    next,
                    b'a' | b'A'
                        | b'b'
                        | b'c'
                        | b'd'
                        | b'e'
                        | b'f'
                        | b'k'
                        | b'M'
                        | b'n'
                        | b'p'
                        | b'r'
                        | b's'
                        | b'S'
                        | b't'
                        | b'u'
                        | b'U'
                        | b'x'
                        | b'z'
                        | b'Z'
                        | b'0'..=b'7'
                ) {
                    return true;
                }
                i += 2;
                continue;
            }
            b'#' => {
                let next = if i + 1 < b.len() { b[i + 1] } else { 0 };
                if matches!(next, b'@' | b'{' | b'$') {
                    return true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- single_quotes (default via same_as_string_literals) ---

    #[test]
    fn flags_double_quoted_symbol_in_single_mode() {
        test::<QuotedSymbols>().expect_offense(indoc! {r#"
            x = :"abc-def"
                ^^^^^^^^^^ Prefer single-quoted symbols when you don't need string interpolation or special symbols.
        "#});
    }

    #[test]
    fn accepts_single_quoted_symbol_in_single_mode() {
        test::<QuotedSymbols>().expect_no_offenses("x = :'abc-def'\n");
    }

    #[test]
    fn accepts_unquoted_symbol_in_single_mode() {
        test::<QuotedSymbols>().expect_no_offenses("x = :foo\n");
    }

    #[test]
    fn accepts_double_quoted_when_contains_single_quote() {
        // Double quotes required because body has `'`.
        test::<QuotedSymbols>().expect_no_offenses("x = :\"a'b\"\n");
    }

    #[test]
    fn accepts_double_quoted_when_has_escape() {
        // \n escape requires double quotes.
        test::<QuotedSymbols>().expect_no_offenses("x = :\"a\\nb\"\n");
    }

    #[test]
    fn corrects_double_to_single() {
        test::<QuotedSymbols>().expect_correction(
            indoc! {r#"
                x = :"abc-def"
                    ^^^^^^^^^^ Prefer single-quoted symbols when you don't need string interpolation or special symbols.
            "#},
            "x = :'abc-def'\n",
        );
    }

    #[test]
    fn skips_autocorrect_when_body_has_backslash() {
        // Offense fires but autocorrect is skipped due to backslash in body.
        test::<QuotedSymbols>().expect_offense(indoc! {r#"
            x = :"a\\b"
                ^^^^^^^ Prefer single-quoted symbols when you don't need string interpolation or special symbols.
        "#});
    }

    // --- double_quotes mode ---

    fn double_opts() -> Options {
        Options { enforced_style: EnforcedStyle::DoubleQuotes }
    }

    #[test]
    fn flags_single_quoted_symbol_in_double_mode() {
        test::<QuotedSymbols>()
            .with_options(&double_opts())
            .expect_offense(indoc! {"
                x = :'abc-def'
                    ^^^^^^^^^^ Prefer double-quoted symbols unless you need single quotes to avoid extra backslashes for escaping.
            "});
    }

    #[test]
    fn accepts_double_quoted_symbol_in_double_mode() {
        test::<QuotedSymbols>()
            .with_options(&double_opts())
            .expect_no_offenses("x = :\"abc-def\"\n");
    }

    #[test]
    fn accepts_single_quoted_when_contains_double_quote_in_double_mode() {
        // Single quotes required because body has `"`.
        test::<QuotedSymbols>()
            .with_options(&double_opts())
            .expect_no_offenses("x = :'a\"b'\n");
    }

    #[test]
    fn corrects_single_to_double() {
        test::<QuotedSymbols>()
            .with_options(&double_opts())
            .expect_correction(
                indoc! {"
                    x = :'abc-def'
                        ^^^^^^^^^^ Prefer double-quoted symbols unless you need single quotes to avoid extra backslashes for escaping.
                "},
                "x = :\"abc-def\"\n",
            );
    }

    // --- same_as_string_literals behaves as single_quotes ---

    fn same_opts() -> Options {
        Options { enforced_style: EnforcedStyle::SameAsStringLiterals }
    }

    #[test]
    fn same_as_string_literals_flags_double_quoted() {
        test::<QuotedSymbols>()
            .with_options(&same_opts())
            .expect_offense(indoc! {r#"
                x = :"abc-def"
                    ^^^^^^^^^^ Prefer single-quoted symbols when you don't need string interpolation or special symbols.
            "#});
    }

    // --- dsym (interpolated symbols) never flagged ---

    #[test]
    fn accepts_interpolated_symbol() {
        // dsym nodes are not subscribed; the cop should not see them.
        test::<QuotedSymbols>().expect_no_offenses("x = :\"#{str}\"\n");
    }

    // --- single_quotes_required_for_symbol ---

    #[test]
    fn sqrs_double_quote_requires_single() {
        use super::single_quotes_required_for_symbol;
        assert!(single_quotes_required_for_symbol("\"hello\""));
    }

    #[test]
    fn sqrs_plain_symbol_does_not_require_single() {
        use super::single_quotes_required_for_symbol;
        assert!(!single_quotes_required_for_symbol("'hello'"));
    }

    #[test]
    fn sqrs_newline_escape_requires_single() {
        use super::single_quotes_required_for_symbol;
        assert!(single_quotes_required_for_symbol("'\\n'"));
    }

    #[test]
    fn sqrs_interpolation_anchor_requires_single() {
        use super::single_quotes_required_for_symbol;
        assert!(single_quotes_required_for_symbol("'#{foo}'"));
    }

    #[test]
    fn sqrs_backslash_backslash_does_not_require_single() {
        use super::single_quotes_required_for_symbol;
        // \\ is a literal backslash escape — not in the meaningful set
        assert!(!single_quotes_required_for_symbol("'\\\\'"));
    }
}
murphy_plugin_api::submit_cop!(QuotedSymbols);
