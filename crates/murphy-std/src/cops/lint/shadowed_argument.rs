//! `Lint/ShadowedArgument` — detect arguments reassigned before first use.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ShadowedArgument
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-4947]
//! notes: >
//!   Covers method and block arguments, block-local exclusion via VarSemanticModel,
//!   RHS self-reference guards, unused argument guards, basic conditional/block
//!   location degradation to the argument declaration, splat/multiple assignment
//!   shapes exposed by VarSemanticModel, and IgnoreImplicitReferences for zsuper
//!   and binding. Full RuboCop VariableForce parity is not available in v1, so
//!   complex control-flow dominance and implicit-reference edge cases may differ.
//! ```
//!
//! ## Matched shapes
//!
//! - `def m(foo); foo = 42; puts foo; end`
//! - `do_something { |foo| foo = 42; puts foo }`
//! - assignments inside conditional/block/rescue ancestors report the argument declaration range
//!
//! ## Autocorrect
//!
//! None.

use murphy_plugin_api::{cop, CopOptions, Cx, NodeId, NodeKind};

const MSG_PREFIX: &str = "Argument `";
const MSG_SUFFIX: &str = "` was shadowed by a local variable before it was used.";

#[derive(Default)]
pub struct ShadowedArgument;

#[derive(CopOptions)]
pub struct ShadowedArgumentOptions {
    #[option(
        name = "IgnoreImplicitReferences",
        default = false,
        description = "Ignore implicit argument references from zero-arity super and binding."
    )]
    pub ignore_implicit_references: bool,
}

#[cop(
    name = "Lint/ShadowedArgument",
    description = "Avoid reassigning arguments before they were used.",
    default_severity = "warning",
    default_enabled = true,
    options = ShadowedArgumentOptions,
)]
impl ShadowedArgument {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ShadowedArgumentOptions>();
        let Some(model) = cx.var_model() else { return };

        for (scope_id, scope) in model.scopes() {
            for variable in scope.variables().iter().filter(|var| var.is_argument) {
                let Some(assignment) = first_shadowing_assignment(scope_id, variable, cx, opts.ignore_implicit_references) else {
                    continue;
                };
                let name = cx.symbol_str(variable.name);
                let message = format!("{MSG_PREFIX}{name}{MSG_SUFFIX}");
                let range = if conditional_assignment(assignment, scope_id, cx) {
                    cx.node(variable.declaration_node).loc.name
                } else {
                    cx.range(assignment)
                };
                cx.emit_offense(range, &message, None);
            }
        }
    }
}

fn first_shadowing_assignment(
    scope_id: NodeId,
    variable: &murphy_plugin_api::var_semantic_model::Variable,
    cx: &Cx<'_>,
    ignore_implicit_references: bool,
) -> Option<NodeId> {
    if variable.references.is_empty() {
        return None;
    }
    for assignment in &variable.assignments {
        let assignment_node = assignment.node_id;
        let assignment_start = cx.range(assignment_node).start;
        if assignment_uses_var(assignment_node, variable.name, cx) {
            continue;
        }
        if variable
            .references
            .iter()
            .any(|reference| reference.pos <= assignment_start)
        {
            continue;
        }
        if ignore_implicit_references && implicit_reference_after(scope_id, assignment_start, cx) {
            continue;
        }
        return Some(assignment_node);
    }
    None
}

fn assignment_uses_var(node: NodeId, name: murphy_plugin_api::Symbol, cx: &Cx<'_>) -> bool {
    cx.descendants(node).into_iter().any(|desc| {
        desc != node && matches!(*cx.kind(desc), NodeKind::Lvar(symbol) if symbol == name)
    })
}

fn implicit_reference_after(scope_id: NodeId, assignment_start: u32, cx: &Cx<'_>) -> bool {
    cx.descendants(scope_id).into_iter().any(|desc| {
        cx.range(desc).start > assignment_start
            && (matches!(*cx.kind(desc), NodeKind::Zsuper)
                || matches!(*cx.kind(desc), NodeKind::Send { receiver, method, .. } if receiver.get().is_none() && cx.symbol_str(method) == "binding"))
    })
}

fn conditional_assignment(assignment: NodeId, scope_id: NodeId, cx: &Cx<'_>) -> bool {
    let mut current = assignment;
    while let Some(parent) = cx.parent(current).get() {
        if parent == scope_id {
            return false;
        }
        if matches!(
            *cx.kind(parent),
            NodeKind::If { .. }
                | NodeKind::Block { .. }
                | NodeKind::Rescue { .. }
                | NodeKind::Resbody { .. }
                | NodeKind::While { .. }
                | NodeKind::Until { .. }
        ) {
            return true;
        }
        current = parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{ShadowedArgument, ShadowedArgumentOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_method_argument_reassigned_before_use() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
              foo = 42
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              puts foo
            end
        "#});
    }

    #[test]
    fn flags_block_argument_reassigned_before_use() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            do_something do |foo|
              foo = 42
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_argument_used_or_unused_before_assignment() {
        test::<ShadowedArgument>()
            .expect_no_offenses(indoc! {r#"
                def do_something(foo)
                  foo = foo + 42
                  puts foo
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                def do_something(foo)
                  puts 'done something'
                end
            "#});
    }

    #[test]
    fn reports_declaration_when_shadowing_assignment_is_conditional() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
                             ^^^ Argument `foo` was shadowed by a local variable before it was used.
              if bar
                foo = 43
              end
              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn ignore_implicit_references_accepts_zsuper_and_binding() {
        test::<ShadowedArgument>()
            .with_options(&ShadowedArgumentOptions {
                ignore_implicit_references: true,
            })
            .expect_no_offenses(indoc! {r#"
                def do_something(foo)
                  foo = 42
                  super
                end
            "#})
            .expect_no_offenses(indoc! {r#"
                def do_something(foo)
                  foo = 42
                  binding
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(ShadowedArgument);
