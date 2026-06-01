//! `Style/FrozenStringLiteralComment` — require the `# frozen_string_literal: true`
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FrozenStringLiteralComment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-q2w9
//! notes: >
//!   v1 implements `EnforcedStyle: always` (require the magic comment to be
//!   present with any value) — the "absent" path only.  The `never` style
//!   (flag files that *have* the comment), the `always_true` style (reject
//!   `false` values), and disabled-value handling are deferred to murphy-q2w9.
//!   Autocorrect inserts `# frozen_string_literal: true` at the correct position
//!   (after shebang / encoding comment if present), matching RuboCop's behaviour.
//!   The correction is unsafe (frozen strings reject mutation) — same as RuboCop.
//! ```
//!
//! magic comment at the top of every Ruby file.
//!
//! ## What is checked
//!
//! In `always` mode (the default and only implemented mode) the cop flags any
//! file that does not contain a `# frozen_string_literal:` magic comment in its
//! leading comment block.  A comment with `false` as the value is still accepted
//! (it is present; checking the value is the job of `always_true` mode).
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

use murphy_plugin_api::{Cx, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct FrozenStringLiteralComment;

#[cop(
    name = "Style/FrozenStringLiteralComment",
    description = "Require the `# frozen_string_literal: true` magic comment at the top of every file.",
    default_severity = "warning",
    default_enabled = false
)]
impl FrozenStringLiteralComment {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let src = cx.source();
        // Skip completely empty files — no Ruby to lint.
        if src.is_empty() {
            return;
        }

        // If the comment is already present (any value) we're done.
        if cx.frozen_string_literal_comment().is_some() {
            return;
        }

        // Report on the first line of the file.
        let first_line_end = src
            .as_bytes()
            .iter()
            .position(|&b| b == b'\n')
            .unwrap_or(src.len());
        let offense_range = Range {
            start: 0,
            end: first_line_end as u32,
        };

        cx.emit_offense(
            offense_range,
            "Missing frozen string literal comment.",
            None,
        );

        // Autocorrect: find the right insertion point.
        let insert_at = insertion_point(cx);
        // A zero-length edit at `insert_at` is a pure insertion.
        cx.emit_edit(
            Range {
                start: insert_at,
                end: insert_at,
            },
            "# frozen_string_literal: true\n",
        );
    }
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
    use super::FrozenStringLiteralComment;
    use murphy_plugin_api::test_support::{indoc, test};

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
}
murphy_plugin_api::submit_cop!(FrozenStringLiteralComment);
