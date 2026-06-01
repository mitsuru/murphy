//! `Style/TopLevelMethodDefinition` — warns against method definitions at the
//! top level.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TopLevelMethodDefinition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Disabled by default (matching RuboCop: "perfectly fine for Ruby scripts").
//!   Detects top-level `def`, `def self.x` (Defs), `define_method` Send, and
//!   `define_method` Block/Numblock/Itblock.
//!   Top-level detection: node is the root, or parent is Begin whose parent is root.
//!   Methods inside Class, Module, Block, Numblock, Itblock (e.g. Struct.new) are
//!   accepted — the parent check naturally excludes them without special-casing.
//!   No autocorrect (matches RuboCop upstream — no AutoCorrector included).
//!   Offense range: for all node types, the first-line range (from expression start
//!   to the end of the same source line, or expression end if single-line).
//! ```
//!
//! ## Matched shapes
//!
//! - `Def` nodes at the top level (including `def self.x` — receiver field is
//!   populated but node kind is still `Def` in Murphy)
//! - `Defs` nodes at the top level (singleton method definitions)
//! - `Send` nodes at the top level whose method is `define_method`
//! - `Block`/`Numblock`/`Itblock` nodes at the top level wrapping a
//!   `define_method` call
//!
//! ## Accepted
//!
//! - Any method definition inside a `Class`, `Module`, or block (`Block`,
//!   `Numblock`, `Itblock`) — the parent check excludes them automatically.
//!
//! ## Top-level detection
//!
//! A node is considered top-level when:
//! - It has no parent (it is the root node), OR
//! - Its parent is a `Begin` node that has no parent (the `Begin` is the root).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Do not define methods at the top-level.";

/// Stateless unit struct.
#[derive(Default)]
pub struct TopLevelMethodDefinition;

#[cop(
    name = "Style/TopLevelMethodDefinition",
    description = "Do not define methods at the top-level.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl TopLevelMethodDefinition {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        if is_top_level(node, cx) {
            cx.emit_offense(first_line_range(node, cx), MSG, None);
        }
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        if is_top_level(node, cx) {
            cx.emit_offense(first_line_range(node, cx), MSG, None);
        }
    }

    #[on_node(kind = "send", methods = ["define_method"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if is_top_level(node, cx) {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        if is_define_method_block(node, cx) && is_top_level(node, cx) {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        if is_define_method_numblock(node, cx) && is_top_level(node, cx) {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        if is_define_method_itblock(node, cx) && is_top_level(node, cx) {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }
}

/// Returns the range of the first line of `node` in the source.
///
/// For single-line nodes this is the full node range. For multi-line nodes
/// (e.g. `def foo\n  body\nend`), this is from `node.start` to the end of
/// the line on which the node starts (not including the `\n`).
fn first_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let start = node_range.start as usize;
    // Find the first newline at or after `start`.
    let end = source[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|p| (start + p) as u32)
        .unwrap_or(node_range.end);
    Range {
        start: node_range.start,
        end: end.min(node_range.end),
    }
}

/// Returns `true` if `node` is at the top level of the file.
///
/// Mirrors RuboCop's `top_level_method_definition?`:
/// - node itself is the root (no parent), OR
/// - node's parent is a `Begin`/`Kwbegin` chain that ultimately reaches the root.
///
/// Multiple levels of `begin..end` blocks at the top level are handled by
/// walking up through any number of `Begin`/`Kwbegin` nodes.
/// Example: `begin; begin; def foo; end; end; end` — `foo` is still top-level.
fn is_top_level(node: NodeId, cx: &Cx<'_>) -> bool {
    let mut current = node;
    loop {
        let parent_opt = cx.parent(current);
        match parent_opt.get() {
            None => {
                // `current` is the root — original node was top-level.
                return true;
            }
            Some(parent) => {
                if matches!(cx.kind(parent), NodeKind::Begin(..) | NodeKind::Kwbegin(..)) {
                    // Traverse up through Begin/Kwbegin wrappers.
                    current = parent;
                } else {
                    // Parent is something other than Begin/Kwbegin — not top-level.
                    return false;
                }
            }
        }
    }
}

/// Returns `true` if `node` is a `Block` whose call is a `define_method` Send.
fn is_define_method_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(node) else {
        return false;
    };
    is_define_method_send(call, cx)
}

