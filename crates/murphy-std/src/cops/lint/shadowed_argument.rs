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
//!   Ports RuboCop's `assignment_without_argument_usage`/`shadowing_assignment`:
//!   the shadowing point is the first *unconditional* assignment that does not
//!   read the argument on its RHS, with conditional (`if`/`while`/`until`/`case`/
//!   `case_match`/`block`/`rescue`) and shorthand (`op=`/`||=`/`&&=`) assignments
//!   degrading the reported location to the argument declaration rather than
//!   emitting. Covers method and block arguments, block-local exclusion via
//!   VarSemanticModel, `meta_assignment_node` RHS self-reference detection for
//!   `masgn`, unused-argument guards, and IgnoreImplicitReferences for zsuper and
//!   binding. Residual gap (murphy-4947): full VariableForce-equivalent dominance
//!   and complete implicit-reference handling (e.g. an implicit reference *before*
//!   the assignment) may still differ.
//! ```
//!
//! ## Matched shapes
//!
//! - `def m(foo); foo = 42; puts foo; end` — flags the assignment
//! - `do_something { |foo| foo = 42; puts foo }` — flags the assignment
//! - a conditional reassignment followed by an unconditional one reports the
//!   argument declaration range (location undecidable)
//!
//! ## Accepted shapes (no offense)
//!
//! - `foo = 5 if bar` — a lone conditional reassignment may never execute
//! - `max_id = "+inf" if max_id.blank?` — modifier-`if` whose condition reads the arg
//! - `_, foo = foo.split("@")` — `masgn` whose RHS reads the argument
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
                let Some((assignment, location_known)) = first_shadowing_assignment(
                    scope_id,
                    variable,
                    cx,
                    opts.ignore_implicit_references,
                ) else {
                    continue;
                };
                let name = cx.symbol_str(variable.name);
                let message = format!("{MSG_PREFIX}{name}{MSG_SUFFIX}");
                // When an earlier conditional or shorthand assignment makes the
                // precise shadowing location undecidable, report the argument
                // declaration instead of the assignment.
                let range = if location_known {
                    cx.range(assignment)
                } else {
                    cx.node(variable.declaration_node).loc.name
                };
                cx.emit_offense(range, &message, None);
            }
        }
    }
}

/// Mirror RuboCop's `assignment_without_argument_usage` + `shadowing_assignment`:
/// the shadowing point is the first *unconditional* assignment that does not read
/// the argument on its RHS. Conditional and shorthand assignments cannot be the
/// shadowing point — it is undecidable whether they execute — but they make the
/// precise location unknown, so a later unconditional shadowing assignment is
/// reported at the argument declaration. Returns `(assignment_node, location_known)`.
fn first_shadowing_assignment(
    scope_id: NodeId,
    variable: &murphy_plugin_api::var_semantic_model::Variable,
    cx: &Cx<'_>,
    ignore_implicit_references: bool,
) -> Option<(NodeId, bool)> {
    if variable.references.is_empty() {
        return None;
    }

    // `variable.assignments` is already in source order: VarSemanticModel builds
    // it with a source-order DFS. `location_known` relies on that order to
    // degrade the same way RuboCop's left-to-right reduce does.
    let mut location_known = true;
    let mut shadowing = None;
    for assignment in &variable.assignments {
        let node = assignment.node_id;
        let meta = meta_assignment_node(node, cx);
        // Shorthand assignments (`op=`, `||=`, `&&=`) always use their argument,
        // so they never shadow it; they only blur the known location.
        if matches!(
            *cx.kind(meta),
            NodeKind::OpAsgn { .. } | NodeKind::OrAsgn { .. } | NodeKind::AndAsgn { .. }
        ) {
            location_known = false;
            continue;
        }
        if assignment_uses_var(meta, variable.name, cx) {
            continue;
        }
        if is_conditional(meta, scope_id, cx) {
            location_known = false;
            continue;
        }
        shadowing = Some(node);
        break;
    }

    let shadowing = shadowing?;
    let shadowing_start = cx.range(shadowing).start;

    // If the argument was read at or before the shadowing assignment, it was used
    // before being shadowed.
    if variable
        .references
        .iter()
        .any(|reference| reference.pos <= shadowing_start)
    {
        return None;
    }
    if ignore_implicit_references && implicit_reference_after(scope_id, shadowing_start, cx) {
        return None;
    }
    Some((shadowing, location_known))
}

