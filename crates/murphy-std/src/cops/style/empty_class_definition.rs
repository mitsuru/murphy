//! `Style/EmptyClassDefinition` — enforces consistent style for empty class definitions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EmptyClassDefinition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle: class_keyword (default) and class_new supported.
//!   class_keyword direction: flags Class.new(constant) and Class.new.
//!   class_new direction: flags class Foo < Bar; end (v1 gap: not yet implemented).
//!   AllowedParentClasses is not yet wired.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

const MSG_CLASS_KEYWORD: &str = "Use the `class` keyword instead of `Class.new` to define an empty class.";

#[derive(Default)]
pub struct EmptyClassDefinition;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "class_keyword")]
    ClassKeyword,
    #[option(value = "class_new")]
    ClassNew,
}

#[derive(CopOptions)]
pub struct EmptyClassDefinitionOptions {
    #[option(name = "EnforcedStyle", 
        default = "class_keyword",
        description = "Enforced style for empty class definitions."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/EmptyClassDefinition",
    description = "Enforce consistent style for empty class definitions.",
    default_severity = "warning",
    default_enabled = true,
    options = EmptyClassDefinitionOptions
)]
impl EmptyClassDefinition {
    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<EmptyClassDefinitionOptions>();
        if opts.enforced_style != EnforcedStyle::ClassKeyword {
            return;
        }
        let NodeKind::Casgn { value, .. } = *cx.kind(node) else {
            return;
        };
        let Some(val_id) = value.get() else {
            return;
        };
        let val_id = unwrap_begin(val_id, cx);
        let NodeKind::Send { receiver, method, args } = *cx.kind(val_id) else {
            return;
        };
        let Some(recv_id) = receiver.get() else {
            return;
        };
        let NodeKind::Const { name, .. } = *cx.kind(recv_id) else {
            return;
        };
        if cx.symbol_str(name) != "Class" || cx.symbol_str(method) != "new" {
            return;
        }
        let arg_list = cx.list(args);
        if arg_list.len() > 1 {
            return;
        }
        if arg_list.len() == 1 {
            let first_arg = arg_list[0];
            if !matches!(cx.kind(first_arg), NodeKind::Const { .. }) {
                return;
            }
        }
        cx.emit_offense(cx.range(node), MSG_CLASS_KEYWORD, None);
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
    use super::{EmptyClassDefinition, EmptyClassDefinitionOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_class_new_assignment() {
        test::<EmptyClassDefinition>().expect_offense(indoc! {"
            FooError = Class.new(StandardError)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the `class` keyword instead of `Class.new` to define an empty class.
        "});
    }

    #[test]
    fn flags_parenthesized_class_new_assignment() {
        test::<EmptyClassDefinition>().expect_offense(indoc! {"
            FooError = (Class.new(StandardError))
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the `class` keyword instead of `Class.new` to define an empty class.
        "});
    }

    #[test]
    fn flags_class_new_without_arguments() {
        test::<EmptyClassDefinition>().expect_offense(indoc! {"
            FooError = Class.new
            ^^^^^^^^^^^^^^^^^^^^ Use the `class` keyword instead of `Class.new` to define an empty class.
        "});
    }

    #[test]
    fn accepts_class_keyword() {
        test::<EmptyClassDefinition>().expect_no_offenses(
            "class FooError < StandardError\nend\n",
        );
    }

    #[test]
    fn class_new_style_accepts_class_new() {
        test::<EmptyClassDefinition>()
            .with_options(&EmptyClassDefinitionOptions { enforced_style: EnforcedStyle::ClassNew })
            .expect_no_offenses("FooError = Class.new(StandardError)\n");
    }

    #[test]
    fn accepts_class_new_with_non_const_arg() {
        test::<EmptyClassDefinition>().expect_no_offenses(
            "FooError = Class.new('some_string')\n",
        );
    }
}
murphy_plugin_api::submit_cop!(EmptyClassDefinition);
