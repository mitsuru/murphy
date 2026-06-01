//! `Style/TrailingBodyOnMethodDefinition` — flags method body code that
//! appears on the same line as the method definition keyword.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TrailingBodyOnMethodDefinition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `def foo; body` and `def self.foo; body` where the body starts
//!   on the same line as the `def` keyword, but the method definition is
//!   multiline (has an `end`). Endless methods (`def foo = expr`) are skipped
//!   (no `end` keyword).
//!   Autocorrect: inserts a newline + 2-space indent before the body, and
//!   removes the `;` separator if present. The indentation is fixed at 2
//!   spaces (configured_indentation_width default). Full alignment-aware
//!   indentation based on nesting depth is not implemented (gap).
//! ```
//!
//! ## Matched shapes
//!
//! `def` and `def self.foo` nodes that:
//! - Are multiline (have an `end` keyword — not endless)
//! - Have a non-empty body
//! - Have the body starting on the same line as the `def` keyword
//!
//! ## Autocorrect
//!
//! Replaces the `;` and any surrounding whitespace between the closing `)` (or
//! method name) and the body's first expression with a newline and 2-space
//! indent.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Place the first line of a multi-line method definition's body on its own line.";

/// Stateless unit struct.
#[derive(Default)]
pub struct TrailingBodyOnMethodDefinition;

#[cop(
    name = "Style/TrailingBodyOnMethodDefinition",
    description = "Method body goes below definition.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl TrailingBodyOnMethodDefinition {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Skip endless methods (no `end` keyword).
    if cx.loc(node).end_keyword() == Range::ZERO {
        return;
    }

    // Skip methods with no body.
    let Some(body) = cx.def_body(node).get() else {
        return;
    };

    // The body must start on the same line as `def`.
    let node_start = cx.range(node).start;
    let first = first_part(body, cx);
    let body_start = cx.range(first).start;

    // Count newlines between node start and body start.
    // If there are any, the body is on a different line.
    let src = cx.source();
    if count_newlines_in(src, node_start, body_start) > 0 {
        return;
    }

    // Offense range is the first part of the body.
    let offense_range = cx.range(first);

    cx.emit_offense(offense_range, MSG, None);

    // Autocorrect: scan from name end forward for `)` or `;` before body_start.
    let search_start = cx.loc(node).name.end;
    let cut_point = find_cut_point(cx, search_start, body_start);

    let separator_range = Range {
        start: cut_point,
        end: body_start,
    };
    cx.emit_edit(separator_range, "\n  ");
}

/// Returns the "first part" of a body node.
/// If the body is a `begin` (multi-statement), this is the first child.
/// Otherwise it's the body itself.
fn first_part(body: NodeId, cx: &Cx<'_>) -> NodeId {
    if let NodeKind::Begin(list) = cx.kind(body) {
        let children = cx.list(*list);
        if let Some(&first_child) = children.first() {
            return first_child;
        }
    }
    body
}

/// Count newlines in source bytes between `from` and `to`.
fn count_newlines_in(src: &str, from: u32, to: u32) -> usize {
    if from >= to {
        return 0;
    }
    let bytes = src.as_bytes();
    let start = from as usize;
    let end = (to as usize).min(bytes.len());
    bytes[start..end].iter().filter(|&&b| b == b'\n').count()
}

/// Find the start of the separator gap between signature and body.
///
/// Returns the byte offset where the `separator_range` should start:
/// - If a `;` is found, returns its start (so the `;` and trailing space
///   are both replaced).
/// - If a `)` is found (args with parens, no `;`), returns its end (so
///   only the space after `)` is replaced).
/// - Otherwise returns `from`.
fn find_cut_point(cx: &Cx<'_>, from: u32, to: u32) -> u32 {
    let mut after_paren = from;
    let mut semi_start: Option<u32> = None;
    let src = cx.source().as_bytes();

    for tok in cx.tokens_in(Range {
        start: from,
        end: to,
    }) {
        match tok.kind {
            SourceTokenKind::RightParen => {
                after_paren = tok.range.end;
            }
            SourceTokenKind::Other => {
                let bytes = &src[tok.range.start as usize..tok.range.end as usize];
                if bytes == b";" {
                    semi_start = Some(tok.range.start);
                    break;
                }
            }
            _ => {}
        }
    }
    // If `;` found, start range at `;` (delete `;` + trailing space).
    // Otherwise start range after `)` or at name end (delete space only).
    semi_start.unwrap_or(after_paren)
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- no offense ---

    #[test]
    fn no_offense_normal_method() {
        test::<TrailingBodyOnMethodDefinition>().expect_no_offenses(indoc! {"
            def some_method
              do_stuff
            end
        "});
    }

    #[test]
    fn no_offense_endless_method() {
        test::<TrailingBodyOnMethodDefinition>().expect_no_offenses("def foo = bar\n");
    }

    #[test]
    fn no_offense_empty_body() {
        test::<TrailingBodyOnMethodDefinition>().expect_no_offenses(indoc! {"
            def foo
            end
        "});
    }

    #[test]
    fn no_offense_body_on_next_line() {
        test::<TrailingBodyOnMethodDefinition>().expect_no_offenses(indoc! {"
            def foo(x)
              bar
            end
        "});
    }

    // --- offense ---

    #[test]
    fn flags_trailing_body_simple() {
        test::<TrailingBodyOnMethodDefinition>().expect_offense(indoc! {"
            def some_method; do_stuff
                             ^^^^^^^^ Place the first line of a multi-line method definition's body on its own line.
            end
        "});
    }

    #[test]
    fn flags_trailing_body_with_args() {
        test::<TrailingBodyOnMethodDefinition>().expect_offense(indoc! {"
            def f(x); b = foo
                      ^^^^^^^ Place the first line of a multi-line method definition's body on its own line.
              b
            end
        "});
    }

    #[test]
    fn flags_trailing_body_singleton_method() {
        test::<TrailingBodyOnMethodDefinition>().expect_offense(indoc! {"
            def self.foo; bar
                          ^^^ Place the first line of a multi-line method definition's body on its own line.
            end
        "});
    }

    // --- autocorrect ---

    #[test]
    fn corrects_trailing_body_simple() {
        test::<TrailingBodyOnMethodDefinition>().expect_correction(
            indoc! {"
                def some_method; do_stuff
                                 ^^^^^^^^ Place the first line of a multi-line method definition's body on its own line.
                end
            "},
            "def some_method\n  do_stuff\nend\n",
        );
    }

    #[test]
    fn corrects_trailing_body_with_args() {
        test::<TrailingBodyOnMethodDefinition>().expect_correction(
            indoc! {"
                def f(x); b = foo
                          ^^^^^^^ Place the first line of a multi-line method definition's body on its own line.
                  b
                end
            "},
            "def f(x)\n  b = foo\n  b\nend\n",
        );
    }

    #[test]
    fn corrects_trailing_body_singleton_method() {
        test::<TrailingBodyOnMethodDefinition>().expect_correction(
            indoc! {"
                def self.foo; bar
                              ^^^ Place the first line of a multi-line method definition's body on its own line.
                end
            "},
            "def self.foo\n  bar\nend\n",
        );
    }
}

murphy_plugin_api::submit_cop!(TrailingBodyOnMethodDefinition);
