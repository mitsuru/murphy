//! `Style/RegexpLiteral` — enforces using `//` or `%r` around regular expressions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RegexpLiteral
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - EnforcedStyle: slashes (default) — flags %r{} that could use //
//!     - EnforcedStyle: percent_r — flags // that should use %r{}
//!     - EnforcedStyle: mixed — flags %r{} for single-line, // for multi-line
//!     - AllowInnerSlashes: false (default) — when style is slashes or mixed,
//!       allows %r{} (or requires it) when the regexp body contains a slash.
//!     - Autocorrect: rewrites // <-> %r{} (with {}) with inner-slash fixup.
//!
//!   Gaps vs RuboCop:
//!     - Preferred %r delimiter (from Style/PercentLiteralDelimiters config)
//!       is hardcoded to `{}`. RuboCop reads it from configuration.
//!     - Preferred `%r` delimiter (from `Style/PercentLiteralDelimiters`) is
//!       hardcoded to `{}`. Non-`%r{}` forms with `{}` body conflicts skip
//!       autocorrect (bare-brace detection now implemented).
//!     - `allowed_omit_parentheses_with_percent_r_literal?` guard for
//!       `Style/MethodCallWithArgsParentheses` omit_parentheses style is
//!       not implemented. `%r{ foo}` (leading-space) or `%r{= val}` in method
//!       argument position may autocorrect to ambiguous slash literals.
//!     - Interpolated regexps starting with space or `=` in argument position
//!       are not guarded — a known v1 gap.
//! ```
//!
//! ## Matched shapes
//!
//! `Regexp` nodes whose source delimiter does not match the configured style.
//!
//! ## Delimiter detection
//!
//! The first byte at `cx.range(node).start` is `/` for slash literals and
//! `%` for percent-r literals. This is reliable because the expression range
//! covers the full literal including delimiters.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG_USE_SLASHES: &str = "Use `//` around regular expression.";
const MSG_USE_PERCENT_R: &str = "Use `%r` around regular expression.";

/// Preferred delimiter style for regexp literals.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Prefer `/` delimiters (default).
    #[default]
    #[option(value = "slashes")]
    Slashes,
    /// Prefer `%r` delimiters.
    #[option(value = "percent_r")]
    PercentR,
    /// Prefer `/` for single-line, `%r` for multi-line.
    #[option(value = "mixed")]
    Mixed,
}

/// Cop options for [`RegexpLiteral`].
#[derive(CopOptions)]
pub struct RegexpLiteralOptions {
    #[option(
        name = "EnforcedStyle",
        default = "slashes",
        description = "Preferred delimiter style for regexp literals."
    )]
    pub enforced_style: EnforcedStyle,

    #[option(
        name = "AllowInnerSlashes",
        default = false,
        description = "When false, requires %r when the regexp body contains a forward slash."
    )]
    pub allow_inner_slashes: bool,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct RegexpLiteral;

#[cop(
    name = "Style/RegexpLiteral",
    description = "Use `//` or `%r` around regular expressions consistently.",
    default_severity = "warning",
    default_enabled = true,
    options = RegexpLiteralOptions,
)]
impl RegexpLiteral {
    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<RegexpLiteralOptions>();

        let range = cx.range(node);
        let src = cx.raw_source(range);

        let is_slash_literal = src.starts_with('/');

        // Determine if the regexp body contains a forward slash.
        // The body is the source between the opening and closing delimiters.
        let body_contains_slash = regexp_body_contains_slash(node, cx);

        // Compute whether this literal is allowed under the current style.
        let message = if is_slash_literal {
            // Current form is slash literal `//`.
            if slash_literal_allowed(opts, body_contains_slash, cx.is_single_line(node)) {
                return;
            }
            MSG_USE_PERCENT_R
        } else {
            // Current form is percent-r literal `%r{}`.
            // Skip if slash literal would create a syntax conflict
            // (e.g. regexp starts with space or `=` as a method argument).
            if percent_r_literal_allowed(opts, body_contains_slash, cx.is_single_line(node)) {
                return;
            }
            MSG_USE_SLASHES
        };

