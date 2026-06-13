//! `Layout/EmptyComment` — flags source-code comments that contain no text.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyComment
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Direct port of RuboCop's `on_new_investigation`. With the default
//!   `AllowMarginComment: true`, consecutive comments (line `i+1` == `i.line`
//!   and the same column) are chunked together via `concat_consecutive_comments`;
//!   the joined stripped text of the whole chunk must match the empty pattern
//!   for *every* comment in the chunk to be flagged. With
//!   `AllowMarginComment: false`, each comment is examined individually.
//!   `comment_text` is `raw_source(range).strip + "\n"`. The empty pattern is
//!   `/\A(#\n)+\z/` when `AllowBorderComment: true` (default), so a border
//!   comment like `####...` (multiple `#`) is allowed; with
//!   `AllowBorderComment: false` the pattern is `/\A(#+\n)+\z/`, flagging
//!   border comments too. Autocorrect: when the comment shares a line with a
//!   preceding token (trailing comment such as `def foo #`), remove the
//!   comment plus surrounding spaces (no newline); otherwise remove the whole
//!   line including its final newline.
//! ```

use murphy_plugin_api::{Comment, CopOptions, Cx, RangeSide, SpaceRangeOptions, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct EmptyComment;

#[derive(CopOptions)]
pub struct EmptyCommentOptions {
    #[option(
        name = "AllowBorderComment",
        default = true,
        description = "Allow comments composed only of repeated `#` border characters (e.g. `####`)."
    )]
    pub allow_border_comment: bool,

    #[option(
        name = "AllowMarginComment",
        default = true,
        description = "Allow margin comments — blank `#` lines surrounding a non-empty comment block."
    )]
    pub allow_margin_comment: bool,
}

#[cop(
    name = "Layout/EmptyComment",
    description = "Checks empty comment.",
    default_severity = "warning",
    default_enabled = true,
    options = EmptyCommentOptions
)]
impl EmptyComment {
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<EmptyCommentOptions>();
        let comments = cx.comments();

        if opts.allow_margin_comment {
            // RuboCop's `concat_consecutive_comments` + `investigate`.
            for chunk in concat_consecutive_comments(cx, comments) {
                let joined = chunk
                    .iter()
                    .map(|c| comment_text(cx, *c))
                    .collect::<String>();
                if !empty_comment_only(&joined, opts.allow_border_comment) {
                    continue;
                }
                for offense_comment in &chunk {
                    emit(cx, *offense_comment);
                }
            }
        } else {
            // RuboCop's per-comment branch.
            for comment in comments {
                let text = comment_text(cx, *comment);
                if empty_comment_only(&text, opts.allow_border_comment) {
                    emit(cx, *comment);
                }
            }
        }
    }
}

/// RuboCop's `add_offense(comment) { autocorrect }` body.
fn emit(cx: &Cx<'_>, comment: Comment) {
    cx.emit_offense(comment.range, "Source code comment is empty.", None);

    // RuboCop's `autocorrect`: if there is a previous token on the same line,
    // remove the comment plus its surrounding spaces (no newlines); otherwise
    // remove the whole line including its trailing newline.
    let range = if previous_token_same_line(cx, comment) {
        cx.range_with_surrounding_space(
            comment.range,
            SpaceRangeOptions {
                side: RangeSide::Both,
                newlines: false,
                whitespace: false,
                continuations: false,
            },
        )
    } else {
        cx.range_by_whole_lines(comment.range, true)
    };
    cx.emit_edit(range, "");
}

/// RuboCop's `comment_text(comment)` — `"#{comment.text.strip}\n"`.
fn comment_text(cx: &Cx<'_>, comment: Comment) -> String {
    let mut text = cx.raw_source(comment.range).trim().to_owned();
    text.push('\n');
    text
}

/// RuboCop's `empty_comment_only?`. With `allow_border`, the pattern is
/// `/\A(#\n)+\z/` (each line a single bare `#`); without it,
/// `/\A(#+\n)+\z/` (each line a run of `#`).
fn empty_comment_only(text: &str, allow_border: bool) -> bool {
    if text.is_empty() {
        return false;
    }
    for line in text.split_inclusive('\n') {
        // Each line must end with `\n` and the prefix be only `#` chars.
        let Some(hashes) = line.strip_suffix('\n') else {
            return false; // last segment lacks the trailing `\n`
        };
        if hashes.is_empty() || !hashes.bytes().all(|b| b == b'#') {
            return false;
        }
        if allow_border {
            // `#\n` — exactly one `#`.
            if hashes.len() != 1 {
                return false;
            }
        }
    }
    true
}

/// RuboCop's `concat_consecutive_comments`: group comments where
/// `prev.line + 1 == cur.line && prev.column == cur.column`. Comments are in
/// source order, so a single forward scan of the source bytes tracks each
/// comment's `(line, column)` in O(N) total rather than recomputing absolute
/// positions per comment.
fn concat_consecutive_comments(cx: &Cx<'_>, comments: &[Comment]) -> Vec<Vec<Comment>> {
    let src = cx.source().as_bytes();
    let mut chunks: Vec<Vec<Comment>> = Vec::new();
    let mut offset = 0usize;
    let mut line = 1usize;
    let mut line_start = 0usize;
    let mut prev_pos: Option<(usize, usize)> = None;

    for &comment in comments {
        let target = comment.range.start as usize;
        while offset < target {
            if src[offset] == b'\n' {
                line += 1;
                line_start = offset + 1;
            }
            offset += 1;
        }
        let column = target - line_start;

        let consecutive = prev_pos
            .is_some_and(|(prev_line, prev_column)| prev_line + 1 == line && prev_column == column);
        if consecutive {
            chunks
                .last_mut()
                .expect("a consecutive comment always follows an existing chunk")
                .push(comment);
        } else {
            chunks.push(vec![comment]);
        }
        prev_pos = Some((line, column));
    }
    chunks
}

/// RuboCop's `previous_token(node) && same_line?(node, previous_token)` — is
/// there a token (comment or code) on the same source line, before this
/// comment? A non-whitespace byte before the comment on its line is the signal:
/// it captures both earlier comments (RuboCop scans the token stream, which
/// includes comment tokens) and any code byte that precedes a trailing comment.
fn previous_token_same_line(cx: &Cx<'_>, comment: Comment) -> bool {
    let src = cx.source().as_bytes();
    let start = comment.range.start as usize;
    let line_start = src[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    src[line_start..start].iter().any(|&b| !is_ruby_space(b))
}

/// Ruby's `\s` byte class: `[ \t\n\r\f\v]`. Rust's `u8::is_ascii_whitespace`
/// omits the vertical tab `\v` (0x0B), so it is not a faithful substitute.
fn is_ruby_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C | 0x0B)
}

murphy_plugin_api::submit_cop!(EmptyComment);

#[cfg(test)]
mod tests {
    use super::{EmptyComment, EmptyCommentOptions};
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, run_cop_with_options};

    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        let mut out = String::with_capacity(source.len());
        let mut last = 0usize;
        let mut ordered: Vec<_> = edits.iter().collect();
        ordered.sort_by_key(|e| e.range.start);
        for e in ordered {
            out.push_str(&source[last..e.range.start as usize]);
            out.push_str(&e.replacement);
            last = e.range.end as usize;
        }
        out.push_str(&source[last..]);
        out
    }

    fn opts(border: bool, margin: bool) -> EmptyCommentOptions {
        EmptyCommentOptions {
            allow_border_comment: border,
            allow_margin_comment: margin,
        }
    }

    #[test]
    fn flags_single_empty_comment_and_removes_line() {
        let run = run_cop_with_edits::<EmptyComment>("#\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.offenses[0].message, "Source code comment is empty.");
        assert_eq!(apply("#\n", &run.edits), "");
    }

    #[test]
    fn flags_two_empty_comment_lines() {
        let run = run_cop_with_edits::<EmptyComment>("#\n#\n");
        assert_eq!(run.offenses.len(), 2);
        assert_eq!(apply("#\n#\n", &run.edits), "");
    }

    #[test]
    fn flags_trailing_empty_comment_keeping_code() {
        // `def foo #` -> remove the comment and surrounding spaces, keep `def foo`.
        let run = run_cop_with_edits::<EmptyComment>("def foo #\nend\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply("def foo #\nend\n", &run.edits), "def foo\nend\n");
    }

    #[test]
    fn flags_trailing_empty_comment_no_space() {
        let run = run_cop_with_edits::<EmptyComment>("def foo#\nend\n");
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(apply("def foo#\nend\n", &run.edits), "def foo\nend\n");
    }

    #[test]
    fn accepts_comment_with_text() {
        assert!(run_cop::<EmptyComment>("# Description of Foo class.\n").is_empty());
    }

    #[test]
    fn accepts_margin_comments_around_text() {
        // `#` / `# Description` / `#` — the middle line has text, so the joined
        // chunk does not match the empty pattern: nothing flagged.
        let src = "#\n# Description\n#\n";
        assert!(run_cop::<EmptyComment>(src).is_empty());
    }

    #[test]
    fn accepts_border_comment_by_default() {
        assert!(run_cop::<EmptyComment>("####################\n").is_empty());
    }

    #[test]
    fn flags_border_comment_when_border_disallowed() {
        let run = run_cop_with_options::<EmptyComment>(
            "####################\n",
            &opts(false, true),
        );
        assert_eq!(run.len(), 1);
    }

    #[test]
    fn flags_margin_comments_when_margin_disallowed() {
        // With margin off, each comment is examined alone: the two bare `#`
        // lines are flagged, the text line is not.
        let src = "#\n# Description\n#\n";
        let run = run_cop_with_options::<EmptyComment>(src, &opts(true, false));
        assert_eq!(run.len(), 2);
    }

    #[test]
    fn accepts_inline_comment_with_text() {
        assert!(run_cop::<EmptyComment>("x = 1 # set x\n").is_empty());
    }
}
