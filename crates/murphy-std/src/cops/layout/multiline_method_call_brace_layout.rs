//! `Layout/MultilineMethodCallBraceLayout` — the closing paren of a
//! multi-line method call must be positioned consistently with the opening
//! paren.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineMethodCallBraceLayout
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `send`/`csend` nodes whose argument list spans more than one
//!   line and whose closing `)` is mispositioned for the configured
//!   `EnforcedStyle`. Mirrors RuboCop's `MultilineLiteralBraceLayout` mixin
//!   (the cop's only override is `children = node.arguments` and the
//!   `single_line_ignoring_receiver?` ignore):
//!
//!   - symmetrical (default): if the opening `(` shares a line with the
//!     first argument, the closing `)` must share a line with the last
//!     argument; otherwise `)` must be on its own line below the last
//!     argument.
//!   - new_line: the closing `)` must be on the line after the last argument.
//!   - same_line: the closing `)` must be on the same line as the last
//!     argument.
//!
//!   RuboCop's `ignored_literal?` skips implicit (paren-less) calls, empty
//!   argument lists, and brace pairs that are single-line *ignoring the
//!   receiver* (`single_line_ignoring_receiver?`). Murphy reproduces all
//!   three: it requires a real `(`/`)` pair (skipping paren-less calls and
//!   `foo[...]` index calls), an at-least-one-argument list, and skips when
//!   the `(` and `)` share a physical line — which is exactly
//!   `single_line_ignoring_receiver?` because it compares only the brace
//!   tokens, not the whole-node span.
//!
//!   RuboCop's `last_line_heredoc?` guard skips a call whose last argument
//!   contains a heredoc (its `last_line` lands on the closer, not the arg).
//!   Murphy reproduces this by skipping when a `HeredocStart` token falls
//!   within the last argument's range.
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop's corrector moves the
//!   closing brace; the detect-only port ships without it.
//! ```
//!
//! ## Matched shapes
//!
//! `send`/`csend` nodes whose `(...)` argument list spans more than one line
//! and whose closing `)` violates the configured brace-layout style.

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, Range, SourceTokenKind, cop,
};

const SAME_LINE_MESSAGE: &str = "Closing method call brace must be on the same \
    line as the last argument when opening brace is on the same line as the \
    first argument.";
const NEW_LINE_MESSAGE: &str = "Closing method call brace must be on the line \
    after the last argument when opening brace is on a separate line from the \
    first argument.";
const ALWAYS_NEW_LINE_MESSAGE: &str =
    "Closing method call brace must be on the line after the last argument.";
const ALWAYS_SAME_LINE_MESSAGE: &str =
    "Closing method call brace must be on the same line as the last argument.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineMethodCallBraceLayout;

/// Options for [`MultilineMethodCallBraceLayout`]. The `EnforcedStyle` key
/// matches RuboCop verbatim; the default is `symmetrical`.
#[derive(CopOptions)]
pub struct MultilineMethodCallBraceLayoutOptions {
    #[option(
        name = "EnforcedStyle",
        default = "symmetrical",
        description = "Where the closing `)` of a multi-line method call sits."
    )]
    pub enforced_style: BraceLayoutStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum BraceLayoutStyle {
    /// Closing brace mirrors the opening brace.
    #[option(value = "symmetrical")]
    Symmetrical,
    /// Closing brace is always on a new line after the last argument.
    #[option(value = "new_line")]
    NewLine,
    /// Closing brace is always on the same line as the last argument.
    #[option(value = "same_line")]
    SameLine,
}

