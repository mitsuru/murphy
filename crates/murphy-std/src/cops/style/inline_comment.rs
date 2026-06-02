//! `Style/InlineComment` — flags trailing inline comments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/InlineComment
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags every inline comment that is not:
//!     - An own-line comment (i.e. only whitespace before the `#` on that line).
//!     - A `rubocop:enable`, `rubocop:disable`, `murphy:enable`, or
//!       `murphy:disable` directive comment.
//!   No autocorrect (RuboCop does not implement one either).
//!   `=begin`/`=end` block comments are never flagged — they span whole lines.
//! ```
//!
//! ## Matched shapes
//!
//! Any inline comment token (`# text`) where non-whitespace code exists to
//! the left on the same line AND the comment text does not start with
//! `# rubocop:` or `# murphy:`.
//!
//! ## Examples
//!
//! ```ruby
//! # good — own-line comment
//! foo.each do |f|
//!   # Standalone comment
//!   f.bar
//! end
//!
//! # bad — trailing inline comment
//! foo.each do |f|
//!   f.bar # Trailing inline comment
//! end
//! ```

use murphy_plugin_api::{Cx, NoOptions, cop};

const MSG: &str = "Avoid trailing inline comments.";

/// Stateless unit struct.
#[derive(Default)]
pub struct InlineComment;

#[cop(
    name = "Style/InlineComment",
    description = "Avoid trailing inline comments.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl InlineComment {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let source = cx.source();
        let bytes = source.as_bytes();

        for &comment in cx.comments() {
            // Skip if this is an own-line comment (only whitespace before `#`
            // on this line). Equivalent to RuboCop's `comment_line?`.
            if is_own_line(bytes, comment.range.start as usize) {
                continue;
            }

            // Skip rubocop:/murphy: directive comments (enable/disable/todo).
            let text = &bytes[comment.range.start as usize..comment.range.end as usize];
            if is_directive_comment(text) {
                continue;
            }

            cx.emit_offense(comment.range, MSG, None);
        }
    }
}

/// Returns `true` if the comment at `comment_start` is an own-line comment —
/// i.e., there is only whitespace between the start of the line and `#`.
fn is_own_line(bytes: &[u8], comment_start: usize) -> bool {
    let line_start = bytes[..comment_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    bytes[line_start..comment_start]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
}

/// Returns `true` if the comment text is a `rubocop:` or `murphy:` directive.
///
/// Matches `# rubocop:enable`, `# rubocop:disable`, `# rubocop:todo`,
/// `# murphy:enable`, `# murphy:disable`, `# murphy:todo` (and variants
/// with extra whitespace after `#`).
fn is_directive_comment(text: &[u8]) -> bool {
    let rest = match text.strip_prefix(b"#") {
        Some(r) => r,
        None => return false,
    };
    // Trim leading spaces/tabs.
    let trimmed = rest
        .iter()
        .position(|&b| b != b' ' && b != b'\t')
        .map_or(&[] as &[u8], |i| &rest[i..]);
    trimmed.starts_with(b"rubocop:") || trimmed.starts_with(b"murphy:")
}

#[cfg(test)]
mod tests {
    use super::InlineComment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_trailing_inline_comment() {
        // "# Trailing inline comment" = 25 chars
        test::<InlineComment>().expect_offense(indoc! {r#"
            foo.each do |f|
              f.bar # Trailing inline comment
                    ^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid trailing inline comments.
            end
        "#});
    }

    #[test]
    fn flags_inline_comment_after_assignment() {
        // "# set x" = 7 chars
        test::<InlineComment>().expect_offense(indoc! {r#"
            x = 1 # set x
                  ^^^^^^^ Avoid trailing inline comments.
        "#});
    }

    #[test]
    fn accepts_own_line_comment() {
        test::<InlineComment>().expect_no_offenses(indoc! {r#"
            foo.each do |f|
              # Standalone comment
              f.bar
            end
        "#});
    }

    #[test]
    fn accepts_rubocop_disable_directive() {
        test::<InlineComment>()
            .expect_no_offenses("x = 1 # rubocop:disable Style/SomeCop\n");
    }

    #[test]
    fn accepts_rubocop_enable_directive() {
        test::<InlineComment>()
            .expect_no_offenses("x = 1 # rubocop:enable Style/SomeCop\n");
    }

    #[test]
    fn accepts_murphy_disable_directive() {
        test::<InlineComment>()
            .expect_no_offenses("x = 1 # murphy:disable Style/SomeCop\n");
    }

    #[test]
    fn accepts_comment_only_file() {
        test::<InlineComment>().expect_no_offenses("# Just a comment\n");
    }

    #[test]
    fn accepts_own_line_comment_with_indentation() {
        test::<InlineComment>().expect_no_offenses(indoc! {r#"
            def foo
              # This comment is own-line
              bar
            end
        "#});
    }

    #[test]
    fn flags_comment_after_method_def() {
        // "# some comment" = 14 chars
        test::<InlineComment>().expect_offense(indoc! {r#"
            def foo # some comment
                    ^^^^^^^^^^^^^^ Avoid trailing inline comments.
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(InlineComment);
