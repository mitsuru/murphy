//! `Lint/ConstantReassignment` — flag a constant assigned twice in the same
//! file and namespace, emulating Ruby's runtime "already initialized constant"
//! warning.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ConstantReassignment
//! upstream_version_checked: 1.86.2
//! version_added: "1.70"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Stateful whole-file cop: a single pre-order walk
//!   (`on_new_investigation` over `cx.descendants(root)`) builds a
//!   namespace-qualified registry. `class`/`module` definitions register their
//!   name first-wins and never emit; only a `casgn` reassignment of an
//!   already-registered name emits. `simple_assignment?` restricts to
//!   assignments directly in class/module bodies, plain `begin` wrappers,
//!   literals, nested casgn, masgn/mlhs, and `freeze` calls — assignments
//!   guarded by `if`/`unless` (e.g. `unless defined?(X)`) or inside methods are
//!   ignored. `remove_const :X` unregisters when inside a class/module.
//!   `||=`/`&&=` are `OrAsgn`/`AndAsgn`, naturally unmatched.
//!
//!   Compound (`A::B = …`, `class A::B`) and cbase-absolute (`::X = 1`,
//!   `::Foo::Bar = 1`, `class ::A`) paths are now fully qualified, matching
//!   RuboCop's `absolute?` / `cbase_type?` branches (murphy-as93 closed).
//!   Murphy's AST drops the `cbase` scope node, so `absolute?` is recovered
//!   from source text: an absolute constant path's expression range starts at
//!   the leading `::`. Absolute casgn keys drop ancestor namespaces; an
//!   immediate-cbase class/module identifier yields `"::<short>"`.
//!
//!   INHERENT LIMITATION (not a fixable gap): cross-file detection
//!   (`AllCops/UseProjectIndex` + rubydex `CROSS_FILE_MSG`) is out of scope —
//!   the single-surface plugin ABI exposes no project index. Murphy is a
//!   per-file cop; only same-file reassignments are detected.
//! ```
//!
//! ## Matched shapes
//!
//! Within one file: a `Casgn` (with a value) whose fully-qualified name was
//! already registered by a prior `Casgn`, `Class`, or `Module` in source order
//! and not since removed via `remove_const`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};
use std::collections::HashSet;

#[derive(Default)]
pub struct ConstantReassignment;

