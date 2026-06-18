//! `Style/ConstantVisibility` — requires explicit visibility for module/class constants.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ConstantVisibility
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-4g1g]
//! notes: >
//!   Flags `casgn` inside a `class`/`module` body without a `public_constant`/
//!   `private_constant` declaration for that constant (matching `on_casgn` +
//!   `class_or_module_scope?` + `visibility_declaration?`).
//!
//!   `IgnoreModules: true` skips constructor assignments, mirroring RuboCop's
//!   `module?(node)` → `class_constructor?`: `Class`/`Module`/`Struct.new`,
//!   `Data.define`, and the block forms (`Struct.new do … end`), with the
//!   receiver allowed to be a top-level (`::`) constant.
//!
//!   Gaps vs upstream:
//!   - `class_or_module_scope?` is checked one `begin` level deep (direct body
//!     or a single wrapping `begin`); RuboCop recurses through arbitrarily
//!     nested `begin` blocks — murphy-4g1g.
//!   - Murphy additionally accepts a `self.private_constant`/`self.public_constant`
//!     receiver; RuboCop's matcher requires no receiver (`nil?`) and would flag
//!     it. This is a deliberate Murphy leniency (an explicit `self.` receiver on
//!     the private `private_constant` method would itself raise at runtime).
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

#[derive(CopOptions)]
pub struct ConstantVisibilityOptions {
    #[option(name = "IgnoreModules", default = false, description = "Ignore modules (Struct.new, etc.)")]
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

/// RuboCop's `module?(node)` → `node.expression.class_constructor?`, matching
/// `Class`/`Module`/`Struct.new`, `Data.define`, and the block forms of each
/// (`Struct.new do … end`), with the receiver allowed to be a top-level
/// (`::`) constant. The send is resolved through `cx.block_call` so the
/// block-wrapped constructor is reached.
fn is_module_assignment(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Casgn { value, .. } = *cx.kind(node) else {
        return false;
    };
    let Some(val_id) = value.get() else {
        return false;
    };
    let val_id = unwrap_begin(val_id, cx);
    // A block-form constructor (`Struct.new do … end`) wraps the send; resolve
    // to the underlying call. A bare send resolves to itself.
    let call_id = cx.block_call(val_id).get().unwrap_or(val_id);
    let Some(recv_id) = cx.call_receiver(call_id).get() else {
        return false;
    };
    let recv_id = unwrap_begin(recv_id, cx);
    match cx.method_name(call_id) {
        Some("new") => cx.is_global_const(recv_id, "Class")
            || cx.is_global_const(recv_id, "Module")
            || cx.is_global_const(recv_id, "Struct"),
        Some("define") => cx.is_global_const(recv_id, "Data"),
        _ => false,
    }
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

    // `class_constructor?` also covers `Data.define`, block-form constructors,
    // and top-level (`::`) constants — all accepted under `IgnoreModules: true`.
    #[test]
    fn ignore_modules_enabled_accepts_data_define() {
        test::<ConstantVisibility>()
            .with_options(&ConstantVisibilityOptions { ignore_modules: true })
            .expect_no_offenses("class Foo\n  MyData = Data.define(:x)\nend\n");
    }

    #[test]
    fn ignore_modules_enabled_accepts_block_form_constructor() {
        test::<ConstantVisibility>()
            .with_options(&ConstantVisibilityOptions { ignore_modules: true })
            .expect_no_offenses("class Foo\n  S = Struct.new(:x) do\n    def y; end\n  end\nend\n");
    }

    #[test]
    fn ignore_modules_enabled_accepts_toplevel_constructor() {
        test::<ConstantVisibility>()
            .with_options(&ConstantVisibilityOptions { ignore_modules: true })
            .expect_no_offenses("class Foo\n  C = ::Class.new\nend\n");
    }
}
murphy_plugin_api::submit_cop!(ConstantVisibility);