/// Returns `true` if `node` is a `Numblock` whose call is a `define_method` Send.
fn is_define_method_numblock(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Numblock { send, .. } = *cx.kind(node) else {
        return false;
    };
    is_define_method_send(send, cx)
}

/// Returns `true` if `node` is an `Itblock` whose call is a `define_method` Send.
fn is_define_method_itblock(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Itblock { send, .. } = *cx.kind(node) else {
        return false;
    };
    is_define_method_send(send, cx)
}

/// Returns `true` if `node` is a `Send` with method `define_method`.
fn is_define_method_send(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { method, .. } = *cx.kind(node) else {
        return false;
    };
    cx.symbol_str(method) == "define_method"
}

#[cfg(test)]
mod tests {
    use super::TopLevelMethodDefinition;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- def cases -----

    #[test]
    fn flags_top_level_def() {
        // Offense range is the first line of the def.
        test::<TopLevelMethodDefinition>().expect_offense(indoc! {"
            def some_method
            ^^^^^^^^^^^^^^^ Do not define methods at the top-level.
            end
        "});
    }

    #[test]
    fn flags_top_level_def_self() {
        // `def self.foo` is a Def node with a receiver in Murphy's AST.
        test::<TopLevelMethodDefinition>().expect_offense(indoc! {"
            def self.some_method
            ^^^^^^^^^^^^^^^^^^^^ Do not define methods at the top-level.
            end
        "});
    }

    #[test]
    fn flags_multiple_top_level_defs() {
        // Both defs are top-level — each gets an offense on its first line.
        test::<TopLevelMethodDefinition>().expect_offense(indoc! {"
            def foo
            ^^^^^^^ Do not define methods at the top-level.
            end
            def bar; end
            ^^^^^^^^^^^^ Do not define methods at the top-level.
        "});
    }

    // ----- define_method -----

    #[test]
    fn flags_top_level_define_method_send() {
        test::<TopLevelMethodDefinition>().expect_offense(indoc! {"
            define_method(:foo)
            ^^^^^^^^^^^^^^^^^^^ Do not define methods at the top-level.
        "});
    }

    #[test]
    fn flags_top_level_define_method_block() {
        test::<TopLevelMethodDefinition>().expect_offense(indoc! {"
            define_method(:foo) { puts 1 }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not define methods at the top-level.
        "});
    }

    // ----- Good cases (inside class/module/block) -----

    #[test]
    fn accepts_def_inside_module() {
        test::<TopLevelMethodDefinition>().expect_no_offenses(indoc! {"
            module Foo
              def some_method
              end
            end
        "});
    }

    #[test]
    fn accepts_def_inside_class() {
        test::<TopLevelMethodDefinition>().expect_no_offenses(indoc! {"
            class Foo
              def some_method
              end
            end
        "});
    }

    #[test]
    fn accepts_singleton_def_inside_class() {
        test::<TopLevelMethodDefinition>().expect_no_offenses(indoc! {"
            class Foo
              def self.some_method
              end
            end
        "});
    }

    #[test]
    fn accepts_def_inside_struct_new() {
        test::<TopLevelMethodDefinition>().expect_no_offenses(indoc! {"
            Struct.new do
              def some_method
              end
            end
        "});
    }

    #[test]
    fn accepts_define_method_inside_class() {
        test::<TopLevelMethodDefinition>().expect_no_offenses(indoc! {"
            class Foo
              define_method(:foo) { puts 1 }
            end
        "});
    }

    #[test]
    fn flags_def_inside_top_level_begin_block() {
        // `begin; def foo; end; end` — still top-level (begin at root).
        test::<TopLevelMethodDefinition>().expect_offense(indoc! {"
            begin
              def foo
              ^^^^^^^ Do not define methods at the top-level.
              end
            end
        "});
    }

    #[test]
    fn flags_def_inside_nested_top_level_begin_blocks() {
        // `begin; begin; def foo; end; end; end` — nested begins at root.
        // Requires recursive parent traversal.
        test::<TopLevelMethodDefinition>().expect_offense(indoc! {"
            begin
              begin
                def foo
                ^^^^^^^ Do not define methods at the top-level.
                end
              end
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(TopLevelMethodDefinition);
