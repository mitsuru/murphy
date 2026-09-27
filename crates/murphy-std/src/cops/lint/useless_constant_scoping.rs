//! `Lint/UselessConstantScoping` — Checks for useless `private` access modifier
//! applied to constant definitions. Private constants must be defined using
//! `private_constant`, not by `private` modifier.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessConstantScoping
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Symbol, cop, def_node_matcher};

// RuboCop parity: `Lint/UselessConstantScoping`
// `class_or_module_definition_assignment?` is
// `{(send (const {nil? cbase} {:Class :Module :Struct}) :new ...)`
// `(send (const {nil? cbase} :Data) :define ...)`
// `(any_block {(send (const {nil? cbase} {:Class :Module :Struct}) :new ...)`
// `(send (const {nil? cbase} :Data) :define ...)} ...)}`
// (top-level only, send-only). In Murphy `::Class` collapses to
// `Const{scope:None}`: `nil?` covers bare + `::` (both suppress, pinned by
// `boundary_accepts_cbase_class_new_definition`). Namespaced `Foo::Class`
// still flags (pinned by `boundary_flags_namespaced_class_new_definition`).
// Predicate-only, no captures, byte-identical except for the intended
// suppression fix (parity-provable FP fix: upstream explicitly documents
// `MyClass = Class.new` etc as allowed after `private`). `send` covers
// `Send` only (matching upstream send-only; `Class&.new` still flags,
// pinned by `boundary_flags_csend_class_new_definition`). Block forms via
// hand-rolled `block_call` unwrap (covers `Block`/`Numblock`/`Itblock`,
// matching upstream `any_block`).
def_node_matcher!(
    is_class_module_struct_new,
    "(send (const nil? {:Class :Module :Struct}) :new ...)"
);
def_node_matcher!(is_data_define_call, "(send (const nil? :Data) :define ...)");

#[derive(Default)]
pub struct UselessConstantScoping;

#[cop(
    name = "Lint/UselessConstantScoping",
    description = "Checks for useless `private` access modifier for constant scope.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessConstantScoping {
    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Casgn { name, value, .. } = *cx.kind(node) else {
            return;
        };

        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        let (NodeKind::Begin(list) | NodeKind::Kwbegin(list)) = *cx.kind(parent) else {
            return;
        };
        let all = cx.list(list);
        let Some(i) = all.iter().position(|&s| s == node) else {
            return;
        };
        let left_siblings = &all[..i];
        let right_siblings = &all[i + 1..];

        if !after_private_modifier(left_siblings, cx) {
            return;
        }
        if private_constantize(right_siblings, name, cx) {
            return;
        }
        // `class_or_module_definition_assignment?`: `MyClass = Class.new`
        // etc are class/module definitions (allowed after `private`,
        // matching upstream). Predicate-only via verbatim const inners.
        if let Some(val) = value.get()
            && is_class_or_module_definition_assignment(val, cx)
        {
            return;
        }

        cx.emit_offense(
            cx.range(node),
            "Useless `private` access modifier for constant scope.",
            None,
        );
    }
}

/// RuboCop `class_or_module_definition_assignment?` verbatim (predicate-only):
/// `(send (const nil? {:Class :Module :Struct}) :new ...)` +
/// `(send (const nil? :Data) :define ...)` + block forms via `block_call`
/// unwrap (covers `Block`/`Numblock`/`Itblock`, matching upstream `any_block`).
/// `send` covers `Send` only (matching upstream send-only; `Class&.new`
/// still flags).
fn is_class_or_module_definition_assignment(node: NodeId, cx: &Cx<'_>) -> bool {
    // Unwrap block forms: `MyClass = Class.new do ... end` etc.
    // `block_call` covers all three block kinds.
    let target = cx.block_call(node).get().unwrap_or(node);
    is_class_module_struct_new(target, cx) || is_data_define_call(target, cx)
}

fn after_private_modifier(left_siblings: &[NodeId], cx: &Cx<'_>) -> bool {
    let mut last_bare_name: Option<&str> = None;
    for &sibling in left_siblings {
        if cx.is_bare_access_modifier(sibling)
            && let Some(name) = cx.method_name(sibling) {
                last_bare_name = Some(name);
            }
    }
    last_bare_name == Some("private")
}