        // Offense range: just the opening delimiter for a targeted, single-line
        // caret. This keeps tests simple and matches the character that needs
        // to change (the delimiter itself).
        let opener_len: u32 = if is_slash_literal { 1 } else { 3 }; //  or 
        let offense_range = Range { start: range.start, end: range.start + opener_len.min(range.end - range.start) };
        cx.emit_offense(offense_range, message, None);

        // Autocorrect: swap delimiters and fix inner slashes.
        emit_autocorrect(node, cx, is_slash_literal, src);
    }
}

/// Returns `true` if a slash literal `//` is allowed under the current style.
fn slash_literal_allowed(
    opts: RegexpLiteralOptions,
    body_contains_slash: bool,
    is_single_line: bool,
) -> bool {
    let disallowed_slash = !opts.allow_inner_slashes && body_contains_slash;
    match opts.enforced_style {
        // slashes: allowed unless there is an inner slash that we don't want.
        EnforcedStyle::Slashes => !disallowed_slash,
        // percent_r: slash literal never allowed.
        EnforcedStyle::PercentR => false,
        // mixed: allowed for single-line without inner slash.
        EnforcedStyle::Mixed => is_single_line && !disallowed_slash,
    }
}

/// Returns `true` if a percent-r literal `%r{}` is allowed under the current style.
fn percent_r_literal_allowed(
    opts: RegexpLiteralOptions,
    body_contains_slash: bool,
    is_single_line: bool,
) -> bool {
    let disallowed_slash = !opts.allow_inner_slashes && body_contains_slash;
    match opts.enforced_style {
        // slashes: %r allowed only when inner slash forces it.
        EnforcedStyle::Slashes => disallowed_slash,
        // percent_r: always allowed.
        EnforcedStyle::PercentR => true,
        // mixed: %r allowed for multi-line or when inner slash forces it.
        EnforcedStyle::Mixed => !is_single_line || disallowed_slash,
    }
}

/// Returns `true` if the regexp body (the text parts) contains a `/` character.
/// This mirrors RuboCop's `contains_slash?` check.
///
/// The check uses the interned string value (the unescaped content), NOT
/// `raw_source`, because the Str node's range covers the entire regexp
/// expression (including delimiters) when the regexp has no interpolation.
fn regexp_body_contains_slash(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Regexp { parts, .. } => {
            for &part in cx.list(parts) {
                match cx.kind(part) {
                    // Plain string content — use the interned value.
                    NodeKind::Str(sid)
                        if cx.string_str(*sid).contains('/') => {
                            return true;
                        }
                    // Interpolation nodes — do not recurse; we only check
                    // static content for the inner-slash check.
                    _ => {}
                }
            }
            false
        }
        _ => false,
    }
}

// Returns `true` if the regexp body contains bare (unescaped) `{` or `}`
// in the raw source. Used to skip `%r{}` autocorrect when the body would

/// Emit the autocorrect edit — swap delimiters and fix inner slash escaping.
///
/// Skips autocorrect for percent-r forms that use non-`{}` delimiters when
/// the close-delimiter search fails, to avoid emitting partial/broken edits.
fn emit_autocorrect(node: NodeId, cx: &Cx<'_>, is_slash_literal: bool, src: &str) {
    let range = cx.range(node);
    if is_slash_literal {
        // `/foo/flags` → `%r{foo}flags`
        // Closing delimiter: find the closing `/`, skipping character classes.
        let close_offset = match find_closing_slash(src) {
            Some(off) => off,
            None => return,
        };

        // Skip autocorrect if the body contains unescaped `{` or `}`.
        // Converting `/a{1}/` to `%r{a{1}}` would be broken because the
        // inner `}` closes the percent literal early.
        if body_contains_brace(&src[1..close_offset]) {
            return;
        }


        // Opening delimiter: the `/` at start → `%r{`
        let open_range = Range { start: range.start, end: range.start + 1 };
        cx.emit_edit(open_range, "%r{");

        let close_start = range.start + close_offset as u32;
        let close_range = Range { start: close_start, end: close_start + 1 };
        cx.emit_edit(close_range, "}");

        // Fix inner slash escaping: `\/` → `/` in the body.
        fix_inner_slashes_slash_to_percent(src, range.start, close_offset, cx);
    } else {
        // `%r{foo}flags` → `/foo/flags`
        // Opening delimiter: `%r{` (3 bytes) → `/`
        let open_end = find_percent_r_body_start(src);
        if open_end == 0 {
            return; // Unexpected format, skip.
        }
        // Find the closing delimiter matching the opener.
        // If we can't find it (unsupported delimiter form), skip autocorrect
        // entirely to avoid emitting partial broken edits.
        let close_offset = match find_percent_r_close(src, open_end) {
            Some(off) => off,
            None => return, // Skip autocorrect for unrecognised %r delimiters.
        };

        let open_range = Range { start: range.start, end: range.start + open_end as u32 };
        cx.emit_edit(open_range, "/");

        let close_start = range.start + close_offset as u32;
        let close_range = Range { start: close_start, end: close_start + 1 };
        cx.emit_edit(close_range, "/");

        // Fix inner slash escaping: `/` → `\/` in the body.
        fix_inner_slashes_percent_to_slash(src, range.start, open_end, close_offset, cx);
    }
}

