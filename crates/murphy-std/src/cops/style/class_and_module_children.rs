//! `Style/ClassAndModuleChildren` — enforces a consistent style for namespaced
//! class and module definitions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ClassAndModuleChildren
//! upstream_version_checked: 1.86.2
//! version_added: "0.19"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues:
//!   - murphy-q9g9
//! notes: >
//!   Detection is complete for the `nested` (default) and `compact` styles,
//!   including `EnforcedStyleForClasses` and `EnforcedStyleForModules` options.
//!   Autocorrect is not implemented because it is `SafeAutoCorrect: false` in
//!   RuboCop — expanding `compact` to `nested` requires knowing whether the
//!   outer parent is a class or module, and compacting `nested` requires
//!   verifying the parent is defined elsewhere; neither is safe to automate.
//!   Known v1 limitation: `::Foo::Bar` (cbase-prefixed path) and `Foo::Bar`
//!   produce identical AST nodes in Murphy (cbase is not preserved by the
//!   translator), so `class ::Foo::Bar` is treated the same as `class Foo::Bar`
//!   and will be flagged in nested mode. This matches the source-text behaviour:
//!   both contain `::` in the identifier, which is the compact style.
//! ```
//!
//! ## Matched shapes
//!
//! ### `nested` style (default)
//!
//! `Class` or `Module` nodes whose name constant has a non-nil scope chain (i.e.
//! the identifier contains `::`, meaning a compact-style `class Foo::Bar`),
//! unless the definition is directly nested inside another `class` or `module`.
//!
//! ### `compact` style
//!
//! `Class` or `Module` nodes whose body is itself a single `class` or `module`
//! (i.e. `class Foo; class Bar; end; end` which could be written as
//! `class Foo::Bar`), unless the definition is directly nested inside another
//! `class` or `module`.
//!
//! ## Why this shape
//!
//! The key invariant from RuboCop: the guard `return if parent&.type?(:class, :module)`
//! prevents flagging inner definitions that are already properly nested. Murphy
//! implements this by checking `cx.parent(node)` against `NodeKind::Class` and
//! `NodeKind::Module`. When a class body holds a single class/module child, the
//! child's parent is the outer class (not a `Begin` wrapper), so the guard fires
//! on the inner definition but not the outer one.
//!
//! ## No autocorrect
//!
//! Autocorrect is `SafeAutoCorrect: false` in RuboCop. The correction requires
//! context that cannot be inferred from the AST alone (whether the outer constant
//! is a class or a module). Users should apply corrections manually.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const NESTED_MSG: &str =
    "Use nested module/class definitions instead of compact style.";
const COMPACT_MSG: &str =
    "Use compact module/class definition instead of nested style.";

#[derive(Default)]
pub struct ClassAndModuleChildren;

/// Options for [`ClassAndModuleChildren`].
///
/// Matches RuboCop's `EnforcedStyle`, `EnforcedStyleForClasses`, and
/// `EnforcedStyleForModules` config keys.
#[derive(CopOptions)]
pub struct ClassAndModuleChildrenOptions {
    /// Overall enforced style: `"nested"` (default) or `"compact"`.
    #[option(
        name = "EnforcedStyle",
        default = "nested",
        description = "Enforced style for namespaced class/module definitions."
    )]
    pub enforced_style: String,
    /// Per-class style override. When `None` (default), falls back to
    /// `EnforcedStyle`.
    #[option(
        name = "EnforcedStyleForClasses",
        description = "Per-class enforced style (overrides EnforcedStyle). Use null to inherit."
    )]
    pub enforced_style_for_classes: Option<String>,
    /// Per-module style override. When `None` (default), falls back to
    /// `EnforcedStyle`.
    #[option(
        name = "EnforcedStyleForModules",
        description = "Per-module enforced style (overrides EnforcedStyle). Use null to inherit."
    )]
    pub enforced_style_for_modules: Option<String>,
}

#[cop(
    name = "Style/ClassAndModuleChildren",
    description = "Checks that namespaced classes and modules are defined with a consistent style.",
    default_severity = "warning",
    default_enabled = true,
    options = ClassAndModuleChildrenOptions
)]
impl ClassAndModuleChildren {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class {
            name: name_id,
            superclass,
            body,
        } = *cx.kind(node)
        else {
            return;
        };

        let opts = cx.options_or_default::<ClassAndModuleChildrenOptions>();
        let effective_style = opts
            .enforced_style_for_classes
            .as_deref()
            .unwrap_or(opts.enforced_style.as_str());

        // RuboCop guard: `return if node.parent_class && style != :nested`
        // Skip inheritance checks on classes with a superclass in compact mode.
        if superclass.get().is_some() && effective_style != "nested" {
            return;
        }

        check_style(node, name_id, body.get(), effective_style, cx);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Module {
            name: name_id,
            body,
        } = *cx.kind(node)
        else {
            return;
        };

        let opts = cx.options_or_default::<ClassAndModuleChildrenOptions>();
        let effective_style = opts
            .enforced_style_for_modules
            .as_deref()
            .unwrap_or(opts.enforced_style.as_str());

