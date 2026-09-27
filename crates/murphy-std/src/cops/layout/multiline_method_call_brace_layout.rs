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
//!   RuboCop's `last_line_heredoc?` guard skips a call when a heredoc
//!   terminator (`HeredocEnd`) in the last argument shares the closing
//!   parenthesis's line.
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

/// Whether the source between two byte offsets contains a newline — an
/// O(span) same-line check that avoids scanning from the file start.
fn spans_newline(src: &[u8], start: u32, end: u32) -> bool {
    start < end && src[start as usize..end as usize].contains(&b'\n')
}

/// Whether two source offsets are on the same physical line.
fn same_line(src: &[u8], a: u32, b: u32) -> bool {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    !src[lo as usize..hi as usize].contains(&b'\n')
}

/// Whether a heredoc terminator in the last argument shares the closing
/// delimiter's line, matching RuboCop's `last_line_heredoc?` guard.
fn last_line_heredoc(last_arg: NodeId, close_start: u32, cx: &Cx<'_>) -> bool {
    let src = cx.source_bytes();
    let r = cx.range(last_arg);
    cx.tokens_in(r)
        .iter()
        .filter(|tok| tok.kind == SourceTokenKind::HeredocEnd)
        .any(|tok| same_line(src, tok.range.start, close_start))
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    // `empty_literal?` — no arguments means no brace layout to enforce.
    if args.is_empty() {
        return;
    }

    // `implicit_literal?` — a call without an explicit `(` (e.g. `foo a, b`)
    // has no brace pair. `begin()`/`end()` resolve only the argument-list
    // parens, so a paren-less call yields `Range::ZERO`.
    let open = cx.loc(node).begin();
    if open == Range::ZERO {
        return;
    }
    let close = cx.loc(node).end();
    if close == Range::ZERO {
        return;
    }

    let src = cx.source_bytes();
    // `single_line_ignoring_receiver?` / `single_line?` — compare only the
    // brace tokens, not the receiver (which may span multiple lines). Most
    // calls stop here without inspecting heredoc tokens or decoding options.
    if !spans_newline(src, open.start, close.start) {
        return;
    }

    let first_arg = args[0];
    let last_arg = args[args.len() - 1];

    // `last_line_heredoc?` — skip only when a heredoc terminator nested in the
    // last argument shares the closing parenthesis's line.
    if last_line_heredoc(last_arg, close.start, cx) {
        return;
    }

    let style = cx
        .options_or_default::<MultilineMethodCallBraceLayoutOptions>()
        .enforced_style;

    // `opening_brace_on_same_line?` = `begin.line == children.first.first_line`:
    // no newline between `(` and the first argument's start.
    let open_with_first = !spans_newline(src, open.start, cx.range(first_arg).start);
    // `closing_brace_on_same_line?` = `end.line == children.last.last_line`:
    // no newline between the last argument's end and `)`.
    let close_with_last = !spans_newline(src, cx.range(last_arg).end, close.start);

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

    #[test]
    fn custom_styles_skip_single_line_calls_after_utf8_crlf_prefix() {
        for ending in ["\n", "\r\n"] {
            let prefix = format!("# コメント{ending}").repeat(800);
            let single = format!("{prefix}outer(inner(a)){ending}obj{ending}  .bar(a, b){ending}");
            test::<MultilineMethodCallBraceLayout>()
                .with_options(&new_line())
                .expect_no_offenses(&single);
            let multiline = format!("{prefix}foo({ending}  a,{ending}  b{ending}){ending}");
            test::<MultilineMethodCallBraceLayout>()
                .with_options(&new_line())
                .expect_no_offenses(&multiline);
            test::<MultilineMethodCallBraceLayout>()
                .with_options(&same_line())
                .expect_no_offenses(&format!("{prefix}foo({ending}  a,{ending}  b){ending}"));
        }
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
    fn flags_heredoc_last_argument_when_terminator_precedes_close() {
        test::<MultilineMethodCallBraceLayout>().expect_offense(indoc! {"
            foo(a, <<~TEXT
              body
            TEXT
            )
            ^ Closing method call brace must be on the same line as the last argument when opening brace is on the same line as the first argument.
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineMethodCallBraceLayout);
