//! `Style/ConstantVisibility` — requires explicit visibility for module/class constants.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ConstantVisibility
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ignores modules when IgnoreModules: true (default false).
//!   Only handles direct casgn inside class/module body.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

#[derive(CopOptions)]
pub struct ConstantVisibilityOptions {
    #[option(default = false, description = "Ignore modules (Struct.new, etc.)")]
    pub ignore_modules: bool,
}

#[derive(Default)]
pub struct ConstantVisibility;

#[cop(
    name = "Style/ConstantVisibility",
    description = "Explicitly declare constant visibility.",
    default_severity = "warning",
    default_enabled = true,
    options = ConstantVisibilityOptions
)]
impl ConstantVisibility {
    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        let parent = cx.parent(node);
        let Some(parent_id) = parent.get() else {
            return;
        };
        let in_class_or_module = match cx.kind(parent_id) {
            NodeKind::Class { .. } | NodeKind::Module { .. } => true,
            NodeKind::Begin(_) => {
                cx.parent(parent_id).get().is_some_and(|grandparent| {
                    matches!(cx.kind(grandparent), NodeKind::Class { .. } | NodeKind::Module { .. })
                })
            }
            _ => false,
        };
        if !in_class_or_module {
            return;
        }
        let opts = cx.options_or_default::<ConstantVisibilityOptions>();
        if opts.ignore_modules && is_module_assignment(node, cx) {
            return;
        }
        let NodeKind::Casgn { name, .. } = *cx.kind(node) else {
            return;
        };
        let name_str = cx.symbol_str(name);
        if has_visibility_declaration(name_str, parent_id, cx) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            &format!(
                "Explicitly make `{}` public or private using either \
                 `#public_constant` or `#private_constant`.",
                name_str
            ),
            None,
        );
    }
}

fn is_module_assignment(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Casgn { value, .. } = *cx.kind(node) else {
        return false;
    };
    let Some(val_id) = value.get() else {
        return false;
    };
    let val_id = unwrap_begin(val_id, cx);
    let NodeKind::Send { receiver, method, .. } = *cx.kind(val_id) else {
        return false;
    };
    if cx.symbol_str(method) != "new" {
        return false;
    }
    let Some(recv_id) = receiver.get() else {
        return false;
    };
    let recv_id = unwrap_begin(recv_id, cx);
    let NodeKind::Const { name, .. } = *cx.kind(recv_id) else {
        return false;
    };
    matches!(cx.symbol_str(name), "Struct" | "Class" | "Module")
}

fn unwrap_begin(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    while let NodeKind::Begin(children) = cx.kind(node) {
        let child_list = cx.list(*children);
        if child_list.len() != 1 {
            break;
        }
        node = child_list[0];
    }
    node
}

fn has_visibility_declaration(name: &str, parent: NodeId, cx: &Cx<'_>) -> bool {
    for &child in cx.children(parent).iter() {
        let NodeKind::Send { receiver, method, args } = *cx.kind(child) else {
            continue;
        };
        let method_str = cx.symbol_str(method);
        if method_str != "public_constant" && method_str != "private_constant" {
            continue;
        }
        if receiver != OptNodeId::NONE
            && !receiver.get().is_some_and(|r| matches!(cx.kind(r), NodeKind::SelfExpr))
        {
            continue;
        }
        let arg_list = cx.list(args);
        for &arg in arg_list.iter() {
            let sym_name = match cx.kind(arg) {
                NodeKind::Sym(s) => cx.symbol_str(*s),
                _ => continue,
            };
            if sym_name == name {
                return true;
            }
        }
    }
    false
}

const _: () = {
    let _ = std::mem::size_of::<OptNodeId>();
};

#[cfg(test)]
mod tests {
    use super::{ConstantVisibility, ConstantVisibilityOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_constant_without_visibility() {
        test::<ConstantVisibility>().expect_offense(indoc! {"
            class Foo
              BAR = 42
              ^^^^^^^^ Explicitly make `BAR` public or private using either `#public_constant` or `#private_constant`.
            end
        "});
    }

    #[test]
    fn accepts_constant_with_visibility() {
        test::<ConstantVisibility>().expect_no_offenses(
            "class Foo\n  BAR = 42\n  private_constant :BAR\nend\n",
        );
    }

    #[test]
    fn accepts_outside_class_scope() {
        test::<ConstantVisibility>().expect_no_offenses("BAR = 42\n");
    }

    #[test]
    fn ignore_modules_enabled_accepts_module() {
        test::<ConstantVisibility>()
            .with_options(&ConstantVisibilityOptions { ignore_modules: true })
            .expect_no_offenses("class Foo\n  MyStruct = Struct.new(:x)\nend\n");
    }

    #[test]
    fn ignore_modules_enabled_still_flags_regular_send_assignment() {
        test::<ConstantVisibility>()
            .with_options(&ConstantVisibilityOptions { ignore_modules: true })
            .expect_offense(indoc! {"
                class Foo
                  BAR = build_value
                  ^^^^^^^^^^^^^^^^^ Explicitly make `BAR` public or private using either `#public_constant` or `#private_constant`.
                end
            "});
    }
}
murphy_plugin_api::submit_cop!(ConstantVisibility);
