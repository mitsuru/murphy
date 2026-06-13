//! `Layout/MultilineHashBraceLayout` — the closing brace of a multi-line hash
//! literal must be positioned consistently with the opening brace.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineHashBraceLayout
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `hash` literal nodes that span more than one line and whose
//!   closing `}` is mispositioned for the configured `EnforcedStyle`. Mirrors
//!   RuboCop's `MultilineLiteralBraceLayout` mixin with `children =
//!   node.children` (the pairs):
//!
//!   - symmetrical (default): if the opening `{` shares a line with the first
//!     element, the closing `}` must share a line with the last element;
//!     otherwise `}` must be on its own line below the last element.
//!   - new_line: the closing `}` must be on the line after the last element.
//!   - same_line: the closing `}` must be on the same line as the last
//!     element.
//!
//!   RuboCop's `ignored_literal?` skips implicit (brace-less) hashes
//!   (`implicit_literal?`, e.g. the `a: 1, b: 2` in `foo(a: 1, b: 2)`),
//!   empty hashes (`empty_literal?`), and single-line hashes
//!   (`node.single_line?`). Murphy reproduces all three: it requires a real
//!   `{` opener (the hash source must start with `{`), at least one element,
//!   and skips when the `{` and `}` share a physical line.
//!
//!   RuboCop's `last_line_heredoc?` guard skips a hash whose last element
//!   contains a heredoc. Murphy reproduces this by skipping when a
//!   `HeredocStart` token falls within the last element's range.
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop's corrector moves the
//!   closing brace; the detect-only port ships without it.
//! ```
//!
//! ## Matched shapes
//!
//! `hash` literal nodes with real `{...}` braces that span more than one line
//! and whose closing `}` violates the configured brace-layout style.

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop,
};

const SAME_LINE_MESSAGE: &str = "Closing hash brace must be on the same line as \
    the last hash element when opening brace is on the same line as the first \
    hash element.";
const NEW_LINE_MESSAGE: &str = "Closing hash brace must be on the line after \
    the last hash element when opening brace is on a separate line from the \
    first hash element.";
const ALWAYS_NEW_LINE_MESSAGE: &str =
    "Closing hash brace must be on the line after the last hash element.";
const ALWAYS_SAME_LINE_MESSAGE: &str =
    "Closing hash brace must be on the same line as the last hash element.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineHashBraceLayout;

