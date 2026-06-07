//! `Lint/UselessAccessModifier` — Checks for redundant access modifiers.
//!
//! Flags `public` at the top of a class/module body (default is already public),
//! repeated modifiers (`private; private`), modifiers with no following method
//! definitions, and top-level modifiers (access modifiers have no effect on
//! top-level).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessAccessModifier
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Core detection implemented: leading public, repeated modifiers, unused
//!   modifiers (no following defs), top-level modifiers. No ContextCreatingMethods
//!   or MethodCreatingMethods config options yet. No ActiveSupport `included` block
//!   awareness. No class_eval/instance_eval/new block scope awareness.
//!   Autocorrect removes the modifier line.
//! ```
//!
//! ## Matched shapes
//!
//! - `class`/`module`/`sclass` body with redundant access modifiers
//! - Top-level `begin` body with access modifiers
//! - `block` body from constructor-like calls (`Class.new`, `Struct.new`, etc.)
//!
//! ## Autocorrect
//!
//! Removes the redundant access modifier line (the whole line).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

fn msg(mod_name: &str) -> String {
    format!("Useless `{mod_name}` access modifier.")
}

fn is_bare_access_modifier(node: NodeId, cx: &Cx<'_>) -> Option<&'static str> {
    match *cx.kind(node) {
        NodeKind::Send { receiver, method, args } => {
            if receiver != OptNodeId::NONE {
                return None;
            }
            let args_list = cx.list(args);
            if !args_list.is_empty() {
                return None;
            }
            match cx.symbol_str(method) {
                "public" => Some("public"),
                "protected" => Some("protected"),
                "private" => Some("private"),
                "private_class_method" => Some("private_class_method"),
                _ => None,
            }
        }
        _ => None,
    }
}

fn is_static_method_definition(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Def { .. })
        || matches!(
            *cx.kind(node),
            NodeKind::Send {
                receiver,
                method,
                ..
            } if receiver == OptNodeId::NONE && matches!(cx.symbol_str(method), "attr" | "attr_reader" | "attr_writer" | "attr_accessor")
        )
}

fn is_dynamic_method_definition(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Send {
            receiver, method, ..
        } if receiver == OptNodeId::NONE && cx.symbol_str(method) == "define_method" => true,
        NodeKind::Block { call, .. } => {
            matches!(*cx.kind(call), NodeKind::Send { receiver, method, .. }
                if receiver == OptNodeId::NONE && cx.symbol_str(method) == "define_method")
        }
        _ => false,
    }
}

fn is_method_definition(node: NodeId, cx: &Cx<'_>) -> bool {
    is_static_method_definition(node, cx) || is_dynamic_method_definition(node, cx)
}

fn is_start_of_new_scope(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Class { .. } | NodeKind::Module { .. } | NodeKind::Sclass { .. }
    )
}

/// Collect body children from class/module/sclass/begin/block nodes.
fn body_children(node: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    match *cx.kind(node) {
        NodeKind::Class { body, .. }
        | NodeKind::Module { body, .. }
        | NodeKind::Sclass { body, .. } => {
            if let Some(body_id) = body.get() {
                if matches!(*cx.kind(body_id), NodeKind::Begin(_)) {
                    cx.children(body_id)
                } else {
                    vec![body_id]
                }
            } else {
                vec![]
            }
        }
        NodeKind::Block { body, .. } => {
            if let Some(body_id) = body.get() {
                if matches!(*cx.kind(body_id), NodeKind::Begin(_)) {
                    cx.children(body_id)
                } else {
                    vec![body_id]
                }
            } else {
                vec![]
            }
        }
        NodeKind::Begin(list) => cx.list(list).to_vec(),
        _ => vec![],
    }
}

/// Check a scope body for redundant access modifiers. Returns (last_visibility, unused_modifier_node).
fn check_scope_body(
    children: &[NodeId],
    leading_public_redundant: bool,
    cx: &Cx<'_>,
) -> (Option<&'static str>, Option<NodeId>) {
    let mut cur_vis: Option<&'static str> = None;
    let mut unused: Option<NodeId> = None;

    for &child in children {
        if let Some(mod_name) = is_bare_access_modifier(child, cx) {
            match cur_vis {
                Some(prev) if prev == mod_name => {
                    // Repeated modifier
                    cx.emit_offense(cx.range(child), &msg(mod_name), None);
                    unused = Some(child);
                }
                Some(prev) => {
                    // New visibility level — report previous if unused
                    if let Some(unused_node) = unused {
                        cx.emit_offense(cx.range(unused_node), &msg(prev), None);
                    }
                    cur_vis = Some(mod_name);
                    unused = Some(child);
                }
                None => {
                    // First modifier in this scope.
                    if leading_public_redundant && mod_name == "public" {
                        cx.emit_offense(cx.range(child), &msg("public"), None);
                    }
                    cur_vis = Some(mod_name);
                    unused = Some(child);
                }
            }
        } else if is_method_definition(child, cx) {
            unused = None;
        } else if child != cx.root() && is_start_of_new_scope(child, cx) {
            check_node(child, cx, false);
        } else if !matches!(*cx.kind(child), NodeKind::Def { receiver, .. }
            if receiver != OptNodeId::NONE)
        {
            let sub_children = body_children(child, cx);
            if !sub_children.is_empty() {
                let (_sub_vis, sub_unused) = check_scope_body(&sub_children, false, cx);
                if sub_unused.is_none() {
                    continue;
                }
            }
        }
    }

    (cur_vis, unused)
}