/// The node whose subtree carries the assignment's RHS. For an `mlhs` target the
/// RHS lives on the enclosing `masgn`, so walk up to it; `op=`/`||=`/`&&=` node
/// ids already point at the compound node.
fn meta_assignment_node(node: NodeId, cx: &Cx<'_>) -> NodeId {
    let mut current = node;
    while let Some(parent) = cx.parent(current).get() {
        match *cx.kind(parent) {
            NodeKind::Mlhs(_) => current = parent,
            NodeKind::Masgn { .. } => return parent,
            _ => break,
        }
    }
    node
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

/// Whether the assignment sits inside a branch/block whose execution is
/// undecidable. Mirrors RuboCop's `node.conditional? || node.type?(:block,
/// :rescue)`, where `conditional?` covers `if`/`while`/`until`/`case`/`case_match`.
fn is_conditional(node: NodeId, scope_id: NodeId, cx: &Cx<'_>) -> bool {
    let mut current = node;
    while let Some(parent) = cx.parent(current).get() {
        if parent == scope_id {
            return false;
        }
        if matches!(
            *cx.kind(parent),
            NodeKind::If { .. }
                | NodeKind::While { .. }
                | NodeKind::Until { .. }
                | NodeKind::Case { .. }
                | NodeKind::CaseMatch { .. }
                | NodeKind::Block { .. }
                | NodeKind::Rescue { .. }
                | NodeKind::Resbody { .. }
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
    fn accepts_conditional_only_shadowing_with_condition_reading_arg() {
        // `max_id` is reassigned only when the modifier-if condition (which
        // reads `max_id`) is true; on the false path the argument survives, so
        // RuboCop does not flag it.
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def get(max_id)
              max_id = "+inf" if max_id.blank?
              puts max_id
            end
        "#});
    }

    #[test]
    fn accepts_conditional_only_shadowing() {
        // A lone conditional reassignment may never execute, so the original
        // argument can still reach the later read.
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              foo = 5 if bar
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_multiple_conditional_only_shadowings() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              foo = 1 if a
              foo = 2 if b
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_masgn_whose_rhs_reads_the_argument() {
        // `_, domain = domain.split("@")` reads `domain` on the RHS, so it is
        // used before being shadowed. The RHS lives on the parent `masgn`, not
        // the individual target.
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def m(domain)
              _, domain = domain.split("@")
              puts domain
            end
        "#});
    }

    #[test]
    fn accepts_conditional_shadow_with_later_argument_use() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def m(foo)
              foo = compute if foo.nil?
              bar = foo
              puts bar
            end
        "#});
    }

    #[test]
    fn accepts_shadowing_inside_case_when() {
        // The reassignment inside a `when` branch is conditional.
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def m(foo)
              case bar
              when 1
                foo = 42
              end
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_op_asgn_before_unconditional_shadow() {
        // `foo += 1` reads `foo` before the later reassignment, so the argument
        // was used before being shadowed.
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def m(foo)
              foo += 1
              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_self_reference_before_unconditional_shadow() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def m(foo)
              foo = foo + 42
              foo = 5
              puts foo
            end
        "#});
    }

    #[test]
    fn flags_unconditional_shadow_before_self_reference() {
        // The first unconditional non-self-using assignment shadows the argument,
        // regardless of a later self-referencing assignment.
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def m(foo)
              foo = 42
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              foo = foo + 1
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
