//! `Style/MissingRespondToMissing` — flags `method_missing` defined without `respond_to_missing?`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MissingRespondToMissing
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags any `def method_missing` (or `def self.method_missing` via `defs`)
//!   when the enclosing class/module body does not also define
//!   `respond_to_missing?` (or `self.respond_to_missing?`).
//!
//!   Ancestor walk: Murphy's AST omits the `begin` wrapper for single-method
//!   class bodies. Instead of the RuboCop `node.parent.parent` shortcut, we
//!   walk ancestors to find the first enclosing `class`, `module`, or `sclass`
//!   node and scan its direct scope for `respond_to_missing?`.
//!
//!   Scope boundary: `implements_respond_to_missing` does NOT recurse into
//!   nested `class`/`module`/`sclass` nodes, so a `respond_to_missing?`
//!   defined inside a nested class does not satisfy the outer class.
//!
//!   Top-level `def method_missing` (no enclosing class) — no offense,
//!   matching RuboCop's `return unless (grand_parent = node.parent.parent)`.
//!
//!   Uses `cx.method_name()` for DRY method-name extraction across `Def`/`Defs`.
//!
//!   Offense range: first line of the `def` signature (matching RuboCop's
//!   single-line highlight on the `add_offense(node)` call).
//!
//!   No autocorrect — same as upstream.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "When using `method_missing`, define `respond_to_missing?`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct MissingRespondToMissing;

#[cop(
    name = "Style/MissingRespondToMissing",
    description = "When using `method_missing`, define `respond_to_missing?`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MissingRespondToMissing {
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
    // Use the centralized helper to extract the method name for both Def and Defs.
    let Some(method_name) = cx.method_name(node) else {
        return;
    };

    if method_name != "method_missing" {
        return;
    }

    // Find the nearest enclosing class, module, or sclass scope.
    // In Murphy's AST, a single-method class body does NOT wrap the def in
    // `begin`, so we cannot simply go to `grandparent`. Instead, walk up
    // ancestors to find the class/module boundary.
    let Some(scope) = cx
        .ancestors(node)
        .find(|&anc| {
            matches!(
                cx.kind(anc),
                NodeKind::Class { .. } | NodeKind::Module { .. } | NodeKind::Sclass { .. }
            )
        })
    else {
        // No enclosing class/module — top-level def, no offense.
        return;
    };

    // Check the scope for `respond_to_missing?`, but do not cross into nested
    // class/module/sclass boundaries (which would cause false negatives).
    if implements_respond_to_missing(scope, cx) {
        return;
    }

    // Offense range: first line of the `def` signature only.
    let offense_range = first_line_range(node, cx);
    cx.emit_offense(offense_range, MSG, None);
}

/// Returns `true` if a `def`/`defs` node named `respond_to_missing?` exists
/// within `scope`, without crossing into nested class/module/sclass boundaries.
fn implements_respond_to_missing(scope: NodeId, cx: &Cx<'_>) -> bool {
    // Manual DFS with boundary pruning: do not enter nested class/module/sclass.
    let mut stack = cx.children(scope);
    stack.reverse();
    while let Some(node) = stack.pop() {
        // Stop descending into nested class/module/sclass scopes.
        if matches!(
            cx.kind(node),
            NodeKind::Class { .. } | NodeKind::Module { .. } | NodeKind::Sclass { .. }
        ) {
            continue;
        }
        // Check if this is a respond_to_missing? definition.
        if matches!(cx.kind(node), NodeKind::Def { .. } | NodeKind::Defs { .. })
            && cx.method_name(node) == Some("respond_to_missing?")
        {
            return true;
        }
        // Continue DFS into non-boundary children.
        let mut kids = cx.children(node);
        kids.reverse();
        stack.extend(kids);
    }
    false
}

/// Returns the range of the first source line of the node (up to the first newline).
fn first_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let node_start = node_range.start as usize;
    let first_line_end = source[node_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(node_range.end as usize, |pos| node_start + pos);
    Range {
        start: node_range.start,
        end: first_line_end as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::MissingRespondToMissing;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_method_missing_without_respond_to_missing() {
        test::<MissingRespondToMissing>().expect_offense(indoc! {"
            class Foo
              def method_missing(m, *args)
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ When using `method_missing`, define `respond_to_missing?`.
                super
              end
            end
        "});
    }

    #[test]
    fn accepts_method_missing_with_respond_to_missing() {
        test::<MissingRespondToMissing>().expect_no_offenses(indoc! {"
            class Foo
              def method_missing(m, *args)
                super
              end

              def respond_to_missing?(m, include_private = false)
                super
              end
            end
        "});
    }

    #[test]
    fn flags_singleton_method_missing_without_respond_to_missing() {
        test::<MissingRespondToMissing>().expect_offense(indoc! {"
            class Foo
              def self.method_missing(m, *args)
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ When using `method_missing`, define `respond_to_missing?`.
                super
              end
            end
        "});
    }

    #[test]
    fn accepts_singleton_method_missing_with_respond_to_missing() {
        test::<MissingRespondToMissing>().expect_no_offenses(indoc! {"
            class Foo
              def self.method_missing(m, *args)
                super
              end

              def self.respond_to_missing?(m, include_private = false)
                super
              end
            end
        "});
    }

    #[test]
    fn accepts_method_missing_at_top_level_no_class() {
        // No enclosing class/module scope — no offense.
        // Matches RuboCop's `return unless (grand_parent = node.parent.parent)`.
        test::<MissingRespondToMissing>().expect_no_offenses(indoc! {"
            def method_missing(m, *args)
              super
            end
        "});
    }

    #[test]
    fn flags_method_missing_in_module() {
        test::<MissingRespondToMissing>().expect_offense(indoc! {"
            module Foo
              def method_missing(m, *args)
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ When using `method_missing`, define `respond_to_missing?`.
                super
              end
            end
        "});
    }

    #[test]
    fn accepts_method_missing_in_module_with_respond_to_missing() {
        test::<MissingRespondToMissing>().expect_no_offenses(indoc! {"
            module Foo
              def method_missing(m, *args)
                super
              end

              def respond_to_missing?(m, include_private = false)
                super
              end
            end
        "});
    }

    #[test]
    fn flags_outer_class_when_nested_class_has_respond_to_missing() {
        // A respond_to_missing? inside a *nested* class must not satisfy the outer one.
        test::<MissingRespondToMissing>().expect_offense(indoc! {"
            class Outer
              def method_missing(m, *args)
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ When using `method_missing`, define `respond_to_missing?`.
                super
              end

              class Inner
                def respond_to_missing?(m, include_private = false)
                  super
                end
              end
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(MissingRespondToMissing);
