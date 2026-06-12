//! `Layout/MultilineMethodDefinitionBraceLayout` — the closing paren of a
//! multi-line method definition must be positioned consistently with the
//! opening paren.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineMethodDefinitionBraceLayout
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `def`/`defs` nodes whose parameter list spans more than one line
//!   and whose closing `)` is mispositioned for the configured
//!   `EnforcedStyle`. Mirrors RuboCop's `MultilineLiteralBraceLayout`:
//!
//!   - symmetrical (default): if the opening `(` shares a line with the first
//!     parameter, the closing `)` must share a line with the last parameter;
//!     if `(` is on its own line above the first parameter, `)` must be on
//!     its own line below the last parameter.
//!   - new_line: the closing `)` must be on the line after the last parameter.
//!   - same_line: the closing `)` must be on the same line as the last
//!     parameter.
//!
//!   The cop only fires when the parameter list spans more than one line and
//!   there is at least one parameter (RuboCop skips empty and single-line
//!   brace pairs).
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop's corrector moves the
//!   closing brace; the detect-only port ships without it.
//! ```
//!
//! ## Matched shapes
//!
//! `def`/`defs` nodes whose `(...)` parameter list spans more than one line
//! and whose closing `)` violates the configured brace-layout style.

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop,
};

const SAME_LINE_MESSAGE: &str = "Closing method definition brace must be on the \
    same line as the last parameter when opening brace is on the same line as the \
    first parameter.";
const NEW_LINE_MESSAGE: &str = "Closing method definition brace must be on the \
    line after the last parameter when opening brace is on a separate line from \
    the first parameter.";
const ALWAYS_NEW_LINE_MESSAGE: &str =
    "Closing method definition brace must be on the line after the last parameter.";
const ALWAYS_SAME_LINE_MESSAGE: &str =
    "Closing method definition brace must be on the same line as the last parameter.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineMethodDefinitionBraceLayout;

/// Options for [`MultilineMethodDefinitionBraceLayout`]. The `EnforcedStyle`
/// key matches RuboCop verbatim; the default is `symmetrical`.
#[derive(CopOptions)]
pub struct MultilineMethodDefinitionBraceLayoutOptions {
    #[option(
        name = "EnforcedStyle",
        default = "symmetrical",
        description = "Where the closing `)` of a multi-line method definition sits."
    )]
    pub enforced_style: BraceLayoutStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum BraceLayoutStyle {
    /// Closing brace mirrors the opening brace.
    #[option(value = "symmetrical")]
    Symmetrical,
    /// Closing brace is always on a new line after the last parameter.
    #[option(value = "new_line")]
    NewLine,
    /// Closing brace is always on the same line as the last parameter.
    #[option(value = "same_line")]
    SameLine,
}

