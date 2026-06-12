//! `Lint/EmptyFile` — flag empty Ruby source files.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EmptyFile
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's empty-source check verbatim (v1.87.0:
//!   `offending? = empty_file? || (!AllowComments && contains_only_comments?)`)
//!   and the AllowComments:true default. The AllowComments override is read live
//!   via cx.options_or_default. No autocorrect (RuboCop has none).

use murphy_plugin_api::{CopOptions, Cx, Range, cop};

#[derive(Default)]
pub struct EmptyFile;

#[derive(CopOptions)]
pub struct Options {
    #[option(default = true, description = "When true, files containing only comments are allowed.")]
    pub allow_comments: bool,
}

#[cop(
    name = "Lint/EmptyFile",
    description = "Flag empty Ruby source files.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl EmptyFile {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        if cx.source().is_empty() || (!opts.allow_comments && contains_only_comments(cx)) {
            cx.emit_offense(Range { start: 0, end: 0 }, "Empty file detected.", None);
        }
    }
}

fn contains_only_comments(cx: &Cx<'_>) -> bool {
    cx.source().lines().all(|line| {
        let trimmed = line.trim_start();
        trimmed.is_empty() || trimmed.starts_with('#')
    })
}

murphy_plugin_api::submit_cop!(EmptyFile);

#[cfg(test)]
mod tests {
    use super::{EmptyFile, Options};
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_options};

    #[test]
    fn flags_empty_source() {
        let offenses = run_cop::<EmptyFile>("");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Empty file detected.");
    }

    #[test]
    fn accepts_code_and_comment_only_files_by_default() {
        assert!(run_cop::<EmptyFile>("foo.bar\n").is_empty());
        assert!(run_cop::<EmptyFile>("# comment\n").is_empty());
    }

    #[test]
    fn flags_comment_only_files_when_comments_are_not_allowed() {
        let offenses = run_cop_with_options::<EmptyFile>("# comment\n", &Options { allow_comments: false });
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Empty file detected.");
    }
}
