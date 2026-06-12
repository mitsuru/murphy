//! `Lint/DuplicateRegexpCharacterClassElement` — flag a duplicate element
//! inside a `Regexp` character class (`/[xyx]/`, `/[0-9x0-9]/`).
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DuplicateRegexpCharacterClassElement
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   RuboCop walks regexp-parser's parsed tree (`node.parsed_tree.each_expression`)
//!   and dedups the members of each `:set` (character class) by `group.to_s`.
//!   Murphy does not expose a regexp parsed tree through the plugin API, so the
//!   character classes are scanned from the raw regexp source, modelled on
//!   `Lint/MixedCaseRange`'s raw-source scanner. Elements are tokenized as:
//!   POSIX classes (`[:alpha:]` / `[:^alpha:]`), escapes (`\d`, `\x41`,
//!   `\u{1F600}`, octal, etc.), merged ranges (`0-9`, `\x41-\x5A`), and single
//!   characters. A `-` at the class start/end or immediately after a completed
//!   range is a literal element, not a range operator. The element source is
//!   compared exactly (matching RuboCop's string identity), so semantic overlaps
//!   like `[a-cb]` are intentionally not flagged. The duplicate's full span is
//!   removed by the autocorrect (unsafe in RuboCop as well, since regexp meaning
//!   can subtly change).
//!
//!   Known v1 limitations (conservative skips rather than guesses):
//!   - Interpolated regexp bodies (`/[a#{x}b]/`) are skipped wholesale, mirroring
//!     `Lint/MixedCaseRange`. RuboCop blanks interpolations and scans the rest;
//!     Murphy lacks regexp-parser token offsets to do that safely.
//!   - Set intersection (`[a-z&&[^aeiou]]`) is not specially handled; the `&&`
//!     operator and its operands are tokenized as ordinary members. RuboCop skips
//!     `:intersection` sets entirely. False positives there are avoided because
//!     the operand sub-classes start a nested `[`, which terminates the scan.
//! ```

use std::collections::HashSet;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Duplicate element inside regexp character class";

#[derive(Default)]
pub struct DuplicateRegexpCharacterClassElement;

#[cop(
    name = "Lint/DuplicateRegexpCharacterClassElement",
    description = "Checks for duplicate elements in Regexp character classes.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicateRegexpCharacterClassElement {
    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Regexp { parts, .. } = *cx.kind(node) else {
            return;
        };
        // Interpolated regexp bodies need regexp-parser-level token positions
        // to blank the interpolation regions; skip them conservatively.
        if cx.list(parts).iter().any(|&part| !matches!(cx.kind(part), NodeKind::Str(_))) {
            return;
        }

        let node_range = cx.range(node);
        let src = cx.raw_source(node_range).as_bytes();
        let Some((body_start, body_end)) = regexp_body_bounds(src) else {
            return;
        };
        scan_regexp_body(&src[body_start..body_end], node_range.start + body_start as u32, cx);
    }
}

