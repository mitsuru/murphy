//! `Layout/MultilineArrayBraceLayout` — the closing bracket of a multi-line
//! array literal must be positioned consistently with the opening bracket.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineArrayBraceLayout
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! gap_summary: autocorrect not implemented (detect-only).
//! notes: >
//!   Ports `on_array` + the shared `MultilineLiteralBraceLayout` mixin
//!   (`check_brace_layout`, `check`, `check_symmetrical`, `check_new_line`,
//!   `check_same_line`). Fires on `array` nodes whose `[` … `]` brackets span
//!   more than one line and whose closing `]` is mispositioned for the
//!   configured `EnforcedStyle`:
//!
//!   - symmetrical (default): if `[` shares a line with the first element, the
//!     closing `]` must share a line with the last element; if `[` is on its
//!     own line above the first element, `]` must be on its own line below the
//!     last element.
//!   - new_line: the closing `]` must be on the line after the last element.
//!   - same_line: the closing `]` must be on the same line as the last element.
//!
//!   Skips (mirroring the mixin's `ignored_literal?`): implicit (bracket-less)
//!   arrays, empty arrays, and single-line arrays. Also skips when the last
//!   element is or contains a heredoc whose terminator ends on/below the
//!   array's last line (`last_line_heredoc?`), since moving the brace would
//!   produce invalid code.
//!
//!   The opening delimiter is the array node's first byte and the closing
//!   delimiter its last byte, so every bracketed form is handled uniformly:
//!   `[ … ]`, `%w[ … ]`, `%w( … )`, `%i{ … }`, `%w< … >`.
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop's
//!   `MultilineLiteralBraceCorrector` moves the closing brace; the detect-only
//!   port ships without it, matching the precedent set by
//!   `Layout/MultilineMethodDefinitionBraceLayout`.
//! ```
//!
//! ## Matched shapes
//!
//! `array` nodes written with `[` … `]` (or a percent literal) whose brackets
//! span more than one line and whose closing `]` violates the configured
//! brace-layout style.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, Range, SourceTokenKind, cop};

const SAME_LINE_MESSAGE: &str = "The closing array brace must be on the same line as the last \
    array element when the opening brace is on the same line as the first array element.";
const NEW_LINE_MESSAGE: &str = "The closing array brace must be on the line after the last \
    array element when the opening brace is on a separate line from the first array element.";
const ALWAYS_NEW_LINE_MESSAGE: &str =
    "The closing array brace must be on the line after the last array element.";
const ALWAYS_SAME_LINE_MESSAGE: &str =
    "The closing array brace must be on the same line as the last array element.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineArrayBraceLayout;

/// Options for [`MultilineArrayBraceLayout`]. The `EnforcedStyle` key matches
/// RuboCop verbatim; the default is `symmetrical`.
#[derive(CopOptions)]
pub struct MultilineArrayBraceLayoutOptions {
    #[option(
        name = "EnforcedStyle",
        default = "symmetrical",
        description = "Where the closing `]` of a multi-line array literal sits."
    )]
    pub enforced_style: BraceLayoutStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum BraceLayoutStyle {
    /// Closing brace mirrors the opening brace.
    #[option(value = "symmetrical")]
    Symmetrical,
    /// Closing brace is always on a new line after the last element.
    #[option(value = "new_line")]
    NewLine,
    /// Closing brace is always on the same line as the last element.
    #[option(value = "same_line")]
    SameLine,
}