/// Returns `true` if the body (raw source between delimiters) contains
/// an unescaped `{` or `}` character. Used to guard slash-to-`%r{}`
/// autocorrect: if the body contains braces, the `%r{...}` form would be
/// broken (e.g. `/a{1}/` → `%r{a{1}}` where the inner `}` closes too early).
fn body_contains_brace(body: &str) -> bool {
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escape
            continue;
        }
        if bytes[i] == b'{' || bytes[i] == b'}' {
            return true;
        }
        i += 1;
    }
    false
}

/// Find the offset of the closing `/` in a slash-delimited regexp source.
/// Tracks `[...]` character classes to skip `/` inside them.
/// Returns byte offset from the start of `src`.
fn find_closing_slash(src: &str) -> Option<usize> {
    // src = `/body/flags`
    // Scan from position 1, skipping `\X` escape sequences and char classes.
    let bytes = src.as_bytes();
    let mut i = 1;
    let mut in_class = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2; // skip escape (handles `\/`, `\[`, `\]`, etc.)
                continue;
            }
            b'[' if !in_class => {
                in_class = true;
            }
            b']' if in_class => {
                in_class = false;
            }
            b'/' if !in_class => {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Find the length of the opening delimiter of a percent-r literal.
/// `%r{` → 3, `%r[` → 3, `%r(` → 3, `%r/` → 3, etc.
fn find_percent_r_body_start(src: &str) -> usize {
    // src = `%r<delim>body<close>flags`
    // The opening is always `%r` + 1 delimiter char = 3 bytes.
    if src.len() >= 3 && &src[..2] == "%r" {
        3
    } else {
        0
    }
}

/// Find the offset of the closing delimiter in a percent-r literal.
///
/// Handles all `%r` delimiter types:
/// - Paired: `{}` `[]` `()` `<>` — depth-tracked matching close delimiter.
/// - Unpaired: any other char — first unescaped occurrence of the same char.
///
/// Returns the byte offset of the closing delimiter within `src`.
/// Returns `None` if the closing delimiter cannot be found.
fn find_percent_r_close(src: &str, open_end: usize) -> Option<usize> {
    if src.len() < open_end {
        return None;
    }
    let opener = src.as_bytes()[open_end - 1]; // last byte of opening delimiter
    let (open_ch, close_ch, paired) = match opener {
        b'{' => (b'{', b'}', true),
        b'[' => (b'[', b']', true),
        b'(' => (b'(', b')', true),
        b'<' => (b'<', b'>', true),
        ch => (ch, ch, false), // unpaired: same char as open
    };

    let bytes = src.as_bytes();
    let mut depth = 1usize;
    let mut i = open_end;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2; // skip escape
                continue;
            }
            ch if paired && ch == open_ch => {
                depth += 1;
            }
            ch if ch == close_ch => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Emit edits to fix `\/` → `/` inside a slash-literal being converted to `%r{}`.
fn fix_inner_slashes_slash_to_percent(
    src: &str,
    node_start: u32,
    close_offset: usize,
    cx: &Cx<'_>,
) {
    // Body is src[1..close_offset]. Find all `\/` and replace with `/`.
    let body = &src[1..close_offset];
    let body_start = node_start + 1;
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            let edit_start = body_start + i as u32;
            cx.emit_edit(Range { start: edit_start, end: edit_start + 2 }, "/");
            i += 2;
        } else {
            i += 1;
        }
    }
}