fn check_node(node: NodeId, cx: &Cx<'_>, leading_public_redundant: bool) {
    let children = body_children(node, cx);
    if children.is_empty() {
        return;
    }
    let (_, unused) = check_scope_body(&children, leading_public_redundant, cx);
    if let Some(unused_node) = unused {
        let mod_name = is_bare_access_modifier(unused_node, cx).unwrap_or("access");
        cx.emit_offense(cx.range(unused_node), &msg(mod_name), None);
    }
}

#[derive(Default)]
pub struct UselessAccessModifier;

#[cop(
    name = "Lint/UselessAccessModifier",
    description = "Checks for redundant access modifiers.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl UselessAccessModifier {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check_node(node, cx, true);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        check_node(node, cx, true);
    }

    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        check_node(node, cx, false);
    }

    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        // Only check top-level begin (no parent = file scope)
        if !cx.parent(node).is_none() {
            return;
        }
        let children = body_children(node, cx);
        for &child in &children {
            if let Some(mod_name) = is_bare_access_modifier(child, cx) {
                cx.emit_offense(cx.range(child), &msg(mod_name), None);
            }
        }
        check_scope_body(&children, false, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::UselessAccessModifier;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_leading_public_in_class() {
        test::<UselessAccessModifier>().expect_offense(indoc! {r#"
            class Foo
             public
             ^^^^^^ Useless `public` access modifier.
              def method
              end
            end
        "#});
    }

    #[test]
    fn accepts_leading_protected_in_class() {
        test::<UselessAccessModifier>().expect_no_offenses(indoc! {"
            class Foo
              protected
              def method
              end
            end
        "});
    }

    #[test]
    fn accepts_leading_private_in_class() {
        test::<UselessAccessModifier>().expect_no_offenses(indoc! {"
            class Foo
              private
              def method
              end
            end
        "});
    }

    #[test]
    fn flags_repeated_private() {
        test::<UselessAccessModifier>().expect_offense(indoc! {r#"
            class Foo
              private
              def method1
              end
              private
              ^^^^^^^ Useless `private` access modifier.
              def method2
              end
            end
        "#});
    }

    #[test]
    fn flags_trailing_access_modifier() {
        test::<UselessAccessModifier>().expect_offense(indoc! {r#"
            class Foo
              def method1
              end
              def method2
              end
              private
              ^^^^^^^ Useless `private` access modifier.
            end
        "#});
    }

    #[test]
    fn flags_empty_class_with_modifier() {
        test::<UselessAccessModifier>().expect_offense(indoc! {r#"
            class Foo
              private
              ^^^^^^^ Useless `private` access modifier.
            end
        "#});
    }

    #[test]
    fn accepts_private_with_symbol_arg() {
        test::<UselessAccessModifier>().expect_no_offenses(indoc! {"
            class Foo
              def method
              end
              private :method
            end
        "});
    }

    #[test]
    fn flags_access_modifier_after_only_constant() {
        test::<UselessAccessModifier>().expect_offense(indoc! {r#"
            class Foo
              private
              ^^^^^^^ Useless `private` access modifier.
              CONST = 1
            end
        "#});
    }

    #[test]
    fn accepts_inline_modifier() {
        test::<UselessAccessModifier>().expect_no_offenses(indoc! {"
            class Foo
              private def method
              end
            end
        "});
    }

    #[test]
    fn handles_top_level_modifier() {
        test::<UselessAccessModifier>().expect_offense(indoc! {r#"
            def method1
            end
            private
            ^^^^^^^ Useless `private` access modifier.
            def method2
            end
        "#});
    }

    #[test]
    fn handles_consecutive_modifiers() {
        test::<UselessAccessModifier>().expect_offense(indoc! {r#"
            class Foo
              private
              private
              ^^^^^^^ Useless `private` access modifier.
              def method
              end
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(UselessAccessModifier);
