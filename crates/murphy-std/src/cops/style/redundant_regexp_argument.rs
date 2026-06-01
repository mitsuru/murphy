//! `Style/RedundantRegexpArgument` — flags method calls where a deterministic
//! regexp argument can be replaced with a string.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantRegexpArgument
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - byteindex, byterindex, gsub, gsub!, partition, rpartition,
//!       scan, split, start_with?, sub, sub!
//!     - Regexp flags present → no offense
//!     - Single-space content → no offense (awk-split semantics)
//!     - Interpolated regexps (non-Str parts) → skipped conservatively
//!     - Autocorrect: replace the whole regexp argument with a quoted string
//!     - both send and csend are handled (mirrors RuboCop's alias on_csend)
//!   Gaps:
//!     - EnforcedStyle (single vs double quotes) from Style/StringLiterals
//!       is not consulted; Murphy always prefers single quotes unless the
//!       content requires double (mirrors RuboCop's fallback to single_quotes).
//! ```
//!
//! ## Detection algorithm
//!
//! A regexp argument is "deterministic" if its full source representation
//! (delimiters included, e.g. `/f/`) matches `\A(?:LITERAL_UNIT)+\Z` where
//! `LITERAL_UNIT` is one of:
//!   - A word character, whitespace, or one of `-, " ' ! # % & < > = ; : ~ /`
//!   - A backslash followed by a char NOT in `[AbBdDgGhHkpPRwWXsSzZ0-9]`
//!
//! When deterministic, the cop builds a preferred string argument by:
//! 1. Extracting the regexp body (content between delimiters).
//! 2. Stripping `\` from escapes that are not in `STR_SPECIAL_CHARS`.
//! 3. Choosing appropriate quotes (single vs double) based on content.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantRegexpArgument;

const MSG: &str = "Use string `%<prefer>s` as argument instead of regexp `%<current>s`.";

/// Method names that trigger this cop.
const FLAGGED_METHODS: &[&str] = &[
    "byteindex",
    "byterindex",
    "gsub",
    "gsub!",
    "partition",
    "rpartition",
    "scan",
    "split",
    "start_with?",
    "sub",
    "sub!",
];

/// Characters that have special meaning in strings when preceded by `\`.
/// From RuboCop's `STR_SPECIAL_CHARS`.
const STR_SPECIAL_CHARS: &[&[u8]] = &[
    b"\\a", b"\\c", b"\\C", b"\\e", b"\\f", b"\\M", b"\\n", b"\\\"", b"\\'",
    b"\\\\", b"\\t", b"\\b", b"\\f", b"\\r", b"\\u", b"\\v", b"\\x",
    b"\\0", b"\\1", b"\\2", b"\\3", b"\\4", b"\\5", b"\\6", b"\\7",
];

/// Characters in the non-metacharacter exclusion set for the backslash branch
/// of `LITERAL_REGEX`. `\\X` is a literal match when X is NOT in this set.
/// From: `\\[^AbBdDgGhHkpPRwWXsSzZ0-9]`
const REGEXP_METACLASS_CHARS: &[u8] =
    b"AbBdDgGhHkpPRwWXsSzZ0123456789";

#[cop(
    name = "Style/RedundantRegexpArgument",
    description = "Identifies places where argument can be replaced from a deterministic regexp to a string.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantRegexpArgument {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method = match cx.method_name(node) {
        Some(m) => m,
        None => return,
    };

    if !FLAGGED_METHODS.contains(&method) {
        return;
    }

    let args = cx.call_arguments(node);
    if args.is_empty() {
        return;
    }

    let first_arg = args[0];

    // First argument must be a regexp.
    let (parts, opts) = match *cx.kind(first_arg) {
        NodeKind::Regexp { parts, opts } => (parts, opts),
        _ => return,
    };

    // Skip if regexp has any flags.
    let flags = cx.symbol_str(opts);
    if !flags.is_empty() {
        return;
    }

    // Skip if regexp is interpolated (non-Str parts).
    let parts_list = cx.list(parts);
    if !parts_list.iter().all(|&p| matches!(cx.kind(p), NodeKind::Str(_))) {
        return;
    }

    // Get the full raw source of the regexp node.
    let regexp_range = cx.range(first_arg);
    let regexp_src = cx.raw_source(regexp_range);

    // Skip single-space regexp: `/  /` (awk-split special case).
    let regexp_content = regexp_body_content(regexp_src);
    if regexp_content == " " {
        return;
    }

    // Check if the regexp source is deterministic.
    if !is_deterministic(regexp_src) {
        return;
    }

    // Build the preferred string argument.
    let prefer = preferred_argument(regexp_content);

    let msg = MSG
        .replace("%<prefer>s", &prefer)
        .replace("%<current>s", regexp_src);

    cx.emit_offense(regexp_range, &msg, None);
    cx.emit_edit(regexp_range, &prefer);
}

