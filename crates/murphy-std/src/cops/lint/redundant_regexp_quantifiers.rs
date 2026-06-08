//! `Lint/RedundantRegexpQuantifiers` — flags redundant quantifiers inside regexp patterns.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantRegexpQuantifiers
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ported from RuboCop Lint/RedundantRegexpQuantifiers.
//!   Covered:
//!     - Duplicate greedy quantifiers: `(?:x+)+`, `(?:x*)*`, `(?:x?)?`
//!     - Mixed greedy quantifiers: `(?:x+)?`, `(?:x*)+`, etc. → `(?:x*)`
//!     - Normalized interval quantifiers: `{1,}` → `+`, `{,1}` → `?`
//!     - Character classes with inner quantifier: `(?:[abc]+)+`
//!     - Nested non-capturing groups: `(?:(?:a)?)+`, `(?:(?:(?:a)?))+)`
//!     - Free-space mode `/x` with whitespace inside groups
//!     - Autocorrect: replaces inner quantifier and drops outer
//!   Gaps vs RuboCop:
//!     - Murphy does not have a regexp AST parser; detection is hand-rolled
//!       on raw source bytes. Complex nested structures may not be fully
//!       recognized.
//!     - Non-greedy (reluctant/possessive) quantifiers are skipped conservatively.
//!     - Deeply nested groups with complex content may produce false negatives.
//! ```
//!
//! ## Matched shapes
//!
//! - `(?:x+)+` — duplicate `+` quantifiers
//! - `(?:x*)*` — duplicate `*` quantifiers
//! - `(?:x?)?` — duplicate `?` quantifiers
//! - `(?:x+)?` — mixed quantifiers → `(?:x*)`
//! - `(?:x+)` — single quantifier only — no offense
//! - `(?:x)?` — single outer quantifier with no inner — no offense
//! - `(?:ab+)+` — multiple children — no offense
//! - `(?:a|b+)+` — alternation — no offense
//! - `(a+)+` — capturing group — no offense
//!
//! ## Autocorrect
//!
//! Drops the outer quantifier and replaces the inner one with the merged
//! result.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Replace redundant quantifiers `%<inner>s` and `%<outer>s` with a single `%<replacement>s`.";

/// Represents a parsed quantifier with original text and normalized form.
#[derive(Debug)]
struct Quantifier {
    /// Start position of the quantifier in the body bytes.
    start: usize,
    /// End position (inclusive).
    end: usize,
    /// Normalized form for merging: `+`, `*`, or `?`.
    normalized: String,
    /// Original text for display (e.g. `{1,}` instead of `+`).
    display: String,
}

#[derive(Default)]
pub struct RedundantRegexpQuantifiers;

#[cop(
    name = "Lint/RedundantRegexpQuantifiers",
    description = "Flags redundant quantifiers in regexp patterns that can be simplified.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantRegexpQuantifiers {
    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Regexp { parts, opts } = *cx.kind(node) else {
        return;
    };

    // Skip interpolated regexps.
    let parts_list = cx.list(parts);
    if parts_list.len() != 1 {
        return;
    }
    if !matches!(cx.kind(parts_list[0]), NodeKind::Str(_)) {
        return;
    }

    // Check for the `x` (extended/free-space) flag.
    let flags = cx.symbol_str(opts);
    let is_extended = flags.contains('x');

    let regexp_range = cx.range(node);
    let full_src = cx.raw_source(regexp_range);
    let full_bytes = full_src.as_bytes();

    let (body_start, body_end) = find_regexp_body_bounds(full_bytes);
    if body_start >= body_end {
        return;
    }

    let body = &full_bytes[body_start..body_end];
    let body_offset = regexp_range.start + body_start as u32;

    scan_content(body, 0, body.len(), body_offset, cx, is_extended);
}

