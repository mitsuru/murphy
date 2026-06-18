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
//!   AllowInnerBackticks option mirrors RuboCop's
//!   allowed_backtick_literal?/allowed_percent_x_literal? across all four
//!   branches (the inner-backtick exemption applies to backtick literals,
//!   %x literals, and the multiline/mixed cases alike).
//!   Command heredocs (`<<`CMD``) are skipped, matching `node.heredoc?`.
//!   Autocorrect is a v1 gap (delimiter swap; deferred).
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
    #[option(name = "EnforcedStyle", 
        default = "backticks",
        description = "Enforced style for command literals."
    )]
    pub enforced_style: EnforcedStyle,
    #[option(name = "AllowInnerBackticks", 
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
        // RuboCop: `return if node.heredoc?`. A command heredoc (`<<`CMD`)
        // is parsed as an xstr whose literal source begins with `<<`.
        let offense_range = command_literal_range(cx.range(node), cx.source());
        let literal = cx.raw_source(offense_range);
        if literal.starts_with("<<") {
            return;
        }

        let is_backtick = literal.starts_with('`');
        // RuboCop's `contains_disallowed_backtick?`: an inner backtick is only
        // disallowed when `AllowInnerBackticks: false`. This exemption applies
        // uniformly across every style branch, not just backtick literals.
        let contains_disallowed_backtick =
            !opts.allow_inner_backticks && node_body(literal, is_backtick).contains('`');
        let is_multiline = literal.contains('\n');

        // Mirror RuboCop's on_xstr dispatch: backtick literal → check_backtick,
        // %x literal → check_percent_x.
        if is_backtick {
            if !allowed_backtick_literal(opts.enforced_style, is_multiline, contains_disallowed_backtick)
            {
                cx.emit_offense(offense_range, MSG_USE_PERCENT_X, None);
            }
        } else if !allowed_percent_x_literal(
            opts.enforced_style,
            is_multiline,
            contains_disallowed_backtick,
        ) {
            cx.emit_offense(offense_range, MSG_USE_BACKTICKS, None);
        }
    }
}

const MSG_USE_BACKTICKS: &str = "Use backticks around command string.";
const MSG_USE_PERCENT_X: &str = "Use `%x` around command string.";

/// RuboCop `allowed_backtick_literal?`: whether a backtick literal is allowed
/// under the given style.
fn allowed_backtick_literal(
    style: EnforcedStyle,
    is_multiline: bool,
    contains_disallowed_backtick: bool,
) -> bool {
    match style {
        EnforcedStyle::Backticks => !contains_disallowed_backtick,
        // `node.single_line? && !contains_disallowed_backtick?`
        EnforcedStyle::Mixed => !is_multiline && !contains_disallowed_backtick,
        // RuboCop's `allowed_backtick_literal?` returns nil (falsey) for
        // `:percent_x`, so a backtick literal is never allowed under percent_x.
        EnforcedStyle::PercentX => false,
    }
}

/// RuboCop `allowed_percent_x_literal?`: whether a `%x` literal is allowed under
/// the given style.
fn allowed_percent_x_literal(
    style: EnforcedStyle,
    is_multiline: bool,
    contains_disallowed_backtick: bool,
) -> bool {
    match style {
        // `%x` is only allowed under backticks style when converting back would
        // break an inner backtick.
        EnforcedStyle::Backticks => contains_disallowed_backtick,
        // `node.multiline? || contains_disallowed_backtick?`
        EnforcedStyle::Mixed => is_multiline || contains_disallowed_backtick,
        EnforcedStyle::PercentX => true,
    }
}

