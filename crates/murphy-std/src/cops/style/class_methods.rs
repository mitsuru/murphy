//! `Style/ClassMethods` — use `self` when defining class/module methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ClassMethods
//! upstream_version_checked: 1.86.2
//! version_added: "0.9"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags singleton method definitions (`def ClassName.method_name`) inside
//!   a class or module body where the explicit receiver matches the enclosing
//!   class/module name. Autocorrects by replacing the receiver with `self`.
//!
//!   Murphy AST note: `def Foo.bar` is represented as `NodeKind::Def` with
//!   `receiver: Some(const_node)` rather than a separate `Defs` node.
//!   Only direct body children are checked (matching RuboCop's
//!   `body.each_child_node(:defs)` behavior).
//!
//!   No options — matches upstream which has no `cop_config` keys.
//! ```
//!
//! ## Matched shapes
//!
//! `Class` or `Module` nodes whose body contains one or more `Def` nodes
//! where the receiver is a `Const` node whose `const_name` equals the
//! enclosing class/module name.
//!
//! ## Why this shape
//!
//! Dispatching on `class`/`module` and iterating direct body children mirrors
//! RuboCop's `on_class` / `alias on_module on_class` hooks which check
//! `node.body.defs_type?` (single singleton def) and
//! `node.body.each_child_node(:defs)` (multiple defs in a begin block).
//!
//! ## Autocorrect
//!
//! Replaces the explicit receiver const node with `self`:
//! `def SomeClass.method` → `def self.method`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Use `self.%{method}` instead of `%{class}.%{method}`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct ClassMethods;

#[cop(
    name = "Style/ClassMethods",
    description = "Use `self` when defining module/class methods.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ClassMethods {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check_class_or_module(node, cx);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        check_class_or_module(node, cx);
    }
}

/// Check a class or module node for singleton methods that use the class name
/// as the receiver instead of `self`.
fn check_class_or_module(node: NodeId, cx: &Cx<'_>) {
    // Extract the class/module name node and body.
    let (name_id, body_opt) = match *cx.kind(node) {
        NodeKind::Class { name, body, .. } => (name, body),
        NodeKind::Module { name, body } => (name, body),
        _ => return,
    };

    let Some(body_id) = body_opt.get() else {
        return;
    };

    // Get the fully-qualified class/module name for comparison.
    let Some(class_name) = cx.const_name(name_id) else {
        return;
    };

    // Collect direct-child Def nodes from the body.
    // If body is a Begin, iterate its children; otherwise treat as single node.
    let stmts: Vec<NodeId> = match cx.kind(body_id) {
        NodeKind::Begin(list) => cx.list(*list).to_vec(),
        _ => vec![body_id],
    };

    for stmt_id in stmts {
        check_def(stmt_id, &class_name, cx);
    }
}

/// Check a single statement node: if it is a singleton `Def` whose receiver
/// const name matches `class_name`, emit an offense with autocorrect.
fn check_def(node: NodeId, class_name: &str, cx: &Cx<'_>) {
    let NodeKind::Def { receiver, name, .. } = *cx.kind(node) else {
        return;
    };

    let Some(recv_id) = receiver.get() else {
        // No receiver — this is a regular instance method definition.
        return;
    };

    // The receiver must be a Const node whose const_name matches the
    // enclosing class/module name.
    let Some(recv_name) = cx.const_name(recv_id) else {
        return;
    };

    if recv_name != class_name {
        return;
    }

    let method = cx.symbol_str(name);
    let msg = MSG
        .replace("%{method}", method)
        .replace("%{class}", class_name);

    // Offense range: the receiver const node's name location (matching
    // RuboCop's `add_offense(node.receiver.loc.name, ...)`).
    let offense_range = cx.range(recv_id);

    cx.emit_offense(offense_range, &msg, None);

    // Autocorrect: replace the entire receiver const node with `self`.
    cx.emit_edit(cx.range(recv_id), "self");
}

