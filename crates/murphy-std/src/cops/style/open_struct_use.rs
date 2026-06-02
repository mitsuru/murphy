//! `Style/OpenStructUse` — flags uses of `OpenStruct`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/OpenStructUse
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags any reference to the top-level `OpenStruct` constant (with or
//!   without `::` cbase prefix — Murphy renders `::OpenStruct` as
//!   `Const { scope: None, name: :OpenStruct }`, identical to bare
//!   `OpenStruct`).
//!
//!   Guard: skip when the `OpenStruct` constant appears as the **name** of
//!   a `class` or `module` definition (sibling_index == 0 in a
//!   `class`/`module` node). This mirrors RuboCop's
//!   `custom_class_or_module_definition?` check (`node.parent.type?(:class,
//!   :module) && node.left_siblings.empty?`). Subclassing and uses in other
//!   positions are still flagged:
//!   - `class Foo < OpenStruct` flagged (OpenStruct is superclass, slot 1)
//!   - `class OpenStruct` not flagged (defining, slot 0)
//!   - `SomeNamespace::OpenStruct` not flagged (scope is non-nil)
//!
//!   No autocorrect — same as upstream.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str =
    "Avoid using `OpenStruct`; use `Struct`, `Hash`, a class or test doubles instead.";

#[derive(Default)]
pub struct OpenStructUse;

#[cop(
    name = "Style/OpenStructUse",
    description = "Avoid using `OpenStruct`; use `Struct`, `Hash`, a class or test doubles instead.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl OpenStructUse {
    #[on_node(kind = "const")]
    fn check_const(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return;
    };

    // Only top-level `OpenStruct` (scope == None covers both bare and `::` cbase).
    if scope.get().is_some() {
        return;
    }
    if cx.symbol_str(name) != "OpenStruct" {
        return;
    }

    // Guard: skip when this is the name node of a class/module definition
    // (i.e., `class OpenStruct ...` or `module OpenStruct ...`).
    // In those cases the const is the first child (sibling_index == 0) of a
    // Class or Module node. Subclasses (`class Foo < OpenStruct`) are at slot 1
    // and must still be flagged.
    if let Some(parent) = cx.parent(node).get() {
        let is_definition_name = matches!(
            cx.kind(parent),
            NodeKind::Class { .. } | NodeKind::Module { .. }
        ) && cx.sibling_index(node) == Some(0);

        if is_definition_name {
            return;
        }
    }

    cx.emit_offense(cx.range(node), MSG, None);
}

#[cfg(test)]
mod tests {
    use super::OpenStructUse;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_open_struct_new() {
        test::<OpenStructUse>().expect_offense(indoc! {"
            a = OpenStruct.new(a: 42)
                ^^^^^^^^^^ Avoid using `OpenStruct`; use `Struct`, `Hash`, a class or test doubles instead.
        "});
    }

    #[test]
    fn flags_cbase_open_struct_new() {
        test::<OpenStructUse>().expect_offense(indoc! {"
            a = ::OpenStruct.new(a: 42)
                ^^^^^^^^^^^^ Avoid using `OpenStruct`; use `Struct`, `Hash`, a class or test doubles instead.
        "});
    }

    #[test]
    fn flags_subclass_open_struct() {
        test::<OpenStructUse>().expect_offense(indoc! {"
            class SubClass < OpenStruct
                             ^^^^^^^^^^ Avoid using `OpenStruct`; use `Struct`, `Hash`, a class or test doubles instead.
            end
        "});
    }

    #[test]
    fn flags_cbase_subclass_open_struct() {
        test::<OpenStructUse>().expect_offense(indoc! {"
            class SubClass < ::OpenStruct
                             ^^^^^^^^^^^^ Avoid using `OpenStruct`; use `Struct`, `Hash`, a class or test doubles instead.
            end
        "});
    }

    #[test]
    fn flags_class_new_open_struct() {
        test::<OpenStructUse>().expect_offense(indoc! {"
            SubClass = Class.new(OpenStruct)
                                 ^^^^^^^^^^ Avoid using `OpenStruct`; use `Struct`, `Hash`, a class or test doubles instead.
        "});
    }

    #[test]
    fn flags_cbase_class_new_open_struct() {
        test::<OpenStructUse>().expect_offense(indoc! {"
            SubClass = Class.new(::OpenStruct)
                                 ^^^^^^^^^^^^ Avoid using `OpenStruct`; use `Struct`, `Hash`, a class or test doubles instead.
        "});
    }

    #[test]
    fn accepts_namespaced_open_struct() {
        test::<OpenStructUse>().expect_no_offenses(indoc! {"
            a = SomeNamespace::OpenStruct.new(a: 42)
        "});
    }

    #[test]
    fn accepts_class_definition_named_open_struct() {
        // Defining a custom class named OpenStruct — not flagged.
        test::<OpenStructUse>().expect_no_offenses(indoc! {"
            class OpenStruct
            end
        "});
    }

    #[test]
    fn accepts_module_definition_named_open_struct() {
        test::<OpenStructUse>().expect_no_offenses(indoc! {"
            module OpenStruct
            end
        "});
    }

    #[test]
    fn accepts_namespaced_subclass_open_struct() {
        // `SomeNamespace::OpenStruct` in superclass position — not flagged.
        test::<OpenStructUse>().expect_no_offenses(indoc! {"
            class Foo < SomeNamespace::OpenStruct
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(OpenStructUse);
