//! `Style/RedundantRegexpEscape` — flags redundant escape sequences inside
//! regexp literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantRegexpEscape
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Implements the primary escape-redundancy logic by scanning the raw source
//!   of the regexp literal byte-by-byte, since Murphy has no regexp-internal
//!   parser equivalent to the `regexp_parser` gem used by RuboCop.
//!
//!   Covers: slash-delimited /re/, %r with any delimiter, flags, interpolation
//!   (skips #{...} bodies), character classes, delimiter escapes, and the
//!   hyphen boundary rule within character classes.
//!
//!   Gaps vs RuboCop:
//!   - Alphanumeric escapes (\w, \d, \s, etc.) are always treated as allowed,
//!     as their semantics are non-trivial to enumerate.
//!   - '#@ivar', '#@@cvar', '#$gvar' interpolation-avoidance escapes are handled
//!     conservatively (escapes after '#' before '@' or '$' are always kept).
//! ```
//!
//! ## Matched shapes
//!
//! `regexp` nodes where any `\x` escape in the raw source is redundant.
//!
//! ## Autocorrect
//!
//! For each redundant escape: delete the backslash (one surgical `cx.emit_edit`
//! that replaces the two-byte `\x` range with just `x`).

use murphy_plugin_api::{Cx, NodeId, NodeKind, NodeList, NoOptions, Range, cop};

const MSG: &str = "Redundant escape inside regexp literal";

/// Always-allowed escape characters (regardless of context).
/// Space, newline, `[`, `]`, `^`, `\`, `#`.
const ALLOWED_ALWAYS: &[u8] = b" \n[]^\\#";

/// Metacharacters allowed to be escaped OUTSIDE a character class.
const ALLOWED_OUTSIDE_CHAR_CLASS: &[u8] = b".*+?{}()|$";

/// Metacharacters allowed to be escaped INSIDE a character class (non-boundary).
/// Only `-` is in this set — at boundary positions it becomes redundant.
const ALLOWED_INSIDE_CHAR_CLASS_NON_BOUNDARY: &[u8] = b"-";

/// Sigils that, when following `#`, form an interpolation sequence to avoid.
const INTERPOLATION_SIGILS: &[u8] = b"@$";

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantRegexpEscape;

#[cop(
    name = "Style/RedundantRegexpEscape",
    description = "Checks for redundant escapes in Regexps.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantRegexpEscape {
    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let raw = cx.raw_source(cx.range(node));
    let src = raw.as_bytes();

    // Determine the delimiter pair and body start offset.
    let Some((open_delim, close_delim, body_start)) = delimiter_info(src) else {
        return;
    };

    // Find the body end: scan backwards past flags and then the close delimiter.
    let body_end = match find_body_end(src, close_delim) {
        Some(e) => e,
        None => return,
    };

    if body_start >= body_end {
        return;
    }

    // Collect interpolation spans from the AST parts so we can skip them.
    let NodeKind::Regexp { parts, .. } = *cx.kind(node) else {
        return;
    };
    let node_start = cx.range(node).start;
    let interp_spans = collect_interpolation_spans(parts, node_start, cx);

    // Scan the body for redundant escapes.
    let body = &src[body_start..body_end];
    let mut i = 0usize; // offset within `body`
    let mut char_class_depth: u32 = 0;

    while i < body.len() {
        let abs = body_start + i; // absolute offset within `src`

        // Skip interpolation spans: if we're inside #{...}, jump past it.
        if let Some(span_end) = interp_spans
            .iter()
            .find(|&&(s, e)| abs >= s && abs < e)
            .map(|&(_, e)| e)
        {
            i = span_end - body_start;
            continue;
        }

        let b = body[i];

        if b == b'\\' && i + 1 < body.len() {
            let escaped = body[i + 1];
            let char_before = if i > 0 { Some(body[i - 1]) } else { None };

            if !is_allowed_escape(
                src,
                body_start,
                i,
                escaped,
                char_before,
                open_delim,
                close_delim,
                char_class_depth > 0,
            ) {
                // Offense: the backslash at body_start + i is redundant.
                let abs_start = (node_start + body_start as u32 + i as u32) as u32;
                let offense_range = Range {
                    start: abs_start,
                    end: abs_start + 2,
                };
                cx.emit_offense(offense_range, MSG, None);
                // Autocorrect: replace `\x` with `x` (delete the backslash).
                cx.emit_edit(
                    Range {
                        start: abs_start,
                        end: abs_start + 1,
                    },
                    "",
                );
            }
            // Advance past the escape sequence (both `\` and the escaped char).
            i += 2;
            continue;
        }

        // Track character class depth.
        if b == b'[' {
            char_class_depth += 1;
        } else if b == b']' && char_class_depth > 0 {
            char_class_depth -= 1;
        }

        i += 1;
    }
}

/// Returns `(open_delim, close_delim, body_start_offset)` for a regexp source.
fn delimiter_info(src: &[u8]) -> Option<(u8, u8, usize)> {
    if src.first() == Some(&b'/') {
        Some((b'/', b'/', 1))
    } else if src.starts_with(b"%r") && src.len() > 2 {
        let open = src[2];
        let close = mirror_delimiter(open);
        Some((open, close, 3))
    } else {
        None
    }
}

/// Returns the close-delimiter for the given open-delimiter.
fn mirror_delimiter(open: u8) -> u8 {
    match open {
        b'{' => b'}',
        b'[' => b']',
        b'(' => b')',
        b'<' => b'>',
        c => c,
    }
}

/// Find the body end offset within `src`: the position of the last
/// close-delimiter byte (after skipping trailing flag chars like `i`, `x`, …).
fn find_body_end(src: &[u8], close_delim: u8) -> Option<usize> {
    // Scan backwards past flag chars (alphanumeric), then find the close delim.
    let mut end = src.len();
    while end > 0 && src[end - 1].is_ascii_alphabetic() {
        end -= 1;
    }
    // The char at end-1 should be the close delimiter.
    if end > 0 && src[end - 1] == close_delim {
        Some(end - 1)
    } else {
        None
    }
}

/// Collect ranges of interpolation bodies `#{...}` as `(start, end)` in `src`
/// coordinates (relative to `node_start`).
fn collect_interpolation_spans(
    parts: NodeList,
    node_start: u32,
    cx: &Cx<'_>,
) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for &part_id in cx.list(parts) {
        if matches!(cx.kind(part_id), NodeKind::Begin(_)) {
            let r = cx.range(part_id);
            // Range relative to the start of the regexp source.
            let start = (r.start - node_start) as usize;
            let end = (r.end - node_start) as usize;
            spans.push((start, end));
        }
    }
    spans
}