/// Returns `(body_start, body_end)` byte offsets of the regexp body (the part
/// between the delimiters), for `/.../ ` and `%r{...}` literals. Lifted from
/// `Lint/MixedCaseRange`.
fn regexp_body_bounds(src: &[u8]) -> Option<(usize, usize)> {
    if src.first() == Some(&b'/') {
        let mut i = 1;
        while i < src.len() {
            if src[i] == b'\\' {
                i += 2;
            } else if src[i] == b'/' {
                return Some((1, i));
            } else {
                i += 1;
            }
        }
        return None;
    }
    if src.starts_with(b"%r") && src.len() >= 3 {
        let open = src[2];
        let close = matching_close(open);
        let mut i = 3;
        let mut depth = 1usize;
        while i < src.len() {
            if src[i] == b'\\' {
                i += 2;
            } else if open != close && src[i] == open {
                depth += 1;
                i += 1;
            } else if src[i] == close {
                depth -= 1;
                if depth == 0 {
                    return Some((3, i));
                }
                i += 1;
            } else {
                i += 1;
            }
        }
    }
    None
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

/// Walk the regexp body, finding each top-level character class `[...]` and
/// dedup'ing its members.
fn scan_regexp_body(body: &[u8], body_offset: u32, cx: &Cx<'_>) {
    let mut i = 0usize;
    while i < body.len() {
        if body[i] == b'\\' {
            i += 2;
            continue;
        }
        if body[i] != b'[' {
            i += 1;
            continue;
        }
        let Some(end) = find_char_class_end(body, i + 1) else {
            // Unterminated / nested class we can't reason about — bail.
            return;
        };
        scan_char_class(&body[i + 1..end], body_offset + i as u32 + 1, cx);
        i = end + 1;
    }
}

/// Find the `]` that closes the character class opened at `i`. POSIX classes
/// (`[:alpha:]`) and a leading `]`/`^]` literal are handled so the class isn't
/// truncated early. Returns `None` if a nested non-POSIX `[` is encountered
/// (we don't reason about set intersection / nesting) or the class is
/// unterminated.
fn find_char_class_end(body: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    // A `]` as the very first member (optionally after `^`) is a literal.
    if body.get(i) == Some(&b'^') {
        i += 1;
    }
    if body.get(i) == Some(&b']') {
        i += 1;
    }
    while i < body.len() {
        if body[i] == b'\\' {
            i += 2;
        } else if body[i] == b'[' {
            // POSIX class member `[:name:]` — skip to its `:]` close.
            if let Some(posix_end) = posix_class_end(body, i) {
                i = posix_end + 1;
            } else {
                // Genuine nested class — bail rather than guess.
                return None;
            }
        } else if body[i] == b']' {
            return Some(i);
        } else {
            i += 1;
        }
    }
    None
}

/// If `body[i..]` begins a POSIX class (`[:name:]` or `[:^name:]`), return the
/// index of the closing `]`, else `None`.
fn posix_class_end(body: &[u8], i: usize) -> Option<usize> {
    if body.get(i) != Some(&b'[') || body.get(i + 1) != Some(&b':') {
        return None;
    }
    let mut j = i + 2;
    while j + 1 < body.len() {
        if body[j] == b':' && body[j + 1] == b']' {
            return Some(j + 1);
        }
        // POSIX names are `[:^?alpha:]` etc — no nested brackets allowed.
        if body[j] == b'[' || body[j] == b']' {
            return None;
        }
        j += 1;
    }
    None
}

/// Tokenize a character class body (between the `[` and `]`, `^` included) into
/// its members and emit an offense+removal for each duplicate.
fn scan_char_class(class: &[u8], class_offset: u32, cx: &Cx<'_>) {
    let mut seen: HashSet<&[u8]> = HashSet::new();
    let mut i = usize::from(class.first() == Some(&b'^'));
    let len = class.len();
    // True when the element just consumed was a range (`a-b`). A `-` directly
    // following a completed range is a literal element, not a range operator
    // (e.g. `[0-9-0-9]` is `0-9`, literal `-`, `0-9`).
    let mut prev_was_range = false;

    while i < len {
        let member_start = i;
        // Consume one member (escape, POSIX class, or single char).
        i = member_end(class, i);

        // A range `member - member` merges into a single element. `-` is the
        // range operator only when it's between two members (not at class
        // start/end), is not itself escaped, and the preceding element was not
        // itself a range (so the `-` is not a dangling literal).
        if !prev_was_range && i < len && class[i] == b'-' && i + 1 < len {
            let after_dash = i + 1;
            let range_member_end = member_end(class, after_dash);
            i = range_member_end;
            let element = &class[member_start..i];
            check_member(element, member_start, class_offset, &mut seen, cx);
            prev_was_range = true;
            continue;
        }

        let member = &class[member_start..i];
        check_member(member, member_start, class_offset, &mut seen, cx);
        prev_was_range = false;
    }
}

/// Return the byte index just past a single member starting at `i`.
fn member_end(class: &[u8], i: usize) -> usize {
    if class[i] == b'\\' {
        // Escape: `\X`. `\x41`, `\u{...}`, octal, etc. are consumed in full so
        // the whole escape is one element.
        return escape_end(class, i);
    }
    if let Some(posix_end) = posix_class_end(class, i) {
        return posix_end + 1;
    }
    // Single byte. Advance over a full UTF-8 char so multi-byte members stay
    // intact. Clamp to the class length so a truncated/malformed multi-byte
    // sequence at the end can't push the index past the slice and panic on the
    // subsequent slicing.
    (i + utf8_char_len(class[i])).min(class.len())
}

/// Consume a backslash escape starting at the `\` at index `i`. Handles
/// `\xHH`, `\uHHHH`, `\u{...}`, and octal `\nnn`; otherwise a single escaped
/// char.
fn escape_end(class: &[u8], i: usize) -> usize {
    let len = class.len();
    // `i` points at `\`; the escaped char is at i+1.
    let Some(&c) = class.get(i + 1) else {
        return len; // lone trailing backslash
    };
    match c {
        b'x' => {
            // `\xHH` (1-2 hex digits).
            let mut j = i + 2;
            let mut count = 0;
            while j < len && count < 2 && class[j].is_ascii_hexdigit() {
                j += 1;
                count += 1;
            }
            j
        }
        b'u' => {
            // `\uHHHH` or `\u{...}`.
            if class.get(i + 2) == Some(&b'{') {
                let mut j = i + 3;
                while j < len && class[j] != b'}' {
                    j += 1;
                }
                if j < len {
                    j + 1
                } else {
                    j
                }
            } else {
                let mut j = i + 2;
                let mut count = 0;
                while j < len && count < 4 && class[j].is_ascii_hexdigit() {
                    j += 1;
                    count += 1;
                }
                j
            }
        }
        b'0'..=b'7' => {
            // Octal escape `\nnn` (1-3 digits).
            let mut j = i + 2;
            let mut count = 1;
            while j < len && count < 3 && (b'0'..=b'7').contains(&class[j]) {
                j += 1;
                count += 1;
            }
            j
        }
        // Clamp to `len` so a truncated multi-byte escaped char can't overrun.
        _ => (i + 1 + utf8_char_len(c)).min(len),
    }
}

fn utf8_char_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

fn check_member<'a>(
    member: &'a [u8],
    member_start: usize,
    class_offset: u32,
    seen: &mut HashSet<&'a [u8]>,
    cx: &Cx<'_>,
) {
    if member.is_empty() {
        return;
    }
    if !seen.insert(member) {
        let range = Range {
            start: class_offset + member_start as u32,
            end: class_offset + member_start as u32 + member.len() as u32,
        };
        cx.emit_offense(range, MSG, None);
        cx.emit_edit(range, "");
    }
}