#[cfg(test)]
mod tests {
    use super::ClassMethods;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- positive cases (offense + autocorrect) ---

    #[test]
    fn flags_class_method_with_class_name_receiver() {
        test::<ClassMethods>().expect_correction(
            indoc! {r#"
                class SomeClass
                  def SomeClass.class_method
                      ^^^^^^^^^ Use `self.class_method` instead of `SomeClass.class_method`.
                  end
                end
            "#},
            indoc! {r#"
                class SomeClass
                  def self.class_method
                  end
                end
            "#},
        );
    }

    #[test]
    fn flags_module_method_with_module_name_receiver() {
        test::<ClassMethods>().expect_correction(
            indoc! {r#"
                module SomeModule
                  def SomeModule.module_method
                      ^^^^^^^^^^ Use `self.module_method` instead of `SomeModule.module_method`.
                  end
                end
            "#},
            indoc! {r#"
                module SomeModule
                  def self.module_method
                  end
                end
            "#},
        );
    }

    #[test]
    fn flags_multiple_singleton_defs_in_begin_body() {
        test::<ClassMethods>().expect_correction(
            indoc! {r#"
                class SomeClass
                  def SomeClass.first_method
                      ^^^^^^^^^ Use `self.first_method` instead of `SomeClass.first_method`.
                  end
                  def SomeClass.second_method
                      ^^^^^^^^^ Use `self.second_method` instead of `SomeClass.second_method`.
                  end
                end
            "#},
            indoc! {r#"
                class SomeClass
                  def self.first_method
                  end
                  def self.second_method
                  end
                end
            "#},
        );
    }

    #[test]
    fn flags_single_def_as_sole_body_statement() {
        // When the class body has only one def, it is not wrapped in a Begin.
        test::<ClassMethods>().expect_correction(
            indoc! {r#"
                class SomeClass
                  def SomeClass.solo
                      ^^^^^^^^^ Use `self.solo` instead of `SomeClass.solo`.
                  end
                end
            "#},
            indoc! {r#"
                class SomeClass
                  def self.solo
                  end
                end
            "#},
        );
    }

    // --- negative cases (no offense) ---

    #[test]
    fn accepts_self_receiver() {
        test::<ClassMethods>().expect_no_offenses(indoc! {r#"
            class SomeClass
              def self.class_method
              end
            end
        "#});
    }

    #[test]
    fn accepts_mismatched_const_receiver() {
        // Receiver is a different class name — no offense.
        test::<ClassMethods>().expect_no_offenses(indoc! {r#"
            class SomeClass
              def OtherClass.class_method
              end
            end
        "#});
    }

    #[test]
    fn accepts_regular_instance_methods() {
        test::<ClassMethods>().expect_no_offenses(indoc! {r#"
            class SomeClass
              def instance_method
              end
            end
        "#});
    }

    #[test]
    fn accepts_empty_class_body() {
        test::<ClassMethods>().expect_no_offenses(indoc! {r#"
            class SomeClass
            end
        "#});
    }

    #[test]
    fn accepts_singleton_class_pattern() {
        // class << self; def foo; end; end — no singleton receiver const.
        test::<ClassMethods>().expect_no_offenses(indoc! {r#"
            class SomeClass
              class << self
                def class_method
                end
              end
            end
        "#});
    }

    #[test]
    fn accepts_outer_class_name_in_nested_class() {
        // The outer class name should not flag when used in a nested class
        // where only the nested class name matches.
        test::<ClassMethods>().expect_no_offenses(indoc! {r#"
            class Outer
              class Inner
                def Outer.bar
                end
              end
            end
        "#});
    }

    #[test]
    fn accepts_module_with_self_receiver() {
        test::<ClassMethods>().expect_no_offenses(indoc! {r#"
            module SomeModule
              def self.module_method
              end
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(ClassMethods);