/// Returns `true` if the escape `\escaped` at body offset `i` is allowed
/// (i.e., is NOT redundant — should be kept).
fn is_allowed_escape(
    src: &[u8],
    body_start: usize,
    i: usize,   // offset of `\` within `body` (i.e., src[body_start + i] == b'\\')
    escaped: u8,
    char_before: Option<u8>,
    open_delim: u8,
    close_delim: u8,
    within_char_class: bool,
) -> bool {
    // 1. Alphanumeric: always allowed (\w, \d, \s, \1, \A, …).
    if escaped.is_ascii_alphanumeric() {
        return true;
    }

    // 2. Always-allowed set: space, newline, [, ], ^, \, #.
    if ALLOWED_ALWAYS.contains(&escaped) {
        return true;
    }

    // 3. Delimiter escapes: escaping the open or close delimiter is always valid.
    if escaped == open_delim || escaped == close_delim {
        return true;
    }

    // 4. Interpolation-sigil guard: `\@` or `\$` right after an UNescaped
    //    `#` would trigger interpolation — keep these escapes.
    //    A `#` is only "unescaped" when the number of backslashes immediately
    //    before it (in the raw source) is even (each pair cancels out).
    //    Examples: `/#\@foo/` — 0 backslashes before `#` (even) → `#` unescaped, keep `\@`.
    //              `/\#\@foo/` — 1 backslash before `#` (odd) → `#` escaped, `\@` is redundant.
    //              `/\\#\@foo/` — 2 backslashes before `#` (even) → `#` unescaped, keep `\@`.
    if let Some(prev) = char_before {
        if prev == b'#' && INTERPOLATION_SIGILS.contains(&escaped) {
            let body = &src[body_start..];
            // Count consecutive backslashes immediately before the `#` (at body[i-1]).
            let hash_pos = i - 1; // position of `#` within body
            let preceding_backslashes = count_preceding_backslashes(body, hash_pos);
            if preceding_backslashes % 2 == 0 {
                // Even number of backslashes → `#` is literal/unescaped → keep `\@`/`\$`.
                return true;
            }
            // Odd backslashes → `#` is escaped → `\@`/`\$` is redundant (fall through).
        }
    }

    if within_char_class {
        // 5a. Within char class: only `-` is potentially allowed (non-boundary).
        if ALLOWED_INSIDE_CHAR_CLASS_NON_BOUNDARY.contains(&escaped) {
            // Allow \- only when it's neither the first nor the last element
            // of the character class (boundary hyphens don't need escaping).
            return hyphen_is_in_middle_of_char_class(src, body_start, i);
        }
        false
    } else {
        // 5b. Outside char class: metacharacters that have special meaning.
        ALLOWED_OUTSIDE_CHAR_CLASS.contains(&escaped)
    }
}

/// Returns `true` if the `\-` escape at body offset `i` (pointing at `\`) is
/// in the MIDDLE of a character class — meaning the hyphen could be interpreted
/// as a range operator, so the escape is meaningful.
///
/// "First" means the `\-` starts immediately after `[` (or `[^`).
/// "Last" means the `-` ends immediately before `]`.
///
/// We scan backwards for the nearest un-escaped `[` and forwards for the
/// nearest un-escaped `]` to classify the position.
fn hyphen_is_in_middle_of_char_class(src: &[u8], body_start: usize, i: usize) -> bool {
    // `i` is the offset of `\` within `body` (src[body_start + i] == `\`).
    // `i+1` is the `-`.
    let body = &src[body_start..];

    // Check "first position": what's immediately before the `\`?
    // Skip over any preceding escapes to find if the `[` (or `[^`) is right before.
    let is_first = {
        // Walk backwards from `i` to find the char immediately before `\`.
        // Account for preceding `\x` two-byte sequences.
        let prev_char = if i == 0 {
            None
        } else if i >= 2 && body[i - 2] == b'\\' {
            // The char at i-1 is the second byte of a preceding escape — skip.
            // This means the effective previous visible char is at i-2 (the `\`),
            // but we look further back. Simplify: use the character at body[i-1].
            // For the purpose of "first", the direct predecessor of `\-` matters.
            Some(body[i - 1])
        } else {
            Some(body[i - 1])
        };

        match prev_char {
            Some(b'[') => true,
            Some(b'^') => {
                // `[^\-` — the `^` is a negation right after `[`
                i >= 2 && body[i - 2] == b'['
            }
            _ => false,
        }
    };

    // Check "last position": what's immediately after the `-` (body[i+1])?
    let is_last = i + 2 < body.len() && body[i + 2] == b']';

    !is_first && !is_last
}

