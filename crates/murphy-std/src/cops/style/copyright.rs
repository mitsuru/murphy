//! `Style/Copyright` — requires a copyright notice in each source file.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Copyright
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Checks first comment block matches the configured Notice regex.
//!   AutocorrectNotice is a v1 gap.
//! ```

use murphy_plugin_api::{CopOptions, Cx, cop};

#[derive(Default)]
pub struct Copyright;

#[derive(CopOptions)]
pub struct CopyrightOptions {
    #[option(default = "", description = "Substring to search for in comments (e.g. 'Copyright'). Regex not yet supported.")]
    pub notice: String,
}

#[cop(
    name = "Style/Copyright",
    description = "Require a copyright notice in each source file.",
    default_severity = "warning",
    default_enabled = false,
    options = CopyrightOptions
)]
impl Copyright {
    #[on_new_investigation]
    fn check_investigation(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<CopyrightOptions>();
        if opts.notice.is_empty() {
            return;
        }
        let source = cx.source();
        if source.trim().is_empty() {
            return;
        }
        // v1 limitation: uses substring matching instead of full regex.
        // RuboCop's regex-based matching is not yet supported.
        let comments = cx.comments();
        let found = comments.first().is_some_and(|first| {
            cx.raw_source(first.range).contains(&opts.notice)
        });
        if !found {
            cx.emit_offense(
                murphy_plugin_api::Range { start: 0, end: 0 },
                &format!("Include a copyright notice matching `{}` before any code.", opts.notice),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Copyright, CopyrightOptions};
    use murphy_plugin_api::test_support::test;

    #[test]
    fn flags_missing_copyright() {
        test::<Copyright>()
            .with_options(&CopyrightOptions {
                notice: "Copyright".to_string(),
            })
            .expect_offense("x = 1\n");
    }

    #[test]
    fn accepts_copyright_present() {
        test::<Copyright>()
            .with_options(&CopyrightOptions {
                notice: "Copyright".to_string(),
            })
            .expect_no_offenses("# Copyright (c) 2024 Acme Inc\nx = 1\n");
    }

    #[test]
    fn empty_notice_does_nothing() {
        test::<Copyright>()
            .with_options(&CopyrightOptions {
                notice: "".to_string(),
            })
            .expect_no_offenses("x = 1\n");
    }
}
murphy_plugin_api::submit_cop!(Copyright);