murphy_plugin_api::submit_cop!(DuplicateRegexpCharacterClassElement);

#[cfg(test)]
mod tests {
    use super::DuplicateRegexpCharacterClassElement as Cop;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_single_char() {
        test::<Cop>().expect_offense(indoc! {r#"
            r = /[xyx]/
                    ^ Duplicate element inside regexp character class
        "#});
    }

    #[test]
    fn flags_duplicate_range() {
        test::<Cop>().expect_offense(indoc! {r#"
            r = /[0-9x0-9]/
                      ^^^ Duplicate element inside regexp character class
        "#});
    }

    #[test]
    fn allows_distinct_elements() {
        test::<Cop>().expect_no_offenses("r = /[xy]/\n");
    }

    #[test]
    fn allows_range_plus_char() {
        test::<Cop>().expect_no_offenses("r = /[0-9x]/\n");
    }

    #[test]
    fn semantic_overlap_is_not_flagged() {
        // `[a-cb]` — `b` is inside `a-c` but the source strings differ, so
        // RuboCop (and Murphy) do not flag it.
        test::<Cop>().expect_no_offenses("r = /[a-cb]/\n");
    }

    #[test]
    fn flags_duplicate_escape() {
        test::<Cop>().expect_offense(indoc! {r#"
            r = /[\d\d]/
                    ^^ Duplicate element inside regexp character class
        "#});
    }

    #[test]
    fn flags_duplicate_hex_escape() {
        test::<Cop>().expect_offense(indoc! {r#"
            r = /[\x41\x41]/
                      ^^^^ Duplicate element inside regexp character class
        "#});
    }

    #[test]
    fn flags_duplicate_posix_class() {
        test::<Cop>().expect_offense(indoc! {r#"
            r = /[[:alpha:][:alpha:]]/
                           ^^^^^^^^^ Duplicate element inside regexp character class
        "#});
    }

    #[test]
    fn literal_dash_at_end_is_not_a_range() {
        // `[a-]` — trailing `-` is a literal, `[a-a-]` would dup `a`.
        test::<Cop>().expect_no_offenses("r = /[a-]/\n");
    }

    #[test]
    fn dash_after_completed_range_is_literal() {
        // `[0-9-0-9]` parses as `0-9`, literal `-`, `0-9` — the trailing `0-9`
        // is a duplicate of the first range. The `-` between the ranges is a
        // literal element, not a range operator.
        test::<Cop>().expect_offense(indoc! {r#"
            r = /[0-9-0-9]/
                      ^^^ Duplicate element inside regexp character class
        "#});
    }

    #[test]
    fn flags_duplicate_in_percent_r() {
        test::<Cop>().expect_offense(indoc! {r#"
            r = %r{[xyx]}
                      ^ Duplicate element inside regexp character class
        "#});
    }

    #[test]
    fn handles_multibyte_chars_without_panic() {
        // Multi-byte members in a character class must not panic the byte-level
        // scanner. `[あい]` has two distinct multi-byte members — no offense.
        test::<Cop>().expect_no_offenses("r = /[あい]/\n");
    }

    #[test]
    fn flags_duplicate_multibyte_char() {
        test::<Cop>().expect_offense(indoc! {r#"
            r = /[ああ]/
                   ^ Duplicate element inside regexp character class
        "#});
    }

    #[test]
    fn skips_interpolated_regexp() {
        test::<Cop>().expect_no_offenses(indoc! {r#"
            r = /[a#{x}a]/
        "#});
    }

    #[test]
    fn autocorrects_duplicate_char() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                r = /[xyx]/
                        ^ Duplicate element inside regexp character class
            "#},
            "r = /[xy]/\n",
        );
    }

    #[test]
    fn autocorrects_duplicate_range() {
        test::<Cop>().expect_correction(
            indoc! {r#"
                r = /[0-9x0-9]/
                          ^^^ Duplicate element inside regexp character class
            "#},
            "r = /[0-9x]/\n",
        );
    }
}
