//! `Layout/EmptyLinesAfterModuleInclusion` — requires a blank line after a
//! module inclusion method (`include`, `extend`, `prepend`), or a group of
//! them.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLinesAfterModuleInclusion
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports `on_send` for `include`/`extend`/`prepend`. Skips calls with a
//!   receiver, no arguments, or a `send`/`block`/`array` parent (so
//!   `obj.include`, `include` as a block call, or `[include x]` are
//!   ignored). Skips when the next line is already blank or holds a
//!   `rubocop:enable`/`murphy:enable` directive comment. Grouped module
//!   inclusions (next statement is also a module inclusion) are allowed.
//!   When the following statement is in an `if` parent the next-line node
//!   is treated as absent (mirrors RuboCop's `next_line_node`). Autocorrect
//!   inserts a newline after the inclusion's whole-line range, or after a
//!   trailing enable-directive comment if present.
//! ```
//!
//! ## Algorithm
//!
//! `on_send` is restricted to the three inclusion methods. After the guard
//! clauses, the cop checks whether the line directly after the node is
//! already blank (or an enable directive). If not, and the next sibling
//! statement exists and is not itself a module inclusion, an offense is
//! raised and a blank line inserted.

use crate::cops::util::{line_is_blank, line_of, whole_line_range_with_newline};
use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, cop};

const MSG: &str = "Add an empty line after module inclusion.";

const MODULE_INCLUSION_METHODS: [&str; 3] = ["include", "extend", "prepend"];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLinesAfterModuleInclusion;

#[cop(
    name = "Layout/EmptyLinesAfterModuleInclusion",
    description = "Keeps track of empty lines after module inclusion methods.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyLinesAfterModuleInclusion {
    #[on_node(kind = "send", methods = ["include", "extend", "prepend"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `node.receiver || node.arguments.empty?`
        if cx.call_receiver(node).get().is_some() || cx.call_arguments(node).is_empty() {
            return;
        }
        // `node.parent&.type?(:send, :any_block, :array)`
        if let Some(parent) = cx.parent(node).get()
            && parent_is_skipped(parent, cx)
        {
            return;
        }

        let node_last_line = line_of(cx.range(node).end.saturating_sub(1), cx);
        if next_line_empty_or_enable_directive_comment(node_last_line, cx) {
            return;
        }

        let Some(next) = next_line_node(node, cx) else {
            return;
        };
        if !require_empty_line(next, cx) {
            return;
        }

        cx.emit_offense(cx.range(node), MSG, None);
        autocorrect(node, node_last_line, cx);
    }
}

/// `node.parent&.type?(:send, :any_block, :array)`
fn parent_is_skipped(parent: NodeId, cx: &Cx<'_>) -> bool {
    use murphy_plugin_api::NodeKind;
    matches!(cx.kind(parent), NodeKind::Send { .. } | NodeKind::Array(..))
        || cx.is_any_block_type(parent)
}

/// `next_line_empty_or_enable_directive_comment?` — the line directly after
/// the node is blank, or it holds an `enable` directive comment whose own
/// following line is blank.
///
/// RuboCop mixes 0-based array access (`processed_source[line]`) with 1-based
/// `comment_at_line`, so its `line_empty?(line)` and
/// `enable_directive_comment?(line + 1)` both reference the *same* physical
/// line — the one directly after the node. In our 0-based terms that line is
/// `last_line + 1`, and the directive's following line is `last_line + 2`.
fn next_line_empty_or_enable_directive_comment(last_line: u32, cx: &Cx<'_>) -> bool {
    let next = last_line + 1;
    line_is_blank(cx, next) || (enable_directive_comment(next, cx) && line_is_blank(cx, next + 1))
}

/// `enable_directive_comment?(line)` — a `rubocop:enable`/`murphy:enable`
/// directive comment occupies 0-based source `line`.
fn enable_directive_comment(line: u32, cx: &Cx<'_>) -> bool {
    comment_on_line(line, cx).is_some_and(is_enable_directive)
}

/// The source text of an own-line comment on 0-based `line`, if any.
fn comment_on_line<'a>(line: u32, cx: &Cx<'a>) -> Option<&'a str> {
    cx.comments().iter().find_map(|comment| {
        if line_of(comment.range.start, cx) == line {
            Some(cx.raw_source(comment.range))
        } else {
            None
        }
    })
}

/// `DirectiveComment#enabled?` — the comment is a `rubocop:enable` or
/// `murphy:enable` directive.
fn is_enable_directive(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('#') else {
        return false;
    };
    let rest = rest.trim_start();
    let Some(rest) = rest
        .strip_prefix("rubocop:")
        .or_else(|| rest.strip_prefix("murphy:"))
    else {
        return false;
    };
    rest.trim_start().starts_with("enable")
}

/// `next_line_node(node)` — `node.right_sibling`, but `nil` if the parent is
/// an `if`.
fn next_line_node(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    use murphy_plugin_api::NodeKind;
    if let Some(parent) = cx.parent(node).get()
        && matches!(cx.kind(parent), NodeKind::If { .. })
    {
        return None;
    }
    cx.right_sibling(node).get()
}

/// `require_empty_line?(node)` — true when a sibling exists and is not itself
/// a module inclusion method (grouped inclusions are allowed).
fn require_empty_line(node: NodeId, cx: &Cx<'_>) -> bool {
    !allowed_method(node, cx)
}