/// Extract the body content of a regexp (between its delimiters).
fn regexp_body_content(src: &str) -> &str {
    let bytes = src.as_bytes();
    if bytes.is_empty() {
        return src;
    }
    if bytes[0] == b'/' {
        // /body/ or /body/flags — find the closing /
        let mut i = 1;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'/' {
                return &src[1..i];
            }
            i += 1;
        }
        return &src[1..];
    }
    if bytes.starts_with(b"%r") && bytes.len() >= 3 {
        let open = bytes[2];
        let close = matching_close(open);
        let body_start = 3;
        if open == close {
            let mut i = body_start;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == close {
                    return &src[body_start..i];
                }
                i += 1;
            }
        } else {
            let mut depth = 1usize;
            let mut i = body_start;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == open {
                    depth += 1;
                } else if bytes[i] == close {
                    depth -= 1;
                    if depth == 0 {
                        return &src[body_start..i];
                    }
                }
                i += 1;
            }
        }
        return &src[body_start..];
    }
    src
}

fn matching_close(open: u8) -> u8 {
    match open {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        other => other,
    }
}

/// Check if the full regexp source (including delimiters) is deterministic.
/// Mirrors `DETERMINISTIC_REGEX = /\A(?:#{LITERAL_REGEX})+\Z/`.
///
/// `LITERAL_REGEX = %r{[\w\s\-,"'!#%&<>=;:`~/]|\\[^AbBdDgGhHkpPRwWXsSzZ0-9]}`
fn is_deterministic(src: &str) -> bool {
    if src.is_empty() {
        return false;
    }
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut matched_any = false;

    while i < bytes.len() {
        if let Some(len) = match_literal_unit(bytes, i) {
            i += len;
            matched_any = true;
        } else {
            return false;
        }
    }
    matched_any
}

/// Attempt to match one LITERAL_UNIT at position `i` in `bytes`.
/// Returns the length of the match, or `None` if no match.
fn match_literal_unit(bytes: &[u8], i: usize) -> Option<usize> {
    if i >= bytes.len() {
        return None;
    }
    let c = bytes[i];

    // Branch 1: Backslash followed by a non-metachar.
    if c == b'\\' {
        if i + 1 >= bytes.len() {
            return None;
        }
        let next = bytes[i + 1];
        if !REGEXP_METACLASS_CHARS.contains(&next) {
            return Some(2);
        }
        return None;
    }

    // Branch 2: One of the literal character set:
    // [\w\s\-,"'!#%&<>=;:`~/]
    if is_literal_char(c) {
        return Some(1);
    }

    None
}

/// Check if a byte is in the `[\w\s\-,"'!#%&<>=;:\`~/]` character set.
fn is_literal_char(c: u8) -> bool {
    // \w = [a-zA-Z0-9_]
    if c.is_ascii_alphanumeric() || c == b'_' {
        return true;
    }
    // \s = whitespace
    if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == b'\x0C' || c == b'\x0B' {
        return true;
    }
    // Explicit chars: -, " ' ! # % & < > = ; : ` ~ /
    matches!(
        c,
        b'-' | b'"' | b'\'' | b'!' | b'#' | b'%' | b'&' | b'<' | b'>' | b'='
            | b';' | b':' | b'`' | b'~' | b'/'
    )
}

/// Build the preferred string argument from regexp content.
/// Mirrors RuboCop's `preferred_argument` method.
fn preferred_argument(regexp_content: &str) -> String {
    let new_argument = build_replacement(regexp_content);
    choose_quotes(&new_argument)
}

/// Build the raw replacement string by stripping non-special backslashes.
/// Mirrors RuboCop's `replacement` method.
fn build_replacement(regexp_content: &str) -> String {
    let bytes = regexp_content.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            let escape = &bytes[i..i + 2];
            if STR_SPECIAL_CHARS.contains(&escape) {
                // Keep the escape as-is.
                result.push(bytes[i]);
                result.push(bytes[i + 1]);
            } else {
                // Strip the backslash.
                result.push(bytes[i + 1]);
            }
            i += 2;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }

    String::from_utf8_lossy(&result).into_owned()
}

/// Choose the appropriate quotes for the replacement string.
/// Mirrors RuboCop's `preferred_argument` quote-selection logic.
fn choose_quotes(s: &str) -> String {
    // If it contains `"`, prefer single quotes.
    if s.contains('"') {
        let escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
        // But unescape `\"` back to `"`.
        let escaped = escaped.replace("\\\"", "\"");
        return format!("'{escaped}'");
    }
    // If it contains `\'`, prefer single quotes (already escaped).
    if s.contains("\\'") {
        let escaped = escape_even_backslash_single_quotes(s);
        return format!("'{escaped}'");
    }
    // If it contains `'`, prefer single quotes with escaped apostrophe.
    if s.contains('\'') {
        let escaped = s.replace('\'', "\\'");
        return format!("'{escaped}'");
    }
    // If it contains `\`, prefer double quotes.
    if s.contains('\\') {
        return format!("\"{s}\"");
    }
    // Default: single quotes.
    format!("'{s}'")
}

