//! `Lint/MixedCaseRange` — flags mixed-case ASCII letter ranges.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/MixedCaseRange
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covers string literal Range objects (`'A'..'z'`, `'A'...'z'`) and
//!   regexp character-class ranges in plain regexp literals. Regexp ranges
//!   are autocorrected to split the upper/lower ASCII ranges.
//!   Known v1 limitation: Murphy does not expose a regexp parsed tree through
//!   the plugin API, so regexp detection is a conservative raw-source scan.
//!   Complex regexp-parser cases such as nested character classes and escaped
//!   multi-codepoint bounds are skipped rather than guessed.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Ranges from upper to lower case ASCII letters may include unintended characters. Instead of `A-z` (which also includes several symbols) specify each range individually: `A-Za-z` and individually specify any symbols.";

#[derive(Default)]
pub struct MixedCaseRange;

#[cop(
    name = "Lint/MixedCaseRange",
    description = "Checks for mixed-case character ranges.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MixedCaseRange {
    #[on_node(kind = "range")]
    fn check_range(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::RangeExpr { begin_, end_, .. } = *cx.kind(node) else {
            return;
        };
        let (Some(begin), Some(end)) = (begin_.get(), end_.get()) else {
            return;
        };
        let (Some(open), Some(close)) = (single_string_char(begin, cx), single_string_char(end, cx)) else {
            return;
        };
        if unsafe_range(open, close) {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }

    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        check_regexp(node, cx);
    }
}

fn single_string_char(node: NodeId, cx: &Cx<'_>) -> Option<u8> {
    let NodeKind::Str(id) = *cx.kind(node) else {
        return None;
    };
    let value = cx.string_str(id).as_bytes();
    if value.len() == 1 { Some(value[0]) } else { None }
}

fn unsafe_range(open: u8, close: u8) -> bool {
    matches!((ascii_case(open), ascii_case(close)), (Some(a), Some(b)) if a != b)
}

fn ascii_case(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(b'U'),
        b'a'..=b'z' => Some(b'L'),
        _ => None,
    }
}

fn check_regexp(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Regexp { parts, .. } = *cx.kind(node) else {
        return;
    };
    // Interpolated regexp bodies need regexp-parser-level token positions.
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
            return;
        };
        scan_char_class(&body[i + 1..end], body_offset + i as u32 + 1, cx);
        i = end + 1;
    }
}

fn find_char_class_end(body: &[u8], mut i: usize) -> Option<usize> {
    while i < body.len() {
        if body[i] == b'\\' {
            i += 2;
        } else if body[i] == b'[' {
            return None;
        } else if body[i] == b']' {
            return Some(i);
        } else {
            i += 1;
        }
    }
    None
}

fn scan_char_class(class: &[u8], class_offset: u32, cx: &Cx<'_>) {
    let mut i = usize::from(class.first() == Some(&b'^'));
    while i + 2 < class.len() {
        if class[i] == b'\\' {
            i += 2;
            continue;
        }
        if class[i + 1] == b'-' && class[i + 2] != b'\\' && unsafe_range(class[i], class[i + 2]) {
            let range = Range { start: class_offset + i as u32, end: class_offset + i as u32 + 3 };
            let replacement = regexp_range_replacement(class[i], class[i + 2]);
            cx.emit_offense(range, MSG, None);
            cx.emit_edit(range, &replacement);
            i += 3;
        } else {
            i += 1;
        }
    }
}

fn regexp_range_replacement(open: u8, close: u8) -> String {
    match (ascii_case(open), ascii_case(close)) {
        (Some(b'U'), Some(b'L')) => format_range(open, b'Z') + &format_range(b'a', close),
        (Some(b'L'), Some(b'U')) => format_range(open, b'z') + &format_range(close, b'Z'),
        _ => String::new(),
    }
}

fn format_range(open: u8, close: u8) -> String {
    if open == close {
        (open as char).to_string()
    } else {
        format!("{}-{}", open as char, close as char)
    }
}

murphy_plugin_api::submit_cop!(MixedCaseRange);

#[cfg(test)]
mod tests {
    use super::MixedCaseRange;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_inclusive_string_range() {
        test::<MixedCaseRange>().expect_offense(indoc! {r#"
            foo = 'A'..'z'
                  ^^^^^^^^ Ranges from upper to lower case ASCII letters may include unintended characters. Instead of `A-z` (which also includes several symbols) specify each range individually: `A-Za-z` and individually specify any symbols.
        "#});
    }

    #[test]
    fn flags_exclusive_string_range() {
        test::<MixedCaseRange>().expect_offense(indoc! {r#"
            foo = 'A'...'z'
                  ^^^^^^^^^ Ranges from upper to lower case ASCII letters may include unintended characters. Instead of `A-z` (which also includes several symbols) specify each range individually: `A-Za-z` and individually specify any symbols.
        "#});
    }

    #[test]
    fn accepts_same_case_and_non_single_character_ranges() {
        test::<MixedCaseRange>()
            .expect_no_offenses("foo = 'A'..'Z'\n")
            .expect_no_offenses("foo = 'aa'..'z'\n")
            .expect_no_offenses("foo = 'a'..'zz'\n")
            .expect_no_offenses("(..'z')\n")
            .expect_no_offenses("('a'..)\n");
    }

    #[test]
    fn flags_and_corrects_regexp_ranges() {
        test::<MixedCaseRange>().expect_correction(
            indoc! {r#"
                foo = /[A-z]/
                        ^^^ Ranges from upper to lower case ASCII letters may include unintended characters. Instead of `A-z` (which also includes several symbols) specify each range individually: `A-Za-z` and individually specify any symbols.
            "#},
            "foo = /[A-Za-z]/\n",
        );
    }

    #[test]
    fn flags_multiple_regexp_ranges() {
        test::<MixedCaseRange>().expect_correction(
            indoc! {r#"
                foo = /[_A-b;Z-a!]/
                         ^^^ Ranges from upper to lower case ASCII letters may include unintended characters. Instead of `A-z` (which also includes several symbols) specify each range individually: `A-Za-z` and individually specify any symbols.
                             ^^^ Ranges from upper to lower case ASCII letters may include unintended characters. Instead of `A-z` (which also includes several symbols) specify each range individually: `A-Za-z` and individually specify any symbols.
            "#},
            "foo = /[_A-Za-b;Za!]/\n",
        );
    }

    #[test]
    fn accepts_safe_or_escaped_regexp_ranges() {
        test::<MixedCaseRange>()
            .expect_no_offenses("foo = /[_a-zA-Z0-9;]/\n")
            .expect_no_offenses(r"foo = /[A\-z]/\n")
            .expect_no_offenses(r"foo = /[\101-z]/\n")
            .expect_no_offenses(r"foo = /[A-\172]/\n")
            .expect_no_offenses("foo = /[a-[bc]]/\n");
    }
}
