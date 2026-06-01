//! `Style/RedundantStringEscape` — flags unnecessary escape sequences in
//! string literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantStringEscape
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - Double-quoted strings ("...") and interpolated strings ("...#{...}...")
//!     - Single-quoted strings ('...') — no redundant escapes (all allowed)
//!     - %Q, % percent-literals (interpolation-enabled)
//!     - %q percent-literals (interpolation-disabled) — no redundant escapes
//!     - %w/%W word arrays — space escapes allowed; %w no redundant escapes
//!     - Interpolation-enabled heredocs (<<~HEREDOC / <<HEREDOC)
//!     - Interpolation-disabled heredocs (<<~'HEREDOC') — no redundant escapes
//!     - Autocorrect: remove the leading backslash from each redundant escape
//!     - Escapes disabling interpolation (\#, \#{, #\{, #\$, #\@) are allowed
//!     - Delimiter escapes (\x when x is the string delimiter) are allowed
//!   Gaps:
//!     - `__FILE__` / `__dir__` character literals not explicitly handled
//!       (skipped because their raw_source won't contain `\`).
//!     - Dsym (:"...") nodes containing Str segments — handled via dstr path.
//!     - Xstr (`%x{...}`) is explicitly skipped (matches RuboCop).
//!     - Regexp nodes are explicitly skipped (matches RuboCop).
//!     - Character literals (`?x`) are not subscribed (no `\` in raw source).
//! ```
//!
//! ## Detection algorithm
//!
//! For each `Str` node, determine:
//! 1. Whether the string uses interpolation (double-quoted, `%Q`, `%W`, heredoc
//!    without `'`-quoted label, or is a segment in an interpolation-enabled Dstr).
//! 2. The string's delimiter character(s).
//! 3. The byte range of the string's contents (excluding outer delimiters).
//! 4. For each `\X` sequence in the contents:
//!    - If interpolation is disabled → always allowed (raw backslash).
//!    - If `X` is alphanumeric, `\`, or `\n` (continuation) → allowed (has meaning).
//!    - If `X` is ` ` and the string is `%w`/`%W` → allowed (word separator).
//!    - If the sequence `\#[{$@]` or `#\[{$@]` disables interpolation → allowed.
//!    - If `X` equals the string's closing delimiter → allowed (necessary escape).
//!    - Otherwise → redundant escape; emit offense on the `\X` range.
//!    - Autocorrect removes the `\` (byte at offense start).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantStringEscape;

const MSG: &str = "Redundant escape of %s inside string literal.";

#[cop(
    name = "Style/RedundantStringEscape",
    description = "Checks for redundant escapes in string literals.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantStringEscape {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        check_str_node(node, cx);
    }

    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        check_dstr_node(node, cx);
    }
}

/// Context for scanning a string's escape sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StringContext {
    /// Interpolation is enabled; the closing delimiter char is stored.
    InterpolationEnabled {
        /// The closing delimiter character (e.g. `"`, `)`, `}`, `]`).
        close_delim: u8,
        /// The opening delimiter character. Equals `close_delim` for non-paired
        /// delimiters (`"`, `/`). For paired delimiters (`%Q{...}`, `%Q(...)`)
        /// this is the corresponding open char (`{`, `(`). Both must be allowed
        /// as necessary escapes inside paired-delimiter strings.
        open_delim: u8,
        /// True if this is a `%w`/`%W` word array element.
        is_percent_word: bool,
    },
    /// Interpolation is disabled (single-quoted, `%q`, non-interpolating heredoc).
    InterpolationDisabled,
}

