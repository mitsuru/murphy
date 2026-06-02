//! `Style/NumberedParameters` — restricts usage of numbered block parameters.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NumberedParameters
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - `EnforcedStyle: allow_single_line` (default) — only multi-line numblocks are flagged.
//!     - `EnforcedStyle: disallow` — all numblocks are flagged.
//!   Gap:
//!     - `allow_multiline` style (not in Murphy's default.yml SupportedStyles) is not supported.
//!   No autocorrect — RuboCop does not extend AutoCorrector for this cop.
//!   Requires Ruby 2.7+ (numbered parameters were introduced in 2.7).
//!   Offense range: for multi-line numblocks, the offense range is restricted to the
//!   first line of the block (from block start to the first newline) so the marker
//!   fits within a single line in test output and IDE diagnostics.
//!   For single-line numblocks (disallow style), the full node range is used.
//! ```
//!
//! ## Matched shapes
//!
//! `Numblock` nodes (blocks that use `_1`, `_2`, etc. instead of named params).
//!
//! - `allow_single_line` (default): flags multi-line numblocks only.
//! - `disallow`: flags all numblocks.
//!
//! ## Examples
//!
//! ```ruby
//! # allow_single_line (default)
//! # good
//! collection.each { puts _1 }
//!
//! # bad
//! collection.each do
//!   puts _1
//! end
//!
//! # disallow
//! # bad
//! collection.each { puts _1 }
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, cop};

const MSG_DISALLOW: &str = "Avoid using numbered parameters.";
const MSG_MULTI_LINE: &str = "Avoid using numbered parameters for multi-line blocks.";

/// Stateless unit struct.
#[derive(Default)]
pub struct NumberedParameters;

/// Enforced style for numbered parameters.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Allow numbered parameters in single-line blocks only (default).
    #[default]
    #[option(value = "allow_single_line")]
    AllowSingleLine,
    /// Disallow all numbered parameters.
    #[option(value = "disallow")]
    Disallow,
}

/// Options for `Style/NumberedParameters`.
#[derive(CopOptions, Debug)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "allow_single_line",
        description = "Restrict the usage of numbered parameters."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/NumberedParameters",
    description = "Restrict the usage of numbered parameters.",
    default_severity = "warning",
    default_enabled = false,
    options = Options,
)]
impl NumberedParameters {
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        let node_range = cx.range(node);
        let offense_range = first_line_range(node_range, cx.source().as_bytes());
        match opts.enforced_style {
            EnforcedStyle::Disallow => {
                cx.emit_offense(offense_range, MSG_DISALLOW, None);
            }
            EnforcedStyle::AllowSingleLine => {
                if cx.is_multiline(node) {
                    cx.emit_offense(offense_range, MSG_MULTI_LINE, None);
                }
            }
        }
    }
}

/// Restrict `range` to the first line: from `range.start` to the first `\n`
/// (exclusive) within the range, or `range.end` if there's no newline.
fn first_line_range(range: Range, source: &[u8]) -> Range {
    let start = range.start as usize;
    let end = range.end as usize;
    let line_end = source[start..end]
        .iter()
        .position(|&b| b == b'\n')
        .map(|pos| start + pos)
        .unwrap_or(end);
    Range { start: range.start, end: line_end as u32 }
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, NumberedParameters, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts_allow_single_line() -> Options {
        Options { enforced_style: EnforcedStyle::AllowSingleLine }
    }

    fn opts_disallow() -> Options {
        Options { enforced_style: EnforcedStyle::Disallow }
    }

    // ----- allow_single_line (default) -----

    #[test]
    fn allow_single_line_accepts_single_line_numblock() {
        test::<NumberedParameters>()
            .with_options(&opts_allow_single_line())
            .expect_no_offenses("collection.each { puts _1 }\n");
    }

    #[test]
    fn allow_single_line_flags_multi_line_numblock() {
        test::<NumberedParameters>()
            .with_options(&opts_allow_single_line())
            .expect_offense(indoc! {"
                collection.each do
                ^^^^^^^^^^^^^^^^^^ Avoid using numbered parameters for multi-line blocks.
                  puts _1
                end
            "});
    }

    // ----- disallow -----

    #[test]
    fn disallow_flags_single_line_numblock() {
        test::<NumberedParameters>()
            .with_options(&opts_disallow())
            .expect_offense(indoc! {"
                collection.each { puts _1 }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid using numbered parameters.
            "});
    }

    #[test]
    fn disallow_flags_multi_line_numblock() {
        test::<NumberedParameters>()
            .with_options(&opts_disallow())
            .expect_offense(indoc! {"
                collection.each do
                ^^^^^^^^^^^^^^^^^^ Avoid using numbered parameters.
                  puts _1
                end
            "});
    }

    // ----- options parsing -----

    #[test]
    fn options_parse_error_not_an_object() {
        use murphy_plugin_api::{ConfigErrorKind, CopOptions};
        let err = <Options as CopOptions>::from_config_json(b"[]")
            .expect_err("array root should be invalid");
        assert_eq!(err.kind(), &ConfigErrorKind::NotAnObject);
    }
}
murphy_plugin_api::submit_cop!(NumberedParameters);
