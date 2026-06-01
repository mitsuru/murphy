//! `Style/TrailingBodyOnModule` — flags module definitions that have code on
//! the same line as the `module` keyword and suggests moving it to its own
//! line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TrailingBodyOnModule
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects module definitions where the body appears on the same line as the
//!   `module` keyword. Autocorrects by inserting a line break with proper
//!   indentation after the module name. Gaps vs RuboCop: inline end-of-line
//!   comment repositioning (RuboCop's move_comment) and Layout/IndentationWidth
//!   config integration (hardcoded 2 spaces).
//! ```
//!
//! ## Matched shapes
//!
//! `Module` nodes that:
//! - Have a non-nil body
//! - Are multiline (have an `end` keyword on a separate line)
//! - The first part of the body is on the same line as the `module` keyword
//!
//! ## Autocorrect
//!
//! Replaces the gap between the module name and the start of the first body
//! statement with `\n` + indentation (keyword column + 2 spaces).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Place the first line of module body on its own line.";

#[derive(Default)]
pub struct TrailingBodyOnModule;

#[cop(
    name = "Style/TrailingBodyOnModule",
    description = "Place the first line of module body on its own line.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl TrailingBodyOnModule {
    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Module { name, body } = *cx.kind(node) else {
        return;
    };

    // Must have a body.
    let Some(body_id) = body.get() else {
        return;
    };

    // Must be multiline — single-line modules like `module Foo; end` are skipped.
    if !cx.is_multiline(node) {
        return;
    }

    // Find the "first part" of the body: for a Begin node, it's the first child;
    // otherwise the body itself.
    let first_part_id = first_part_of(body_id, cx);
    let first_part_range = cx.range(first_part_id);

    // Check if the first part is on the same line as the module keyword.
    // Strategy: scan for '\n' in source between node.start and first_part.start.
    // If there's no newline, the body is on the keyword line.
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let has_newline =
        source[node_range.start as usize..first_part_range.start as usize].contains(&b'\n');
    if has_newline {
        return;
    }

    // Offense: the first part of the body.
    cx.emit_offense(first_part_range, MSG, None);

    // Autocorrect: replace the gap between name.end and first_part.start with
    // a newline + indentation (keyword column + 2 spaces).
    let header_end = cx.range(name).end;
    let gap = Range {
        start: header_end,
        end: first_part_range.start,
    };

    // Compute keyword column: bytes from last '\n' before node.start to node.start.
    let node_start = node_range.start as usize;
    let line_start = source[..node_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    let keyword_col = node_start - line_start;
    let indent = " ".repeat(keyword_col + 2);
    let replacement = format!("\n{indent}");
    cx.emit_edit(gap, &replacement);
}

/// Returns the first meaningful child of a body: for a `Begin` node, the
/// first child; for any other node, the body itself.
fn first_part_of(body_id: NodeId, cx: &Cx<'_>) -> NodeId {
    if let NodeKind::Begin(list) = *cx.kind(body_id) {
        let children = cx.list(list);
        if let Some(&first) = children.first() {
            return first;
        }
    }
    body_id
}

#[cfg(test)]
mod tests {
    use super::TrailingBodyOnModule;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offense + autocorrect cases ---

    #[test]
    fn flags_simple_trailing_body() {
        test::<TrailingBodyOnModule>().expect_correction(
            indoc! {"
                module Foo extend self
                           ^^^^^^^^^^^ Place the first line of module body on its own line.
                end
            "},
            "module Foo\n  extend self\nend\n",
        );
    }

    #[test]
    fn flags_trailing_body_with_semicolon() {
        test::<TrailingBodyOnModule>().expect_correction(
            indoc! {"
                module Foo; extend self
                            ^^^^^^^^^^^ Place the first line of module body on its own line.
                end
            "},
            "module Foo\n  extend self\nend\n",
        );
    }

    #[test]
    fn flags_first_part_of_begin_body() {
        test::<TrailingBodyOnModule>().expect_correction(
            indoc! {"
                module Foo; extend self; include Bar
                            ^^^^^^^^^^^ Place the first line of module body on its own line.
                end
            "},
            "module Foo\n  extend self; include Bar\nend\n",
        );
    }

    #[test]
    fn flags_module_def_in_body() {
        test::<TrailingBodyOnModule>().expect_correction(
            indoc! {"
                module Foo; def bar; end
                            ^^^^^^^^^^^^ Place the first line of module body on its own line.
                end
            "},
            "module Foo\n  def bar; end\nend\n",
        );
    }

    // --- negative cases ---

    #[test]
    fn accepts_body_on_next_line() {
        test::<TrailingBodyOnModule>().expect_no_offenses(indoc! {"
            module Foo
              extend self
            end
        "});
    }

    #[test]
    fn accepts_empty_module() {
        test::<TrailingBodyOnModule>().expect_no_offenses(indoc! {"
            module Foo
            end
        "});
    }

    #[test]
    fn accepts_single_line_module() {
        // Single-line modules are not flagged (not multiline).
        test::<TrailingBodyOnModule>().expect_no_offenses("module Foo; end\n");
    }
}

murphy_plugin_api::submit_cop!(TrailingBodyOnModule);