/// Handle a plain (non-interpolated) `Str` node at the top level.
/// Skips segments inside Dstr — those are handled by `check_dstr_node`.
fn check_str_node(node: NodeId, cx: &Cx<'_>) {
    // Skip if parent is Regexp or Xstr — RuboCop explicitly exempts these.
    if let Some(parent) = cx.parent(node).get() {
        match cx.kind(parent) {
            NodeKind::Regexp { .. } | NodeKind::Xstr(_) => return,
            NodeKind::Dstr(_) | NodeKind::Dsym(_) => {
                // Segment inside a Dstr/Dsym — skip here; check_dstr_node handles it.
                return;
            }
            _ => {}
        }
    }

    let raw = cx.raw_source(cx.range(node));
    let ctx = classify_standalone_str(raw);
    scan_and_emit(cx, cx.range(node), raw, ctx);
}

/// Handle a `Dstr` node by scanning its literal `Str` segments.
fn check_dstr_node(node: NodeId, cx: &Cx<'_>) {
    // Skip if parent is Regexp or Xstr.
    if let Some(parent) = cx.parent(node).get() {
        match cx.kind(parent) {
            NodeKind::Regexp { .. } | NodeKind::Xstr(_) => return,
            _ => {}
        }
    }

    // Determine if this Dstr has interpolation enabled by examining the opener token.
    let (interp_enabled, close_delim, open_delim, is_heredoc, is_percent_word) =
        classify_dstr(node, cx);

    if !interp_enabled {
        return; // No redundant escapes in non-interpolating strings.
    }

    let NodeKind::Dstr(children) = cx.kind(node) else {
        return;
    };

    for &child_id in cx.list(*children) {
        // Only look at Str segments (literal text portions).
        if !matches!(cx.kind(child_id), NodeKind::Str(_)) {
            continue;
        }

        // Skip segments inside interpolation (#{...}) — they're quoted separately.
        if is_inside_interpolation(child_id, node, cx) {
            continue;
        }

        let seg_range = cx.range(child_id);
        if seg_range.start >= seg_range.end {
            continue;
        }

        let raw = cx.source();
        let raw_bytes = raw.as_bytes();

        let (start, end) = if is_heredoc {
            // Heredoc body segments include trailing \n but have no delimiter prefix.
            (seg_range.start as usize, seg_range.end as usize)
        } else {
            // For inline Dstr segments, the range covers only the literal text
            // (no quotes). Scan it directly.
            (seg_range.start as usize, seg_range.end as usize)
        };

        scan_bytes_and_emit(
            cx,
            raw_bytes,
            start,
            end,
            StringContext::InterpolationEnabled {
                close_delim,
                open_delim,
                is_percent_word,
            },
            is_heredoc,
        );
    }
}

/// Returns true if `node` is directly under a `Begin(...)` that is itself
/// a child of the given `dstr_id` (i.e., it's inside `#{...}`).
fn is_inside_interpolation(node: NodeId, dstr_id: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !matches!(cx.kind(parent), NodeKind::Begin(_)) {
        return false;
    }
    let Some(grandparent) = cx.parent(parent).get() else {
        return false;
    };
    grandparent == dstr_id
}

/// Classify a standalone `Str` node by looking at its raw source.
fn classify_standalone_str(raw: &str) -> StringContext {
    let bytes = raw.as_bytes();
    if bytes.is_empty() {
        return StringContext::InterpolationDisabled;
    }
    let first = bytes[0];
    match first {
        b'"' => StringContext::InterpolationEnabled {
            close_delim: b'"',
            open_delim: b'"',
            is_percent_word: false,
        },
        b'\'' => StringContext::InterpolationDisabled,
        b'%' => classify_percent_literal(bytes),
        // Heredoc start token: the Str node with a heredoc has a range
        // pointing to the opener line `<<~HEREDOC`. Heredoc bodies are
        // Dstr nodes, so this case shouldn't arise for plain Str here
        // — unless it's a single-line heredoc or a non-interpolating one.
        // Conservatively treat unknown delimiters as disabled.
        _ => StringContext::InterpolationDisabled,
    }
}

