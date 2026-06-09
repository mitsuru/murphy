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
    #[on_node(kind = "send", methods = ["instance_methods"])]
    fn check_instance_methods(&self, node: NodeId, cx: &Cx<'_>) {
        let parent = cx.parent(node);
        let Some(parent_id) = parent.get() else {
            return;
        };
        let NodeKind::Send { method, args: parent_args, .. } = *cx.kind(parent_id) else {
            return;
        };
        let parent_method = cx.symbol_str(method);
        if parent_method != "include?" && parent_method != "member?" {
            return;
        }
        let parent_arg_list = cx.list(parent_args);
        if parent_arg_list.is_empty() {
            return;
        }
        cx.emit_offense(cx.range(node), MSG, None);
    }
}

#[cfg(test)]
mod tests {
    use super::ModuleMemberExistenceCheck;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_instance_methods_include() {
        test::<ModuleMemberExistenceCheck>().expect_offense(indoc! {"
            Array.instance_methods.include?(:size)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `method_defined?` instead.
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
}
murphy_plugin_api::submit_cop!(ModuleMemberExistenceCheck);