/// For the `\'` case: add a backslash before single quotes preceded by an
/// even number of backslashes (i.e., unescaped quotes).
fn escape_even_backslash_single_quotes(s: &str) -> String {
    // Matches: zero or even number of backslashes followed by a single quote
    // that is not already preceded by an odd number of backslashes.
    // Simplified: scan and track backslash count.
    let mut result = String::with_capacity(s.len() + 4);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // Count consecutive backslashes.
            let start = i;
            while i < bytes.len() && bytes[i] == b'\\' {
                i += 1;
            }
            let backslash_count = i - start;
            // Add the backslashes.
            result.push_str(&s[start..i]);
            // If next char is `'` and backslash_count is even, add escaping.
            if i < bytes.len() && bytes[i] == b'\'' {
                if backslash_count % 2 == 0 {
                    result.push('\\');
                }
                result.push('\'');
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::RedundantRegexpArgument;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Offense cases -----

    #[test]
    fn flags_gsub_simple_char() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                'foo'.gsub(/f/, 'x')
                           ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "'foo'.gsub('f', 'x')\n",
        );
    }

    #[test]
    fn flags_gsub_bang() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                str.gsub!(/f/, 'x')
                          ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "str.gsub!('f', 'x')\n",
        );
    }

    #[test]
    fn flags_split() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                'foo'.split(/f/)
                            ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "'foo'.split('f')\n",
        );
    }

    #[test]
    fn flags_scan() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                'foo'.scan(/f/)
                           ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "'foo'.scan('f')\n",
        );
    }

    #[test]
    fn flags_partition() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                'foo'.partition(/f/)
                                ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "'foo'.partition('f')\n",
        );
    }

    #[test]
    fn flags_rpartition() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                'foo'.rpartition(/f/)
                                 ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "'foo'.rpartition('f')\n",
        );
    }

    #[test]
    fn flags_start_with() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                'foo'.start_with?(/f/)
                                  ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "'foo'.start_with?('f')\n",
        );
    }

    #[test]
    fn flags_sub() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                'foo'.sub(/f/, 'x')
                          ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "'foo'.sub('f', 'x')\n",
        );
    }

    #[test]
    fn flags_sub_bang() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                str.sub!(/f/, 'x')
                         ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "str.sub!('f', 'x')\n",
        );
    }

    #[test]
    fn flags_byteindex() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                'foo'.byteindex(/f/)
                                ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "'foo'.byteindex('f')\n",
        );
    }

    #[test]
    fn flags_byterindex() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                'foo'.byterindex(/f/)
                                 ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "'foo'.byterindex('f')\n",
        );
    }

    #[test]
    fn flags_csend() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                str&.gsub(/f/, 'x')
                          ^^^ Use string `'f'` as argument instead of regexp `/f/`.
            "#},
            "str&.gsub('f', 'x')\n",
        );
    }

    #[test]
    fn flags_escaped_dot() {
        // /\./ is deterministic \u2192 '.'
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                'foo'.gsub(/\./, 'x')
                           ^^^^ Use string `'.'` as argument instead of regexp `/\./`.
            "#},
            "'foo'.gsub('.', 'x')\n",
        );
    }

    #[test]
    fn flags_multi_char_regexp() {
        test::<RedundantRegexpArgument>().expect_correction(
            indoc! {r#"
                'foo'.gsub(/foo/, 'x')
                           ^^^^^ Use string `'foo'` as argument instead of regexp `/foo/`.
            "#},
            "'foo'.gsub('foo', 'x')\n",
        );
    }

    // ----- No-offense cases -----

    #[test]
    fn no_offense_regexp_with_flags() {
        test::<RedundantRegexpArgument>().expect_no_offenses("'foo'.gsub(/f/i, 'x')\n");
    }

    #[test]
    fn no_offense_single_space_regexp() {
        // Single space: special awk-split semantics.
        test::<RedundantRegexpArgument>().expect_no_offenses("'foo'.split(/ /)\n");
    }

    #[test]
    fn no_offense_regexp_metachar_dot() {
        // /f.o/ \u2014 `.` is not in LITERAL_REGEX (not an escaped dot) \u2014 not deterministic.
        test::<RedundantRegexpArgument>().expect_no_offenses("'foo'.gsub(/f.o/, 'x')\n");
    }

    #[test]
    fn no_offense_regexp_metachar_plus() {
        test::<RedundantRegexpArgument>().expect_no_offenses("'foo'.gsub(/f+/, 'x')\n");
    }

    #[test]
    fn no_offense_regexp_class_shorthand() {
        // /\d/ \u2014 `\d` is in the metaclass exclusion set \u2192 not deterministic.
        test::<RedundantRegexpArgument>().expect_no_offenses("'foo'.gsub(/\\d/, 'x')\n");
    }

    #[test]
    fn no_offense_non_flagged_method() {
        test::<RedundantRegexpArgument>().expect_no_offenses("'foo'.match(/f/)\n");
    }

    #[test]
    fn no_offense_string_argument_already() {
        test::<RedundantRegexpArgument>().expect_no_offenses("'foo'.gsub('f', 'x')\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantRegexpArgument);