/// Classify a percent-literal string from its raw bytes.
fn classify_percent_literal(bytes: &[u8]) -> StringContext {
    if bytes.len() < 2 || bytes[0] != b'%' {
        return StringContext::InterpolationDisabled;
    }
    // Determine what follows `%`: letter or delimiter.
    let (letter, open_delim_pos) = match bytes[1] {
        b'q' | b'Q' | b'w' | b'W' | b'r' | b'i' | b'I' | b's' | b'x' => {
            (bytes[1], 2)
        }
        _ => (b'Q', 1), // bare % acts like %Q
    };

    // Interpolation-disabled forms.
    if matches!(letter, b'q' | b'w') {
        return StringContext::InterpolationDisabled;
    }

    // For interpolation-enabled forms, determine the closing delimiter.
    let close_delim = if open_delim_pos < bytes.len() {
        matching_close_delim(bytes[open_delim_pos])
    } else {
        return StringContext::InterpolationDisabled;
    };

    let is_percent_word = matches!(letter, b'W');

    // For percent literals, the open delimiter character.
    let open_delim = if open_delim_pos < bytes.len() { bytes[open_delim_pos] } else { 0 };
    StringContext::InterpolationEnabled {
        close_delim,
        open_delim,
        is_percent_word,
    }
}

/// Returns the matching closing delimiter for a percent-literal opener.
fn matching_close_delim(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        other => other, // non-paired: same char
    }
}

/// Determine if a Dstr has interpolation enabled, what its close delimiter is,
/// and whether it's a heredoc.
/// Returns `(interp_enabled, close_delim, open_delim, is_heredoc, is_percent_word)`.
fn classify_dstr(node: NodeId, cx: &Cx<'_>) -> (bool, u8, u8, bool, bool) {
    // Check if there's a HeredocStart token at or before this Dstr's range start.
    let range = cx.range(node);
    let src = cx.source();
    let src_bytes = src.as_bytes();

    // Look for a HeredocStart token that starts at the same position as the Dstr.
    // For heredocs, the Dstr's range starts at the `<<` opener.
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < range.start);
    if let Some(tok) = toks.get(idx)
        && tok.kind == SourceTokenKind::HeredocStart
        && tok.range.start == range.start
    {
        // It's a heredoc. Check if the label ends with `'` (no interpolation).
        let heredoc_src = &src_bytes[tok.range.start as usize..tok.range.end as usize];
        let disabled = heredoc_src.ends_with(b"'");
        return (!disabled, b'\0', b'\0', true, false);
    }

    // Not a heredoc — look at the raw source opening.
    let raw = cx.raw_source(range);
    let bytes = raw.as_bytes();
    if bytes.is_empty() {
        return (false, b'\0', b'\0', false, false);
    }
    match bytes[0] {
        b'"' => (true, b'"', b'"', false, false),
        b'\'' => (false, b'\'', b'\0', false, false),
        b'%' => {
            let ctx = classify_percent_literal(bytes);
            match ctx {
                StringContext::InterpolationEnabled { close_delim, open_delim, is_percent_word } =>
                    (true, close_delim, open_delim, false, is_percent_word),
                StringContext::InterpolationDisabled => (false, b'\0', b'\0', false, false),
            }
        }
        _ => (false, b'\0', b'\0', false, false),
    }
}

/// Scan a standalone str's raw source for redundant escapes and emit offenses.
fn scan_and_emit(cx: &Cx<'_>, full_range: Range, raw: &str, ctx: StringContext) {
    let bytes = raw.as_bytes();
    let src_bytes = cx.source().as_bytes();
    let file_offset = full_range.start as usize;

    match ctx {
        StringContext::InterpolationDisabled => {
            // No redundant escapes in non-interpolating strings.
        }
        StringContext::InterpolationEnabled { close_delim, open_delim, is_percent_word } => {
            // Find the content range (skip opening delimiter).
            // For "...", skip 1 byte (the `"`).
            // For %Q(...), skip 3 bytes (`%Q(`).
            // For %(...), skip 2 bytes (`%(`).
            // For %W[...], skip 3 bytes (`%W[`).
            let content_start = find_content_start(bytes);
            let content_end = bytes.len().saturating_sub(1); // skip closing delimiter
            scan_bytes_and_emit(
                cx,
                src_bytes,
                file_offset + content_start,
                file_offset + content_end,
                StringContext::InterpolationEnabled { close_delim, open_delim, is_percent_word },
                false,
            );
        }
    }
}

