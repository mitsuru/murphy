//! `Lint/ShadowingOuterLocalVariable` — detect block arguments that shadow
//! local variables from an outer scope.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ShadowingOuterLocalVariable
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   This implementation is position-insensitive: an outer variable assigned
//!   *after* the block (e.g. `[1].each do |x|; end; x = 1`) is still flagged,
//!   whereas RuboCop considers assignment order. This is a known v1 limitation.
//!   `_`-prefixed arguments are not flagged (underscore-prefix exclusion is
//!   handled by VarSemanticModel which never records them).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop};

#[derive(Default)]
pub struct ShadowingOuterLocalVariable;

#[cop(
    name = "Lint/ShadowingOuterLocalVariable",
    description = "Detect block arguments that shadow local variables from an outer scope.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ShadowingOuterLocalVariable {
    #[on_node(kind = "block")]
    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(model) = cx.var_model() else { return };
        let Some(scope) = model.scope(node) else {
            return;
        };

        for var in scope.variables().iter().filter(|v| v.is_argument) {
            // Walk up the scope chain to find outer scopes.
            let mut outer = scope.parent_scope(model);
            while let Some(s) = outer {
                if s.variables().iter().any(|v| v.name == var.name) {
                    let name_str = cx.symbol_str(var.name);
                    let range = cx.node(var.declaration_node).loc.name;
                    cx.emit_offense(
                        range,
                        &format!("Shadowing outer local variable - `{name_str}`."),
                        None,
                    );
                    break;
                }
                outer = s.parent_scope(model);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ShadowingOuterLocalVariable;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_block_arg_shadowing_outer_local() {
        test::<ShadowingOuterLocalVariable>().expect_offense(indoc! {r#"
            x = 1
            [1].each do |x|
                         ^ Shadowing outer local variable - `x`.
              puts x
            end
        "#});
    }

    #[test]
    fn no_offense_when_no_outer_variable() {
        test::<ShadowingOuterLocalVariable>().expect_no_offenses(indoc! {r#"
            [1].each do |y|
              puts y
            end
        "#});
    }

    #[test]
    fn no_offense_for_underscore_arg() {
        test::<ShadowingOuterLocalVariable>().expect_no_offenses(indoc! {r#"
            x = 1
            [1].each do |_x|
              puts _x
            end
        "#});
    }

    #[test]
    fn flags_nested_shadow() {
        // Variable defined in outer def shadows inner block arg
        test::<ShadowingOuterLocalVariable>().expect_offense(indoc! {r#"
            def foo
              x = 1
              [1].each do |x|
                           ^ Shadowing outer local variable - `x`.
                puts x
              end
            end
        "#});
    }
}