        check_style(node, name_id, body.get(), effective_style, cx);
    }
}

/// Dispatch to the appropriate style check.
fn check_style(node: NodeId, name_id: NodeId, body: Option<NodeId>, style: &str, cx: &Cx<'_>) {
    // RuboCop: `return if node.identifier.namespace&.cbase_type?`
    // Murphy's translator does not preserve cbase, so we check the source:
    // if the identifier starts with `::` it is an absolute path — skip.
    let name_src = cx.raw_source(cx.range(name_id));
    if name_src.starts_with("::") {
        return;
    }

    if style == "compact" {
        check_compact_style(node, body, cx);
    } else {
        check_nested_style(node, name_id, cx);
    }
}

/// Flag compact-style names (`class Foo::Bar`) when nested style is enforced.
fn check_nested_style(node: NodeId, name_id: NodeId, cx: &Cx<'_>) {
    if !is_compact_name(name_id, cx) {
        return;
    }
    // RuboCop: `return if node.parent&.type?(:class, :module)`
    if parent_is_class_or_module(node, cx) {
        return;
    }
    cx.emit_offense(cx.range(name_id), NESTED_MSG, None);
}

/// Flag nested single-child class/module when compact style is enforced.
fn check_compact_style(node: NodeId, body: Option<NodeId>, cx: &Cx<'_>) {
    // RuboCop: `return if parent&.type?(:class, :module)`
    if parent_is_class_or_module(node, cx) {
        return;
    }
    // Only flag when the body is a single class or module definition
    // (RuboCop's `needs_compacting?` check).
    let Some(body_id) = body else {
        return;
    };
    if !needs_compacting(body_id, cx) {
        return;
    }
    // Offense on the name of the outer class/module.
    let name_id = match *cx.kind(node) {
        NodeKind::Class { name, .. } => name,
        NodeKind::Module { name, .. } => name,
        _ => return,
    };
    cx.emit_offense(cx.range(name_id), COMPACT_MSG, None);
}

/// Returns `true` when the name constant has a non-nil scope, meaning
/// the identifier contains `::` (e.g. `Foo::Bar`).
fn is_compact_name(name_id: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(name_id) {
        NodeKind::Const { scope, .. } => scope.get().is_some(),
        _ => false,
    }
}

/// Returns `true` when `node`'s direct parent is a `class` or `module`.
fn parent_is_class_or_module(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent_id) = cx.parent(node).get() else {
        return false;
    };
    matches!(*cx.kind(parent_id), NodeKind::Class { .. } | NodeKind::Module { .. })
}

