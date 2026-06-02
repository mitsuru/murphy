//! `Style/MultilineMemoization` — checks wrapping style for multiline `||=`
//! memoization expressions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MultilineMemoization
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Both EnforcedStyle values are supported:
//!   - `keyword` (default): flags `foo ||= (\n  ...\n)` (paren form), suggests `begin/end`.
//!   - `braces`: flags `foo ||= begin\n  ...\nend` (begin form), suggests `(...)`.
//!   Detection is token-based: checks the first/last token of the RHS value range,
//!   so it works for both the Begin node (begin/end) and Unknown node ((...)) forms.
//!   Offense range is narrowed to the first line of the `or_asgn` node, matching
//!   RuboCop's highlighted range in practice.
//!   Autocorrect:
//!   - `keyword` style: replaces `(` with `begin`, `)` with `end`.
//!   - `braces` style: replaces `begin` with `(`, `end` with `)`.
//!   The `:keyword` style autocorrect does not adjust indentation (a minor cosmetic
//!   gap vs RuboCop). A second-pass Layout cop handles indentation.
//!   Single-line memoizations are not flagged.
//! ```
//!
//! ## Matched shapes
//!
//! `OrAsgn` nodes where:
//! - The RHS value is multiline
//! - Under `:keyword` style: the RHS is wrapped with `(` and `)`
//! - Under `:braces` style: the RHS is wrapped with `begin` and `end`
//!
//! ## Autocorrect
//!
//! Surgical two-edit form:
//! - Replace the opening delimiter token with the target delimiter
//! - Replace the closing delimiter token with the target delimiter

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Enforced wrapping style for multiline memoization.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MemoizationStyle {
    #[default]
    #[option(value = "keyword")]
    Keyword,
    #[option(value = "braces")]
    Braces,
}

#[derive(CopOptions)]
pub struct MultilineMemoizationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "keyword",
        description = "Whether to wrap multiline memoizations in `begin/end` or `()`."
    )]
    pub enforced_style: MemoizationStyle,
}

const KEYWORD_MSG: &str = "Wrap multiline memoization blocks in `begin` and `end`.";
const BRACES_MSG: &str = "Wrap multiline memoization blocks in `(` and `)`.";

#[derive(Default)]
pub struct MultilineMemoization;

#[cop(
    name = "Style/MultilineMemoization",
    description = "Wrap multiline memoization blocks in `begin` and `end` or `(` and `)`.",
    default_severity = "warning",
    default_enabled = true,
    options = MultilineMemoizationOptions,
)]
impl MultilineMemoization {
    #[on_node(kind = "or_asgn")]
    fn check_or_asgn(&self, node: NodeId, cx: &Cx<'_>, opts: &MultilineMemoizationOptions) {
        check(node, cx, opts.enforced_style);
    }
}

fn check(node: NodeId, cx: &Cx<'_>, style: MemoizationStyle) {
    // Extract the RHS value.
    let NodeKind::OrAsgn { value, .. } = *cx.kind(node) else {
        return;
    };

    // Must be multiline.
    if !cx.is_multiline(value) {
        return;
    }

    // Detect the wrapping style of the RHS using tokens in the value range.
    let value_range = cx.range(value);
    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();

    // Collect tokens fully contained in the value range.
    let tokens_in_value: &[_] = {
        let start_idx = toks.partition_point(|t| t.range.start < value_range.start);
        let end_idx = start_idx + toks[start_idx..].partition_point(|t| t.range.end <= value_range.end);
        &toks[start_idx..end_idx]
    };

    let Some(first) = tokens_in_value.first() else {
        return;
    };
    let Some(last) = tokens_in_value.last() else {
        return;
    };

    let first_text = &src[first.range.start as usize..first.range.end as usize];
    let last_text = &src[last.range.start as usize..last.range.end as usize];

    let is_paren_form = first.kind == SourceTokenKind::LeftParen
        && last.kind == SourceTokenKind::RightParen;
    let is_begin_form = first.kind == SourceTokenKind::Other
        && first_text == b"begin"
        && last.kind == SourceTokenKind::Other
        && last_text == b"end";

    // Narrow the offense to the first line of the or_asgn node.
    let offense_range = first_line_range(cx.range(node), cx);

    match style {
        MemoizationStyle::Keyword => {
            // Bad: paren form `(...)`. Should use `begin...end`.
            if !is_paren_form {
                return;
            }
            cx.emit_offense(offense_range, KEYWORD_MSG, None);
            // Autocorrect: `(` -> `begin`, `)` -> `end`.
            cx.emit_edit(first.range, "begin");
            cx.emit_edit(last.range, "end");
        }
        MemoizationStyle::Braces => {
            // Bad: begin form `begin...end`. Should use `(...)`.
            if !is_begin_form {
                return;
            }
            cx.emit_offense(offense_range, BRACES_MSG, None);
            // Autocorrect: `begin` -> `(`, `end` -> `)`.
            cx.emit_edit(first.range, "(");
            cx.emit_edit(last.range, ")");
        }
    }
}

