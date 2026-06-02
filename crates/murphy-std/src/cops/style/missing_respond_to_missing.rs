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
//!   RuboCop's `add_offense(node)` highlights the full node expression. In
//!   Murphy, `cx.range(node)` for a `def` covers the entire definition
//!   including body and `end`. To match RuboCop's highlight (which uses
//!   `node.source_range` trimmed to the first source line), we limit the
//!   offense to the first line of the `def` signature.
//!
//!   Ancestor walk: Murphy's AST omits the `begin` wrapper for single-method
//!   class bodies. Instead of the RuboCop `node.parent.parent` shortcut, we
//!   walk ancestors to find the first enclosing `class`, `module`, or `sclass`
//!   node and scan its descendants for `respond_to_missing?`.
//!
//!   Top-level `def method_missing` (no enclosing class) — no offense,
//!   matching RuboCop's `return unless (grand_parent = node.parent.parent)`.
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
    // Extract the method name.
    let method_name = match *cx.kind(node) {
        NodeKind::Def { name, .. } => name,
        NodeKind::Defs { name, .. } => name,
        _ => return,
    };

    if cx.symbol_str(method_name) != "method_missing" {
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

    // Check all descendant `def`/`defs` in the class scope.
    // If any is named `respond_to_missing?`, we are satisfied.
    if implements_respond_to_missing(scope, cx) {
        return;
    }

    // Offense range: first line of the `def` signature only.
    let offense_range = first_line_range(node, cx);
    cx.emit_offense(offense_range, MSG, None);
}

/// Returns `true` if any descendant `def` or `defs` node in `scope` defines
/// `respond_to_missing?`.
fn implements_respond_to_missing(scope: NodeId, cx: &Cx<'_>) -> bool {
    for desc in cx.descendants(scope) {
        match *cx.kind(desc) {
            NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => {
                if cx.symbol_str(name) == "respond_to_missing?" {
                    return true;
                }
            }
            _ => {}
        }
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
}

murphy_plugin_api::submit_cop!(MissingRespondToMissing);
