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

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Symbol, cop};

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
        let NodeKind::Casgn { name, .. } = *cx.kind(node) else {
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

        cx.emit_offense(
            cx.range(node),
            "Useless `private` access modifier for constant scope.",
            None,
        );
    }
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
}
murphy_plugin_api::submit_cop!(UselessConstantScoping);
