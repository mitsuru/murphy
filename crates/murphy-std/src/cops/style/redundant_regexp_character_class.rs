//! `Style/RedundantRegexpCharacterClass` — flags unnecessary single-element
//! `Regexp` character classes.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantRegexpCharacterClass
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - Single-element character classes `[x]` → `x` (fire offense)
//!     - Negated character classes `[^x]` → skipped (correct)
//!     - Multi-element `[ab]` → skipped (correct)
//!     - `[\b]` → skipped (backspace vs word-boundary difference)
//!     - Octal `[\1]`–`[\7]` → skipped (backreference outside class)
//!     - Chars that require escaping outside a class (`.*+?{}()|$`) → skipped
//!     - Free-space mode `x` flag with whitespace element → skipped
//!     - POSIX classes `[[:alpha:]]` → skipped
//!     - `[#]` → `\#` (prevents string interpolation)
//!     - Autocorrect: replace `[x]` with `x` (or `\#` for `[#]`)
//!   Gaps:
//!     - Interpolated regexps (multiple Str parts) are skipped conservatively.
//!     - Murphy does not use regexp_parser, so character class detection is
//!       hand-rolled on raw source bytes. Complex nested classes or
//!       multi-codepoint escapes may not be recognized and are skipped.
//! ```
//!
//! ## Detection algorithm
//!
//! For each `Regexp` node with a single `Str` part (non-interpolated), scan
//! the raw source body for single-element character classes. A character class
//! `[elem]` is redundant if:
//! 1. It is not negated (`[^...]`).
//! 2. It contains exactly one element (single char or escape sequence).
//! 3. None of the keep conditions apply.
//!
//! The offense is on the full `[elem]` range; autocorrect replaces it with
//! just `elem` (or `\#` for `[#]`).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantRegexpCharacterClass;

const MSG: &str =
    "Redundant single-element character class, `%<char_class>s` can be replaced with `%<element>s`.";

/// Characters that must be escaped when outside a character class.
/// RuboCop: `REQUIRES_ESCAPE_OUTSIDE_CHAR_CLASS_CHARS = '.*+?{}()|$'.chars`
const REQUIRES_ESCAPE_OUTSIDE: &[u8] = b".*+?{}()|$";

#[cop(
    name = "Style/RedundantRegexpCharacterClass",
    description = "Checks for unnecessary single-element Regexp character classes.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantRegexpCharacterClass {
    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Regexp { parts, opts } = *cx.kind(node) else {
        return;
    };

    let parts_list = cx.list(parts);

    // Only handle non-interpolated regexps (single Str part).
    if parts_list.len() != 1 {
        return;
    }
    if !matches!(cx.kind(parts_list[0]), NodeKind::Str(_)) {
        return;
    }

    // Check if the regexp has the `x` (free-space / extended) flag.
    let flags = cx.symbol_str(opts);
    let is_extended = flags.contains('x');

    // Get the raw source of the whole regexp node (e.g. "/[x]/i").
    let regexp_range = cx.range(node);
    let full_src = cx.raw_source(regexp_range);
    let full_bytes = full_src.as_bytes();

    // Determine the body range within the full source.
    let (body_start, body_end) = find_regexp_body_bounds(full_bytes);
    if body_start >= body_end {
        return;
    }

    let body = &full_bytes[body_start..body_end];
    let body_offset = regexp_range.start + body_start as u32;

    scan_body(body, body_offset, cx, is_extended);
}

/// Returns `(body_start, body_end)` as byte offsets within `full_bytes`.
fn find_regexp_body_bounds(bytes: &[u8]) -> (usize, usize) {
    if bytes.is_empty() {
        return (0, 0);
    }
    if bytes[0] == b'/' {
        // /body/flags — find the closing /
        let mut i = 1;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'/' {
                return (1, i);
            }
            i += 1;
        }
        (1, bytes.len())
    } else if bytes.starts_with(b"%r") && bytes.len() >= 3 {
        // %r{body}flags or %r[body]flags etc.
        let open = bytes[2];
        let close = matching_close(open);
        let body_start = 3;
        if open == close {
            // Non-paired delimiter — find the next occurrence.
            let mut i = body_start;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == close {
                    return (body_start, i);
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
                        return (body_start, i);
                    }
                }
                i += 1;
            }
        }
        (body_start, bytes.len())
    } else {
        (0, 0)
    }
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