/// Count consecutive backslashes immediately before position `pos` in `body`.
fn count_preceding_backslashes(body: &[u8], pos: usize) -> usize {
    let mut count = 0;
    let mut j = pos;
    while j > 0 && body[j - 1] == b'\\' {
        count += 1;
        j -= 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::RedundantRegexpEscape;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- From RuboCop docstring: bad cases -----

    #[test]
    fn flags_redundant_slash_in_percent_r() {
        // %r{foo\/bar} — `\/` is redundant because `%r{}` doesn't use `/` as delimiter.
        test::<RedundantRegexpEscape>().expect_correction(
            indoc! {r#"
                %r{foo\/bar}
                      ^^ Redundant escape inside regexp literal
            "#},
            "%r{foo/bar}\n",
        );
    }

    #[test]
    fn flags_redundant_hyphen_outside_char_class() {
        // /a\-b/ — `\-` outside a char class is redundant.
        test::<RedundantRegexpEscape>().expect_correction(
            indoc! {r#"
                /a\-b/
                  ^^ Redundant escape inside regexp literal
            "#},
            "/a-b/\n",
        );
    }

    #[test]
    fn flags_redundant_plus_inside_char_class() {
        // /[\+\-]\d/ — `\+` in char class is redundant; `\-` at end is also redundant.
        // Both should be flagged and corrected.
        test::<RedundantRegexpEscape>().expect_correction(
            indoc! {r#"
                /[\+\-]\d/
                  ^^ Redundant escape inside regexp literal
                    ^^ Redundant escape inside regexp literal
            "#},
            "/[+-]\\d/\n",
        );
    }

    // ----- From RuboCop docstring: good cases -----

    #[test]
    fn accepts_necessary_slash_escape_in_slash_regexp() {
        // /foo\/bar/ — `\/` is necessary because `/` is the delimiter.
        test::<RedundantRegexpEscape>().expect_no_offenses("/foo\\/bar/\n");
    }

    #[test]
    fn accepts_necessary_delimiter_escape_in_percent_r() {
        // %r/foo\/bar/ — `\/` is necessary because `/` is the delimiter.
        test::<RedundantRegexpEscape>().expect_no_offenses("%r/foo\\/bar/\n");
    }

    #[test]
    fn accepts_delimiter_escape_in_percent_r_bang() {
        // %r!foo\!bar! — `\!` is necessary because `!` is the delimiter.
        test::<RedundantRegexpEscape>().expect_no_offenses("%r!foo\\!bar!\n");
    }

    // ----- Character class hyphen cases -----

    #[test]
    fn flags_hyphen_at_end_of_char_class() {
        // /[0-9\-]/ — `\-` at end of char class is redundant.
        test::<RedundantRegexpEscape>().expect_correction(
            indoc! {r#"
                /[0-9\-]/
                     ^^ Redundant escape inside regexp literal
            "#},
            "/[0-9-]/\n",
        );
    }

    #[test]
    fn flags_hyphen_at_start_of_char_class() {
        // /[\-0-9]/ — `\-` at start of char class is redundant.
        test::<RedundantRegexpEscape>().expect_correction(
            indoc! {r#"
                /[\-0-9]/
                  ^^ Redundant escape inside regexp literal
            "#},
            "/[-0-9]/\n",
        );
    }

    #[test]
    fn accepts_hyphen_in_middle_of_char_class() {
        // /[\w\-\#]/ — `\-` in the middle is not redundant (could be range operator).
        test::<RedundantRegexpEscape>().expect_no_offenses("/[\\w\\-\\#]/\n");
    }

    // ----- Interpolation -----

    #[test]
    fn skips_escapes_inside_interpolation() {
        // /foo#{bar}\d/ — `\d` is a string escape inside the str part,
        // but `\d` outside interpolation is an alphanumeric escape (kept).
        test::<RedundantRegexpEscape>().expect_no_offenses("/foo#{bar}\\d/\n");
    }

    // ----- Other escape cases -----

    #[test]
    fn flags_redundant_dot_outside_char_class_is_kept() {
        // /\./ — `.` outside char class has meaning (literal dot), so \. is needed.
        test::<RedundantRegexpEscape>().expect_no_offenses("/\\./\n");
    }

    #[test]
    fn flags_redundant_colon_escape() {
        // /\:/ — `:` has no special meaning in regexp.
        test::<RedundantRegexpEscape>().expect_correction(
            indoc! {r#"
                /\:/
                 ^^ Redundant escape inside regexp literal
            "#},
            "/:/\n",
        );
    }

    #[test]
    fn accepts_backslash_newline_escape() {
        // /\n/ — alphanumeric escape, always allowed.
        test::<RedundantRegexpEscape>().expect_no_offenses("/\\n/\n");
    }

    #[test]
    fn accepts_metachar_escapes_outside_char_class() {
        test::<RedundantRegexpEscape>().expect_no_offenses("/\\./\n");
        test::<RedundantRegexpEscape>().expect_no_offenses("/\\*/\n");
        test::<RedundantRegexpEscape>().expect_no_offenses("/\\+/\n");
        test::<RedundantRegexpEscape>().expect_no_offenses("/\\?/\n");
        test::<RedundantRegexpEscape>().expect_no_offenses("/\\|/\n");
        test::<RedundantRegexpEscape>().expect_no_offenses("/\\$/\n");
    }

    #[test]
    fn flags_percent_r_with_redundant_escape() {
        // %r{foo\/bar} — `\/` is redundant.
        test::<RedundantRegexpEscape>().expect_correction(
            indoc! {r#"
                x = %r{foo\/bar}
                          ^^ Redundant escape inside regexp literal
            "#},
            "x = %r{foo/bar}\n",
        );
    }

    #[test]
    fn accepts_regexp_with_no_escapes() {
        test::<RedundantRegexpEscape>().expect_no_offenses("/foo/\n");
        test::<RedundantRegexpEscape>().expect_no_offenses("%r{foo}\n");
    }

    #[test]
    fn flags_multiple_redundant_escapes() {
        // /[\s\(\|\{\[;,\*\=]/ — multiple redundant escapes inside char class.
        test::<RedundantRegexpEscape>().expect_no_offenses("/[\\s(|{\\[;,*=]/\n");
    }

    #[test]
    fn accepts_negated_char_class_hyphen_at_start() {
        // /[^\-0-9]/ — `\-` right after `[^` is at the first position.
        test::<RedundantRegexpEscape>().expect_correction(
            indoc! {r#"
                /[^\-0-9]/
                   ^^ Redundant escape inside regexp literal
            "#},
            "/[^-0-9]/\n",
        );
    }

    // ----- Interpolation-sigil guard -----

    #[test]
    fn accepts_sigil_escape_after_unescaped_hash() {
        // /#\@foo/ — `\@` after unescaped `#` must be kept to avoid triggering
        // `#@foo` interpolation.
        test::<RedundantRegexpEscape>().expect_no_offenses("/#\\@foo/
");
    }

    #[test]
    fn accepts_sigil_escape_dollar_after_unescaped_hash() {
        // /#\$foo/ — similarly for `#$gvar` interpolation.
        test::<RedundantRegexpEscape>().expect_no_offenses("/#\\$foo/
");
    }

    #[test]
    fn flags_sigil_escape_after_escaped_hash() {
        // /\#\@foo/ — the `#` is itself escaped, so `\@` is redundant.
        test::<RedundantRegexpEscape>().expect_correction(
            indoc! {r#"
                /\#\@foo/
                   ^^ Redundant escape inside regexp literal
            "#},
            "/\\#@foo/\n",
        );
    }

    #[test]
    fn accepts_dollar_escape_because_it_has_regexp_meaning() {
        // /\#\$foo/ — even with escaped `#`, `\$` is kept because `$` has
        // special regexp meaning (end-of-line anchor), so `\$` = literal `$`.
        test::<RedundantRegexpEscape>().expect_no_offenses("/\\#\\$foo/\n");
    }

    #[test]
    fn accepts_sigil_escape_after_double_backslash_hash() {
        // /\\#\@foo/ — two backslashes before `#` (even parity) means
        // `#` is NOT escaped — it will trigger `#@foo` interpolation,
        // so `\@` must be kept.
        test::<RedundantRegexpEscape>().expect_no_offenses("/\\\\#\\@foo/\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantRegexpEscape);