#[cop(
    name = "Lint/ConstantReassignment",
    description = "Checks for constant reassignments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ConstantReassignment {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        // Source-order pre-order walk, root included, mirroring RuboCop's
        // node-visit order so the registry is built deterministically.
        let root = cx.root();
        let mut registry: HashSet<String> = HashSet::new();

        for node in std::iter::once(root).chain(cx.descendants(root)) {
            match *cx.kind(node) {
                NodeKind::Class { .. } | NodeKind::Module { .. } => {
                    if unconditional_definition(node, cx)
                        && let Some(name) = definition_name(node, cx)
                    {
                        // `||=`: first definition wins (insert is idempotent).
                        registry.insert(name);
                    }
                }
                NodeKind::Casgn { value, .. } => {
                    // RuboCop registers/checks only assignments with a value
                    // (a bare value-less `Casgn` is an op-assign target, not a
                    // definition).
                    if value.get().is_none() {
                        continue;
                    }
                    if !fixed_constant_path(node, cx) || !simple_assignment(node, cx) {
                        continue;
                    }
                    let Some(name) = fully_qualified_constant_name(node, cx) else {
                        continue;
                    };
                    // `insert` returns false when the name was already present
                    // → reassignment.
                    if !registry.insert(name) {
                        let display = constant_display_name(node, cx);
                        let message =
                            format!("Constant `{display}` is already assigned in this namespace.");
                        cx.emit_offense(cx.range(node), &message, None);
                    }
                }
                NodeKind::Send { .. } => {
                    if let Some(constant) = remove_const_arg(node, cx) {
                        let namespaces = ancestor_namespaces(node, cx);
                        // `return if namespaces.none?`: top-level `remove_const`
                        // is a no-op for the registry.
                        if !namespaces.is_empty() {
                            registry.remove(&fully_qualified_name_for(&namespaces, constant));
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// `unconditional_definition?`: every ancestor of the class/module is a
/// `begin`/`module`/`class` (no `if`/block/method wrapping).
fn unconditional_definition(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(node).all(|a| {
        matches!(
            cx.kind(a),
            NodeKind::Begin(_) | NodeKind::Module { .. } | NodeKind::Class { .. }
        )
    })
}

/// `simple_assignment?`: walk casgn ancestors. Accept as soon as a
/// module/class ancestor is reached; otherwise every ancestor must be a
/// `begin`, literal, nested casgn, masgn/mlhs, or a `freeze` send.
fn simple_assignment(node: NodeId, cx: &Cx<'_>) -> bool {
    for a in cx.ancestors(node) {
        match cx.kind(a) {
            NodeKind::Module { .. } | NodeKind::Class { .. } => return true,
            NodeKind::Begin(_)
            | NodeKind::Casgn { .. }
            | NodeKind::Masgn { .. }
            | NodeKind::Mlhs(_) => {}
            _ if cx.is_literal(a) => {}
            _ if is_freeze_call(a, cx) => {}
            _ => return false,
        }
    }
    true
}

/// `freeze_method?`: a `Send` whose selector is `freeze`.
fn is_freeze_call(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Send { .. }) && cx.method_name(node) == Some("freeze")
}

/// `fixed_constant_path?`: the casgn's scope chain is only cbase/const/self.
fn fixed_constant_path(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Casgn { mut scope, .. } = *cx.kind(node) else {
        return false;
    };
    while let Some(s) = scope.get() {
        match *cx.kind(s) {
            NodeKind::Cbase | NodeKind::SelfExpr => break,
            NodeKind::Const { scope: inner, .. } => scope = inner,
            _ => return false,
        }
    }
    true
}

/// `remove_constant`: `(send {nil? self} :remove_const ({sym str} $_))` →
/// the symbol/string argument's text.
fn remove_const_arg<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    if cx.method_name(node) != Some("remove_const") {
        return None;
    }
    // Receiver must be nil or self.
    match cx.call_receiver(node).get() {
        None => {}
        Some(r) if matches!(cx.kind(r), NodeKind::SelfExpr) => {}
        Some(_) => return None,
    }
    let args = cx.call_arguments(node);
    let [arg] = args else { return None };
    match *cx.kind(*arg) {
        NodeKind::Sym(sym) => Some(cx.symbol_str(sym)),
        NodeKind::Str(sid) => Some(cx.string_str(sid)),
        _ => None,
    }
}

/// `ancestor_namespaces`: enclosing class/module short names, outermost first.
fn ancestor_namespaces<'a>(node: NodeId, cx: &Cx<'a>) -> Vec<&'a str> {
    let mut names: Vec<&'a str> = cx
        .ancestors(node)
        .filter_map(|a| match *cx.kind(a) {
            NodeKind::Class { name, .. } | NodeKind::Module { name, .. } => {
                const_short_name(name, cx)
            }
            _ => None,
        })
        .collect();
    names.reverse();
    names
}

/// `constant_namespaces`: the casgn's own const-path scope short names.
///
/// Note: Murphy's translate layer drops the `cbase` node, so this walk alone
/// cannot tell `::Foo::Bar` from `Foo::Bar`. The absolute/relative
/// distinction is handled by the caller via [`is_cbase_absolute`] (source-text
/// based), which decides whether ancestor namespaces are prepended.
fn constant_namespaces<'a>(node: NodeId, cx: &Cx<'a>) -> Vec<&'a str> {
    let NodeKind::Casgn { scope, .. } = *cx.kind(node) else {
        return Vec::new();
    };
    scope_const_short_names(scope, cx)
}

