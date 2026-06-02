//! `Style/DoubleCopDisableDirective` — flags double disable directives on a single line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/DoubleCopDisableDirective
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detects comments that contain more than one `# rubocop:disable` or
//!   `# rubocop:todo` directive on the same line. Matches RuboCop exactly:
//!   only `rubocop:disable` and `rubocop:todo` are counted — `rubocop:enable`
//!   is not. The autocorrect merges subsequent directives into a comma-separated
//!   list by removing the ` # rubocop:(disable|todo)` fragments (with leading
//!   space) from the comment text.
//!
//!   Murphy-only `murphy:disable`/`murphy:todo` directives are NOT counted —
//!   RuboCop upstream only checks `rubocop:` prefixes, and murphy-native
//!   directives are governed by murphy's own engine.
//! ```
//!
//! ## Matched shapes
//!
//! Any comment token whose text contains more than one occurrence of
//! `rubocop:disable` or `rubocop:todo`.
//!
//! ## Examples
//!
//! ```ruby
//! # bad
//! def f # rubocop:disable Style/For # rubocop:disable Metrics/AbcSize
//! end
//!
//! # good
//! # rubocop:disable Metrics/AbcSize
//! def f # rubocop:disable Style/For
//! end
//! # rubocop:enable Metrics/AbcSize
//!
//! # good — both on one line with comma
//! def f # rubocop:disable Style/For, Metrics/AbcSize
//! end
//! ```

use murphy_plugin_api::{Cx, NoOptions, cop};

const MSG: &str = "More than one disable comment on one line.";

/// Count occurrences of `rubocop:disable` or `rubocop:todo` in `text`.
/// Mirrors RuboCop's `text.scan(/# rubocop:(?:disable|todo)/).size > 1`.
fn count_disable_directives(text: &[u8]) -> usize {
    let mut count = 0usize;
    let mut i = 0usize;
    while i < text.len() {
        if text[i..].starts_with(b"rubocop:disable") {
            count += 1;
            i += b"rubocop:disable".len();
        } else if text[i..].starts_with(b"rubocop:todo") {
            count += 1;
            i += b"rubocop:todo".len();
        } else {
            i += 1;
        }
    }
    count
}

/// Produce the corrected comment by replacing each ` # rubocop:(disable|todo)`
/// fragment with `,`. Mirrors RuboCop's
///   `comment.text.gsub(%r{ # rubocop:(disable|todo)}, ',')`
fn corrected_comment(text: &[u8]) -> String {
    let mut result = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < text.len() {
        let rest = &text[i..];
        if rest.starts_with(b" # rubocop:disable") {
            result.push(',');
            i += b" # rubocop:disable".len();
        } else if rest.starts_with(b" # rubocop:todo") {
            result.push(',');
            i += b" # rubocop:todo".len();
        } else {
            // Source is valid UTF-8; iterate via chars for correctness.
            let ch = rest[0] as char;
            result.push(ch);
            i += 1;
        }
    }
    result
}

/// Stateless unit struct.
#[derive(Default)]
pub struct DoubleCopDisableDirective;

#[cop(
    name = "Style/DoubleCopDisableDirective",
    description = "Checks for double rubocop:disable comments on a single line.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl DoubleCopDisableDirective {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let source = cx.source().as_bytes();

        for &comment in cx.comments() {
            let text = &source[comment.range.start as usize..comment.range.end as usize];

            if count_disable_directives(text) <= 1 {
                continue;
            }

            let fixed = corrected_comment(text);
            cx.emit_offense(comment.range, MSG, None);
            cx.emit_edit(comment.range, &fixed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DoubleCopDisableDirective;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_double_disable_on_same_line() {
        test::<DoubleCopDisableDirective>().expect_offense(indoc! {r#"
            def f # rubocop:disable Style/For # rubocop:disable Metrics/AbcSize
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ More than one disable comment on one line.
            end
        "#});
    }

    #[test]
    fn flags_double_todo_on_same_line() {
        test::<DoubleCopDisableDirective>().expect_offense(indoc! {r#"
            def f # rubocop:todo Style/For # rubocop:todo Metrics/AbcSize
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ More than one disable comment on one line.
            end
        "#});
    }

    #[test]
    fn flags_mixed_disable_todo_on_same_line() {
        test::<DoubleCopDisableDirective>().expect_offense(indoc! {r#"
            def f # rubocop:disable Style/For # rubocop:todo Metrics/AbcSize
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ More than one disable comment on one line.
            end
        "#});
    }

    #[test]
    fn accepts_single_disable_directive() {
        test::<DoubleCopDisableDirective>()
            .expect_no_offenses("def f # rubocop:disable Style/For\nend\n");
    }

    #[test]
    fn accepts_comma_separated_single_directive() {
        test::<DoubleCopDisableDirective>()
            .expect_no_offenses("def f # rubocop:disable Style/For, Metrics/AbcSize\nend\n");
    }

    #[test]
    fn accepts_enable_only() {
        test::<DoubleCopDisableDirective>()
            .expect_no_offenses("def f # rubocop:enable Style/For\nend\n");
    }

    #[test]
    fn corrects_double_disable_to_comma_separated() {
        test::<DoubleCopDisableDirective>().expect_correction(
            indoc! {r#"
                def f # rubocop:disable Style/For # rubocop:disable Metrics/AbcSize
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ More than one disable comment on one line.
                end
            "#},
            "def f # rubocop:disable Style/For, Metrics/AbcSize\nend\n",
        );
    }

    #[test]
    fn corrects_double_todo_to_comma_separated() {
        test::<DoubleCopDisableDirective>().expect_correction(
            indoc! {r#"
                def f # rubocop:todo Style/For # rubocop:todo Metrics/AbcSize
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ More than one disable comment on one line.
                end
            "#},
            "def f # rubocop:todo Style/For, Metrics/AbcSize\nend\n",
        );
    }

    #[test]
    fn idempotent_after_correction() {
        // After correction: "# rubocop:disable Style/For, Metrics/AbcSize" has
        // only one directive — no re-flag.
        test::<DoubleCopDisableDirective>()
            .expect_no_offenses("def f # rubocop:disable Style/For, Metrics/AbcSize\nend\n");
    }
}

murphy_plugin_api::submit_cop!(DoubleCopDisableDirective);
