//! `Style/CommentedKeyword` — flags comments on same line as keywords.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CommentedKeyword
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Checks for comments on same line as begin/class/def/end/module.
//!   Exempts :nodoc:, :yields:, rubocop:disable, steep:ignore.
//!   Autocorrect is a v1 gap.
//! ```

use murphy_plugin_api::{Cx, cop};

const MSG_PREFIX: &str = "Do not place comments on the same line as the `";
const MSG_SUFFIX: &str = "` keyword.";
const KEYWORDS: &[&str] = &["begin", "class", "def", "end", "module"];
const ALLOWED: &[&str] = &[":nodoc:", ":yields:", "rubocop:disable", "rubocop:todo", "steep:ignore"];

#[derive(Default)]
pub struct CommentedKeyword;

#[cop(
    name = "Style/CommentedKeyword",
    description = "Do not place comments on the same line as certain keywords.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl CommentedKeyword {
    #[on_new_investigation]
    fn check_investigation(&self, cx: &Cx<'_>) {
        let source = cx.source();
        let bytes = source.as_bytes();
        for comment in cx.comments() {
            let src = cx.raw_source(comment.range);
            let text = src.trim();
            if !text.starts_with('#') {
                continue;
            }
            let body = text.trim_start_matches('#').trim();
            let is_allowed = ALLOWED.iter().any(|p| body.starts_with(p));
            if is_allowed {
                continue;
            }
            let line_start = bytes[..comment.range.start as usize]
                .iter()
                .rposition(|&b| b == b'\n')
                .map_or(0, |p| p + 1);
            let line = &bytes[line_start..comment.range.start as usize];
            let line_str = core::str::from_utf8(line).unwrap_or("").trim();
            for &kw in KEYWORDS {
                if line_str.starts_with(kw) && line_str.len() > kw.len()
                    && !line_str[kw.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
                {
                    cx.emit_offense(
                        comment.range,
                        &format!("{}{}{}", MSG_PREFIX, kw, MSG_SUFFIX),
                        None,
                    );
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CommentedKeyword;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_end_comment() {
        test::<CommentedKeyword>().expect_offense(indoc! {"
            if condition
              statement
            end # end if
                ^^^^^^^^ Do not place comments on the same line as the `end` keyword.
        "});
    }

    #[test]
    fn flags_class_comment() {
        test::<CommentedKeyword>().expect_offense(indoc! {"
            class X # comment
                    ^^^^^^^^^ Do not place comments on the same line as the `class` keyword.
              statement
            end
        "});
    }

    #[test]
    fn accepts_nodoc() {
        test::<CommentedKeyword>().expect_no_offenses(
            "class X # :nodoc:\n  y\nend\n",
        );
    }

    #[test]
    fn accepts_no_comment() {
        test::<CommentedKeyword>().expect_no_offenses(
            "class X\nend\n",
        );
    }
}
murphy_plugin_api::submit_cop!(CommentedKeyword);
