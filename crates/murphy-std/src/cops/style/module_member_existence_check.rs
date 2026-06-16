//! `Style/ModuleMemberExistenceCheck` — prefer predicate methods over inclusion checks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ModuleMemberExistenceCheck
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   instance_methods/include?: member? handled.
//!   Other method pairs (class_variables, etc.) not yet handled (v1 gap).
//!   csend (safe-navigation) variant is not handled.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

const MSG: &str = "Use `method_defined?` instead.";

#[derive(Default)]
pub struct ModuleMemberExistenceCheck;

#[cop(
    name = "Style/ModuleMemberExistenceCheck",
    description = "Use predicate methods instead of inclusion checks on Module methods.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions
)]
impl ModuleMemberExistenceCheck {
    #[on_node(kind = "send", methods = ["include?", "member?"])]
    fn check_include_member(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        let Some(recv_id) = receiver.get() else {
            return;
        };
        let recv_id = unwrap_begin(recv_id, cx);
        let NodeKind::Send { method, args: recv_args, .. } = *cx.kind(recv_id) else {
            return;
        };
        if cx.symbol_str(method) != "instance_methods" {
            return;
        }
        let recv_arg_list = cx.list(recv_args);
        if !recv_arg_list.is_empty() {
            return;
        }
        let arg_list = cx.list(args);
        if arg_list.is_empty() {
            return;
        }
        cx.emit_offense(cx.range(node), MSG, None);
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

#[cfg(test)]
mod tests {
    use super::ModuleMemberExistenceCheck;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_instance_methods_include() {
        test::<ModuleMemberExistenceCheck>().expect_offense(indoc! {"
            Array.instance_methods.include?(:size)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `method_defined?` instead.
        "});
    }

    #[test]
    fn flags_parenthesized_instance_methods_include() {
        test::<ModuleMemberExistenceCheck>().expect_offense(indoc! {"
            (Array.instance_methods).include?(:size)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `method_defined?` instead.
        "});
    }

    #[test]
    fn flags_instance_methods_member() {
        test::<ModuleMemberExistenceCheck>().expect_offense(indoc! {"
            Array.instance_methods.member?(:size)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `method_defined?` instead.
        "});
    }

    #[test]
    fn accepts_plain_instance_methods() {
        test::<ModuleMemberExistenceCheck>().expect_no_offenses(
            "Array.instance_methods\n",
        );
    }

    #[test]
    fn accepts_method_defined() {
        test::<ModuleMemberExistenceCheck>().expect_no_offenses(
            "Array.method_defined?(:size)\n",
        );
    }

    #[test]
    fn accepts_instance_methods_with_arg() {
        test::<ModuleMemberExistenceCheck>().expect_no_offenses(
            "Array.instance_methods(false).include?(:size)\n",
        );
    }
}
murphy_plugin_api::submit_cop!(ModuleMemberExistenceCheck);
