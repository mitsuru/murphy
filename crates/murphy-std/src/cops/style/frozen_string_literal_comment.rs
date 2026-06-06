//! `Style/FrozenStringLiteralComment` — require the `# frozen_string_literal: true`
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FrozenStringLiteralComment
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Implements all supported `EnforcedStyle` values: `always`, `always_true`,
//!   and `never`.  Autocorrect inserts `# frozen_string_literal: true` at the
//!   correct position (after shebang / encoding comment if present), enables
//!   disabled comments for `always_true`, and removes existing comments for
//!   `never`.  The correction is unsafe (frozen strings reject mutation) — same
//!   as RuboCop.
//! ```
//!
//! magic comment at the top of every Ruby file.
//!
//! ## What is checked
//!
//! In `always` mode (the default) the cop flags any file that does not contain a
//! `# frozen_string_literal:` magic comment in its leading comment block.  A
//! comment with `false` as the value is still accepted.  In `always_true` mode,
//! a missing comment is flagged and an existing disabled comment must be set to
//! `true`.  In `never` mode, any existing frozen string literal comment is
//! flagged.
//!
//! ## Offense position
//!
//! The offense is reported on the **first line of the file**, spanning from
//! byte 0 to the end of the first line (exclusive of the newline).  When the
//! source starts with a shebang or encoding comment the caret spans that line
//! instead — matching RuboCop's convention of highlighting the first token.
//!
//! ## Autocorrect
//!
//! Insert `# frozen_string_literal: true\n` at the right position:
//!
//! - No leading special comments → prepend at byte 0.
//! - Shebang only → insert after the shebang line.
//! - Encoding only → insert before the encoding comment line.
//! - Shebang + encoding → insert after the encoding comment line.
//!
//! Insertion is implemented as `cx.emit_edit` with an empty range at the
//! target byte offset (a pure insertion).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, Range, cop};

const MSG_MISSING_TRUE: &str = "Missing magic comment `# frozen_string_literal: true`.";
const MSG_MISSING: &str = "Missing frozen string literal comment.";
const MSG_UNNECESSARY: &str = "Unnecessary frozen string literal comment.";
const MSG_DISABLED: &str = "Frozen string literal comment must be set to `true`.";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct FrozenStringLiteralComment;

#[derive(CopOptions)]
pub struct FrozenStringLiteralCommentOptions {
    #[option(
        name = "EnforcedStyle",
        default = "always",
        description = "Whether to require, require true, or forbid the frozen string literal comment."
    )]
    pub enforced_style: FrozenStringLiteralStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum FrozenStringLiteralStyle {
    #[option(value = "always")]
    Always,
    #[option(value = "always_true")]
    AlwaysTrue,
    #[option(value = "never")]
    Never,
}

#[cop(
    name = "Style/FrozenStringLiteralComment",
    description = "Require the `# frozen_string_literal: true` magic comment at the top of every file.",
    default_severity = "warning",
    default_enabled = false,
    options = FrozenStringLiteralCommentOptions,
)]
impl FrozenStringLiteralComment {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        // Skip completely empty files — no Ruby to lint.
        if cx.source().is_empty() {
            return;
        }

        match cx
            .options_or_default::<FrozenStringLiteralCommentOptions>()
            .enforced_style
        {
            FrozenStringLiteralStyle::Always => ensure_comment(cx, MSG_MISSING),
            FrozenStringLiteralStyle::AlwaysTrue => ensure_enabled_comment(cx),
            FrozenStringLiteralStyle::Never => ensure_no_comment(cx),
        }
    }
}

fn ensure_no_comment(cx: &Cx<'_>) {
    let Some(comment) = cx.frozen_string_literal_comment() else {
        return;
    };

    cx.emit_offense(comment.range, MSG_UNNECESSARY, None);
    cx.emit_edit(line_range_with_newline(cx, comment.range), "");
}

fn ensure_comment(cx: &Cx<'_>, message: &'static str) {
    if cx.frozen_string_literal_comment().is_some() {
        return;
    }

    cx.emit_offense(first_line_range(cx), message, None);
    emit_insert_comment(cx);
}

fn ensure_enabled_comment(cx: &Cx<'_>) {
    let Some(comment) = cx.frozen_string_literal_comment() else {
        ensure_comment(cx, MSG_MISSING_TRUE);
        return;
    };

    if comment.value_bool == 1 {
        return;
    }

    cx.emit_offense(comment.range, MSG_DISABLED, None);
    cx.emit_edit(comment.value_range, "true");
}

fn first_line_range(cx: &Cx<'_>) -> Range {
    let src = cx.source();
    let first_line_end = src
        .as_bytes()
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(src.len());
    Range {
        start: 0,
        end: first_line_end as u32,
    }
}

fn line_range_with_newline(cx: &Cx<'_>, range: Range) -> Range {
    let src = cx.source().as_bytes();
    let start = src[..range.start as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    let end = src[range.end as usize..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(src.len(), |pos| range.end as usize + pos + 1);
    Range {
        start: start as u32,
        end: end as u32,
    }
}

fn emit_insert_comment(cx: &Cx<'_>) {
    let insert_at = insertion_point(cx);
    cx.emit_edit(
        Range {
            start: insert_at,
            end: insert_at,
        },
        "# frozen_string_literal: true\n",
    );
}

/// Compute the byte offset where the frozen_string_literal comment should be
/// inserted, mirroring RuboCop's placement logic:
///
/// - If neither shebang nor encoding comment: byte 0 (prepend).
/// - If shebang but no encoding: after the shebang line.
/// - If encoding but no shebang: before the encoding line (insert above it so
///   that `# encoding` stays in its canonical position).
/// - If both shebang and encoding: after the encoding comment line.
fn insertion_point(cx: &Cx<'_>) -> u32 {
    let shebang = cx.shebang();
    let encoding = cx.encoding_comment();

    match (shebang, encoding) {
        (None, None) => 0,
        (Some(sh), None) => {
            // After the shebang line (include the newline).
            let src = cx.source().as_bytes();
            let after = sh.range.end as usize;
            let tail = &src[after..];
            if tail.starts_with(b"\r\n") {
                (after + 2) as u32
            } else if tail.starts_with(b"\n") {
                (after + 1) as u32
            } else {
                after as u32
            }
        }
        (None, Some(enc)) => {
            // Before the encoding comment — insert above it.
            let src = cx.source().as_bytes();
            let line_start = src[..enc.range.start as usize]
                .iter()
                .rposition(|&b| b == b'\n')
                .map_or(0, |pos| pos + 1);
            line_start as u32
        }
        (Some(_sh), Some(enc)) => {
            // After the encoding comment line.
            let src = cx.source().as_bytes();
            let after = enc.range.end as usize;
            let tail = &src[after..];
            if tail.starts_with(b"\r\n") {
                (after + 2) as u32
            } else if tail.starts_with(b"\n") {
                (after + 1) as u32
            } else {
                after as u32
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrozenStringLiteralComment, FrozenStringLiteralCommentOptions, FrozenStringLiteralStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn always_true_opts() -> FrozenStringLiteralCommentOptions {
        FrozenStringLiteralCommentOptions {
            enforced_style: FrozenStringLiteralStyle::AlwaysTrue,
        }
    }

    fn never_opts() -> FrozenStringLiteralCommentOptions {
        FrozenStringLiteralCommentOptions {
            enforced_style: FrozenStringLiteralStyle::Never,
        }
    }

    // ---- offense detection ---------------------------------------------------

    #[test]
    fn flags_file_without_magic_comment() {
        test::<FrozenStringLiteralComment>().expect_offense(indoc! {r#"
            x = 1
            ^^^^^ Missing frozen string literal comment.
        "#});
    }

    #[test]
    fn accepts_file_with_frozen_true() {
        test::<FrozenStringLiteralComment>()
            .expect_no_offenses("# frozen_string_literal: true\nx = 1\n");
    }

    #[test]
    fn accepts_file_with_frozen_false() {
        // `false` counts as present — always_true style would reject this but
        // that is deferred to murphy-q2w9.
        test::<FrozenStringLiteralComment>()
            .expect_no_offenses("# frozen_string_literal: false\nx = 1\n");
    }

    #[test]
    fn empty_file_no_offense() {
        test::<FrozenStringLiteralComment>().expect_no_offenses("");
    }

    #[test]
    fn flags_file_with_shebang_but_no_magic_comment() {
        test::<FrozenStringLiteralComment>().expect_offense(indoc! {"
            #!/usr/bin/env ruby
            ^^^^^^^^^^^^^^^^^^^ Missing frozen string literal comment.
            x = 1
        "});
    }

    // ---- autocorrect: simple prepend ----------------------------------------

    #[test]
    fn autocorrects_plain_file_by_prepending() {
        test::<FrozenStringLiteralComment>().expect_correction(
            indoc! {r#"
                x = 1
                ^^^^^ Missing frozen string literal comment.
            "#},
            "# frozen_string_literal: true\nx = 1\n",
        );
    }

    // ---- autocorrect: after shebang -----------------------------------------

    #[test]
    fn autocorrects_file_with_shebang_inserts_after_shebang() {
        test::<FrozenStringLiteralComment>().expect_correction(
            indoc! {"
                #!/usr/bin/env ruby
                ^^^^^^^^^^^^^^^^^^^ Missing frozen string literal comment.
                x = 1
            "},
            "#!/usr/bin/env ruby\n# frozen_string_literal: true\nx = 1\n",
        );
    }

    // ---- autocorrect: with encoding comment ---------------------------------

    #[test]
    fn autocorrects_file_with_encoding_inserts_before_encoding() {
        test::<FrozenStringLiteralComment>().expect_correction(
            indoc! {"
                # encoding: utf-8
                ^^^^^^^^^^^^^^^^^ Missing frozen string literal comment.
                x = 1
            "},
            "# frozen_string_literal: true\n# encoding: utf-8\nx = 1\n",
        );
    }

    // ---- autocorrect: shebang + encoding ------------------------------------

    #[test]
    fn autocorrects_file_with_shebang_and_encoding_inserts_after_encoding() {
        test::<FrozenStringLiteralComment>().expect_correction(
            indoc! {"
                #!/usr/bin/env ruby
                ^^^^^^^^^^^^^^^^^^^ Missing frozen string literal comment.
                # encoding: utf-8
                x = 1
            "},
            "#!/usr/bin/env ruby\n# encoding: utf-8\n# frozen_string_literal: true\nx = 1\n",
        );
    }

    // ---- idempotency --------------------------------------------------------

    #[test]
    fn no_offense_on_already_corrected_file() {
        test::<FrozenStringLiteralComment>()
            .expect_no_offenses("# frozen_string_literal: true\nx = 1\n");
    }

    #[test]
    fn no_offense_after_shebang_with_magic_comment() {
        test::<FrozenStringLiteralComment>()
            .expect_no_offenses("#!/usr/bin/env ruby\n# frozen_string_literal: true\nx = 1\n");
    }

    #[test]
    fn always_true_flags_disabled_magic_comment() {
        test::<FrozenStringLiteralComment>()
            .with_options(&always_true_opts())
            .expect_offense(indoc! {"
                # frozen_string_literal: false
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Frozen string literal comment must be set to `true`.
                x = 1
            "});
    }

    #[test]
    fn always_true_autocorrects_disabled_magic_comment() {
        test::<FrozenStringLiteralComment>()
            .with_options(&always_true_opts())
            .expect_correction(
                indoc! {"
                    # frozen_string_literal: false
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Frozen string literal comment must be set to `true`.
                    x = 1
                "},
                "# frozen_string_literal: true\nx = 1\n",
            );
    }

    #[test]
    fn always_true_flags_missing_magic_comment_with_true_message() {
        test::<FrozenStringLiteralComment>()
            .with_options(&always_true_opts())
            .expect_offense(indoc! {r#"
                x = 1
                ^^^^^ Missing magic comment `# frozen_string_literal: true`.
            "#});
    }

    #[test]
    fn never_flags_present_magic_comment() {
        test::<FrozenStringLiteralComment>()
            .with_options(&never_opts())
            .expect_offense(indoc! {"
                # frozen_string_literal: true
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Unnecessary frozen string literal comment.
                x = 1
            "});
    }

    #[test]
    fn never_autocorrects_by_removing_magic_comment_line() {
        test::<FrozenStringLiteralComment>()
            .with_options(&never_opts())
            .expect_correction(
                indoc! {"
                    # frozen_string_literal: true
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Unnecessary frozen string literal comment.
                    x = 1
                "},
                "x = 1\n",
            );
    }
}
murphy_plugin_api::submit_cop!(FrozenStringLiteralComment);