/// `allowed_method?(node)` — the next statement is itself a module inclusion
/// (`include`/`extend`/`prepend`), unwrapping a modifier-form send first.
fn allowed_method(node: NodeId, cx: &Cx<'_>) -> bool {
    // `node = node.body if modifier_form?` — for a modifier `include X if c`,
    // RuboCop inspects the modified body. The def_modifier helper does not
    // cover this; handle the common modifier-if/unless shape via the
    // condition node's underlying call.
    let target = modifier_body(node, cx).unwrap_or(node);
    // Unwrap any (arbitrarily nested) parentheses, e.g. `(include Bar) if c`,
    // whose body node is a `Begin`. `unwrap_parenthesized` loops internally.
    let target = crate::cops::util::unwrap_parenthesized(target, cx);
    if !matches!(*cx.kind(target), murphy_plugin_api::NodeKind::Send { .. }) {
        return false;
    }
    cx.method_name(target)
        .is_some_and(|m| MODULE_INCLUSION_METHODS.contains(&m))
}

/// For a modifier-form `if`/`unless` (`stmt if cond`), the modified body.
fn modifier_body(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    use murphy_plugin_api::NodeKind;
    let NodeKind::If { .. } = *cx.kind(node) else {
        return None;
    };
    // Modifier-form `if` has no `end` keyword.
    if cx.loc(node).end_keyword() != Range::ZERO {
        return None;
    }
    cx.children(node).into_iter().find(|&child| {
        let r = cx.range(child);
        // The body of a modifier-if starts at the node's start.
        r.start == cx.range(node).start
    })
}

/// Insert a blank line after the node's whole-line range, or after a trailing
/// enable-directive comment if one follows.
fn autocorrect(node: NodeId, node_last_line: u32, cx: &Cx<'_>) {
    let mut range = whole_line_range_with_newline(
        crate::cops::util::nth_line_start(cx, line_of(cx.range(node).start, cx)).unwrap_or(0),
        cx,
    );
    // Extend through to the node's last whole line (multi-line inclusion calls).
    let last_line_start = crate::cops::util::nth_line_start(cx, node_last_line).unwrap_or(range.end);
    range.end = whole_line_range_with_newline(last_line_start, cx).end;

    // If the directly-following line is an enable directive, insert after it.
    let next = node_last_line + 1;
    if enable_directive_comment(next, cx)
        && let Some(start) = crate::cops::util::nth_line_start(cx, next)
    {
        let directive_range = whole_line_range_with_newline(start, cx);
        cx.emit_edit(
            Range {
                start: directive_range.end,
                end: directive_range.end,
            },
            "\n",
        );
        return;
    }

    cx.emit_edit(
        Range {
            start: range.end,
            end: range.end,
        },
        "\n",
    );
}

#[cfg(test)]
mod tests {
    use super::EmptyLinesAfterModuleInclusion;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, test};

    #[test]
    fn flags_missing_empty_line_after_include() {
        let src = "class Foo\n  include Bar\n  attr_reader :baz\nend\n";
        let offenses = run_cop::<EmptyLinesAfterModuleInclusion>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(offenses[0].message, "Add an empty line after module inclusion.");
    }

    #[test]
    fn accepts_empty_line_after_include() {
        test::<EmptyLinesAfterModuleInclusion>()
            .expect_no_offenses("class Foo\n  include Bar\n\n  attr_reader :baz\nend\n");
    }

    #[test]
    fn accepts_grouped_module_inclusions() {
        test::<EmptyLinesAfterModuleInclusion>()
            .expect_no_offenses("class Foo\n  extend Bar\n  include Baz\n  prepend Qux\nend\n");
    }

    #[test]
    fn flags_last_of_grouped_inclusions() {
        // The group is allowed internally, but the final inclusion still needs
        // a blank line before the following non-inclusion statement.
        let src = "class Foo\n  extend Bar\n  include Baz\n  attr_reader :x\nend\n";
        let offenses = run_cop::<EmptyLinesAfterModuleInclusion>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
    }

    #[test]
    fn accepts_include_as_last_statement() {
        // No following statement → nothing to separate from.
        test::<EmptyLinesAfterModuleInclusion>()
            .expect_no_offenses("class Foo\n  attr_reader :x\n  include Bar\nend\n");
    }

    #[test]
    fn accepts_include_with_receiver() {
        test::<EmptyLinesAfterModuleInclusion>()
            .expect_no_offenses("class Foo\n  obj.include Bar\n  attr_reader :x\nend\n");
    }

    #[test]
    fn corrects_missing_empty_line() {
        let src = "class Foo\n  include Bar\n  attr_reader :baz\nend\n";
        let result = run_cop_with_edits::<EmptyLinesAfterModuleInclusion>(src);
        assert_eq!(result.offenses.len(), 1);
        let edit = &result.edits[0];
        assert_eq!(edit.replacement, "\n");
        // Inserted right after the `include Bar\n` line.
        let inserted_at = edit.range.start as usize;
        assert_eq!(&src[..inserted_at], "class Foo\n  include Bar\n");
    }

    #[test]
    fn accepts_enable_directive_after_include() {
        // `include Bar` followed by an enable directive on its own line is OK.
        test::<EmptyLinesAfterModuleInclusion>().expect_no_offenses(
            "class Foo\n  include Bar\n  # rubocop:enable Style/Foo\nend\n",
        );
    }

    /// Regression (Gemini PR #377): a parenthesized next inclusion is a
    /// `Begin` node; `allowed_method` must unwrap it so grouped inclusions
    /// stay exempt and no false-positive offense fires.
    #[test]
    fn accepts_grouped_with_parenthesized_next_inclusion() {
        test::<EmptyLinesAfterModuleInclusion>()
            .expect_no_offenses("class Foo\n  include Bar\n  (include Baz)\nend\n");
    }
}

murphy_plugin_api::submit_cop!(EmptyLinesAfterModuleInclusion);
