//! `Lint/MissingSuper` — require `super` in constructors and lifecycle callbacks.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/MissingSuper
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's constructor checks for class inheritance and Class.new
//!   blocks, plus class/method lifecycle callbacks. `AllowedParentClasses` is
//!   exported and read through current `Cx` options.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

use crate::cops::util::unwrap_parenthesized;

const CONSTRUCTOR_MSG: &str = "Call `super` to initialize state of the parent class.";
const CALLBACK_MSG: &str = "Call `super` to invoke callback defined in the parent class.";
const STATELESS_CLASSES: &[&str] = &["BasicObject", "Object"];
const CALLBACKS: &[&str] = &[
    "inherited",
    "method_added",
    "method_removed",
    "method_undefined",
    "singleton_method_added",
    "singleton_method_removed",
    "singleton_method_undefined",
];

#[derive(Default)]
pub struct MissingSuper;

#[derive(CopOptions)]
pub struct Options {
    #[option(default = [], description = "Parent classes allowed without super.")]
    pub allowed_parent_classes: Vec<String>,
}

#[cop(
    name = "Lint/MissingSuper",
    description = "Require super in constructors and lifecycle callbacks.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl MissingSuper {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_def_like(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check_def_like(node, cx);
    }
}

fn check_def_like(node: NodeId, cx: &Cx<'_>) {
    if contains_super(node, cx) {
        return;
    }
    let Some(name) = cx.method_name(node) else { return; };
    if name == "initialize" && inside_class_with_stateful_parent(node, cx) {
        cx.emit_offense(first_line_range(node, cx), CONSTRUCTOR_MSG, None);
    } else if CALLBACKS.contains(&name) && inside_class_module_or_sclass(node, cx) {
        cx.emit_offense(first_line_range(node, cx), CALLBACK_MSG, None);
    }
}

fn contains_super(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.descendants(node)
        .iter()
        .any(|&id| matches!(*cx.kind(id), NodeKind::Super(_) | NodeKind::Zsuper))
}

fn inside_class_with_stateful_parent(node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        if cx.is_any_block_type(ancestor)
            && let Some(parent_class) = class_new_parent(ancestor, cx)
        {
            return !allowed_class(parent_class, cx);
        }
        if let NodeKind::Class { superclass, .. } = *cx.kind(ancestor) {
            return superclass.get().is_some_and(|id| !allowed_class(id, cx));
        }
        if matches!(*cx.kind(ancestor), NodeKind::Module { .. }) {
            return false;
        }
    }
    false
}

fn class_new_parent(block: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let call = match *cx.kind(block) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => send,
        _ => return None,
    };
    let NodeKind::Send { receiver, method, args } = *cx.kind(call) else {
        return None;
    };
    if cx.symbol_str(method) != "new" {
        return None;
    }
    let receiver = unwrap_parenthesized(receiver.get()?, cx);
    if !matches!(cx.const_name(receiver).as_deref(), Some("Class")) {
        return None;
    }
    cx.list(args).first().copied()
}

fn allowed_class(id: NodeId, cx: &Cx<'_>) -> bool {
    let id = unwrap_parenthesized(id, cx);
    let Some(name) = cx.const_name(id) else { return false; };
    if STATELESS_CLASSES.contains(&name.as_str()) {
        return true;
    }
    cx.options_or_default::<Options>()
        .allowed_parent_classes
        .iter()
        .any(|allowed| allowed == &name)
}

fn inside_class_module_or_sclass(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(node).any(|ancestor| {
        matches!(
            *cx.kind(ancestor),
            NodeKind::Class { .. } | NodeKind::Module { .. } | NodeKind::Sclass { .. }
        )
    })
}

fn first_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let range = cx.range(node);
    let start = range.start as usize;
    let end = cx
        .source()
        .as_bytes()
        .get(start..)
        .and_then(|suffix| suffix.iter().position(|&b| b == b'\n'))
        .map_or(range.end as usize, |idx| start + idx);
    Range {
        start: range.start,
        end: end as u32,
    }
}

murphy_plugin_api::submit_cop!(MissingSuper);

#[cfg(test)]
mod tests {
    use super::MissingSuper;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_initialize_without_super_in_stateful_subclass() {
        test::<MissingSuper>().expect_offense(indoc! {r#"
            class Child < Parent
              def initialize
              ^^^^^^^^^^^^^^ Call `super` to initialize state of the parent class.
              end
            end
        "#});
    }

    #[test]
    fn accepts_initialize_with_super() {
        test::<MissingSuper>().expect_no_offenses(indoc! {r#"
            class Child < Parent
              def initialize
                super
              end
            end
        "#});
    }

    #[test]
    fn accepts_stateless_parent_class() {
        test::<MissingSuper>().expect_no_offenses(indoc! {r#"
            class Child < Object
              def initialize
              end
            end
        "#});
    }

    #[test]
    fn flags_class_new_initialize_without_super() {
        test::<MissingSuper>().expect_offense(indoc! {r#"
            Class.new(Parent) do
              def initialize
              ^^^^^^^^^^^^^^ Call `super` to initialize state of the parent class.
              end
            end
        "#});
    }

    #[test]
    fn flags_class_new_with_parenthesized_receiver() {
        test::<MissingSuper>().expect_offense(indoc! {r#"
            (Class).new(Parent) do
              def initialize
              ^^^^^^^^^^^^^^ Call `super` to initialize state of the parent class.
              end
            end
        "#});
    }

    #[test]
    fn accepts_parenthesized_allowed_parent_class() {
        test::<MissingSuper>().expect_no_offenses(indoc! {r#"
            class Child < (Object)
              def initialize
              end
            end
        "#});
    }

    #[test]
    fn accepts_class_new_with_parenthesized_allowed_parent_class() {
        test::<MissingSuper>().expect_no_offenses(indoc! {r#"
            Class.new((Object)) do
              def initialize
              end
            end
        "#});
    }

    #[test]
    fn flags_lifecycle_callback_without_super() {
        test::<MissingSuper>().expect_offense(indoc! {r#"
            class Foo
              def self.inherited(base)
              ^^^^^^^^^^^^^^^^^^^^^^^^ Call `super` to invoke callback defined in the parent class.
              end
            end
        "#});
    }
}