/// Emit edits to fix `/` → `\/` inside a `%r{}`-literal being converted to `//`.
/// Skips `#{...}` interpolation blocks — `/` inside Ruby interpolation is a
/// division operator, not a regexp delimiter, and must not be escaped.
fn fix_inner_slashes_percent_to_slash(
    src: &str,
    node_start: u32,
    open_end: usize,
    close_offset: usize,
    cx: &Cx<'_>,
) {
    // Body is src[open_end..close_offset]. Find all bare `/` and escape them.
    let body = &src[open_end..close_offset];
    let body_start = node_start + open_end as u32;
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip existing escape
            continue;
        }
        // Skip interpolation `#{...}` — `/` inside Ruby interpolation is a
        // division operator and must not be escaped.
        if bytes[i] == b'#' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            i += 2; // skip `#{`
            let mut depth = 1usize;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' {
            let edit_start = body_start + i as u32;
            cx.emit_edit(Range { start: edit_start, end: edit_start + 1 }, "\\/");
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- EnforcedStyle: slashes (default) -----

    #[test]
    fn flags_percent_r_when_slashes_preferred() {
        test::<RegexpLiteral>().expect_correction(
            indoc! {r#"
                snake_case = %r{^[\dA-Z_]+$}
                             ^^^ Use `//` around regular expression.
            "#},
            "snake_case = /^[\\dA-Z_]+$/\n",
        );
    }

    #[test]
    fn accepts_slash_literal_with_slashes_style() {
        test::<RegexpLiteral>().expect_no_offenses("snake_case = /^[\\dA-Z_]+$/\n");
    }

    #[test]
    fn accepts_percent_r_when_inner_slash_present_slashes_style() {
        // Default AllowInnerSlashes: false => %r is REQUIRED when inner / present.
        test::<RegexpLiteral>().expect_no_offenses("x =~ %r{home/}\n");
    }

    #[test]
    fn flags_slash_literal_with_inner_slash_slashes_style() {
        // /home\// has an inner slash — should use %r{}.
        test::<RegexpLiteral>().expect_correction(
            "x =~ /home\\//\n     ^ Use `%r` around regular expression.\n",
            "x =~ %r{home/}\n",
        );
    }

    // ----- EnforcedStyle: percent_r -----

    #[test]
    fn flags_slash_literal_when_percent_r_preferred() {
        test::<RegexpLiteral>()
            .with_options(&RegexpLiteralOptions { enforced_style: EnforcedStyle::PercentR, ..Default::default() })
            .expect_correction(
                indoc! {r#"
                    snake_case = /^[\dA-Z_]+$/
                                 ^ Use `%r` around regular expression.
                "#},
                "snake_case = %r{^[\\dA-Z_]+$}\n",
            );
    }

    #[test]
    fn accepts_percent_r_with_percent_r_style() {
        test::<RegexpLiteral>()
            .with_options(&RegexpLiteralOptions { enforced_style: EnforcedStyle::PercentR, ..Default::default() })
            .expect_no_offenses("snake_case = %r{^[\\dA-Z_]+$}\n");
    }

    // ----- EnforcedStyle: mixed -----

    #[test]
    fn accepts_slash_for_single_line_mixed_style() {
        test::<RegexpLiteral>()
            .with_options(&RegexpLiteralOptions { enforced_style: EnforcedStyle::Mixed, ..Default::default() })
            .expect_no_offenses("snake_case = /^[\\dA-Z_]+$/\n");
    }

    #[test]
    fn flags_percent_r_for_single_line_mixed_style() {
        test::<RegexpLiteral>()
            .with_options(&RegexpLiteralOptions { enforced_style: EnforcedStyle::Mixed, ..Default::default() })
            .expect_correction(
                indoc! {r#"
                    snake_case = %r{^[\dA-Z_]+$}
                                 ^^^ Use `//` around regular expression.
                "#},
                "snake_case = /^[\\dA-Z_]+$/\n",
            );
    }

    #[test]
    fn flags_slash_for_multiline_mixed_style() {
        test::<RegexpLiteral>()
            .with_options(&RegexpLiteralOptions { enforced_style: EnforcedStyle::Mixed, ..Default::default() })
            .expect_offense(indoc! {"
                regex = /
                        ^ Use `%r` around regular expression.
                  foo
                /x
            "});
    }

    #[test]
    fn accepts_percent_r_for_multiline_mixed_style() {
        test::<RegexpLiteral>()
            .with_options(&RegexpLiteralOptions { enforced_style: EnforcedStyle::Mixed, ..Default::default() })
            .expect_no_offenses(indoc! {"
                regex = %r{
                  foo
                }x
            "});
    }

    // ----- Non-brace %r delimiters -----

    #[test]
    fn flags_slash_literal_with_body_brace_but_no_autocorrect() {
        // Body contains bare `}` — offense is emitted but autocorrect is skipped
        // to avoid generating invalid `%r{foo}bar}`.
        test::<RegexpLiteral>()
            .with_options(&RegexpLiteralOptions { enforced_style: EnforcedStyle::PercentR, ..Default::default() })
            .expect_offense(indoc! {r#"
                r = /foo}bar/
                    ^ Use `%r` around regular expression.
            "#});
    }

    #[test]
    fn corrects_slash_literal_with_escaped_brace() {
        // Escaped `\{` in raw source is NOT a bare brace — autocorrect is safe.
        // `/foo\{bar/` → `%r{foo\{bar}` (the `\{` remains escaped).
        test::<RegexpLiteral>()
            .with_options(&RegexpLiteralOptions { enforced_style: EnforcedStyle::PercentR, ..Default::default() })
            .expect_correction(
                indoc! {r#"
                    r = /foo\{bar/
                        ^ Use `%r` around regular expression.
                "#},
                "r = %r{foo\\{bar}\n",
            );
    }

    #[test]
    fn corrects_percent_r_bracket_to_slashes() {
        // %r[foo] uses `[` delimiter — the fix should produce /foo/, not /foo]
        test::<RegexpLiteral>().expect_correction(
            indoc! {r#"
                snake_case = %r[foo]
                             ^^^ Use `//` around regular expression.
            "#},
            "snake_case = /foo/\n",
        );
    }

    #[test]
    fn corrects_percent_r_paren_to_slashes() {
        test::<RegexpLiteral>().expect_correction(
            indoc! {r#"
                snake_case = %r(foo)
                             ^^^ Use `//` around regular expression.
            "#},
            "snake_case = /foo/\n",
        );
    }

    // ----- AllowInnerSlashes: true -----

    #[test]
    fn accepts_slash_with_inner_slash_when_allow_inner_slashes_true() {
        test::<RegexpLiteral>()
            .with_options(&RegexpLiteralOptions { allow_inner_slashes: true, ..Default::default() })
            .expect_no_offenses("x =~ /home\\//\n");
    }

    // ----- Non-{} percent-r delimiters (autocorrect robustness) -----

    #[test]
    fn flags_slash_with_braces_offense_but_no_autocorrect() {
        // /a{1}/ under percent_r style: offense is emitted but autocorrect is
        // suppressed because the body contains `{}`  which would be broken
        // in %r{a{1}} form.
        test::<RegexpLiteral>()
            .with_options(&RegexpLiteralOptions { enforced_style: EnforcedStyle::PercentR, ..Default::default() })
            .expect_offense("x =~ /a{1}/\n     ^ Use `%r` around regular expression.\n");
    }

    #[test]
    fn flags_percent_r_bracket_delimiter_and_autocorrects() {
        // %r[foo] is flagged and autocorrected to /foo/
        test::<RegexpLiteral>().expect_correction(
            "x =~ %r[foo]\n     ^^^ Use `//` around regular expression.\n",
            "x =~ /foo/\n",
        );
    }

    #[test]
    fn flags_percent_r_paren_delimiter_and_autocorrects() {
        // %r(foo) is flagged and autocorrected to /foo/
        test::<RegexpLiteral>().expect_correction(
            "x =~ %r(foo)\n     ^^^ Use `//` around regular expression.\n",
            "x =~ /foo/\n",
        );
    }

}
murphy_plugin_api::submit_cop!(RegexpLiteral);