#[cop(
    name = "Layout/MultilineMethodDefinitionBraceLayout",
    description = "Enforce closing-paren placement in multi-line method definitions.",
    default_severity = "warning",
    default_enabled = true,
    options = MultilineMethodDefinitionBraceLayoutOptions,
)]
impl MultilineMethodDefinitionBraceLayout {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns the 1-based line number of a byte offset.
fn line_of(offset: u32, src: &[u8]) -> usize {
    1 + src[..offset as usize].iter().filter(|&&b| b == b'\n').count()
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<MultilineMethodDefinitionBraceLayoutOptions>();
    let style = opts.enforced_style;

    let Some(args_node) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(list) = cx.kind(args_node) else {
        return;
    };
    let params = cx.list(*list);
    if params.is_empty() {
        return;
    }

    let first_param = params[0];
    let last_param = params[params.len() - 1];

    // Locate the opening `(` (last LeftParen before the first parameter, at or
    // after the def keyword) and the matching closing `)`.
    let Some((open_paren_start, close_paren_start)) = brace_pair(node, first_param, cx) else {
        // Method defined without parens cannot have a brace-layout offense.
        return;
    };

    let src = cx.source().as_bytes();

    // RuboCop only fires when the braces are on different lines from each
    // other (multi-line literal). A single-line `def foo(a, b)` is ignored.
    let open_line = line_of(open_paren_start, src);
    let close_line = line_of(close_paren_start, src);
    if open_line == close_line {
        return;
    }

    let first_param_line = line_of(cx.range(first_param).start, src);
    let last_param_line = line_of(cx.range(last_param).end, src);

    // `opening_brace_on_same_line` = opening `(` shares a line with first param.
    let open_with_first = open_line == first_param_line;
    // `closing_brace_on_same_line` = closing `)` shares a line with last param.
    let close_with_last = close_line == last_param_line;

    let close_range = Range {
        start: close_paren_start,
        end: close_paren_start + 1,
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

/// Find the `(` opening the parameter list and the matching `)`.
///
/// Returns `(open_paren_start, close_paren_start)` byte offsets, or `None` if
/// the method has no parentheses.
fn brace_pair(node: NodeId, first_param: NodeId, cx: &Cx<'_>) -> Option<(u32, u32)> {
    let toks = cx.sorted_tokens();
    let node_start = cx.range(node).start;
    let node_end = cx.range(node).end;
    let first_param_start = cx.range(first_param).start;

    // Opening `(` is the last LeftParen strictly before the first parameter
    // and at or after the def keyword (skips a receiver paren in
    // `def (obj).foo(arg)`).
    let idx = toks.partition_point(|t| t.range.start < first_param_start);
    let open_paren_start = toks[..idx]
        .iter()
        .rev()
        .take_while(|t| t.range.start >= node_start)
        .find(|t| t.kind == SourceTokenKind::LeftParen)
        .map(|t| t.range.start)?;

    // Matching `)` via depth counting.
    let search_start = open_paren_start + 1;
    let idx2 = toks.partition_point(|t| t.range.start < search_start);
    let mut depth: i32 = 1;
    for tok in &toks[idx2..] {
        if tok.range.start >= node_end {
            break;
        }
        match tok.kind {
            SourceTokenKind::LeftParen => depth += 1,
            SourceTokenKind::RightParen => {
                depth -= 1;
                if depth == 0 {
                    return Some((open_paren_start, tok.range.start));
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        BraceLayoutStyle, MultilineMethodDefinitionBraceLayout,
        MultilineMethodDefinitionBraceLayoutOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    fn new_line() -> MultilineMethodDefinitionBraceLayoutOptions {
        MultilineMethodDefinitionBraceLayoutOptions {
            enforced_style: BraceLayoutStyle::NewLine,
        }
    }

    fn same_line() -> MultilineMethodDefinitionBraceLayoutOptions {
        MultilineMethodDefinitionBraceLayoutOptions {
            enforced_style: BraceLayoutStyle::SameLine,
        }
    }

    // symmetrical (default) -----------------------------------------------

    #[test]
    fn symmetrical_flags_open_with_first_close_on_new_line() {
        test::<MultilineMethodDefinitionBraceLayout>().expect_offense(indoc! {"
            def foo(a,
              b
            )
            ^ Closing method definition brace must be on the same line as the last parameter when opening brace is on the same line as the first parameter.
            end
        "});
    }

    #[test]
    fn symmetrical_flags_open_on_own_line_close_with_last() {
        test::<MultilineMethodDefinitionBraceLayout>().expect_offense(indoc! {"
            def foo(
              a,
              b)
               ^ Closing method definition brace must be on the line after the last parameter when opening brace is on a separate line from the first parameter.
            end
        "});
    }

    #[test]
    fn symmetrical_accepts_open_with_first_close_with_last() {
        test::<MultilineMethodDefinitionBraceLayout>().expect_no_offenses(indoc! {"
            def foo(a,
              b)
            end
        "});
    }

    #[test]
    fn symmetrical_accepts_open_own_line_close_own_line() {
        test::<MultilineMethodDefinitionBraceLayout>().expect_no_offenses(indoc! {"
            def foo(
              a,
              b
            )
            end
        "});
    }

    #[test]
    fn accepts_single_line_signature() {
        test::<MultilineMethodDefinitionBraceLayout>().expect_no_offenses(indoc! {"
            def foo(a, b)
            end
        "});
    }

    #[test]
    fn accepts_no_args() {
        test::<MultilineMethodDefinitionBraceLayout>().expect_no_offenses(indoc! {"
            def foo
            end
        "});
    }

    // new_line -------------------------------------------------------------

    #[test]
    fn new_line_flags_close_with_last() {
        test::<MultilineMethodDefinitionBraceLayout>()
            .with_options(&new_line())
            .expect_offense(indoc! {"
                def foo(a,
                  b)
                   ^ Closing method definition brace must be on the line after the last parameter.
                end
            "});
    }

    #[test]
    fn new_line_accepts_close_on_new_line() {
        test::<MultilineMethodDefinitionBraceLayout>()
            .with_options(&new_line())
            .expect_no_offenses(indoc! {"
                def foo(a,
                  b
                )
                end
            "});
    }

    // same_line ------------------------------------------------------------

    #[test]
    fn same_line_flags_close_on_new_line() {
        test::<MultilineMethodDefinitionBraceLayout>()
            .with_options(&same_line())
            .expect_offense(indoc! {"
                def foo(
                  a,
                  b
                )
                ^ Closing method definition brace must be on the same line as the last parameter.
                end
            "});
    }

    #[test]
    fn same_line_accepts_close_with_last() {
        test::<MultilineMethodDefinitionBraceLayout>()
            .with_options(&same_line())
            .expect_no_offenses(indoc! {"
                def foo(
                  a,
                  b)
                end
            "});
    }

    #[test]
    fn symmetrical_flags_singleton_method() {
        test::<MultilineMethodDefinitionBraceLayout>().expect_offense(indoc! {"
            def self.foo(a,
              b
            )
            ^ Closing method definition brace must be on the same line as the last parameter when opening brace is on the same line as the first parameter.
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineMethodDefinitionBraceLayout);
