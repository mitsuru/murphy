//! `Style/CommandLiteral` — enforces consistent command literal style (backticks vs `%x`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CommandLiteral
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: backticks (default), mixed, percent_x supported.
//!   AllowInnerBackticks option supported.
//!   Heredoc detection is a v1 gap.
//!   Autocorrect is a v1 gap.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

#[derive(Default)]
pub struct CommandLiteral;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "backticks")]
    Backticks,
    #[option(value = "mixed")]
    Mixed,
    #[option(value = "percent_x")]
    PercentX,
}

#[derive(CopOptions)]
pub struct CommandLiteralOptions {
    #[option(
        default = "backticks",
        description = "Enforced style for command literals."
    )]
    pub enforced_style: EnforcedStyle,
    #[option(
        default = false,
        description = "Allow inner backticks."
    )]
    pub allow_inner_backticks: bool,
}

#[cop(
    name = "Style/CommandLiteral",
    description = "Enforce consistent command literal style.",
    default_severity = "warning",
    default_enabled = true,
    options = CommandLiteralOptions
)]
impl CommandLiteral {
    #[on_node(kind = "xstr")]
    fn check_xstr(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<CommandLiteralOptions>();
        let node_range = cx.range(node);
        let src = cx.raw_source(node_range);
        let is_backtick = src.starts_with('`');
        let offense_range = command_literal_range(node_range, cx.source());
        let body = if is_backtick {
            &src[1..src.len() - 1]
        } else {
            let start = src.find('(').or_else(|| src.find('{')).or_else(|| src.find('['));
            let end = src.rfind(')').or_else(|| src.rfind('}')).or_else(|| src.rfind(']'));
            match (start, end) {
                (Some(s), Some(e)) => &src[s + 1..e],
                _ => &src[3..src.len().saturating_sub(1)],
            }
        };
        let has_backtick = body.contains('`');
        match opts.enforced_style {
            EnforcedStyle::Backticks => {
                if is_backtick {
                    if !opts.allow_inner_backticks && has_backtick {
                        cx.emit_offense(offense_range, "Use `%x` around command string.", None);
                    }
                } else {
                    cx.emit_offense(offense_range, "Use backticks around command string.", None);
                }
            }
            EnforcedStyle::Mixed => {
                if is_backtick {
                    let is_multiline = src.contains('\n');
                    if is_multiline || has_backtick {
                        cx.emit_offense(offense_range, "Use `%x` around command string.", None);
                    }
                } else {
                    let is_single_line = !src.contains('\n');
                    if is_single_line && !has_backtick {
                        cx.emit_offense(offense_range, "Use backticks around command string.", None);
                    }
                }
            }
            EnforcedStyle::PercentX => {
                if is_backtick {
                    cx.emit_offense(offense_range, "Use `%x` around command string.", None);
                }
            }
        }
    }
}

fn command_literal_range(range: Range, source: &str) -> Range {
    if range.start > 0 && source.as_bytes().get(range.start as usize - 1) == Some(&b'%') {
        Range { start: range.start - 1, end: range.end }
    } else {
        range
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandLiteral, CommandLiteralOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn backticks_flags_percent_x() {
        test::<CommandLiteral>().expect_offense(indoc! {"
            folders = %x(find . -type d)
                      ^^^^^^^^^^^^^^^^^^ Use backticks around command string.
        "});
    }

    #[test]
    fn backticks_accepts_backticks() {
        test::<CommandLiteral>().expect_no_offenses("`find . -type d`\n");
    }

    #[test]
    fn percent_x_flags_backticks() {
        test::<CommandLiteral>()
            .with_options(&CommandLiteralOptions {
                enforced_style: EnforcedStyle::PercentX,
                allow_inner_backticks: false,
            })
            .expect_offense(indoc! {"
                `find . -type d`
                ^^^^^^^^^^^^^^^^^ Use `%x` around command string.
            "});
    }

    #[test]
    fn default_style_is_backticks() {
        let opts = CommandLiteralOptions::default();
        assert_eq!(opts.enforced_style, EnforcedStyle::Backticks);
    }
}
murphy_plugin_api::submit_cop!(CommandLiteral);