/// Options for [`MultilineHashBraceLayout`]. The `EnforcedStyle` key matches
/// RuboCop verbatim; the default is `symmetrical`.
#[derive(CopOptions)]
pub struct MultilineHashBraceLayoutOptions {
    #[option(
        name = "EnforcedStyle",
        default = "symmetrical",
        description = "Where the closing `}` of a multi-line hash literal sits."
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
    name = "Layout/MultilineHashBraceLayout",
    description = "Enforce closing-brace placement in multi-line hash literals.",
    default_severity = "warning",
    default_enabled = true,
    options = MultilineHashBraceLayoutOptions,
)]
impl MultilineHashBraceLayout {
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Whether the source between two byte offsets contains a newline — an
/// O(span) same-line check that avoids scanning from the file start.
fn spans_newline(src: &[u8], start: u32, end: u32) -> bool {
    start < end && src[start as usize..end as usize].contains(&b'\n')
}

/// Whether the last element contains a heredoc opener (`HeredocStart` token)
/// anywhere within its source range — RuboCop's `last_line_heredoc?` analog.
fn last_element_has_heredoc(last_elem: NodeId, cx: &Cx<'_>) -> bool {
    let r = cx.range(last_elem);
    cx.tokens_in(r)
        .iter()
        .any(|t| t.kind == SourceTokenKind::HeredocStart)
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<MultilineHashBraceLayoutOptions>();
    let style = opts.enforced_style;

    let NodeKind::Hash(list) = cx.kind(node) else {
        return;
    };
    let pairs = cx.list(*list);
    // `empty_literal?` — no elements means no brace layout to enforce.
    if pairs.is_empty() {
        return;
    }

    // `implicit_literal?` — a brace-less hash (the trailing-kwargs `a: 1`) has
    // no `{`. Require the hash source to start with `{`; the matching `}` is
    // therefore the last byte of the node range.
    let hash_range = cx.range(node);
    if !cx.raw_source(hash_range).starts_with('{') {
        return;
    }
    let open = Range {
        start: hash_range.start,
        end: hash_range.start + 1,
    };
    let close = Range {
        start: hash_range.end - 1,
        end: hash_range.end,
    };

    let src = cx.source().as_bytes();

    let first_elem = pairs[0];
    let last_elem = pairs[pairs.len() - 1];

    // `last_line_heredoc?` — RuboCop recursively descends the last element
    // looking for a heredoc whose closer lands on the literal's last line
    // (which would make `last_line` unreliable). We approximate it by
    // skipping when a `HeredocStart` token falls within the last element's
    // source range (covers a heredoc nested as a pair's value).
    if last_element_has_heredoc(last_elem, cx) {
        return;
    }

    // `node.single_line?` — skip when the brace pair is on one physical line.
    if !spans_newline(src, open.start, close.start) {
        return;
    }

    // `opening_brace_on_same_line?` = `begin.line == children.first.first_line`:
    // no newline between `{` and the first element's start.
    let open_with_first = !spans_newline(src, open.start, cx.range(first_elem).start);
    // `closing_brace_on_same_line?` = `end.line == children.last.last_line`:
    // no newline between the last element's end and `}`.
    let close_with_last = !spans_newline(src, cx.range(last_elem).end, close.start);

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
        BraceLayoutStyle, MultilineHashBraceLayout, MultilineHashBraceLayoutOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    fn new_line() -> MultilineHashBraceLayoutOptions {
        MultilineHashBraceLayoutOptions {
            enforced_style: BraceLayoutStyle::NewLine,
        }
    }

    fn same_line() -> MultilineHashBraceLayoutOptions {
        MultilineHashBraceLayoutOptions {
            enforced_style: BraceLayoutStyle::SameLine,
        }
    }

    // symmetrical (default) -----------------------------------------------

    #[test]
    fn symmetrical_flags_open_with_first_close_on_new_line() {
        test::<MultilineHashBraceLayout>().expect_offense(indoc! {"
            x = {a: 1,
              b: 2
            }
            ^ Closing hash brace must be on the same line as the last hash element when opening brace is on the same line as the first hash element.
        "});
    }

    #[test]
    fn symmetrical_flags_open_on_own_line_close_with_last() {
        test::<MultilineHashBraceLayout>().expect_offense(indoc! {"
            x = {
              a: 1,
              b: 2}
                  ^ Closing hash brace must be on the line after the last hash element when opening brace is on a separate line from the first hash element.
        "});
    }

    #[test]
    fn symmetrical_accepts_open_with_first_close_with_last() {
        test::<MultilineHashBraceLayout>().expect_no_offenses(indoc! {"
            x = {a: 1,
              b: 2}
        "});
    }

    #[test]
    fn symmetrical_accepts_open_own_line_close_own_line() {
        test::<MultilineHashBraceLayout>().expect_no_offenses(indoc! {"
            x = {
              a: 1,
              b: 2
            }
        "});
    }

    #[test]
    fn accepts_single_line_hash() {
        test::<MultilineHashBraceLayout>().expect_no_offenses("x = {a: 1, b: 2}\n");
    }

    #[test]
    fn accepts_empty_hash() {
        test::<MultilineHashBraceLayout>().expect_no_offenses("x = {}\n");
    }

    // `implicit_literal?` — a brace-less trailing-kwargs hash has no braces.
    #[test]
    fn accepts_braceless_kwargs_hash() {
        test::<MultilineHashBraceLayout>().expect_no_offenses(indoc! {"
            foo(a: 1,
              b: 2)
        "});
    }

    // new_line -------------------------------------------------------------

    #[test]
    fn new_line_flags_close_with_last() {
        test::<MultilineHashBraceLayout>()
            .with_options(&new_line())
            .expect_offense(indoc! {"
                x = {a: 1,
                  b: 2}
                      ^ Closing hash brace must be on the line after the last hash element.
            "});
    }

    #[test]
    fn new_line_accepts_close_on_new_line() {
        test::<MultilineHashBraceLayout>()
            .with_options(&new_line())
            .expect_no_offenses(indoc! {"
                x = {a: 1,
                  b: 2
                }
            "});
    }

    // same_line ------------------------------------------------------------

    #[test]
    fn same_line_flags_close_on_new_line() {
        test::<MultilineHashBraceLayout>()
            .with_options(&same_line())
            .expect_offense(indoc! {"
                x = {
                  a: 1,
                  b: 2
                }
                ^ Closing hash brace must be on the same line as the last hash element.
            "});
    }

    #[test]
    fn same_line_accepts_close_with_last() {
        test::<MultilineHashBraceLayout>()
            .with_options(&same_line())
            .expect_no_offenses(indoc! {"
                x = {
                  a: 1,
                  b: 2}
            "});
    }

    #[test]
    fn flags_braced_hash_argument() {
        test::<MultilineHashBraceLayout>().expect_offense(indoc! {"
            foo({a: 1,
              b: 2
            })
            ^ Closing hash brace must be on the same line as the last hash element when opening brace is on the same line as the first hash element.
        "});
    }

    #[test]
    fn accepts_heredoc_last_element() {
        test::<MultilineHashBraceLayout>().expect_no_offenses(indoc! {"
            x = {a: 1, b: <<~TEXT
              body
            TEXT
            }
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineHashBraceLayout);
