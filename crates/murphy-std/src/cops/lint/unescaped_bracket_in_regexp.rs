//! `Lint/UnescapedBracketInRegexp` — Checks for unescaped `]` in regular expressions.
//!
//! An unescaped `]` outside a character class is likely a mistake. Ruby itself
//! emits a warning for this pattern.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnescapedBracketInRegexp
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags unescaped `]` in regexp literal bodies (both /.../ and %r{...}
//!   forms). Character classes ([...]) are correctly skipped. A leading `]`
//!   (first character, no Ruby warning) is not flagged. `Regexp.new` /
//!   `Regexp.compile` calls are not yet checked.
//! ```
//!
//! ## Matched shapes
//!
//! - `/abc]123/` — unescaped bracket in `/.../` regexp — offense
//! - `%r{abc]123}` — unescaped bracket in `%r{...}` regexp — offense
//! - `/abc\]123/` — escaped bracket — no offense
//! - `/[abc]/` — bracket inside character class — no offense
//! - `/]/` — bracket as first character (Ruby doesn't warn) — no offense
//!
//! ## No autocorrect
//!
//! Escaping the bracket could change regexp semantics if the bracket was
//! intentionally unescaped, so no autocorrect is provided.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Regular expression has `]` without escape.";

fn regexp_body_bounds(src: &str) -> (usize, usize) {
    let bytes = src.as_bytes();
    if bytes.is_empty() {
        return (0, 0);
    }

    let body_start: usize;
    let close_byte: u8;

    if bytes[0] == b'/' {
        body_start = 1;
        close_byte = b'/';
    } else if bytes.len() >= 3 && bytes[0] == b'%' && bytes[1] == b'r' {
        body_start = 3;
        let open = bytes[2];
        close_byte = match open {
            b'{' => b'}',
            b'(' => b')',
            b'[' => b']',
            b'<' => b'>',
            _ => open,
        };
    } else {
        return (0, 0);
    }

    let mut body_end = bytes.len();
    for i in (body_start..bytes.len()).rev() {
        if bytes[i].is_ascii_alphabetic() {
            continue;
        }
        if bytes[i] == close_byte {
            body_end = i;
            break;
        }
    }

    (body_start, body_end)
}

fn find_unescaped_brackets(s: &str, body_start: usize, body_end: usize) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut in_cc = false;
    let mut cc_just_opened = false;
    let mut chars = s.char_indices();

    while let Some((idx, ch)) = chars.next() {
        if idx < body_start || idx >= body_end {
            continue;
        }
        match ch {
            '\\' => {
                chars.next();
                cc_just_opened = false;
            }
            '[' if !in_cc => {
                in_cc = true;
                cc_just_opened = true;
            }
            '^' if cc_just_opened => {
                cc_just_opened = true;
            }
            ']' => {
                if cc_just_opened {
                    cc_just_opened = false;
                } else if in_cc {
                    in_cc = false;
                    cc_just_opened = false;
                } else if idx > body_start {
                    positions.push(idx);
                }
            }
            _ => {
                cc_just_opened = false;
            }
        }
    }
    positions
}

#[derive(Default)]
pub struct UnescapedBracketInRegexp;

#[cop(
    name = "Lint/UnescapedBracketInRegexp",
    description = "Checks for unescaped `]` in regular expressions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl UnescapedBracketInRegexp {
    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Regexp { .. } = *cx.kind(node) else {
            unreachable!()
        };
        let node_range = cx.range(node);
        let src = cx.raw_source(node_range);
        let (body_start, body_end) = regexp_body_bounds(src);
        for offset in find_unescaped_brackets(src, body_start, body_end) {
            let pos = node_range.start + offset as u32;
            cx.emit_offense(
                Range {
                    start: pos,
                    end: pos + 1,
                },
                MSG,
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UnescapedBracketInRegexp;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_unescaped_bracket_in_slash_regexp() {
        test::<UnescapedBracketInRegexp>().expect_offense(indoc! {r#"
            /abc]123/
                ^ Regular expression has `]` without escape.
        "#});
    }

    #[test]
    fn flags_unescaped_bracket_with_regexp_options() {
        test::<UnescapedBracketInRegexp>().expect_offense(indoc! {r#"
            /abc]123/i
                ^ Regular expression has `]` without escape.
        "#});
    }

    #[test]
    fn flags_multiple_unescaped_brackets() {
        test::<UnescapedBracketInRegexp>().expect_offense(indoc! {r#"
            /abc]123]/
                ^ Regular expression has `]` without escape.
                    ^ Regular expression has `]` without escape.
        "#});
    }

    #[test]
    fn flags_unescaped_bracket_in_percent_r_regexp() {
        test::<UnescapedBracketInRegexp>().expect_offense(indoc! {r#"
            %r{abc]123}
                  ^ Regular expression has `]` without escape.
        "#});
    }

    #[test]
    fn flags_unescaped_bracket_in_percent_r_with_options() {
        test::<UnescapedBracketInRegexp>().expect_offense(indoc! {r#"
            %r{abc]123}i
                  ^ Regular expression has `]` without escape.
        "#});
    }

    #[test]
    fn flags_multiple_brackets_in_percent_r() {
        test::<UnescapedBracketInRegexp>().expect_offense(indoc! {r#"
            %r{abc]123]}
                  ^ Regular expression has `]` without escape.
                      ^ Regular expression has `]` without escape.
        "#});
    }

    #[test]
    fn accepts_escaped_bracket_in_slash_regexp() {
        test::<UnescapedBracketInRegexp>().expect_no_offenses(indoc! {r#"
            /abc\]123/
        "#});
    }

    #[test]
    fn accepts_escaped_bracket_in_percent_r_regexp() {
        test::<UnescapedBracketInRegexp>().expect_no_offenses(indoc! {r#"
            %r{abc\]123}
        "#});
    }

    #[test]
    fn accepts_character_class_in_slash_regexp() {
        test::<UnescapedBracketInRegexp>().expect_no_offenses(indoc! {r#"
            /[abc]/
        "#});
    }

    #[test]
    fn accepts_character_class_in_percent_r_regexp() {
        test::<UnescapedBracketInRegexp>().expect_no_offenses(indoc! {r#"
            %r{[abc]}
        "#});
    }

    #[test]
    fn accepts_leading_bracket_in_slash_regexp() {
        test::<UnescapedBracketInRegexp>().expect_no_offenses(indoc! {r#"
            /]/
        "#});
    }

    #[test]
    fn accepts_leading_bracket_in_percent_r_regexp() {
        test::<UnescapedBracketInRegexp>().expect_no_offenses(indoc! {r#"
            %r{]}
        "#});
    }

    #[test]
    fn accepts_character_class_with_lookbehind() {
        test::<UnescapedBracketInRegexp>().expect_no_offenses(indoc! {r#"
            /(?<=[<>=:])/
        "#});
    }
}

murphy_plugin_api::submit_cop!(UnescapedBracketInRegexp);