/// Returns the byte offset within `raw` where string content begins
/// (after the opening delimiter(s)).
fn find_content_start(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    match bytes[0] {
        b'"' | b'\'' => 1,
        b'%' if bytes.len() >= 2 => {
            match bytes[1] {
                b'q' | b'Q' | b'w' | b'W' | b'r' | b'i' | b'I' | b's' | b'x' => {
                    3 // %X<delim>
                }
                _ => 2, // %<delim>
            }
        }
        _ => 0,
    }
}

/// Core scanner: scan bytes from `start` to `end` (exclusive) for `\X` sequences
/// and emit offenses for redundant ones.
fn scan_bytes_and_emit(
    cx: &Cx<'_>,
    src_bytes: &[u8],
    start: usize,
    end: usize,
    ctx: StringContext,
    _is_heredoc: bool,
) {
    let StringContext::InterpolationEnabled { close_delim, open_delim, is_percent_word } = ctx else {
        return;
    };

    let mut i = start;
    while i < end {
        if src_bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        // Found a backslash at position `i`.
        let next_pos = i + 1;
        if next_pos >= end {
            // Trailing backslash — continuation; skip.
            i += 1;
            continue;
        }
        let next_byte = src_bytes[next_pos];

        // Determine if this escape is redundant.
        if is_redundant_escape(src_bytes, i, next_byte, close_delim, open_delim, is_percent_word) {
            let escape_range = Range {
                start: i as u32,
                end: (i + 2) as u32,
            };
            let escaped_char = describe_char(next_byte);
            let msg = MSG.replace("%s", &escaped_char);
            cx.emit_offense(escape_range, &msg, None);
            // Autocorrect: remove the leading backslash.
            cx.emit_edit(
                Range {
                    start: i as u32,
                    end: (i + 1) as u32,
                },
                "",
            );
        }
        // Skip over the two-byte escape sequence.
        i += 2;
    }
}

/// Returns `true` if the escape `\X` at `pos` (where `next` = X) is redundant.
fn is_redundant_escape(
    src: &[u8],
    pos: usize,
    next: u8,
    close_delim: u8,
    open_delim: u8,
    is_percent_word: bool,
) -> bool {
    // Alphanumeric, backslash: never redundant (has semantic meaning).
    if next.is_ascii_alphanumeric() || next == b'\\' {
        return false;
    }

    // Newline (line continuation): never redundant.
    if next == b'\n' {
        return false;
    }

    // Space in %w/%W word arrays: escapes the word boundary.
    if next == b' ' && is_percent_word {
        return false;
    }

    // `\#{...}`, `\#$...`, `\#@...`: disabling interpolation — allowed.
    // Pattern: `\#` followed by `{`, `$`, or `@`.
    if next == b'#' {
        let after = if pos + 2 < src.len() { src[pos + 2] } else { 0 };
        if matches!(after, b'{' | b'$' | b'@') {
            return false;
        }
    }

    // `#\{...}`, `#\$...`, `#\@...`: allow when preceded by `#`.
    // This is the `#\{foo}` form that also disables interpolation.
    if matches!(next, b'{' | b'$' | b'@') && pos > 0 && src[pos - 1] == b'#' {
        return false;
    }

    // For `\#\{foo}` — allow `\#` (which is already handled above via
    // `\#` + looking at next byte). But the `\{` is redundant.
    // The `\#` case above already handles `pos+2 == b'{'`.

    // The closing delimiter: `\"` in `"..."`, or `\)` in `%(...)`, etc.
    if next == close_delim {
        return false;
    }

    // The opening delimiter for paired percent literals: `{` in `%Q{...}`, `(` in `%Q(...)`.
    // Inside nested string literals, escaping the open delimiter is necessary to break nesting.
    if open_delim != close_delim && next == open_delim {
        return false;
    }

    // Everything else is redundant.
    true
}

