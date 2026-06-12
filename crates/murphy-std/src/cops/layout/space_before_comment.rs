//! `Layout/SpaceBeforeComment` — flags an end-of-line comment that abuts the
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceBeforeComment
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Direct port of RuboCop's token-pair scan: for each consecutive token
//!   pair, flag when the second token is a comment on the same line as the
//!   first and the first token's end position equals the comment's start
//!   position (no space). Autocorrect inserts a single space before the
//!   comment. The same-line guard is implemented by skipping pairs where the
//!   preceding token is a newline (a comment alone on its line is preceded by
//!   a `Newline`/`IgnoredNewline` token whose end abuts the comment start).
//! ```
//!
//! preceding code token with no separating space. Mirrors RuboCop's
//! same-named cop: `1 + 1# comment` → `1 + 1 # comment`.

use murphy_plugin_api::{Cx, NoOptions, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceBeforeComment;

#[cop(
    name = "Layout/SpaceBeforeComment",
    description = "Put a space before an end-of-line comment.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl SpaceBeforeComment {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        for pair in cx.sorted_tokens().windows(2) {
            let token1 = pair[0];
            let token2 = pair[1];

            // RuboCop: `next unless token2.comment?`
            if token2.kind != SourceTokenKind::Comment {
                continue;
            }
            // RuboCop: `next unless token1.pos.end == token2.pos.begin`
            if token1.range.end != token2.range.start {
                continue;
            }
            // RuboCop: `next unless same_line?(token1, token2)`. Since the two
            // tokens are adjacent (`token1.end == token2.start`), they share a
            // line iff `token1` itself contains no line break. Murphy folds the
            // trailing newline into the preceding token (a standalone comment's
            // predecessor is the previous comment or a `Newline`, both ending
            // in `\n`; a `Newline`/`IgnoredNewline` token IS a line break), so
            // a newline anywhere in `token1` means `token2` starts a new line.
            let token1_src = cx.raw_source(token1.range);
            if token1_src.bytes().any(|b| b == b'\n' || b == b'\r') {
                continue;
            }

            // RuboCop's offense range is `token2.pos` (the comment text only).
            // Murphy's `Comment` token may include the trailing line break;
            // trim any trailing `\r`/`\n` so the offense covers just `# …`.
            let mut end = token2.range.end;
            let bytes = cx.source().as_bytes();
            while end > token2.range.start && matches!(bytes[end as usize - 1], b'\n' | b'\r') {
                end -= 1;
            }
            let range = Range {
                start: token2.range.start,
                end,
            };
            cx.emit_offense(range, "Put a space before an end-of-line comment.", None);
            // RuboCop: `corrector.insert_before(range, ' ')`.
            cx.emit_edit(
                Range {
                    start: range.start,
                    end: range.start,
                },
                " ",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SpaceBeforeComment;
    use murphy_plugin_api::test_support::{indoc, run_cop_with_edits, test};

    #[test]
    fn flags_and_corrects_comment_abutting_code() {
        test::<SpaceBeforeComment>().expect_correction(
            indoc! {r#"
                1 + 1# comment
                     ^^^^^^^^^ Put a space before an end-of-line comment.
            "#},
            "1 + 1 # comment\n",
        );
    }

    #[test]
    fn accepts_comment_with_space() {
        test::<SpaceBeforeComment>().expect_no_offenses("1 + 1 # comment\n");
    }

    #[test]
    fn accepts_comment_alone_on_line() {
        test::<SpaceBeforeComment>().expect_no_offenses(indoc! {r#"
            # a standalone comment
            x = 1
        "#});
    }

    #[test]
    fn accepts_comment_alone_after_code_line() {
        test::<SpaceBeforeComment>().expect_no_offenses(indoc! {r#"
            x = 1
            # comment on its own line
        "#});
    }

    #[test]
    fn accepts_consecutive_standalone_comment_lines() {
        // Murphy folds the trailing `\n` into the comment token, so the
        // second comment's predecessor is the first comment (ending in `\n`),
        // not a separate Newline token. The same-line guard must still skip it.
        test::<SpaceBeforeComment>().expect_no_offenses(indoc! {r#"
            # line one
            # line two
            # line three
        "#});
    }

    #[test]
    fn flags_comment_abutting_identifier() {
        let result = run_cop_with_edits::<SpaceBeforeComment>("foo# bar\n");
        assert_eq!(result.offenses.len(), 1);
        assert_eq!(result.edits[0].replacement, " ");
    }

    #[test]
    fn accepts_hash_inside_string_literal() {
        test::<SpaceBeforeComment>().expect_no_offenses("x = \"a#b\"\n");
    }

    #[test]
    fn flags_multiple_comments() {
        let result = run_cop_with_edits::<SpaceBeforeComment>("a = 1#one\nb = 2#two\n");
        assert_eq!(
            result.offenses.len(),
            2,
            "expected 2 offenses, got {:?}",
            result.offenses
        );
    }
}
murphy_plugin_api::submit_cop!(SpaceBeforeComment);
