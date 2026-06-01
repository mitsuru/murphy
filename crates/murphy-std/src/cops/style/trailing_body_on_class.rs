//! `Style/TrailingBodyOnClass` — flags class definitions (including singleton
//! classes) that have code on the same line as the `class` keyword and
//! suggests moving it to its own line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TrailingBodyOnClass
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects class definitions (including singleton classes via `class << obj`)
//!   where the body appears on the same line as the `class` keyword.
//!   Autocorrects by inserting a line break with proper indentation after the
//!   class header. Gaps vs RuboCop: inline end-of-line comment repositioning
//!   (RuboCop's move_comment) and Layout/IndentationWidth config integration
//!   (hardcoded 2 spaces).
//! ```
//!
//! ## Matched shapes
//!
//! `Class` and `Sclass` nodes that:
//! - Have a non-nil body
//! - Are multiline (have an `end` keyword on a separate line)
//! - The first part of the body is on the same line as the `class` keyword
//!
//! ## Autocorrect
//!
//! Replaces the gap between the class header end and the start of the first
//! body statement with `\n` + indentation (keyword column + 2 spaces).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Place the first line of class body on its own line.";

#[derive(Default)]
pub struct TrailingBodyOnClass;

#[cop(
    name = "Style/TrailingBodyOnClass",
    description = "Place the first line of class body on its own line.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl TrailingBodyOnClass {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check_class_node(node, cx);
    }

    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        check_sclass_node(node, cx);
    }
}

fn check_class_node(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Class {
        name,
        superclass,
        body,
    } = *cx.kind(node)
    else {
        return;
    };

    // Must have a body.
    let Some(body_id) = body.get() else {
        return;
    };

    // Must be multiline.
    if !cx.is_multiline(node) {
        return;
    }

    // The header end is: superclass.end if present, else name.end.
    let header_end = if let Some(super_id) = superclass.get() {
        cx.range(super_id).end
    } else {
        cx.range(name).end
    };

    check_trailing(node, body_id, header_end, cx);
}

fn check_sclass_node(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Sclass { expr, body } = *cx.kind(node) else {
        return;
    };

    // Must have a body.
    let Some(body_id) = body.get() else {
        return;
    };

    // Must be multiline.
    if !cx.is_multiline(node) {
        return;
    }

    // For `class << expr`, the header ends after the expr.
    let header_end = cx.range(expr).end;

    check_trailing(node, body_id, header_end, cx);
}

fn check_trailing(node: NodeId, body_id: NodeId, header_end: u32, cx: &Cx<'_>) {
    // Find the "first part" of the body.
    let first_part_id = first_part_of(body_id, cx);
    let first_part_range = cx.range(first_part_id);

    // Check if the first part is on the same line as the class keyword.
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let has_newline =
        source[node_range.start as usize..first_part_range.start as usize].contains(&b'\n');
    if has_newline {
        return;
    }

    // Offense: the first part of the body.
    cx.emit_offense(first_part_range, MSG, None);

    // Autocorrect: replace the gap between header_end and first_part.start.
    let gap = Range {
        start: header_end,
        end: first_part_range.start,
    };

    // Compute line indentation: extract only leading whitespace (spaces/tabs)
    // from the start of the line up to the `class` keyword. Using only
    // whitespace (not the full byte distance) avoids misindenting when the
    // class keyword is preceded by non-whitespace (e.g. `private class`).
    let node_start = node_range.start as usize;
    let line_start = source[..node_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    let leading_ws_len = source[line_start..node_start]
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .count();
    let leading_ws =
        std::str::from_utf8(&source[line_start..line_start + leading_ws_len]).unwrap_or("");
    let indent = format!("{leading_ws}  ");
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
    use super::TrailingBodyOnClass;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- class offense + autocorrect cases ---

    #[test]
    fn flags_simple_trailing_body_on_class() {
        test::<TrailingBodyOnClass>().expect_correction(
            indoc! {"
                class Foo; def bar; end
                           ^^^^^^^^^^^^ Place the first line of class body on its own line.
                end
            "},
            "class Foo\n  def bar; end\nend\n",
        );
    }

    #[test]
    fn flags_trailing_body_with_superclass() {
        test::<TrailingBodyOnClass>().expect_correction(
            indoc! {"
                class Foo < Bar; def bar; end
                                 ^^^^^^^^^^^^ Place the first line of class body on its own line.
                end
            "},
            "class Foo < Bar\n  def bar; end\nend\n",
        );
    }

    #[test]
    fn flags_trailing_begin_body_on_class() {
        test::<TrailingBodyOnClass>().expect_correction(
            indoc! {"
                class Foo; def bar; end; def baz; end
                           ^^^^^^^^^^^^ Place the first line of class body on its own line.
                end
            "},
            "class Foo\n  def bar; end; def baz; end\nend\n",
        );
    }

    // --- sclass offense + autocorrect cases ---

    #[test]
    fn flags_trailing_body_on_sclass() {
        test::<TrailingBodyOnClass>().expect_correction(
            indoc! {"
                class << self; def foo; end
                               ^^^^^^^^^^^^ Place the first line of class body on its own line.
                end
            "},
            "class << self\n  def foo; end\nend\n",
        );
    }

    // --- negative cases ---

    #[test]
    fn accepts_body_on_next_line() {
        test::<TrailingBodyOnClass>().expect_no_offenses(indoc! {"
            class Foo
              def bar; end
            end
        "});
    }

    #[test]
    fn accepts_empty_class() {
        test::<TrailingBodyOnClass>().expect_no_offenses(indoc! {"
            class Foo
            end
        "});
    }

    #[test]
    fn accepts_single_line_class() {
        // Single-line classes are not flagged (not multiline).
        test::<TrailingBodyOnClass>().expect_no_offenses("class Foo; end\n");
    }

    #[test]
    fn accepts_sclass_body_on_next_line() {
        test::<TrailingBodyOnClass>().expect_no_offenses(indoc! {"
            class << self
              def foo; end
            end
        "});
    }

    #[test]
    fn accepts_empty_sclass() {
        test::<TrailingBodyOnClass>().expect_no_offenses(indoc! {"
            class << self
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(TrailingBodyOnClass);