/// Returns the range covering the first line of `range` (from `range.start`
/// to the first `\n` character, exclusive). If no newline is found, returns
/// the full range.
fn first_line_range(range: Range, cx: &Cx<'_>) -> Range {
    let src = cx.source().as_bytes();
    let start = range.start as usize;
    let end = range.end as usize;
    let first_newline = src[start..end]
        .iter()
        .position(|&b| b == b'\n')
        .map(|p| start + p)
        .unwrap_or(end);
    Range {
        start: range.start,
        end: first_newline as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoizationStyle, MultilineMemoization, MultilineMemoizationOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- EnforcedStyle: keyword (default) ---

    #[test]
    fn flags_paren_form_keyword_style() {
        test::<MultilineMemoization>().expect_offense(indoc! {"
            foo ||= (
            ^^^^^^^^^ Wrap multiline memoization blocks in `begin` and `end`.
              bar
              baz
            )
        "});
    }

    #[test]
    fn corrects_paren_to_begin_end() {
        test::<MultilineMemoization>().expect_correction(
            indoc! {"
                foo ||= (
                ^^^^^^^^^ Wrap multiline memoization blocks in `begin` and `end`.
                  bar
                  baz
                )
            "},
            "foo ||= begin\n  bar\n  baz\nend\n",
        );
    }

    #[test]
    fn accepts_begin_form_keyword_style() {
        test::<MultilineMemoization>().expect_no_offenses(indoc! {"
            foo ||= begin
              bar
              baz
            end
        "});
    }

    // --- EnforcedStyle: braces ---

    #[test]
    fn flags_begin_form_braces_style() {
        test::<MultilineMemoization>()
            .with_options(&MultilineMemoizationOptions {
                enforced_style: MemoizationStyle::Braces,
            })
            .expect_offense(indoc! {"
                foo ||= begin
                ^^^^^^^^^^^^^ Wrap multiline memoization blocks in `(` and `)`.
                  bar
                  baz
                end
            "});
    }

    #[test]
    fn corrects_begin_end_to_braces() {
        test::<MultilineMemoization>()
            .with_options(&MultilineMemoizationOptions {
                enforced_style: MemoizationStyle::Braces,
            })
            .expect_correction(
                indoc! {"
                    foo ||= begin
                    ^^^^^^^^^^^^^ Wrap multiline memoization blocks in `(` and `)`.
                      bar
                      baz
                    end
                "},
                "foo ||= (\n  bar\n  baz\n)\n",
            );
    }

    #[test]
    fn accepts_paren_form_braces_style() {
        test::<MultilineMemoization>()
            .with_options(&MultilineMemoizationOptions {
                enforced_style: MemoizationStyle::Braces,
            })
            .expect_no_offenses(indoc! {"
                foo ||= (
                  bar
                  baz
                )
            "});
    }

    // --- single-line: not flagged ---

    #[test]
    fn accepts_single_line_memoization() {
        test::<MultilineMemoization>().expect_no_offenses("foo ||= bar\n");
    }

    #[test]
    fn accepts_single_line_memoization_braces_style() {
        test::<MultilineMemoization>()
            .with_options(&MultilineMemoizationOptions {
                enforced_style: MemoizationStyle::Braces,
            })
            .expect_no_offenses("foo ||= bar\n");
    }

    // --- instance variable ---

    #[test]
    fn flags_ivar_paren_form() {
        test::<MultilineMemoization>().expect_offense(indoc! {"
            @foo ||= (
            ^^^^^^^^^^ Wrap multiline memoization blocks in `begin` and `end`.
              bar
            )
        "});
    }

    #[test]
    fn accepts_ivar_begin_form() {
        test::<MultilineMemoization>().expect_no_offenses(indoc! {"
            @foo ||= begin
              bar
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineMemoization);