/// Returns `(body_start, body_end)` as byte offsets within `full_bytes`.
fn find_regexp_body_bounds(bytes: &[u8]) -> (usize, usize) {
    if bytes.is_empty() {
        return (0, 0);
    }
    if bytes[0] == b'/' {
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

/// Recursively scan a region of the regexp body for redundant quantifiers.
/// `start` and `end` define the range `[start, end)` in `body`.
fn scan_content(
    body: &[u8],
    start: usize,
    end: usize,
    body_offset: u32,
    cx: &Cx<'_>,
    is_extended: bool,
) {
    let mut i = start;
    while i < end {
        match body[i] {
            b'\\' => {
                i += 2;
            }
            b'[' => {
                i = skip_char_class(body, i, end);
            }
            b'(' => {
                if is_non_capturing(body, i) {
                    if let Some(close_pos) = find_closing_paren(body, i) {
                        // First, recurse into the content for nested offenses.
                        scan_content(body, i + 3, close_pos, body_offset, cx, is_extended);

                        // Then check for outer quantifier after ')'.
                        let mut outer_start = close_pos + 1;
                        if is_extended {
                            outer_start = skip_whitespace(body, outer_start, end);
                        }
                        if let Some(outer_quant) =
                            parse_greedy_quantifier(body, outer_start, end, is_extended)
                        {
                            let content_start = i + 3;
                            let content_end = close_pos;
                            if let Some(inner_quant) = find_inner_quantifier(
                                body, content_start, content_end, is_extended,
                            ) {
                                let replacement =
                                    merge_quantifiers(&inner_quant.normalized, &outer_quant.normalized);

                                let off_start = body_offset + inner_quant.start as u32;
                                let off_end = body_offset + outer_quant.end as u32 + 1;
                                let offense_range = Range { start: off_start, end: off_end };

                                let msg = MSG
                                    .replace("%<inner>s", &inner_quant.display)
                                    .replace("%<outer>s", &outer_quant.display)
                                    .replace("%<replacement>s", &replacement);

                                cx.emit_offense(offense_range, &msg, None);

                                // Autocorrect: replace inner quantifier, drop outer.
                                let inner_range = Range {
                                    start: body_offset + inner_quant.start as u32,
                                    end: body_offset + inner_quant.end as u32 + 1,
                                };
                                let outer_range = Range {
                                    start: body_offset + outer_quant.start as u32,
                                    end: body_offset + outer_quant.end as u32 + 1,
                                };
                                cx.emit_edit(inner_range, &replacement);
                                cx.emit_edit(outer_range, "");
                            }
                            i = outer_quant.end + 1;
                        } else {
                            i = close_pos + 1;
                        }
                        continue;
                    }
                } else if is_capturing_group(body, i) {
                    // Skip capturing groups (don't check for redundancy).
                    if let Some(close_pos) = find_closing_paren(body, i) {
                        i = close_pos + 1;
                        continue;
                    }
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
}

/// Returns true if bytes[start..] starts with `(?:`.
fn is_non_capturing(bytes: &[u8], start: usize) -> bool {
    if start + 3 > bytes.len() {
        return false;
    }
    bytes[start] == b'(' && bytes[start + 1] == b'?' && bytes[start + 2] == b':'
}

/// Returns true if bytes[start..] starts with `(` but NOT `(?` (a capturing group).
fn is_capturing_group(bytes: &[u8], start: usize) -> bool {
    if start + 1 > bytes.len() {
        return false;
    }
    bytes[start] == b'(' && (start + 2 > bytes.len() || bytes[start + 1] != b'?')
}

/// Find the position of the closing `)` that matches the `(` at `open_pos`.
fn find_closing_paren(bytes: &[u8], open_pos: usize) -> Option<usize> {
    let mut depth = 1u32;
    let mut i = open_pos + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' => {
                i = skip_char_class_infinite(bytes, i);
                continue;
            }
            b'(' => {
                depth += 1;
            }
            b')' => {
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

/// Skip from the `[` at `start` to the character after the closing `]`.
fn skip_char_class(bytes: &[u8], start: usize, bound: usize) -> usize {
    let mut i = start + 1;
    let mut depth = 1u32;
    while i < bound {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' => {
                depth += 1;
            }
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    start + 1
}

/// Same as skip_char_class but without a bound check (for find_closing_paren).
fn skip_char_class_infinite(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    let mut depth = 1u32;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' => {
                depth += 1;
            }
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    start + 1
}

/// Skip whitespace bytes starting at `start`, returning the first non-whitespace position.
fn skip_whitespace(bytes: &[u8], mut start: usize, bound: usize) -> usize {
    while start < bound && (bytes[start] == b' ' || bytes[start] == b'\t' || bytes[start] == b'\n' || bytes[start] == b'\r') {
        start += 1;
    }
    start
}

/// Parse a greedy quantifier. Skips leading whitespace in extended mode.
/// Returns `None` for reluctant (`+?`) or possessive (`++`) quantifiers.
fn parse_greedy_quantifier(
    bytes: &[u8],
    mut start: usize,
    bound: usize,
    is_extended: bool,
) -> Option<Quantifier> {
    if is_extended {
        start = skip_whitespace(bytes, start, bound);
    }
    if start >= bound {
        return None;
    }
    match bytes[start] {
        b'+' | b'*' | b'?' => {
            // Check for reluctant/possessive modifier.
            if start + 1 < bound {
                let next = bytes[start + 1];
                if next == b'+' || next == b'?' {
                    return None;
                }
            }
            let display = (bytes[start] as char).to_string();
            Some(Quantifier {
                start,
                end: start,
                normalized: display.clone(),
                display,
            })
        }
        b'{' => {
            let end = find_closing_brace(bytes, start, bound)?;
            let inner = std::str::from_utf8(&bytes[start + 1..end]).ok()?;
            // Check for modifier after `}`.
            if end + 1 < bound {
                let next = bytes[end + 1];
                if next == b'+' || next == b'?' {
                    return None;
                }
            }
            let normalized = normalize_interval(inner)?;
            let display = format!("{{{}}}", inner);
            Some(Quantifier {
                start,
                end,
                normalized,
                display,
            })
        }
        _ => None,
    }
}

/// Find the closing `}` for a `{` at `start`. Returns position of `}`.
fn find_closing_brace(bytes: &[u8], start: usize, bound: usize) -> Option<usize> {
    let mut i = start + 1;
    while i < bound {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'}' => {
                return Some(i);
            }
            _ => {
                i += 1;
            }
        }
    }
    None
}

/// Normalize an interval quantifier content to its single-char equivalent.
fn normalize_interval(inner: &str) -> Option<String> {
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }
    let comma_pos = inner.find(',');
    match comma_pos {
        None => None,
        Some(pos) => {
            let left = inner[..pos].trim();
            let right = inner[pos + 1..].trim();
            if left.is_empty() && right == "1" {
                Some("?".to_string())
            } else if left == "0" && right.is_empty() {
                Some("*".to_string())
            } else if left == "0" && right == "1" {
                Some("?".to_string())
            } else if left == "1" && right.is_empty() {
                Some("+".to_string())
            } else {
                None
            }
        }
    }
}

/// Find the inner quantifier within a non-capturing group's content.
fn find_inner_quantifier(
    bytes: &[u8],
    content_start: usize,
    content_end: usize,
    is_extended: bool,
) -> Option<Quantifier> {
    if content_start >= content_end {
        return None;
    }

    if has_alternation_in_content(bytes, content_start, content_end) {
        return None;
    }

    if has_capturing_group_in_content(bytes, content_start, content_end) {
        return None;
    }

    find_last_quantifiable_expression(bytes, content_start, content_end, is_extended)
}

/// Returns true if there's a `|` (alternation) at the top level of the content.
fn has_alternation_in_content(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut i = start;
    while i < end {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' => {
                i = skip_char_class(bytes, i, end);
                continue;
            }
            b'(' => {
                if let Some(close) = find_closing_paren(bytes, i) {
                    i = close + 1;
                    continue;
                }
            }
            b'|' => {
                return true;
            }
            _ => {
                i += 1;
            }
        }
    }
    false
}

/// Returns true if there's a capturing group in the content.
fn has_capturing_group_in_content(bytes: &[u8], start: usize, end: usize) -> bool {
    let mut i = start;
    while i < end {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'[' => {
                i = skip_char_class(bytes, i, end);
                continue;
            }
            b'(' => {
                if i + 2 <= bytes.len() && bytes[i + 1] != b'?' {
                    return true;
                }
                if let Some(close) = find_closing_paren(bytes, i) {
                    i = close + 1;
                    continue;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    false
}

/// Walk backward from `content_end` to find the last quantifiable expression
/// with a greedy quantifier.
fn find_last_quantifiable_expression(
    bytes: &[u8],
    start: usize,
    end: usize,
    is_extended: bool,
) -> Option<Quantifier> {
    let mut i = end;
    while i > start {
        i -= 1;

        match bytes[i] {
            b'}' => {
                if let Some(open) = find_opening_brace(bytes, start, i) {
                    let interval = std::str::from_utf8(&bytes[open + 1..i]).ok()?;
                    if i + 1 < end {
                        let next = bytes[i + 1];
                        if next == b'+' || next == b'?' {
                            i = open;
                            continue;
                        }
                    }
                    let normalized = normalize_interval(interval)?;
                    if open > start {
                        if find_expression_end_before(bytes, start, open, is_extended).is_some() {
                            let display = format!("{{{}}}", interval);
                            return Some(Quantifier { start: open, end: i, normalized, display });
                        }
                    }
                    i = open;
                }
                continue;
            }
            b'+' | b'*' | b'?' => {
                if i + 1 < end {
                    let next = bytes[i + 1];
                    if next == b'+' || next == b'?' {
                        continue;
                    }
                }
                if i > start {
                    if find_expression_end_before(bytes, start, i, is_extended).is_some() {
                        let display = (bytes[i] as char).to_string();
                        return Some(Quantifier { start: i, end: i, normalized: display.clone(), display });
                    }
                }
                continue;
            }
            b')' => {
                continue;
            }
            _ => {
                continue;
            }
        }
    }
    None
}

/// Find the opening `{` that pairs with the closing `}` at position `close_pos`.
fn find_opening_brace(bytes: &[u8], min_pos: usize, close_pos: usize) -> Option<usize> {
    let mut i = close_pos;
    while i > min_pos {
        i -= 1;
        if bytes[i] == b'{' {
            if i == min_pos || bytes[i - 1] != b'\\' {
                return Some(i);
            }
        }
    }
    None
}

/// Find the end position of an expression that ends right before `quant_pos`.
///
/// In extended mode, skips trailing whitespace between the expression and the
/// quantifier (e.g. `a ?` in `/x` mode where the space is ignored).
fn find_expression_end_before(
    bytes: &[u8],
    low_bound: usize,
    quant_pos: usize,
    is_extended: bool,
) -> Option<usize> {
    if quant_pos <= low_bound {
        return None;
    }

    // In extended mode, skip whitespace backward from quant_pos-1.
    let mut expr_end = quant_pos - 1;
    if is_extended {
        while expr_end > low_bound && (bytes[expr_end] == b' ' || bytes[expr_end] == b'\t' || bytes[expr_end] == b'\n' || bytes[expr_end] == b'\r') {
            expr_end -= 1;
        }
    }

    // Character class [...] before the quantifier.
    if bytes[expr_end] == b']' {
        let class_start = find_char_class_start(bytes, low_bound, expr_end)?;
        if is_insignificant_prefix(bytes, low_bound, class_start, is_extended) {
            return Some(quant_pos);
        }
        return None;
    }

    // Non-capturing group (?:...) before the quantifier.
    if bytes[expr_end] == b')' {
        let group_open = find_group_open(bytes, low_bound, expr_end)?;
        if is_non_capturing(bytes, group_open) {
            if is_insignificant_prefix(bytes, low_bound, group_open, is_extended) {
                return Some(quant_pos);
            }
        }
        return None;
    }

    // Single char or escape before the quantifier.
    if bytes[expr_end] == b'\\' {
        if expr_end >= low_bound {
            if is_insignificant_prefix(bytes, low_bound, expr_end, is_extended) {
                return Some(quant_pos);
            }
        }
    } else if is_single_char(bytes[expr_end]) {
        if is_insignificant_prefix(bytes, low_bound, expr_end, is_extended) {
            return Some(quant_pos);
        }
    }

    None
}

/// Find the start of a character class ending at `close_pos` (which is `]`).
fn find_char_class_start(bytes: &[u8], low: usize, close_pos: usize) -> Option<usize> {
    let mut i = close_pos;
    while i > low {
        i -= 1;
        if bytes[i] == b'[' {
            if i == low || bytes[i - 1] != b'\\' {
                return Some(i);
            }
        }
    }
    None
}

/// Find the opening `(` for a group ending at `close_pos` (which is `)`).
fn find_group_open(bytes: &[u8], _low: usize, close_pos: usize) -> Option<usize> {
    let mut depth = 1u32;
    for i in (0..close_pos).rev() {
        match bytes[i] {
            b')' => {
                depth += 1;
            }
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Returns true if the bytes in `[start, end)` are insignificant: whitespace
/// (in extended mode) or a non-capturing group wrapper that encloses the
/// entire expression (i.e. the group IS the expression minus its wrapper).
fn is_insignificant_prefix(
    bytes: &[u8],
    start: usize,
    end: usize,
    is_extended: bool,
) -> bool {
    if start >= end {
        return true;
    }
    if is_extended
        && bytes[start..end]
            .iter()
            .all(|&b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
    {
        return true;
    }
    // Non-capturing group that wraps the entire expression: skip it so the
    // inner content can be analyzed. Only valid when the group IS the
    // expression (closing paren at end boundary).
    if bytes[start] == b'('
        && is_non_capturing(bytes, start)
        && let Some(close) = find_closing_paren(bytes, start)
        && close + 1 == end
    {
        return true;
    }
    false
}

/// Returns true if `c` is a valid single literal regexp character.
fn is_single_char(c: u8) -> bool {
    !matches!(
        c,
        b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'|' | b'\\' | b'^' | b'$' | b'.' | b'+'
            | b'*' | b'?'
    )
}

/// Merge two greedy quantifiers into a single equivalent.
fn merge_quantifiers(inner: &str, outer: &str) -> String {
    if inner == outer {
        inner.to_string()
    } else {
        "*".to_string()
    }
}

murphy_plugin_api::submit_cop!(RedundantRegexpQuantifiers);

#[cfg(test)]
mod tests {
    use super::RedundantRegexpQuantifiers;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Offense cases: duplicate quantifiers -----

    #[test]
    fn flags_duplicate_plus_quantifiers() {
        test::<RedundantRegexpQuantifiers>().expect_correction(
            indoc! {r#"
                foo = /(?:a+)+/
                           ^^^ Replace redundant quantifiers `+` and `+` with a single `+`.
            "#},
            "foo = /(?:a+)/\n",
        );
    }

    #[test]
    fn flags_duplicate_star_quantifiers() {
        test::<RedundantRegexpQuantifiers>().expect_correction(
            indoc! {r#"
                foo = /(?:a*)*/
                           ^^^ Replace redundant quantifiers `*` and `*` with a single `*`.
            "#},
            "foo = /(?:a*)/\n",
        );
    }

    #[test]
    fn flags_duplicate_question_quantifiers() {
        test::<RedundantRegexpQuantifiers>().expect_correction(
            indoc! {r#"
                foo = /(?:a?)?/
                           ^^^ Replace redundant quantifiers `?` and `?` with a single `?`.
            "#},
            "foo = /(?:a?)/\n",
        );
    }

    // ----- Offense cases: mixed quantifiers -----

    #[test]
    fn flags_mixed_plus_question_quantifiers() {
        test::<RedundantRegexpQuantifiers>().expect_correction(
            indoc! {r#"
                foo = /(?:a+)?/
                           ^^^ Replace redundant quantifiers `+` and `?` with a single `*`.
            "#},
            "foo = /(?:a*)/\n",
        );
    }

    // ----- Offense cases: character classes -----

    #[test]
    fn flags_duplicate_quantifiers_on_character_class() {
        test::<RedundantRegexpQuantifiers>().expect_correction(
            indoc! {r#"
                foo = /(?:[abc]+)+/
                               ^^^ Replace redundant quantifiers `+` and `+` with a single `+`.
            "#},
            "foo = /(?:[abc]+)/\n",
        );
    }

    // ----- Offense cases: nested non-capturing groups -----

    #[test]
    fn flags_nested_redundant_quantifiers() {
        // Two offenses with overlapping ranges. Check offenses only —
        // overlapping autocorrect edits on the shared `+` quantifier
        // are inherently racy with sequential range-based edits.
        test::<RedundantRegexpQuantifiers>().expect_offense(
            indoc! {r#"
                foo = /(?:(?:a?)+)+/
                              ^^^ Replace redundant quantifiers `?` and `+` with a single `*`.
                                ^^^ Replace redundant quantifiers `+` and `+` with a single `+`.
            "#},
        );
    }

    #[test]
    fn flags_deeply_nested_redundant_quantifiers() {
        // With the safer is_insignificant_prefix (whitespace + exact-wrapper only),
        // deeply nested groups like this are not detected. This is intentional:
        // non-capturing groups that contain non-whitespace are no longer skipped as
        // insignificant, preventing false positives on patterns like (?:(?:a)b+)+.
        test::<RedundantRegexpQuantifiers>().expect_no_offenses(indoc! {r#"
            foo = /(?:(?:(?:(?:a)?))+)/
        "#});
    }

    // ----- Offense cases: interval quantifiers -----

    #[test]
    fn flags_normalized_interval_quantifiers() {
        test::<RedundantRegexpQuantifiers>().expect_correction(
            indoc! {r#"
                foo = /(?:a{1,})?/
                           ^^^^^^ Replace redundant quantifiers `{1,}` and `?` with a single `*`.
            "#},
            "foo = /(?:a*)/\n",
        );
    }

    // ----- Offense cases: free-space mode /x -----

    #[test]
    fn flags_quantifiers_in_x_mode() {
        test::<RedundantRegexpQuantifiers>().expect_correction(
            indoc! {r#"
                foo = /(?: a ? ) + /x
                             ^^^^^ Replace redundant quantifiers `?` and `+` with a single `*`.
            "#},
            "foo = /(?: a * )  /x\n",
        );
    }

    // ----- No-offense cases: non-redundant quantifiers -----

    #[test]
    fn accepts_group_with_multiple_children() {
        test::<RedundantRegexpQuantifiers>().expect_no_offenses("foo = /(?:ab+)+/\n");
    }

    #[test]
    fn accepts_group_with_alternation() {
        test::<RedundantRegexpQuantifiers>().expect_no_offenses("foo = /(?:a|b+)+/\n");
    }

    #[test]
    fn accepts_capturing_group() {
        test::<RedundantRegexpQuantifiers>().expect_no_offenses("foo = /(a+)+/\n");
    }

    #[test]
    fn accepts_group_containing_capture_group() {
        test::<RedundantRegexpQuantifiers>().expect_no_offenses("foo = /(?:(a+))+/ \n");
    }

    #[test]
    fn accepts_single_quantifier_no_redundancy() {
        test::<RedundantRegexpQuantifiers>().expect_no_offenses("foo = /(?:x+)  /\n");
    }

    #[test]
    fn accepts_outer_quantifier_only() {
        test::<RedundantRegexpQuantifiers>().expect_no_offenses("foo = /(?:x)?  /\n");
    }

    // ----- No-offense cases: non-greedy quantifiers -----

    #[test]
    fn accepts_possessive_quantifier() {
        test::<RedundantRegexpQuantifiers>().expect_no_offenses("foo = /(?:a++)+/\n");
    }

    #[test]
    fn accepts_reluctant_quantifier() {
        test::<RedundantRegexpQuantifiers>().expect_no_offenses("foo = /(?:a+?)+/\n");
    }

    // ----- No-offense cases: interpolation -----

    #[test]
    fn accepts_interpolated_regexp() {
        test::<RedundantRegexpQuantifiers>()
            .expect_no_offenses("foo = /(?:a*\\#{interpolation})?/x\n");
    }

    // ----- No-offense cases: multiple terminal children -----

    #[test]
    fn accepts_group_with_multiple_terminals() {
        test::<RedundantRegexpQuantifiers>().expect_no_offenses("foo = /(?:\\d\\D+)+/\n");
    }

    #[test]
    fn accepts_group_with_mixed_options() {
        test::<RedundantRegexpQuantifiers>().expect_no_offenses("foo = /(?:a+|b)+/\n");
    }
}
