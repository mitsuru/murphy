//! `Style/RedundantConstantBase` — flags unnecessary `::` prefix on constants
//! where `Module.nesting` is empty (i.e. top-level scope).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantConstantBase
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Core detection implemented: flags `::Const` (scope=None, raw source starts
//!   with `::`) when not nested in a `class`/`module` body. `class << self`
//!   (sclass) is not considered nesting, matching RuboCop.
//!   The super-class exception (class Foo < ::Base) is implemented: `::Base` in
//!   the superclass position is flagged even though it is inside a class node.
//!   Murphy's AST folds `::Const` and bare `Const` to the same
//!   `Const { scope: None }` shape; the `::` is detected via raw source.
//!   The `Lint/ConstantResolution`-enabled check is not implemented (no cross-cop
//!   config access in Murphy v1).
//! ```
//!
//! ## Matched shapes
//!
//! `Const { scope: None }` where `cx.raw_source(cx.range(node)).starts_with("::")`
//! AND the node is not in the body of a `class` or `module` ancestor
//! (but IS flagged in the superclass position of a `class`).
//!
//! ## Autocorrect
//!
//! Delete the leading `::` (2 bytes) — surgical `emit_edit` on the `::` range.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Remove redundant `::`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantConstantBase;

#[cop(
    name = "Style/RedundantConstantBase",
    description = "Avoid redundant `::` prefix on constant.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantConstantBase {
    #[on_node(kind = "const")]
    fn check_const(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns `true` if `needle` is in the body of `class_or_module` ancestor.
/// For a `Class` ancestor, the superclass position is NOT considered "body".
fn is_in_nesting_body(needle: NodeId, ancestor: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(ancestor) {
        NodeKind::Class {
            name: _,
            superclass,
            body,
        } => {
            let Some(body_id) = body.get() else {
                return false;
            };
            // needle must be in body, not in superclass.
            // Check superclass first: if needle is in superclass subtree, NOT in body.
            if let Some(super_id) = superclass.get() {
                if needle == super_id
                    || cx
                        .ancestors(needle)
                        .take_while(|&a| a != ancestor)
                        .any(|a| a == super_id)
                {
                    return false;
                }
            }
            // Check if needle is in body.
            needle == body_id
                || cx
                    .ancestors(needle)
                    .take_while(|&a| a != ancestor)
                    .any(|a| a == body_id)
        }
        NodeKind::Module { name: _, body } => {
            let Some(body_id) = body.get() else {
                return false;
            };
            needle == body_id
                || cx
                    .ancestors(needle)
                    .take_while(|&a| a != ancestor)
                    .any(|a| a == body_id)
        }
        _ => false,
    }
}

/// Returns `true` if `node` is nested inside a `class` or `module` body
/// (not the class name or superclass position).
fn has_protecting_nesting(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(node).any(|ancestor| {
        matches!(
            cx.kind(ancestor),
            NodeKind::Class { .. } | NodeKind::Module { .. }
        ) && is_in_nesting_body(node, ancestor, cx)
    })
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Const { scope, .. } = *cx.kind(node) else {
        return;
    };

    // Only flag top-level `::Const` (scope = None) where the source starts with `::`.
    if !scope.is_none() {
        return;
    }

    let src = cx.raw_source(cx.range(node));
    if !src.starts_with("::") {
        return;
    }

    // Skip if nested in a class/module body (they may need the :: for clarity).
    if has_protecting_nesting(node, cx) {
        return;
    }

    // Offense range: the whole `::Const` node.
    let node_range = cx.range(node);
    cx.emit_offense(node_range, MSG, None);

    // Autocorrect: delete the leading `::` (2 bytes).
    let colon_range = Range {
        start: node_range.start,
        end: node_range.start + 2,
    };
    cx.emit_edit(colon_range, "");
}

#[cfg(test)]
mod tests {
    use super::RedundantConstantBase;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- offense cases -----

    #[test]
    fn flags_toplevel_constant() {
        test::<RedundantConstantBase>().expect_correction(
            indoc! {"
                ::Const
                ^^^^^^^ Remove redundant `::`.
            "},
            "Const\n",
        );
    }

    #[test]
    fn flags_toplevel_constant_in_sclass() {
        test::<RedundantConstantBase>().expect_correction(
            indoc! {"
                class << self
                  ::Const
                  ^^^^^^^ Remove redundant `::`.
                end
            "},
            indoc! {"
                class << self
                  Const
                end
            "},
        );
    }

    #[test]
    fn flags_constant_in_superclass_position() {
        test::<RedundantConstantBase>().expect_offense(indoc! {"
            class Foo < ::Base
                        ^^^^^^ Remove redundant `::`.
            end
        "});
    }

    // ----- no-offense cases -----

    #[test]
    fn accepts_bare_constant() {
        test::<RedundantConstantBase>().expect_no_offenses("Const\n");
    }

    #[test]
    fn accepts_constant_in_class_body() {
        test::<RedundantConstantBase>().expect_no_offenses(indoc! {"
            class A
              ::Const
            end
        "});
    }

    #[test]
    fn accepts_constant_in_module_body() {
        test::<RedundantConstantBase>().expect_no_offenses(indoc! {"
            module A
              ::Const
            end
        "});
    }

    #[test]
    fn accepts_nested_constant_path() {
        // `A::B` has scope=Some(A), not None — not matched.
        test::<RedundantConstantBase>().expect_no_offenses("A::B\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantConstantBase);