#[cop(
    name = "Layout/MultilineArrayBraceLayout",
    description = "Enforce closing-bracket placement in multi-line array literals.",
    default_severity = "warning",
    default_enabled = true,
    options = MultilineArrayBraceLayoutOptions,
)]
impl MultilineArrayBraceLayout {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns the 1-based line number of a byte offset.
fn line_of(offset: u32, src: &[u8]) -> usize {
    1 + src[..offset as usize].iter().filter(|&&b| b == b'\n').count()
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // RuboCop `ignored_literal?`: skip implicit (bracket-less), empty, or
    // single-line arrays. `is_bracketed` covers `[ … ]` and `%w[ … ]`.
    if !cx.is_bracketed(node) {
        return;
    }
    let elements = cx.array_elements(node);
    if elements.is_empty() {
        return;
    }

    // The opening delimiter is the array node's first byte (`[`, or `%w[`,
    // `%i(`, … for percent literals). The closing delimiter is the node's last
    // byte (`]`/`)`/`}`/`>`). A chained call (`[…].map`) or trailing comment
    // lives on the parent / outside the node range, so `range.end - 1` is
    // reliably the closing bracket. (A heredoc as the last element could
    // extend the range, but `last_line_heredoc` guards that case below.)
    let range = cx.range(node);
    let open_start = range.start;
    let close_start = range.end - 1;

    let src = cx.source().as_bytes();

    // RuboCop only fires on multi-line literals; a single-line array is skipped.
    let open_line = line_of(open_start, src);
    let close_line = line_of(close_start, src);
    if open_line == close_line {
        return;
    }

    let first_element = elements[0];
    let last_element = elements[elements.len() - 1];

    // RuboCop `last_line_heredoc?`: if the last element is/contains a heredoc
    // whose terminator ends on or after the array's last line, moving the
    // brace would yield invalid code — skip.
    if last_line_heredoc(last_element, close_line, cx) {
        return;
    }

    let first_element_line = line_of(cx.range(first_element).start, src);
    let last_element_line = line_of(cx.range(last_element).end, src);

    let opts = cx.options_or_default::<MultilineArrayBraceLayoutOptions>();
    let style = opts.enforced_style;

    // `opening_brace_on_same_line?` = opening `[` shares a line with first element.
    let open_with_first = open_line == first_element_line;
    // `closing_brace_on_same_line?` = closing `]` shares a line with last element.
    let close_with_last = close_line == last_element_line;

    let close_range = Range {
        start: close_start,
        end: close_start + 1,
    };

    match style {
        BraceLayoutStyle::SameLine => {
            if !close_with_last {
                cx.emit_offense(close_range, ALWAYS_SAME_LINE_MESSAGE, None);
            }
        }
        BraceLayoutStyle::NewLine => {
            if close_with_last {
                cx.emit_offense(close_range, ALWAYS_NEW_LINE_MESSAGE, None);
            }
        }
        BraceLayoutStyle::Symmetrical => {
            if open_with_first && !close_with_last {
                cx.emit_offense(close_range, SAME_LINE_MESSAGE, None);
            } else if !open_with_first && close_with_last {
                cx.emit_offense(close_range, NEW_LINE_MESSAGE, None);
            }
        }
    }
}

/// RuboCop `last_line_heredoc?`: true when `node` (or a descendant) is a
/// heredoc whose terminator ends on or after `parent_last_line`.
///
/// Detected via `HeredocStart`/`HeredocEnd` token pairs contained within the
/// element's range. If any heredoc's terminator line is `>= parent_last_line`,
/// the brace must not move.
fn last_line_heredoc(element: NodeId, parent_last_line: usize, cx: &Cx<'_>) -> bool {
    let src = cx.source().as_bytes();
    let el = cx.range(element);
    for tok in cx.tokens_in(el) {
        if tok.kind == SourceTokenKind::HeredocEnd {
            let term_line = line_of(tok.range.start, src);
            if term_line >= parent_last_line {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{
        BraceLayoutStyle, MultilineArrayBraceLayout, MultilineArrayBraceLayoutOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    fn new_line() -> MultilineArrayBraceLayoutOptions {
        MultilineArrayBraceLayoutOptions {
            enforced_style: BraceLayoutStyle::NewLine,
        }
    }

    fn same_line() -> MultilineArrayBraceLayoutOptions {
        MultilineArrayBraceLayoutOptions {
            enforced_style: BraceLayoutStyle::SameLine,
        }
    }

    // symmetrical (default) -----------------------------------------------

    #[test]
    fn symmetrical_flags_open_with_first_close_on_new_line() {
        test::<MultilineArrayBraceLayout>().expect_offense(indoc! {"
            a = [1,
              2
            ]
            ^ The closing array brace must be on the same line as the last array element when the opening brace is on the same line as the first array element.
        "});
    }

    #[test]
    fn symmetrical_flags_open_on_own_line_close_with_last() {
        test::<MultilineArrayBraceLayout>().expect_offense(indoc! {"
            a = [
              1,
              2]
               ^ The closing array brace must be on the line after the last array element when the opening brace is on a separate line from the first array element.
        "});
    }

    #[test]
    fn symmetrical_accepts_open_with_first_close_with_last() {
        test::<MultilineArrayBraceLayout>().expect_no_offenses(indoc! {"
            a = [1,
              2]
        "});
    }

    #[test]
    fn symmetrical_accepts_open_own_line_close_own_line() {
        test::<MultilineArrayBraceLayout>().expect_no_offenses(indoc! {"
            a = [
              1,
              2
            ]
        "});
    }

    #[test]
    fn accepts_single_line_array() {
        test::<MultilineArrayBraceLayout>().expect_no_offenses("a = [1, 2]\n");
    }

    #[test]
    fn accepts_empty_array() {
        test::<MultilineArrayBraceLayout>().expect_no_offenses("a = [\n]\n");
    }

    #[test]
    fn accepts_implicit_array() {
        // Bracket-less array literal: not this cop's concern.
        test::<MultilineArrayBraceLayout>().expect_no_offenses(indoc! {"
            a = 1,
              2
        "});
    }

    // new_line -------------------------------------------------------------

    #[test]
    fn new_line_flags_close_with_last() {
        test::<MultilineArrayBraceLayout>()
            .with_options(&new_line())
            .expect_offense(indoc! {"
                a = [1,
                  2]
                   ^ The closing array brace must be on the line after the last array element.
            "});
    }

    #[test]
    fn new_line_accepts_close_on_new_line() {
        test::<MultilineArrayBraceLayout>()
            .with_options(&new_line())
            .expect_no_offenses(indoc! {"
                a = [1,
                  2
                ]
            "});
    }

    // same_line ------------------------------------------------------------

    #[test]
    fn same_line_flags_close_on_new_line() {
        test::<MultilineArrayBraceLayout>()
            .with_options(&same_line())
            .expect_offense(indoc! {"
                a = [
                  1,
                  2
                ]
                ^ The closing array brace must be on the same line as the last array element.
            "});
    }

    #[test]
    fn same_line_accepts_close_with_last() {
        test::<MultilineArrayBraceLayout>()
            .with_options(&same_line())
            .expect_no_offenses(indoc! {"
                a = [
                  1,
                  2]
            "});
    }

    #[test]
    fn accepts_percent_literal_array() {
        // `%w[...]` spanning lines, closing on its own line, open on own line.
        test::<MultilineArrayBraceLayout>().expect_no_offenses(indoc! {"
            a = %w[
              foo
              bar
            ]
        "});
    }

    #[test]
    fn flags_percent_literal_paren_delimiter() {
        // `%w(...)` with the open delimiter sharing the first element's line but
        // the close `)` on a new line → symmetrical offense, proving non-`[]`
        // percent delimiters are handled.
        test::<MultilineArrayBraceLayout>().expect_offense(indoc! {"
            a = %w(foo
              bar
            )
            ^ The closing array brace must be on the same line as the last array element when the opening brace is on the same line as the first array element.
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineArrayBraceLayout);