/// `fully_qualified_constant_name`. RuboCop branches on `node.absolute?`:
///
/// - **absolute** (`::X`, `::Foo::Bar`): the constant path is rooted at the
///   top level, so ancestor namespaces are dropped — the key is
///   `['', *const_ns, name].join('::')`. A bare cbase like `::X` has no const
///   namespace (its scope is the cbase, not a const), giving `"::X"`;
///   `::Foo::Bar` keeps `Foo`, giving `"::Foo::Bar"`.
/// - **relative** (`X`, `Foo::Bar`): ancestor namespaces are included, as
///   before.
///
/// Murphy's translate layer drops the cbase scope node, so `node.absolute?`
/// cannot be read from the AST. It is recovered from source text instead: the
/// casgn's expression range starts at the leading `::` for an absolute path.
fn fully_qualified_constant_name(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let NodeKind::Casgn { name, .. } = *cx.kind(node) else {
        return None;
    };
    let short = cx.symbol_str(name);
    let mut namespaces = if is_cbase_absolute(node, cx) {
        Vec::new()
    } else {
        ancestor_namespaces(node, cx)
    };
    namespaces.extend(constant_namespaces(node, cx));
    Some(fully_qualified_name_for(&namespaces, short))
}

/// `node.absolute?`: true when the constant path is rooted at the top level
/// with a leading `::`. Recovered from source text because Murphy's AST drops
/// the cbase scope node (see [`fully_qualified_constant_name`]).
fn is_cbase_absolute(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.raw_source(cx.range(node)).starts_with("::")
}

/// `fully_qualified_name_for`: leading `::` plus joined namespaces.
fn fully_qualified_name_for(namespaces: &[&str], constant: &str) -> String {
    let mut parts = Vec::with_capacity(namespaces.len() + 2);
    parts.push("");
    parts.extend_from_slice(namespaces);
    parts.push(constant);
    parts.join("::")
}

/// `constant_display_name`: `[*const_ns, name].join('::')` (no leading `::`).
fn constant_display_name(node: NodeId, cx: &Cx<'_>) -> String {
    let NodeKind::Casgn { name, .. } = *cx.kind(node) else {
        return String::new();
    };
    let mut parts = constant_namespaces(node, cx);
    parts.push(cx.symbol_str(name));
    parts.join("::")
}

/// `definition_name` for a class/module. RuboCop special-cases an identifier
/// whose immediate namespace is a cbase (`class ::A`): it drops every ancestor
/// and yields `"::<short>"`. Only the *immediate* cbase counts —
/// `class ::A::B`'s identifier namespace is the const `::A` (not a cbase), so
/// it takes the ordinary ancestors + identifier-namespaces branch.
///
/// Murphy's AST drops the cbase node, so "immediate cbase" is reconstructed as:
/// the rightmost const has no const scope (`identifier.namespace` would be the
/// dropped cbase) *and* the identifier's source starts with `::`.
fn definition_name(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let name_const = match *cx.kind(node) {
        NodeKind::Class { name, .. } | NodeKind::Module { name, .. } => name,
        _ => return None,
    };
    let short = const_short_name(name_const, cx)?;
    if has_immediate_cbase(name_const, cx) {
        return Some(fully_qualified_name_for(&[], short));
    }
    let mut namespaces = ancestor_namespaces(node, cx);
    namespaces.extend(identifier_namespaces(name_const, cx));
    Some(fully_qualified_name_for(&namespaces, short))
}

/// `identifier.namespace&.cbase_type?`: the const identifier's immediate
/// namespace is a cbase (`::A` but not `::A::B` and not `A::B`). Reconstructed
/// from the dropped-cbase AST: rightmost const has no const scope and the
/// identifier's source begins with `::`.
fn has_immediate_cbase(name_const: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Const { scope, .. } = *cx.kind(name_const) else {
        return false;
    };
    scope.get().is_none() && cx.raw_source(cx.range(name_const)).starts_with("::")
}