/// Returns `true` when `body_id` is itself a `class` or `module` node
/// (mirrors RuboCop's `needs_compacting?`).
fn needs_compacting(body_id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(body_id),
        NodeKind::Class { .. } | NodeKind::Module { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::{ClassAndModuleChildren, ClassAndModuleChildrenOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- nested style (default) ---

    #[test]
    fn nested_flags_compact_class_definition() {
        test::<ClassAndModuleChildren>().expect_offense(indoc! {"
            class Foo::Bar
                  ^^^^^^^^ Use nested module/class definitions instead of compact style.
            end
        "});
    }

    #[test]
    fn nested_flags_compact_module_definition() {
        test::<ClassAndModuleChildren>().expect_offense(indoc! {"
            module Foo::Bar
                   ^^^^^^^^ Use nested module/class definitions instead of compact style.
            end
        "});
    }

    #[test]
    fn nested_accepts_already_nested_class() {
        test::<ClassAndModuleChildren>().expect_no_offenses(indoc! {"
            class Foo
              class Bar
              end
            end
        "});
    }

    #[test]
    fn nested_accepts_already_nested_module() {
        test::<ClassAndModuleChildren>().expect_no_offenses(indoc! {"
            module Foo
              module Bar
              end
            end
        "});
    }

    #[test]
    fn nested_accepts_plain_class_no_namespace() {
        test::<ClassAndModuleChildren>().expect_no_offenses(indoc! {"
            class Foo
            end
        "});
    }

    #[test]
    fn nested_accepts_plain_module_no_namespace() {
        test::<ClassAndModuleChildren>().expect_no_offenses(indoc! {"
            module Foo
            end
        "});
    }

    #[test]
    fn nested_flags_class_with_superclass_and_compact_name() {
        // class Foo::Bar < Baz — has superclass; nested mode still flags
        // compact notation (superclass guard only skips in compact mode)
        test::<ClassAndModuleChildren>().expect_offense(indoc! {"
            class Foo::Bar < Baz
                  ^^^^^^^^ Use nested module/class definitions instead of compact style.
            end
        "});
    }

    #[test]
    fn nested_inner_compact_directly_nested_is_skipped() {
        // The inner compact `class Bar::Baz` is directly nested in `class Foo`
        // so the parent guard fires — no offense on the inner class.
        test::<ClassAndModuleChildren>().expect_no_offenses(indoc! {"
            class Foo
              class Bar::Baz
              end
            end
        "});
    }

    #[test]
    fn nested_inner_compact_with_siblings_is_flagged() {
        // When there are siblings, the inner `class Bar::Baz` is inside a
        // `begin` node, whose parent is `class Foo`. The class's parent is
        // `Begin`, not `Class`, so the guard does NOT fire.
        test::<ClassAndModuleChildren>().expect_offense(indoc! {"
            class Foo
              X = 1
              class Bar::Baz
                    ^^^^^^^^ Use nested module/class definitions instead of compact style.
              end
            end
        "});
    }

    // --- compact style ---

    #[test]
    fn compact_flags_nested_class_with_single_child_class() {
        test::<ClassAndModuleChildren>()
            .with_options(&ClassAndModuleChildrenOptions {
                enforced_style: "compact".to_string(),
                enforced_style_for_classes: None,
                enforced_style_for_modules: None,
            })
            .expect_offense(indoc! {"
                class Foo
                      ^^^ Use compact module/class definition instead of nested style.
                  class Bar
                  end
                end
            "});
    }

    #[test]
    fn compact_flags_nested_module_with_single_child_module() {
        test::<ClassAndModuleChildren>()
            .with_options(&ClassAndModuleChildrenOptions {
                enforced_style: "compact".to_string(),
                enforced_style_for_classes: None,
                enforced_style_for_modules: None,
            })
            .expect_offense(indoc! {"
                module Foo
                       ^^^ Use compact module/class definition instead of nested style.
                  module Bar
                  end
                end
            "});
    }

    #[test]
    fn compact_accepts_already_compact_class() {
        test::<ClassAndModuleChildren>()
            .with_options(&ClassAndModuleChildrenOptions {
                enforced_style: "compact".to_string(),
                enforced_style_for_classes: None,
                enforced_style_for_modules: None,
            })
            .expect_no_offenses(indoc! {"
                class Foo::Bar
                end
            "});
    }

    #[test]
    fn compact_accepts_class_with_non_class_body() {
        // Body has a method, not a nested class/module — no offense
        test::<ClassAndModuleChildren>()
            .with_options(&ClassAndModuleChildrenOptions {
                enforced_style: "compact".to_string(),
                enforced_style_for_classes: None,
                enforced_style_for_modules: None,
            })
            .expect_no_offenses(indoc! {"
                class Foo
                  def bar; end
                end
            "});
    }

    #[test]
    fn compact_parent_guard_fires_for_inner_class_inside_class() {
        // `class Inner` is directly inside `class Outer` (parent = class Outer).
        // The parent guard fires for `class Inner` — it is not independently
        // flagged even though it has a single class-body child (`class Leaf`).
        // Only `class Outer` gets flagged (body = `class Inner` -> needs_compacting).
        test::<ClassAndModuleChildren>()
            .with_options(&ClassAndModuleChildrenOptions {
                enforced_style: "compact".to_string(),
                enforced_style_for_classes: None,
                enforced_style_for_modules: None,
            })
            .expect_offense(indoc! {"
                class Outer
                      ^^^^^ Use compact module/class definition instead of nested style.
                  class Inner
                    class Leaf
                    end
                  end
                end
            "});
    }

    #[test]
    fn compact_skips_class_with_superclass() {
        // `return if node.parent_class && style != :nested`
        // In compact mode, a class with a superclass is skipped entirely.
        test::<ClassAndModuleChildren>()
            .with_options(&ClassAndModuleChildrenOptions {
                enforced_style: "compact".to_string(),
                enforced_style_for_classes: None,
                enforced_style_for_modules: None,
            })
            .expect_no_offenses(indoc! {"
                class Foo < Bar
                  class Baz
                  end
                end
            "});
    }

    // --- per-type style options ---

    #[test]
    fn enforced_style_for_classes_overrides_global_nested_to_compact() {
        // Global: nested; classes: compact — outer class with single inner class
        // should be flagged (compact expected, nested given).
        test::<ClassAndModuleChildren>()
            .with_options(&ClassAndModuleChildrenOptions {
                enforced_style: "nested".to_string(),
                enforced_style_for_classes: Some("compact".to_string()),
                enforced_style_for_modules: None,
            })
            .expect_offense(indoc! {"
                class Foo
                      ^^^ Use compact module/class definition instead of nested style.
                  class Bar
                  end
                end
            "});
    }

    #[test]
    fn enforced_style_for_modules_overrides_global_nested_to_compact() {
        // Global: nested; modules: compact — outer module with single inner module
        // should be flagged.
        test::<ClassAndModuleChildren>()
            .with_options(&ClassAndModuleChildrenOptions {
                enforced_style: "nested".to_string(),
                enforced_style_for_classes: None,
                enforced_style_for_modules: Some("compact".to_string()),
            })
            .expect_offense(indoc! {"
                module Foo
                       ^^^ Use compact module/class definition instead of nested style.
                  module Bar
                  end
                end
            "});
    }

    // --- no autocorrect ---

    #[test]
    fn no_corrections_emitted_for_nested_offense() {
        test::<ClassAndModuleChildren>()
            .expect_no_corrections(indoc! {"
                class Foo::Bar
                end
            "});
    }
}

murphy_plugin_api::submit_cop!(ClassAndModuleChildren);