/// Returns a human-readable description for the escaped character.
fn describe_char(byte: u8) -> String {
    match byte {
        b'\n' => "\\n".to_string(),
        b'\t' => "\\t".to_string(),
        b' ' => "space".to_string(),
        c if c.is_ascii_graphic() => std::str::from_utf8(&[c]).unwrap_or("?").to_string(),
        _ => format!("\\x{byte:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::RedundantStringEscape;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No-offense cases: double-quoted strings ---

    #[test]
    fn no_offense_plain_string_no_escapes() {
        test::<RedundantStringEscape>().expect_no_offenses(r#""hello""#);
    }

    #[test]
    fn no_offense_escaped_backslash() {
        test::<RedundantStringEscape>().expect_no_offenses(r#""foo\\bar""#);
    }

    #[test]
    fn no_offense_escaped_newline_sequence() {
        test::<RedundantStringEscape>().expect_no_offenses(r#""foo\nbar""#);
    }

    #[test]
    fn no_offense_escaped_tab() {
        test::<RedundantStringEscape>().expect_no_offenses(r#""foo\tbar""#);
    }

    #[test]
    fn no_offense_escaped_closing_delimiter() {
        // `\"` inside a double-quoted string is necessary.
        test::<RedundantStringEscape>().expect_no_offenses(r#""foo\"bar""#);
    }

    #[test]
    fn no_offense_disable_interpolation_hash_brace() {
        // `\#{foo}` disables interpolation — not redundant.
        test::<RedundantStringEscape>().expect_no_offenses(r#""\#{foo}""#);
    }

    #[test]
    fn no_offense_disable_interpolation_bracket_hash() {
        // `#\{foo}` form.
        test::<RedundantStringEscape>().expect_no_offenses(r##""#\{foo}""##);
    }

    #[test]
    fn no_offense_disable_interpolation_hash_dollar() {
        // `\#$foo` disables gvar interpolation.
        test::<RedundantStringEscape>().expect_no_offenses(r#""\#$foo""#);
    }

    #[test]
    fn no_offense_disable_interpolation_hash_at() {
        // `\#@foo` disables ivar interpolation.
        test::<RedundantStringEscape>().expect_no_offenses(r#""\#@foo""#);
    }

    #[test]
    fn no_offense_escaped_unicode() {
        test::<RedundantStringEscape>().expect_no_offenses(r#""o""#);
    }

    #[test]
    fn no_offense_escaped_hex() {
        test::<RedundantStringEscape>().expect_no_offenses(r#""\x6f""#);
    }

    #[test]
    fn no_offense_escaped_letter_d() {
        // \d has no special meaning in strings but RuboCop allows it
        // (technically unnecessary but the cop ignores alphanumeric escapes).
        test::<RedundantStringEscape>().expect_no_offenses(r#""\d""#);
    }

    // --- Offense cases: double-quoted strings ---

    #[test]
    fn flags_escaped_semicolon() {
        test::<RedundantStringEscape>().expect_offense(indoc! {r#"
            "\;"
             ^^ Redundant escape of ; inside string literal.
        "#});
    }

    #[test]
    fn flags_escaped_hash_without_interpolation_chars() {
        // `\#` where next char is not `{`, `$`, `@` is redundant.
        test::<RedundantStringEscape>().expect_offense(indoc! {r#"
            "\#foo"
             ^^ Redundant escape of # inside string literal.
        "#});
    }

    #[test]
    fn flags_escaped_single_quote_in_double_quoted() {
        test::<RedundantStringEscape>().expect_offense(indoc! {r#"
            "\'"
             ^^ Redundant escape of ' inside string literal.
        "#});
    }

    // --- No-offense: single-quoted strings ---

    #[test]
    fn no_offense_single_quoted_semicolon() {
        // In single-quoted strings all escapes are literal backslashes — never redundant.
        test::<RedundantStringEscape>().expect_no_offenses(r"'\;'");
    }

    #[test]
    fn no_offense_single_quoted_hash() {
        test::<RedundantStringEscape>().expect_no_offenses(r"'\#'");
    }

    // --- No-offense: %q literals ---

    #[test]
    fn no_offense_percent_q_literal() {
        test::<RedundantStringEscape>().expect_no_offenses(r"%q(foo\;bar)");
    }

    // --- Offense: %Q / % literals ---

    #[test]
    fn flags_escaped_semicolon_in_percent_q_upper() {
        test::<RedundantStringEscape>().expect_offense(indoc! {r#"
            %Q(foo\;bar)
                  ^^ Redundant escape of ; inside string literal.
        "#});
    }

    #[test]
    fn flags_escaped_semicolon_in_bare_percent() {
        test::<RedundantStringEscape>().expect_offense(indoc! {r#"
            %(foo\;bar)
                 ^^ Redundant escape of ; inside string literal.
        "#});
    }

    // --- No-offense: %w ---

    #[test]
    fn no_offense_percent_w_literal() {
        // %w is non-interpolating — no redundant escapes.
        test::<RedundantStringEscape>().expect_no_offenses(r"%w[foo\;bar]");
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_escaped_semicolon() {
        test::<RedundantStringEscape>().expect_correction(
            indoc! {r#"
                "\;"
                 ^^ Redundant escape of ; inside string literal.
            "#},
            indoc! {r#"
                ";"
            "#},
        );
    }

    #[test]
    fn corrects_escaped_hash_not_interpolation() {
        test::<RedundantStringEscape>().expect_correction(
            indoc! {r#"
                "\#foo"
                 ^^ Redundant escape of # inside string literal.
            "#},
            indoc! {r##"
                "#foo"
            "##},
        );
    }

    #[test]
    fn corrects_escaped_single_quote_in_double_quoted() {
        test::<RedundantStringEscape>().expect_correction(
            indoc! {r#"
                "\'"
                 ^^ Redundant escape of ' inside string literal.
            "#},
            indoc! {r#"
                "'"
            "#},
        );
    }

    // --- Interpolated strings (Dstr) ---

    #[test]
    fn flags_escaped_semicolon_in_dstr_segment() {
        test::<RedundantStringEscape>().expect_offense(indoc! {r#"
            "foo\;#{x}"
                ^^ Redundant escape of ; inside string literal.
        "#});
    }

    #[test]
    fn no_offense_disable_interpolation_in_dstr() {
        test::<RedundantStringEscape>().expect_no_offenses(r#""\#{x}bar""#);
    }

    // --- No-offense: paired percent literal open delimiter ---

    #[test]
    fn no_offense_escaped_open_delim_in_percent_q_brace() {
        // `\{` inside `%Q{...}` is necessary to include a literal `{` that
        // would otherwise start nesting. The fix must not autocorrect this.
        test::<RedundantStringEscape>().expect_no_offenses(r"%Q{foo\{bar}");
    }

    #[test]
    fn no_offense_escaped_open_delim_in_percent_paren() {
        // `\(` inside `%Q(...)` is the paired opening delimiter — not redundant.
        test::<RedundantStringEscape>().expect_no_offenses(r"%Q(foo\(bar)");
    }

    // --- Skip regexp and xstr ---

    #[test]
    fn no_offense_regexp_literal() {
        test::<RedundantStringEscape>().expect_no_offenses(r"/\#/");
    }
}
murphy_plugin_api::submit_cop!(RedundantStringEscape);