/// Scan the regexp body bytes and emit offenses for redundant character classes.
fn scan_body(body: &[u8], body_offset: u32, cx: &Cx<'_>, is_extended: bool) {
    let mut i = 0;
    while i < body.len() {
        match body[i] {
            b'\\' => {
                // Skip escape sequence.
                i += 2;
            }
            b'[' => {
                // Try to parse a character class starting at `i`.
                if let Some((class_end, element)) = parse_single_element_class(body, i) {
                    if is_redundant(&element, is_extended) {
                        let class_src =
                            std::str::from_utf8(&body[i..=class_end]).unwrap_or("");
                        let replacement = replacement_for(&element);
                        let msg = MSG
                            .replace("%<char_class>s", class_src)
                            .replace("%<element>s", &replacement);

                        let offense_start = body_offset + i as u32;
                        let offense_end = body_offset + class_end as u32 + 1;
                        let offense_range = Range {
                            start: offense_start,
                            end: offense_end,
                        };
                        cx.emit_offense(offense_range, &msg, None);
                        cx.emit_edit(offense_range, &replacement);

                        i = class_end + 1;
                        continue;
                    }
                    i = class_end + 1;
                } else {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
}

/// Attempt to parse a single-element character class starting at `start` in `body`.
/// `body[start]` must be `[`.
/// Returns `(close_idx, element_bytes)` where `close_idx` is the index of `]`.
/// Returns `None` if the class is negated, multi-element, empty, or malformed.
fn parse_single_element_class(body: &[u8], start: usize) -> Option<(usize, Vec<u8>)> {
    debug_assert_eq!(body[start], b'[');

    let mut i = start + 1;

    // Skip negated classes.
    if i < body.len() && body[i] == b'^' {
        return None;
    }

    // Empty class: [].
    if i < body.len() && body[i] == b']' {
        return None;
    }

    // POSIX class: [[:alpha:]]. After `[`, check if next is `[` (nested) or `:`.
    if i < body.len() && body[i] == b'[' {
        return None;
    }
    if i < body.len() && body[i] == b':' {
        return None;
    }

    // Read the first element.
    let elem = read_one_element(body, &mut i)?;

    // After reading the element, we must be at `]`.
    if i >= body.len() || body[i] != b']' {
        // Multi-element or unterminated.
        return None;
    }

    let close_idx = i;
    Some((close_idx, elem))
}

/// Read one "element" from the body starting at `*i`, advancing `*i` past it.
/// An element is one of:
/// - A single non-special byte
/// - A backslash escape `\X` (two bytes)
/// Returns the raw bytes of the element, or `None` if we hit EOF unexpectedly.
fn read_one_element(body: &[u8], i: &mut usize) -> Option<Vec<u8>> {
    if *i >= body.len() {
        return None;
    }
    if body[*i] == b'\\' {
        // Backslash escape.
        if *i + 1 >= body.len() {
            return None;
        }
        let elem = vec![body[*i], body[*i + 1]];
        *i += 2;
        Some(elem)
    } else if body[*i] == b'[' {
        // Nested character class — treat as multi-element (complex), return None.
        None
    } else {
        let elem = vec![body[*i]];
        *i += 1;
        Some(elem)
    }
}

/// Returns true if the character class `[elem]` is redundant (safe to remove brackets).
fn is_redundant(elem: &[u8], is_extended: bool) -> bool {
    if elem.is_empty() {
        return false;
    }

    if elem.len() == 2 && elem[0] == b'\\' {
        let c = elem[1];
        // `[\b]` — backspace inside class vs word-boundary outside.
        if c == b'b' {
            return false;
        }
        // Octal `[\1]`–`[\7]` — backreference outside class.
        if c.is_ascii_digit() && c >= b'1' && c <= b'7' {
            return false;
        }
        // Other backslash escapes: `[\s]`, `[\d]`, `[\n]`, `[\.]` → redundant.
        return true;
    }

    if elem.len() == 1 {
        let c = elem[0];
        // Chars requiring escape outside a character class.
        if REQUIRES_ESCAPE_OUTSIDE.contains(&c) {
            return false;
        }
        // Whitespace in free-space mode (extended flag `x`).
        if is_extended && (c == b' ' || c == b'\t' || c == b'\n' || c == b'\r') {
            return false;
        }
        // Everything else (including `[#]`) is redundant.
        return true;
    }

    // Multi-byte element — conservative: not redundant.
    false
}

/// Returns the replacement string for removing the character class brackets.
fn replacement_for(elem: &[u8]) -> String {
    if elem == b"#" {
        // `[#]` → `\#` to prevent string interpolation.
        return "\\#".to_string();
    }
    String::from_utf8_lossy(elem).into_owned()
}

#[cfg(test)]
mod tests {
    use super::RedundantRegexpCharacterClass;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Offense cases -----

    #[test]
    fn flags_single_char_class() {
        test::<RedundantRegexpCharacterClass>().expect_correction(
            indoc! {r#"
                r = /[x]/
                     ^^^ Redundant single-element character class, `[x]` can be replaced with `x`.
            "#},
            "r = /x/\n",
        );
    }

    #[test]
    fn flags_single_escape_class_whitespace() {
        test::<RedundantRegexpCharacterClass>().expect_correction(
            indoc! {"
                r = /[\\s]/
                     ^^^^ Redundant single-element character class, `[\\s]` can be replaced with `\\s`.
            "},
            "r = /\\s/\n",
        );
    }

    #[test]
    fn flags_percent_r_with_single_char() {
        test::<RedundantRegexpCharacterClass>().expect_correction(
            indoc! {r#"
                r = %r{/[b]}
                        ^^^ Redundant single-element character class, `[b]` can be replaced with `b`.
            "#},
            "r = %r{/b}\n",
        );
    }

    #[test]
    fn flags_hash_class_with_backslash_replacement() {
        test::<RedundantRegexpCharacterClass>().expect_correction(
            indoc! {r#"
                r = /[#]/
                     ^^^ Redundant single-element character class, `[#]` can be replaced with `\#`.
            "#},
            "r = /\\#/\n",
        );
    }

    #[test]
    fn flags_newline_escape_class() {
        test::<RedundantRegexpCharacterClass>().expect_correction(
            indoc! {"
                r = /[\\n]/
                     ^^^^ Redundant single-element character class, `[\\n]` can be replaced with `\\n`.
            "},
            "r = /\\n/\n",
        );
    }

    // ----- No-offense cases -----

    #[test]
    fn no_offense_multi_element_class() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[ab]/\n");
    }

    #[test]
    fn no_offense_negated_class() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[^x]/\n");
    }

    #[test]
    fn no_offense_backslash_b() {
        // [\b] = backspace inside class; \b = word boundary outside.
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[\\b]/\n");
    }

    #[test]
    fn no_offense_octal_backreference() {
        // [\1] = octal char inside class; \1 = backreference outside.
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[\\1]/\n");
    }

    #[test]
    fn no_offense_metachar_dot() {
        // [.] is a literal dot; . outside is any char.
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[.]/\n");
    }

    #[test]
    fn no_offense_metachar_star() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[*]/\n");
    }

    #[test]
    fn no_offense_metachar_plus() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[+]/\n");
    }

    #[test]
    fn no_offense_metachar_question() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[?]/\n");
    }

    #[test]
    fn no_offense_metachar_dollar() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[$]/\n");
    }

    #[test]
    fn no_offense_metachar_pipe() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[|]/\n");
    }

    #[test]
    fn no_offense_metachar_open_paren() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[(]/\n");
    }

    #[test]
    fn no_offense_metachar_close_paren() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[)]/\n");
    }

    #[test]
    fn no_offense_metachar_open_brace() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[{]/\n");
    }

    #[test]
    fn no_offense_metachar_close_brace() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[}]/\n");
    }

    #[test]
    fn no_offense_posix_class() {
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[[:alpha:]]/\n");
    }

    #[test]
    fn no_offense_free_space_whitespace_class() {
        // In extended mode, whitespace is significant.
        test::<RedundantRegexpCharacterClass>().expect_no_offenses("r = /[ ]/x\n");
    }

    #[test]
    fn flags_non_whitespace_in_extended_mode() {
        test::<RedundantRegexpCharacterClass>().expect_correction(
            indoc! {r#"
                r = /[x]/x
                     ^^^ Redundant single-element character class, `[x]` can be replaced with `x`.
            "#},
            "r = /x/x\n",
        );
    }
}
murphy_plugin_api::submit_cop!(RedundantRegexpCharacterClass);