/// RuboCop `node_body`: the source between the opening and closing delimiters.
/// For a backtick literal the delimiters are a single `` ` `` each; for a `%x`
/// literal the opening is `%x` + a delimiter char and the closing is its mate.
fn node_body(literal: &str, is_backtick: bool) -> &str {
    if is_backtick {
        return literal
            .strip_prefix('`')
            .and_then(|s| s.strip_suffix('`'))
            .unwrap_or("");
    }
    // `%x<delim>...<close>` — skip `%x` and the one-char opener, drop the
    // trailing closer. `char_indices` keeps us on UTF-8 boundaries.
    let Some(rest) = literal.strip_prefix("%x") else {
        return "";
    };
    let mut chars = rest.char_indices();
    let Some((_, _open)) = chars.next() else {
        return "";
    };
    let body_start = chars.next().map_or(rest.len(), |(i, _)| i);
    let body_end = rest
        .char_indices()
        .next_back()
        .map_or(body_start, |(i, _)| i);
    rest.get(body_start..body_end).unwrap_or("")
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
                ^^^^^^^^^^^^^^^^ Use `%x` around command string.
            "});
    }

    #[test]
    fn default_style_is_backticks() {
        let opts = CommandLiteralOptions::default();
        assert_eq!(opts.enforced_style, EnforcedStyle::Backticks);
    }

    // RuboCop: under `backticks` style, a `%x` literal with an inner backtick
    // is allowed — converting to backticks would break the inner backtick.
    #[test]
    fn backticks_accepts_percent_x_with_inner_backtick() {
        test::<CommandLiteral>().expect_no_offenses("%x(echo `date`)\n");
    }

    // RuboCop: under `backticks` style, a backtick literal containing an inner
    // backtick must use `%x`.
    #[test]
    fn backticks_flags_inner_backtick() {
        test::<CommandLiteral>().expect_offense(indoc! {"
            `echo \\`date\\``
            ^^^^^^^^^^^^^^^ Use `%x` around command string.
        "});
    }

    // AllowInnerBackticks: true exempts backtick literals with inner backticks.
    #[test]
    fn backticks_allow_inner_backticks_accepts() {
        test::<CommandLiteral>()
            .with_options(&CommandLiteralOptions {
                enforced_style: EnforcedStyle::Backticks,
                allow_inner_backticks: true,
            })
            .expect_no_offenses("`echo \\`date\\``\n");
    }

    // AllowInnerBackticks: true under backticks style flips the `%x`-with-inner-
    // backtick case to an offense (it should be backticks now).
    #[test]
    fn backticks_allow_inner_backticks_flags_percent_x() {
        test::<CommandLiteral>()
            .with_options(&CommandLiteralOptions {
                enforced_style: EnforcedStyle::Backticks,
                allow_inner_backticks: true,
            })
            .expect_offense(indoc! {"
                %x(echo `date`)
                ^^^^^^^^^^^^^^^ Use backticks around command string.
            "});
    }

    #[test]
    fn mixed_accepts_single_line_backticks() {
        test::<CommandLiteral>()
            .with_options(&CommandLiteralOptions {
                enforced_style: EnforcedStyle::Mixed,
                allow_inner_backticks: false,
            })
            .expect_no_offenses("`find . -type d`\n");
    }

    // A multiline backtick literal under `mixed` style is an offense. The
    // offense range spans multiple lines, which the caret grammar cannot
    // express, so assert the underlying `allowed_*` decision directly.
    #[test]
    fn mixed_multiline_backtick_decision() {
        // multiline backtick: not allowed → offense expected.
        assert!(!super::allowed_backtick_literal(EnforcedStyle::Mixed, true, false));
        // multiline %x: allowed → no offense.
        assert!(super::allowed_percent_x_literal(EnforcedStyle::Mixed, true, false));
    }

    #[test]
    fn mixed_accepts_multiline_percent_x() {
        test::<CommandLiteral>()
            .with_options(&CommandLiteralOptions {
                enforced_style: EnforcedStyle::Mixed,
                allow_inner_backticks: false,
            })
            .expect_no_offenses("%x(\nfind .\n)\n");
    }

    // RuboCop returns early on command heredocs (`node.heredoc?`).
    #[test]
    fn skips_command_heredoc() {
        test::<CommandLiteral>().expect_no_offenses("<<`CMD`\nls\nCMD\n");
    }
}
murphy_plugin_api::submit_cop!(CommandLiteral);