/// Short (rightmost) name of a const node.
fn const_short_name<'a>(const_node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(const_node) {
        NodeKind::Const { name, .. } => Some(cx.symbol_str(name)),
        _ => None,
    }
}

/// The const-path namespaces of a class/module identifier (`class A::B` →
/// `["A"]`).
fn identifier_namespaces<'a>(const_node: NodeId, cx: &Cx<'a>) -> Vec<&'a str> {
    let NodeKind::Const { scope, .. } = *cx.kind(const_node) else {
        return Vec::new();
    };
    scope_const_short_names(scope, cx)
}

/// Walk a const scope chain, collecting each `Const` segment's short name,
/// outermost first. Shared by `constant_namespaces` and
/// `identifier_namespaces`. Stops at any non-`Const` scope (Murphy drops the
/// `cbase` node, so an absolute prefix simply terminates the walk).
fn scope_const_short_names<'a>(scope: OptNodeId, cx: &Cx<'a>) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut cur = scope;
    while let Some(s) = cur.get() {
        match *cx.kind(s) {
            NodeKind::Const {
                scope: inner, name, ..
            } => {
                out.push(cx.symbol_str(name));
                cur = inner;
            }
            _ => break,
        }
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::ConstantReassignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_top_level_reassignment() {
        test::<ConstantReassignment>().expect_offense(indoc! {r#"
            X = :foo
            X = :bar
            ^^^^^^^^ Constant `X` is already assigned in this namespace.
        "#});
    }

    #[test]
    fn flags_reassignment_in_class() {
        test::<ConstantReassignment>().expect_offense(indoc! {r#"
            class A
              X = :foo
              X = :bar
              ^^^^^^^^ Constant `X` is already assigned in this namespace.
            end
        "#});
    }

    #[test]
    fn flags_reassignment_in_module() {
        test::<ConstantReassignment>().expect_offense(indoc! {r#"
            module A
              X = :foo
              X = :bar
              ^^^^^^^^ Constant `X` is already assigned in this namespace.
            end
        "#});
    }

    #[test]
    fn flags_class_then_casgn_reassignment() {
        test::<ConstantReassignment>().expect_offense(indoc! {r#"
            class FooError < StandardError; end
            FooError = Class.new(RuntimeError)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Constant `FooError` is already assigned in this namespace.
        "#});
    }

    #[test]
    fn flags_module_then_casgn_reassignment() {
        test::<ConstantReassignment>().expect_offense(indoc! {r#"
            module M; end
            M = 1
            ^^^^^ Constant `M` is already assigned in this namespace.
        "#});
    }

    #[test]
    fn does_not_flag_single_assignment() {
        test::<ConstantReassignment>().expect_no_offenses(indoc! {r#"
            X = :bar
        "#});
    }

    #[test]
    fn does_not_flag_reopened_class() {
        // class/module definitions register first-wins and never emit.
        test::<ConstantReassignment>().expect_no_offenses(indoc! {r#"
            class A; end
            class A; end
        "#});
    }

    #[test]
    fn does_not_flag_or_assignment() {
        // `X ||= :bar` is `OrAsgn`, not `Casgn`.
        test::<ConstantReassignment>().expect_no_offenses(indoc! {r#"
            X = :foo
            X ||= :bar
        "#});
    }

    #[test]
    fn does_not_flag_conditional_assignment() {
        // `X = :bar unless defined?(X)` has an `if` ancestor → not simple.
        test::<ConstantReassignment>().expect_no_offenses(indoc! {r#"
            X = :foo
            X = :bar unless defined?(X)
        "#});
    }

    #[test]
    fn does_not_flag_assignment_inside_method() {
        test::<ConstantReassignment>().expect_no_offenses(indoc! {r#"
            def f
              X = 1
              X = 2
            end
        "#});
    }

    #[test]
    fn does_not_flag_after_remove_const() {
        test::<ConstantReassignment>().expect_no_offenses(indoc! {r#"
            class A
              X = :foo
              remove_const :X
              X = :bar
            end
        "#});
    }

    #[test]
    fn does_not_flag_same_name_in_different_namespaces() {
        test::<ConstantReassignment>().expect_no_offenses(indoc! {r#"
            class A
              X = :foo
            end
            class B
              X = :bar
            end
        "#});
    }

    #[test]
    fn does_not_flag_definition_inside_conditional() {
        // `unconditional_definition?` fails → not registered, so the later
        // casgn is the first assignment.
        test::<ConstantReassignment>().expect_no_offenses(indoc! {r#"
            if cond
              class A; end
            end
            A = 1
        "#});
    }

    // --- murphy-as93: cbase-absolute path fidelity ---

    #[test]
    fn does_not_flag_cbase_absolute_in_class_vs_relative() {
        // RuboCop: `::X` qualifies to `"::X"` (cbase drops ancestor `A`),
        // while `X = 1` inside `class A` qualifies to `"::A::X"`. Different
        // keys → no reassignment. Murphy must not collide them.
        test::<ConstantReassignment>().expect_no_offenses(indoc! {r#"
            class A
              X = 1
              ::X = 2
            end
        "#});
    }

    #[test]
    fn flags_cbase_absolute_reassignment_at_top_level() {
        // `::X` and `X` at the top level both qualify to `"::X"`, so the
        // second assignment is a reassignment.
        test::<ConstantReassignment>().expect_offense(indoc! {r#"
            X = :foo
            ::X = :bar
            ^^^^^^^^^^ Constant `X` is already assigned in this namespace.
        "#});
    }

    #[test]
    fn flags_cbase_then_relative_reassignment_at_top_level() {
        test::<ConstantReassignment>().expect_offense(indoc! {r#"
            ::X = :foo
            X = :bar
            ^^^^^^^^ Constant `X` is already assigned in this namespace.
        "#});
    }

    #[test]
    fn flags_cbase_absolute_self_reassignment_in_nested_namespace() {
        // Two `::X` assignments inside `class A` both qualify to `"::X"`.
        test::<ConstantReassignment>().expect_offense(indoc! {r#"
            class A
              ::X = 1
              ::X = 2
              ^^^^^^^ Constant `X` is already assigned in this namespace.
            end
        "#});
    }

    #[test]
    fn flags_cbase_qualified_path_reassignment() {
        // `::Foo::Bar` qualifies to `"::Foo::Bar"` (namespace `::Foo` is a
        // const, so it is kept).
        test::<ConstantReassignment>().expect_offense(indoc! {r#"
            ::Foo::Bar = 1
            ::Foo::Bar = 2
            ^^^^^^^^^^^^^^ Constant `Foo::Bar` is already assigned in this namespace.
        "#});
    }

    #[test]
    fn flags_compound_const_path_reassignment() {
        // Compound (`A::B = …`) already qualifies to `"::A::B"`; the second
        // assignment collides. This pins that compound paths keep working.
        test::<ConstantReassignment>().expect_offense(indoc! {r#"
            A::B = 1
            A::B = 2
            ^^^^^^^^ Constant `A::B` is already assigned in this namespace.
        "#});
    }

    #[test]
    fn flags_cbase_class_definition_then_reassignment() {
        // `class ::A` defines `"::A"`; the later `A = 1` at top level also
        // qualifies to `"::A"`, so it is a reassignment.
        test::<ConstantReassignment>().expect_offense(indoc! {r#"
            class ::A; end
            A = 1
            ^^^^^ Constant `A` is already assigned in this namespace.
        "#});
    }

    #[test]
    fn does_not_flag_cbase_class_definition_inside_other_namespace() {
        // Inside `module M`, `class ::A` defines top-level `"::A"`, while a
        // relative `A = 1` inside `M` is `"::M::A"`. Different keys.
        test::<ConstantReassignment>().expect_no_offenses(indoc! {r#"
            module M
              class ::A; end
              A = 1
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(ConstantReassignment);