#[cop(
    name = "Layout/MultilineMethodCallBraceLayout",
    description = "Enforce closing-paren placement in multi-line method calls.",
    default_severity = "warning",
    default_enabled = true,
    options = MultilineMethodCallBraceLayoutOptions,
)]
impl MultilineMethodCallBraceLayout {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns the 1-based line number of a byte offset.
fn line_of(offset: u32, src: &[u8]) -> usize {
    1 + src[..offset as usize].iter().filter(|&&b| b == b'\n').count()
}

/// Whether the last argument contains a heredoc opener (`HeredocStart` token)
/// anywhere within its source range — RuboCop's `last_line_heredoc?` analog.
fn last_arg_has_heredoc(last_arg: NodeId, cx: &Cx<'_>) -> bool {
    let r = cx.range(last_arg);
    cx.tokens_in(r)
        .iter()
        .any(|t| t.kind == SourceTokenKind::HeredocStart)
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<MultilineMethodCallBraceLayoutOptions>();
    let style = opts.enforced_style;

    let args = cx.call_arguments(node);
    // `empty_literal?` — no arguments means no brace layout to enforce.
    if args.is_empty() {
        return;
    }

    // `implicit_literal?` — a call without an explicit `(` (e.g. `foo a, b`)
    // has no brace pair. `begin()`/`end()` resolve only the argument-list
    // parens, so a paren-less call yields `Range::ZERO`.
    let open = cx.loc(node).begin();
    let close = cx.loc(node).end();
    if open == Range::ZERO || close == Range::ZERO {
        return;
    }

    let src = cx.source().as_bytes();

    let first_arg = args[0];
    let last_arg = args[args.len() - 1];

    // `last_line_heredoc?` — RuboCop recursively descends the last argument
    // looking for a heredoc whose closer lands on the call's last line. We
    // approximate it by skipping when a `HeredocStart` token falls within the
    // last argument's source range.
    if last_arg_has_heredoc(last_arg, cx) {
        return;
    }

    // `single_line_ignoring_receiver?` / `single_line?` — skip when the brace
    // pair sits on one physical line. Comparing only the brace tokens (not the
    // whole-node span) reproduces RuboCop's receiver-ignoring single-line
    // check.
    let open_line = line_of(open.start, src);
    let close_line = line_of(close.start, src);
    if open_line == close_line {
        return;
    }

    // `opening_brace_on_same_line?` = `begin.line == children.first.first_line`.
    let first_arg_line = line_of(cx.range(first_arg).start, src);
    // `closing_brace_on_same_line?` = `end.line == children.last.last_line`.
    let last_arg_line = line_of(cx.range(last_arg).end, src);

    let open_with_first = open_line == first_arg_line;
    let close_with_last = close_line == last_arg_line;

    match style {
        BraceLayoutStyle::SameLine => {
            if !close_with_last {
                cx.emit_offense(close, ALWAYS_SAME_LINE_MESSAGE, None);
            }
        }
        BraceLayoutStyle::NewLine => {
            if close_with_last {
                cx.emit_offense(close, ALWAYS_NEW_LINE_MESSAGE, None);
            }
        }
        BraceLayoutStyle::Symmetrical => {
            if open_with_first && !close_with_last {
                cx.emit_offense(close, SAME_LINE_MESSAGE, None);
            } else if !open_with_first && close_with_last {
                cx.emit_offense(close, NEW_LINE_MESSAGE, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BraceLayoutStyle, MultilineMethodCallBraceLayout,
        MultilineMethodCallBraceLayoutOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    fn new_line() -> MultilineMethodCallBraceLayoutOptions {
        MultilineMethodCallBraceLayoutOptions {
            enforced_style: BraceLayoutStyle::NewLine,
        }
    }

    fn same_line() -> MultilineMethodCallBraceLayoutOptions {
        MultilineMethodCallBraceLayoutOptions {
            enforced_style: BraceLayoutStyle::SameLine,
        }
    }

    // symmetrical (default) -----------------------------------------------

    #[test]
    fn symmetrical_flags_open_with_first_close_on_new_line() {
        test::<MultilineMethodCallBraceLayout>().expect_offense(indoc! {"
            foo(a,
              b
            )
            ^ Closing method call brace must be on the same line as the last argument when opening brace is on the same line as the first argument.
        "});
    }

    #[test]
    fn symmetrical_flags_open_on_own_line_close_with_last() {
        test::<MultilineMethodCallBraceLayout>().expect_offense(indoc! {"
            foo(
              a,
              b)
               ^ Closing method call brace must be on the line after the last argument when opening brace is on a separate line from the first argument.
        "});
    }

    #[test]
    fn symmetrical_accepts_open_with_first_close_with_last() {
        test::<MultilineMethodCallBraceLayout>().expect_no_offenses(indoc! {"
            foo(a,
              b)
        "});
    }

    #[test]
    fn symmetrical_accepts_open_own_line_close_own_line() {
        test::<MultilineMethodCallBraceLayout>().expect_no_offenses(indoc! {"
            foo(
              a,
              b
            )
        "});
    }

    #[test]
    fn accepts_single_line_call() {
        test::<MultilineMethodCallBraceLayout>().expect_no_offenses("foo(a, b)\n");
    }

    #[test]
    fn accepts_no_args() {
        test::<MultilineMethodCallBraceLayout>().expect_no_offenses("foo()\n");
    }

    #[test]
    fn accepts_paren_less_call() {
        test::<MultilineMethodCallBraceLayout>().expect_no_offenses(indoc! {"
            foo a,
              b
        "});
    }

    // RuboCop's `single_line_ignoring_receiver?`: the brace pair sits on one
    // line even though the receiver chain spans multiple lines.
    #[test]
    fn accepts_single_line_braces_with_multiline_receiver() {
        test::<MultilineMethodCallBraceLayout>().expect_no_offenses(indoc! {"
            foo
              .bar(a, b)
        "});
    }

    // new_line -------------------------------------------------------------

    #[test]
    fn new_line_flags_close_with_last() {
        test::<MultilineMethodCallBraceLayout>()
            .with_options(&new_line())
            .expect_offense(indoc! {"
                foo(a,
                  b)
                   ^ Closing method call brace must be on the line after the last argument.
            "});
    }

    #[test]
    fn new_line_accepts_close_on_new_line() {
        test::<MultilineMethodCallBraceLayout>()
            .with_options(&new_line())
            .expect_no_offenses(indoc! {"
                foo(a,
                  b
                )
            "});
    }

    // same_line ------------------------------------------------------------

    #[test]
    fn same_line_flags_close_on_new_line() {
        test::<MultilineMethodCallBraceLayout>()
            .with_options(&same_line())
            .expect_offense(indoc! {"
                foo(
                  a,
                  b
                )
                ^ Closing method call brace must be on the same line as the last argument.
            "});
    }

    #[test]
    fn same_line_accepts_close_with_last() {
        test::<MultilineMethodCallBraceLayout>()
            .with_options(&same_line())
            .expect_no_offenses(indoc! {"
                foo(
                  a,
                  b)
            "});
    }

    #[test]
    fn symmetrical_flags_method_call_with_receiver() {
        test::<MultilineMethodCallBraceLayout>().expect_offense(indoc! {"
            obj.foo(a,
              b
            )
            ^ Closing method call brace must be on the same line as the last argument when opening brace is on the same line as the first argument.
        "});
    }

    #[test]
    fn accepts_heredoc_last_argument() {
        test::<MultilineMethodCallBraceLayout>().expect_no_offenses(indoc! {"
            foo(a, <<~TEXT
              body
            TEXT
            )
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineMethodCallBraceLayout);
