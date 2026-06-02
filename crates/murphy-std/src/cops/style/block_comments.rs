//! `Style/BlockComments` — flags `=begin`/`=end` block comments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/BlockComments
//! upstream_version_checked: 1.86.2
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full parity with RuboCop. Detects `=begin`/`=end` block comments and
//!   autocorrects by converting each line in the body to a `# ` prefix
//!   line comment, matching RuboCop's three-gsub transform. Empty body
//!   (`=begin`/`=end` with nothing between) removes both markers entirely.
//!   No cop_config keys or options in upstream.
//! ```
//!
//! ## Matched shapes
//!
//! Any `=begin`/`=end` block comment found in the file
//! (`CommentKind::Block`). These are sourced from `cx.comments()` rather
//! than the AST, since block comments are not part of the node tree.
//!
//! ## Examples
//!
//! ```ruby
//! # bad
//! =begin
//! Multiple lines
//! of comments...
//! =end
//!
//! # good
//! # Multiple lines
//! # of comments...
//! ```
//!
//! ## Autocorrect
//!
//! Removes the `=begin` and `=end` markers and converts each body line to
//! a `# ` prefixed line comment. Empty lines in the body become `#` (a
//! comment with no content). Lines already starting with `#` are left as-is.
//! This mirrors RuboCop's three-gsub transform:
//!   1. Prepend `# ` to the start of the body.
//!   2. Replace `\n\n` (blank lines) with `\n#\n`.
//!   3. Prepend `# ` after each `\n` not followed by `#`.
//!
//! The autocorrect output is idempotent: the resulting `# ...` lines are
//! `CommentKind::Inline` comments that won't be flagged on a second pass.

use murphy_plugin_api::{CommentKind, Cx, NoOptions, Range, cop};

const MSG: &str = "Do not use block comments.";

/// Stateless unit struct.
#[derive(Default)]
pub struct BlockComments;

#[cop(
    name = "Style/BlockComments",
    description = "Do not use block comments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl BlockComments {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        for &comment in cx.comments() {
            if comment.kind != CommentKind::Block {
                continue;
            }
            cx.emit_offense(comment.range, MSG, None);
            emit_correction(cx, comment.range);
        }
    }
}

/// Compute and emit the autocorrect edit for one `=begin`/`=end` block comment.
///
/// The `range` is the full byte span of the block comment as stored in the
/// comment table. We reconstruct the replacement by:
///   1. Stripping the `=begin\n` header (first line of the range).
///   2. Stripping the `=end` (and optional trailing `\n`) footer.
///   3. Converting every body line: empty -> `#`, non-empty -> `# <line>`.
///
/// The final replacement string ends with a `\n` if and only if the original
/// range ended with `\n` (preserving the trailing-newline contract).
fn emit_correction(cx: &Cx<'_>, range: Range) {
    let src = cx.source().as_bytes();
    let start = range.start as usize;
    let end = range.end as usize;

    // The full text covered by the block comment range.
    let text = &src[start..end];

    // Determine whether the range ends with a newline (LF or CRLF).
    let trailing_newline = text.ends_with(b"\n");

    let text_str = match std::str::from_utf8(text) {
        Ok(s) => s,
        Err(_) => return, // Non-UTF-8 source -- skip autocorrect.
    };

    // Split the whole block into lines. For `=begin\nfoo\nbar\n=end\n`,
    // splitting by `\n` gives: ["=begin", "foo", "bar", "=end", ""].
    // Strip any trailing `\r` from each line to handle CRLF files.
    let lines: Vec<&str> = text_str.split('\n').map(|l| l.trim_end_matches('\r')).collect();

    // Validate: must start with `=begin`.
    if lines.is_empty() || lines[0] != "=begin" {
        return;
    }

    // Find the `=end` line. Search from the end (the last non-empty element
    // when there is a trailing newline).
    let eq_end_idx = match lines.iter().rposition(|&l| l == "=end") {
        Some(i) => i,
        None => return,
    };

    // Body lines: everything between line 0 (`=begin`) and `eq_end_idx` (`=end`).
    let body_lines = &lines[1..eq_end_idx];

    // Build the replacement applying the same transform as RuboCop:
    //   - empty line -> `#`
    //   - line already starting with `#` -> left as-is
    //   - other line -> `# <line>`
    let replacement = if body_lines.is_empty() {
        // Empty body: remove both markers entirely.
        String::new()
    } else {
        let mut out = String::new();
        for line in body_lines {
            if line.is_empty() {
                out.push_str("#\n");
            } else if line.starts_with('#') {
                out.push_str(line);
                out.push('\n');
            } else {
                out.push_str("# ");
                out.push_str(line);
                out.push('\n');
            }
        }
        // If the original range did NOT end with a newline, strip the trailing
        // newline we added for the last body line.
        if !trailing_newline && out.ends_with('\n') {
            out.pop();
        }
        out
    };

    cx.emit_edit(range, &replacement);
}

#[cfg(test)]
mod tests {
    use super::BlockComments;
    use murphy_plugin_api::test_support::{indoc, run_cop, run_cop_with_edits, test};

    // -- Detection tests ----------------------------------------------------

    #[test]
    fn flags_block_comment() {
        // Multi-line range: use run_cop to verify offense count and message.
        let src = "=begin\nMultiple lines\nof comments...\n=end\n";
        let offenses = run_cop::<BlockComments>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(offenses[0].message, "Do not use block comments.");
    }

    #[test]
    fn flags_block_comment_range_starts_at_begin() {
        // Verify the offense range starts at byte 0 (`=begin`).
        let src = "=begin\nfoo\n=end\n";
        let offenses = run_cop::<BlockComments>(src);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].range.start, 0, "range should start at `=begin`");
    }

    #[test]
    fn accepts_inline_comment() {
        test::<BlockComments>().expect_no_offenses("# Just a regular comment\n");
    }

    #[test]
    fn accepts_plain_ruby_code() {
        test::<BlockComments>().expect_no_offenses(indoc! {r#"
            def foo
              bar
            end
        "#});
    }

    #[test]
    fn accepts_empty_file() {
        test::<BlockComments>().expect_no_offenses("");
    }

    // -- Autocorrect tests --------------------------------------------------

    #[test]
    fn corrects_single_body_line() {
        let src = "=begin\nfoo\n=end\n";
        let run = run_cop_with_edits::<BlockComments>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.edits.len(), 1);
        assert_eq!(run.edits[0].replacement, "# foo\n");
    }

    #[test]
    fn corrects_multiple_body_lines() {
        let src = "=begin\nMultiple lines\nof comments...\n=end\n";
        let run = run_cop_with_edits::<BlockComments>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.edits.len(), 1);
        assert_eq!(
            run.edits[0].replacement,
            "# Multiple lines\n# of comments...\n"
        );
    }

    #[test]
    fn corrects_body_with_blank_line() {
        // Blank lines in the body become `#` (no trailing space).
        let src = "=begin\nfoo\n\nbar\n=end\n";
        let run = run_cop_with_edits::<BlockComments>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.edits.len(), 1);
        assert_eq!(run.edits[0].replacement, "# foo\n#\n# bar\n");
    }

    #[test]
    fn corrects_empty_body() {
        // `=begin\n=end\n` -- body is empty, remove both markers.
        let src = "=begin\n=end\n";
        let run = run_cop_with_edits::<BlockComments>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.edits.len(), 1);
        assert_eq!(run.edits[0].replacement, "");
    }

    #[test]
    fn corrects_body_line_already_starting_with_hash() {
        // Lines already starting with `#` are passed through unchanged.
        let src = "=begin\n# already commented\nnot yet\n=end\n";
        let run = run_cop_with_edits::<BlockComments>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.edits.len(), 1);
        assert_eq!(run.edits[0].replacement, "# already commented\n# not yet\n");
    }

    #[test]
    fn corrects_block_comment_before_code() {
        // Block comment followed by regular code -- only the comment is changed.
        let src = "=begin\nfoo\n=end\nx = 1\n";
        let run = run_cop_with_edits::<BlockComments>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(run.edits.len(), 1);
        // The edit range covers the block comment only; code after is untouched.
        assert_eq!(run.edits[0].replacement, "# foo\n");
        // Verify the edit ends before `x = 1`.
        let edit_end = run.edits[0].range.end as usize;
        assert!(
            src[edit_end..].starts_with("x = 1"),
            "edit should end before the code"
        );
    }

    #[test]
    fn correction_is_idempotent() {
        // After correction, running the cop again produces no offenses.
        // The corrected output uses `#` inline comments, which are not block comments.
        let corrected = "# Multiple lines\n# of comments...\n";
        let offenses = run_cop::<BlockComments>(corrected);
        assert_eq!(
            offenses.len(),
            0,
            "corrected source should not trigger the cop again"
        );
    }
}

murphy_plugin_api::submit_cop!(BlockComments);