fn private_constantize(right_siblings: &[NodeId], const_name: Symbol, cx: &Cx<'_>) -> bool {
    for &sibling in right_siblings {
        if cx.method_name(sibling) == Some("private_constant") {
            for &arg in cx.call_arguments(sibling) {
                if let NodeKind::Sym(sym) = *cx.kind(arg)
                    && sym == const_name {
                        return true;
                    }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::UselessConstantScoping;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_private_before_const() {
        test::<UselessConstantScoping>().expect_offense(indoc! {r#"
            class Foo
              private
              CONST = 42
              ^^^^^^^^^^ Useless `private` access modifier for constant scope.
            end
        "#});
    }

    #[test]
    fn accepts_public_before_const() {
        test::<UselessConstantScoping>().expect_no_offenses(indoc! {r#"
            class Foo
              private
              public
              CONST = 42
            end
        "#});
    }

    #[test]
    fn accepts_private_constant() {
        test::<UselessConstantScoping>().expect_no_offenses(indoc! {r#"
            class Foo
              private
              CONST = 42
              private_constant :CONST
            end
        "#});
    }

    #[test]
    fn flags_private_before_const_in_sclass() {
        test::<UselessConstantScoping>().expect_offense(indoc! {r#"
            class Foo
              class << self
                private
                CONST = 42
                ^^^^^^^^^^ Useless `private` access modifier for constant scope.
              end
            end
        "#});
    }

    #[test]
    fn accepts_private_constant_in_sclass() {
        test::<UselessConstantScoping>().expect_no_offenses(indoc! {r#"
            class Foo
              class << self
                private
                CONST = 42
                private_constant :CONST
              end
            end
        "#});
    }

    #[test]
    fn accepts_no_modifier() {
        test::<UselessConstantScoping>().expect_no_offenses(indoc! {r#"
            class Foo
              CONST = 42
            end
        "#});
    }

    #[test]
    fn flags_non_modifier_call_between() {
        test::<UselessConstantScoping>().expect_offense(indoc! {r#"
            class Foo
              private
              do_something
              CONST = 42
              ^^^^^^^^^^ Useless `private` access modifier for constant scope.
            end
        "#});
    }

    #[test]
    fn accepts_multiple_private_constants_with_multiple_args() {
        test::<UselessConstantScoping>().expect_no_offenses(indoc! {r#"
            class Foo
              private
              CONST_A = 1
              CONST_B = 2
              private_constant :CONST_A, :CONST_B
            end
        "#});
    }

    // --- Boundary characterization (murphy-ft88.31): pin the exact node set
    // for the verbatim `class_or_module_definition_assignment?` suppression
    // fix (parity-provable FP fix: upstream explicitly documents
    // `MyClass = Class.new` etc as allowed after `private`, verified with
    // rubocop 1.91.0 `Lint/UselessConstantScoping` = no offense).
    // `(const nil? {:Class :Module :Struct})` + `(const nil? :Data)`
    // top-level only (bare + `::` suppress, namespaced flags); `send`
    // send-only (csend flags); block forms via `block_call` suppress.

    #[test]
    fn boundary_accepts_class_new_definition() {
        test::<UselessConstantScoping>().expect_no_offenses(indoc! {r#"
            class Foo
              private
              MyClass = Class.new
            end
        "#});
    }

    #[test]
    fn boundary_accepts_module_new_definition() {
        test::<UselessConstantScoping>().expect_no_offenses(indoc! {r#"
            class Foo
              private
              MyModule = Module.new
            end
        "#});
    }

    #[test]
    fn boundary_accepts_struct_new_definition() {
        test::<UselessConstantScoping>().expect_no_offenses(indoc! {r#"
            class Foo
              private
              MyStruct = Struct.new(:name)
            end
        "#});
    }

    #[test]
    fn boundary_accepts_data_define_definition() {
        test::<UselessConstantScoping>().expect_no_offenses(indoc! {r#"
            class Foo
              private
              MyData = Data.define(:name)
            end
        "#});
    }

    #[test]
    fn boundary_accepts_cbase_class_new_definition() {
        // `::Class` collapses to scope-less `Const`, so `nil?` covers it.
        test::<UselessConstantScoping>().expect_no_offenses(indoc! {r#"
            class Foo
              private
              MyClass = ::Class.new
            end
        "#});
    }

    #[test]
    fn boundary_flags_namespaced_class_new_definition() {
        // Upstream `(const {nil? cbase} ...)` top-level only: `Foo::Class`
        // still flags (not suppressed).
        test::<UselessConstantScoping>().expect_offense(indoc! {r#"
            class Foo
              private
              MyClass = Foo::Class.new
              ^^^^^^^^^^^^^^^^^^^^^^^^ Useless `private` access modifier for constant scope.
            end
        "#});
    }

    #[test]
    fn boundary_flags_csend_class_new_definition() {
        // Upstream send-only: `Class&.new` still flags (not suppressed).
        test::<UselessConstantScoping>().expect_offense(indoc! {r#"
            class Foo
              private
              MyClass = Class&.new
              ^^^^^^^^^^^^^^^^^^^^ Useless `private` access modifier for constant scope.
            end
        "#});
    }

    #[test]
    fn boundary_accepts_block_class_new_definition() {
        // Block form `Class.new do ... end` also suppresses (via `block_call`).
        test::<UselessConstantScoping>().expect_no_offenses(indoc! {r#"
            class Foo
              private
              MyClass = Class.new do
                def foo; end
              end
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(UselessConstantScoping);
