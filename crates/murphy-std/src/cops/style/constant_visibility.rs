//! `Style/ConstantVisibility` — requires explicit visibility for module/class constants.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ConstantVisibility
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `casgn` inside a `class`/`module` body without a `public_constant`/
//!   `private_constant` declaration for that constant (matching `on_casgn` +
//!   `class_or_module_scope?` + `visibility_declaration?`).
//!
//!   `class_or_module_scope?` recurses through arbitrarily nested `begin`
//!   blocks: the node is in scope iff the first non-`begin` ancestor is a
//!   `class`/`module`. `visibility_declaration?` searches the casgn's
//!   *immediate* parent's child `send` nodes (matching RuboCop's
//!   `node.parent.each_child_node(:send)`); the declaration must be a sibling.
//!   The matcher `(send nil? {:public_constant :private_constant} $...)`
//!   requires no receiver — a `self.private_constant` receiver does NOT count.
//!   Both `sym` and `str` argument literals name a constant
//!   (`argument.type?(:sym, :str)`), and a leading splat of an array literal
//!   (`private_constant(*%i[A B])`) is flattened to its elements, mirroring
//!   `arguments.first.children.first.to_a if arguments.first&.splat_type?`.
//!
//!   `IgnoreModules: true` skips constructor assignments, mirroring RuboCop's
//!   `module?(node)` → `class_constructor?`: `Class`/`Module`/`Struct.new`,
//!   `Data.define`, and the block forms (`Struct.new do … end`), with the
//!   receiver allowed to be a top-level (`::`) constant.
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
        if !class_or_module_scope(node, cx) {
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

/// RuboCop's `class_or_module_scope?`: walk up through arbitrarily nested
/// `begin` blocks; the node is in class/module scope iff the first non-`begin`
/// ancestor is a `class`/`module`.
fn class_or_module_scope(node: NodeId, cx: &Cx<'_>) -> bool {
    let mut current = node;
    loop {
        let Some(parent_id) = cx.parent(current).get() else {
            return false;
        };
        match cx.kind(parent_id) {
            NodeKind::Class { .. } | NodeKind::Module { .. } => return true,
            NodeKind::Begin(_) => current = parent_id,
            _ => return false,
        }
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

/// RuboCop's `visibility_declaration?`: among the immediate parent's child
/// `send` nodes, find a receiverless `public_constant`/`private_constant` whose
/// `sym`/`str` arguments include `name`. The matcher is `(send nil? …)`, so a
/// `self.`-receiver call does NOT count.
fn has_visibility_declaration(name: &str, parent: NodeId, cx: &Cx<'_>) -> bool {
    for &child in cx.children(parent).iter() {
        let NodeKind::Send { receiver, method, args } = *cx.kind(child) else {
            continue;
        };
        let method_str = cx.symbol_str(method);
        if method_str != "public_constant" && method_str != "private_constant" {
            continue;
        }
        // RuboCop's pattern requires `nil?` — no receiver at all.
        if receiver != OptNodeId::NONE {
            continue;
        }
        let arg_list = cx.list(args);
        // RuboCop: `arguments = arguments.first.children.first.to_a if
        // arguments.first&.splat_type?` — when the first argument is a splat,
        // `arguments` is *replaced* by its flattened children. A splat of an
        // array literal (`private_constant(*%i[A B])`) flattens to that array's
        // elements; any other splat flattens to a list with no `sym`/`str`.
        if is_splat(arg_list.first().copied(), cx) {
            if splat_array_elems(arg_list[0], cx)
                .is_some_and(|elems| elems.iter().any(|&e| arg_names_constant(e, name, cx)))
            {
                return true;
            }
            continue;
        }
        if arg_list.iter().any(|&arg| arg_names_constant(arg, name, cx)) {
            return true;
        }
    }
    false
}

fn is_splat(arg: Option<NodeId>, cx: &Cx<'_>) -> bool {
    arg.is_some_and(|a| matches!(cx.kind(a), NodeKind::Splat(_)))
}

/// The element list of a splat-of-array-literal, e.g. the `[:A, :B]` in `*[:A, :B]`.
fn splat_array_elems<'a>(splat: NodeId, cx: &Cx<'a>) -> Option<&'a [NodeId]> {
    let NodeKind::Splat(inner) = cx.kind(splat) else {
        return None;
    };
    let inner_id = inner.get()?;
    match cx.kind(inner_id) {
        NodeKind::Array(elems) => Some(cx.list(*elems)),
        _ => None,
    }
}

/// `argument.value.to_sym if argument.type?(:sym, :str)` — a `sym`/`str`
/// literal whose value equals the constant `name`.
fn arg_names_constant(arg: NodeId, name: &str, cx: &Cx<'_>) -> bool {
    match cx.kind(arg) {
        NodeKind::Sym(s) => cx.symbol_str(*s) == name,
        NodeKind::Str(s) => cx.string_str(*s) == name,
        _ => false,
    }
}

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

    // RuboCop's `class_or_module_scope?` recurses through arbitrarily nested
    // `begin` blocks. A `casgn` wrapped in two `begin` levels inside a class is
    // still in class scope and must be flagged (murphy-4g1g).
    #[test]
    fn flags_constant_nested_in_begin() {
        test::<ConstantVisibility>().expect_offense(indoc! {"
            class Foo
              begin
                begin
                  BAR = 42
                  ^^^^^^^^ Explicitly make `BAR` public or private using either `#public_constant` or `#private_constant`.
                end
              end
            end
        "});
    }

    // The visibility declaration must be a sibling of the `casgn` (RuboCop
    // searches `node.parent.each_child_node(:send)`); the recursion only
    // establishes scope, it does not change where the declaration is searched.
    #[test]
    fn accepts_nested_constant_with_sibling_visibility() {
        test::<ConstantVisibility>().expect_no_offenses(
            "class Foo\n  begin\n    BAR = 42\n    private_constant :BAR\n  end\nend\n",
        );
    }

    // RuboCop's matcher is `(send nil? {:public_constant :private_constant} ...)`
    // — a `self.private_constant` receiver does NOT count as a visibility
    // declaration, so the constant is still flagged (murphy-4g1g).
    #[test]
    fn flags_self_receiver_visibility() {
        test::<ConstantVisibility>().expect_offense(indoc! {"
            class Foo
              BAR = 42
              ^^^^^^^^ Explicitly make `BAR` public or private using either `#public_constant` or `#private_constant`.
              self.private_constant :BAR
            end
        "});
    }

    // `private_constant` accepts string arguments too (the matcher allows
    // `argument.type?(:sym, :str)`).
    #[test]
    fn accepts_string_argument_visibility() {
        test::<ConstantVisibility>().expect_no_offenses(
            "class Foo\n  BAR = 42\n  private_constant 'BAR'\nend\n",
        );
    }

    // A leading splat of an array literal is flattened to its elements.
    #[test]
    fn accepts_splat_array_visibility() {
        test::<ConstantVisibility>().expect_no_offenses(
            "class Foo\n  BAR = 42\n  private_constant(*[:BAR])\nend\n",
        );
    }
}
murphy_plugin_api::submit_cop!(ConstantVisibility);
