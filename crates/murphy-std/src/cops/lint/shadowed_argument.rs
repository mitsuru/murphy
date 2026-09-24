//! `Lint/ShadowedArgument` — detect arguments reassigned before first use.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ShadowedArgument
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop 1.87 `assignment_without_argument_usage`/
//!   `shadowing_assignment`: the shadowing point is the first *unconditional*
//!   assignment whose RHS does not read the argument, with conditional
//!   (`if`/`while`/`until`/`case`/`case_match`/`block`/`rescue`, post-condition
//!   loops excluded) and shorthand (`op=`/`||=`/`&&=`) assignments degrading
//!   the reported location to the argument declaration. Covers method
//!   (`def`/`defs`) and block (`block`) arguments only, skips explicit
//!   block-locals (`shadowarg`), handles `masgn`/`splat`/`for` via
//!   `meta_assignment_node`, `masgn`-adjusted `reference_pos`, and
//!   VariableForce-equivalent implicit references: zero-arity `super`
//!   (`zsuper`) counts only for method arguments while zero-arg `binding`
//!   counts for all accessible scopes (hard boundaries `def`/`defs`/`class`/
//!   `module`/`sclass` excluded, `block`/`lambda`/closures transparent), with
//!   `IgnoreImplicitReferences` suppressing on any implicit reference.
//! ```
//!
//! ## Matched shapes
//!
//! - `def m(foo); foo = 42; puts foo; end` — flags the assignment
//! - `do_something { |foo| foo = 42; puts foo }` — flags the assignment
//! - a conditional reassignment followed by an unconditional one reports the
//!   argument declaration range (location undecidable)
//! - `def m(foo); foo = 42; super; end` — flags (implicit `zsuper` after)
//! - `def m(q); q = super; q.select("*"); end` — flags
//! - `def m(*items); *items, last = [42, 42]; puts items; end` — flags
//!
//! ## Accepted shapes (no offense)
//!
//! - `foo = 5 if bar` — a lone conditional reassignment may never execute
//! - `max_id = "+inf" if max_id.blank?` — modifier-`if` whose condition reads the arg
//! - `_, foo = foo.split("@")` — `masgn` whose RHS reads the argument
//! - `*items, last = items` — `masgn`/`splat` RHS reads the argument
//! - `do_something { |i; j| j = i * 2 }` — explicit block-local (`shadowarg`)
//! - `do_something { |foo| foo = 42; super }` — `zsuper` never counts for blocks
//!
//! ## Autocorrect
//!
//! None.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

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
            // RuboCop checks only `method_argument? || block_argument?`:
            // `any_def` (`def`/`defs`) or `block` (not `numblock`/`itblock`).
            if !is_method_or_block_scope(cx, scope_id) {
                continue;
            }
            for variable in scope.variables().iter().filter(|var| var.is_argument) {
                // Explicit block-locals (`|x; y|`) are not arguments.
                if matches!(*cx.kind(variable.declaration_node), NodeKind::Shadowarg(_)) {
                    continue;
                }
                // Defensive: VarSemanticModel skips `_`-prefixed names, but
                // anonymous rest (`*`/`**`/`&` with empty name) would otherwise
                // appear here with an empty symbol.
                let name_str = cx.symbol_str(variable.name);
                if name_str.is_empty() || name_str.starts_with('_') {
                    continue;
                }
                let Some((assignment, location_known)) = first_shadowing_assignment(
                    scope_id,
                    variable,
                    cx,
                    opts.ignore_implicit_references,
                ) else {
                    continue;
                };
                let message = format!("{MSG_PREFIX}{name_str}{MSG_SUFFIX}");
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

fn is_method_or_block_scope(cx: &Cx<'_>, scope_id: NodeId) -> bool {
    matches!(
        *cx.kind(scope_id),
        NodeKind::Def { .. } | NodeKind::Defs { .. } | NodeKind::Block { .. }
    )
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
    let is_method_scope = matches!(
        *cx.kind(scope_id),
        NodeKind::Def { .. } | NodeKind::Defs { .. }
    );
    let implicit_refs = collect_implicit_references(scope_id, cx, is_method_scope);

    // RuboCop's `argument.referenced?` includes implicit (`zsuper`/`binding`)
    // references created by VariableForce.
    if variable.references.is_empty() && implicit_refs.is_empty() {
        return None;
    }
    // RuboCop: `next true if !reference.explicit? && ignore_implicit_references?`
    // inside `references.any?` — any implicit reference anywhere suppresses
    // the offense when the option is enabled.
    if ignore_implicit_references && !implicit_refs.is_empty() {
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
        // RuboCop: `next false unless assignment_node.parent`.
        if cx.parent(meta).get().is_none() {
            location_known = false;
            continue;
        }
        if assignment_uses_var(meta, variable.name, cx) {
            continue;
        }
        if conditional_assignment(meta, scope_id, cx) {
            location_known = false;
            continue;
        }
        shadowing = Some(node);
        break;
    }

    let shadowing = shadowing?;
    let shadowing_start = cx.range(shadowing).start;

    // If the argument was read at or before the shadowing assignment, it was used
    // before being shadowed. `reference_pos` mirrors RuboCop: a reference whose
    // parent is `masgn` reports the `masgn` start.
    if variable
        .references
        .iter()
        .any(|reference| reference_pos(reference.node_id, reference.pos, cx) <= shadowing_start)
    {
        return None;
    }
    if implicit_refs.iter().any(|&(_, pos)| pos <= shadowing_start) {
        return None;
    }
    Some((shadowing, location_known))
}

/// RuboCop's `reference_pos`: `node = node.parent if node.parent.masgn_type?`.
fn reference_pos(node: NodeId, pos: u32, cx: &Cx<'_>) -> u32 {
    if let Some(parent) = cx.parent(node).get()
        && matches!(*cx.kind(parent), NodeKind::Masgn { .. })
    {
        return cx.range(parent).start;
    }
    pos
}

/// Implicit references for one scope, mirroring VariableForce:
/// `zsuper` references only method arguments (`def`/`defs`), while zero-arg
/// `binding` references every accessible variable. Hard scope boundaries
/// (`def`/`defs`/`class`/`module`/`sclass`) are not descended into; closures
/// (`block`/`lambda`/etc.) are transparent so `zsuper`/`binding` inside an
/// inner block still counts for an outer method argument.
fn collect_implicit_references(
    scope_id: NodeId,
    cx: &Cx<'_>,
    is_method_scope: bool,
) -> Vec<(NodeId, u32)> {
    let mut out = Vec::new();
    let mut stack = cx.children(scope_id);
    while let Some(node) = stack.pop() {
        if node != scope_id
            && matches!(
                *cx.kind(node),
                NodeKind::Def { .. }
                    | NodeKind::Defs { .. }
                    | NodeKind::Class { .. }
                    | NodeKind::Module { .. }
                    | NodeKind::Sclass { .. }
            )
        {
            continue;
        }
        match *cx.kind(node) {
            NodeKind::Zsuper if is_method_scope => {
                out.push((node, cx.range(node).start));
            }
            NodeKind::Zsuper => {}
            NodeKind::Send {
                receiver,
                method,
                args,
            } if receiver.get().is_none()
                && cx.symbol_str(method) == "binding"
                && cx.list(args).is_empty() =>
            {
                out.push((node, cx.range(node).start));
            }
            _ => {}
        }
        let mut kids = cx.children(node);
        kids.reverse();
        stack.extend(kids);
    }
    out
}

/// The node whose subtree carries the assignment's RHS. Mirrors RuboCop's
/// `Assignment#meta_assignment_node`: operator assignments (`op=`/`||=`/`&&=`),
/// multiple assignment (`masgn` via `mlhs`, traversing `splat` as RuboCop does),
/// and `for` loop variables. For an `mlhs`/`splat` target the RHS lives on the
/// enclosing `masgn`/`for`, so walk up through them; compound node ids already
/// point at the compound node.
fn meta_assignment_node(node: NodeId, cx: &Cx<'_>) -> NodeId {
    if matches!(
        *cx.kind(node),
        NodeKind::OpAsgn { .. } | NodeKind::OrAsgn { .. } | NodeKind::AndAsgn { .. }
    ) {
        return node;
    }
    let mut current = node;
    while let Some(parent) = cx.parent(current).get() {
        match *cx.kind(parent) {
            NodeKind::Mlhs(_) | NodeKind::Splat(_) => {
                current = parent;
                continue;
            }
            NodeKind::Masgn { .. } | NodeKind::For { .. } => return parent,
            NodeKind::OpAsgn { .. } | NodeKind::OrAsgn { .. } | NodeKind::AndAsgn { .. } => {
                return parent;
            }
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

/// Whether the assignment sits inside a branch/block whose execution is
/// undecidable. Mirrors RuboCop's `conditional_assignment?` exactly:
/// `node.conditional? || node.type?(:block, :rescue)` walked up to the scope
/// boundary. `cx.is_conditional` is RuboCop's `conditional?`
/// (`if`/`while`/`until`/`case`/`case_match`) and already excludes
/// post-condition (`begin..end while/until`) loops, whose body always runs once.
/// Only the regular `block` type counts (not `numblock`/`itblock`), matching
/// RuboCop's literal `type?(:block, :rescue)`.
fn conditional_assignment(node: NodeId, scope_id: NodeId, cx: &Cx<'_>) -> bool {
    let mut current = node;
    while let Some(parent) = cx.parent(current).get() {
        if parent == scope_id {
            return false;
        }
        if cx.is_conditional(parent)
            || matches!(
                *cx.kind(parent),
                NodeKind::Block { .. } | NodeKind::Rescue { .. }
            )
        {
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
    fn flags_shadow_in_post_condition_loop() {
        // A `begin..end while`/`until` body always runs at least once, so the
        // reassignment is unconditional (RuboCop's `conditional?` excludes
        // `while_post`/`until_post`).
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def m(foo)
              begin
                foo = 1
                ^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              end while bar
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_shadowing_inside_while_loop() {
        // A pre-condition `while`/`until` may execute zero times, so it stays
        // conditional and is not flagged.
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def m(foo)
              while bar
                foo = 1
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

    #[test]
    fn flags_method_zsuper_after_shadow() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
              foo = 42
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              super
            end
        "#});
    }

    #[test]
    fn flags_argument_shadowed_by_zsuper_rhs() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def select_fields(query, current_time)
              query = super
              ^^^^^^^^^^^^^ Argument `query` was shadowed by a local variable before it was used.
              query.select('*')
            end
        "#});
    }

    #[test]
    fn ignore_implicit_accepts_zsuper_rhs() {
        test::<ShadowedArgument>()
            .with_options(&ShadowedArgumentOptions {
                ignore_implicit_references: true,
            })
            .expect_no_offenses(indoc! {r#"
                def select_fields(query, current_time)
                  query = super
                  query.select('*')
                end
            "#});
    }

    #[test]
    fn flags_method_binding_after_shadow() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
              foo = 42
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              binding
            end
        "#});
    }

    #[test]
    fn accepts_shorthand_after_conditional() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(bar)
              bar = 'baz' if foo
              bar ||= {}
            end
        "#});
    }

    #[test]
    fn flags_splat_rest_shadow() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(*items)
              *items, last = [42, 42]
               ^^^^^ Argument `items` was shadowed by a local variable before it was used.
              puts items
            end
        "#});
    }

    #[test]
    fn accepts_splat_rest_rhs_use() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(*items)
              *items, last = items
              puts items
            end
        "#});
    }

    #[test]
    fn accepts_self_assign_block_arg_in_for() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            for item in items
              do_something { |arg| arg = arg }
            end
        "#});
    }

    #[test]
    fn accepts_block_local_shadowarg() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            numbers = [1, 2, 3]
            numbers.each do |i; j|
              j = i * 2
              puts j
            end
        "#});
    }

    #[test]
    fn accepts_block_zsuper_after_shadow() {
        // `zsuper` never counts for block arguments, and with no other use
        // the argument is not considered referenced.
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            do_something do |foo|
              foo = 42
              super
            end
        "#});
    }

    #[test]
    fn flags_block_binding_after_shadow() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            do_something do |foo|
              foo = 42
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              binding
            end
        "#});
    }

    #[test]
    fn ignore_implicit_accepts_block_binding() {
        test::<ShadowedArgument>()
            .with_options(&ShadowedArgumentOptions {
                ignore_implicit_references: true,
            })
            .expect_no_offenses(indoc! {r#"
                do_something do |foo|
                  foo = 42
                  binding
                end
            "#});
    }

    #[test]
    fn accepts_use_before_shadow_inside_conditional() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              if bar
                puts foo
                foo = 43
              end
              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_only_conditional_shadow() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              if bar
                foo = 42
              end

              puts foo
            end
        "#});
    }

    #[test]
    fn flags_unconditional_before_conditional() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
              foo = 43
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              if bar
                foo = 42
              end
              puts foo
            end
        "#});
    }

    #[test]
    fn reports_declaration_for_nested_conditional_shadow() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
                             ^^^ Argument `foo` was shadowed by a local variable before it was used.
              if bar
                if baz
                  foo = 43
                end
              end
              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_nested_conditional_use_before_shadow() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              if bar
                puts foo
                if baz
                  foo = 43
                end
              end
              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn reports_declaration_for_block_shadow_before_unconditional() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
                             ^^^ Argument `foo` was shadowed by a local variable before it was used.
              something { foo = 43 }

              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_block_use_before_shadow() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              lambda do
                puts foo
                foo = 43
              end

              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_only_block_shadow() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              something { foo = 43 }

              puts foo
            end
        "#});
    }

    #[test]
    fn flags_unconditional_before_block() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
              foo = 43
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              something { foo = 42 }
              puts foo
            end
        "#});
    }

    #[test]
    fn flags_rescue_before_shadow() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
              foo = bar
              ^^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              begin
              rescue
                foo = baz
              end
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_only_rescue_shadow() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              begin
              rescue
                foo = bar
              end
              puts foo
            end
        "#});
    }

    #[test]
    fn flags_only_outside_shadow_with_lambda() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo, bar)
              lambda do
                bar = 42
              end

              foo = 43
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              puts(foo, bar)
            end
        "#});
    }

    #[test]
    fn accepts_implicit_before_shadow() {
        // An implicit reference before the reassignment counts as use.
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              super
              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn ignore_implicit_accepts_any_implicit_before_shadow() {
        test::<ShadowedArgument>()
            .with_options(&ShadowedArgumentOptions {
                ignore_implicit_references: true,
            })
            .expect_no_offenses(indoc! {r#"
                def do_something(foo)
                  super
                  foo = 42
                  puts foo
                end
            "#});
    }

    #[test]
    fn reports_declaration_for_lambda_conditional_shadow() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
                             ^^^ Argument `foo` was shadowed by a local variable before it was used.
              lambda do
                if baz
                  foo = 43
                end
              end
              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_lambda_conditional_use_before_shadow() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              lambda do
                puts foo
                if baz
                  foo = 43
                end
              end
              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn reports_declaration_for_block_in_conditional_shadow() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
                             ^^^ Argument `foo` was shadowed by a local variable before it was used.
              if baz
                lambda do
                  foo = 43
                end
              end

              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_block_in_conditional_use_before_shadow() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              if baz
                puts foo
                lambda do
                  foo = 43
                end
              end
              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn reports_declaration_for_block_arg_conditional_shadow() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            do_something do |foo|
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
    fn accepts_block_arg_use_before_shadow_inside_conditional() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            do_something do |foo|
              if bar
                puts foo
                foo = 43
              end
              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_block_arg_only_conditional_shadow() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            do_something do |foo|
              if bar
                foo = 42
              end

              puts foo
            end
        "#});
    }

    #[test]
    fn flags_block_arg_unconditional_before_conditional() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            do_something do |foo|
              foo = 43
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              if bar
                foo = 42
              end
              puts foo
            end
        "#});
    }

    #[test]
    fn reports_declaration_for_block_arg_nested_block_shadow() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            do_something do |foo|
                             ^^^ Argument `foo` was shadowed by a local variable before it was used.
              something { foo = 43 }

              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_block_arg_only_block_shadow() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            do_something do |foo|
              something { foo = 43 }

              puts foo
            end
        "#});
    }

    #[test]
    fn flags_block_arg_unconditional_before_block() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            do_something do |foo|
              foo = 43
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              something { foo = 42 }
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_binding_before_shadow() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            def do_something(foo)
              binding
              foo = 42
              puts foo
            end
        "#});
    }

    #[test]
    fn accepts_block_arg_unused() {
        test::<ShadowedArgument>().expect_no_offenses(indoc! {r#"
            do_something do |foo|
              puts 'done something'
            end
        "#});
    }

    #[test]
    fn flags_block_arg_multiple_with_lambda() {
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            do_something do |foo, bar|
              lambda do
                bar = 42
              end

              foo = 43
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              puts(foo, bar)
            end
        "#});
    }

    #[test]
    fn accepts_binding_with_args_after_shadow() {
        // `binding(x)` is not an implicit reference.
        test::<ShadowedArgument>().expect_offense(indoc! {r#"
            def do_something(foo)
              foo = 42
              ^^^^^^^^ Argument `foo` was shadowed by a local variable before it was used.
              binding(x)
              puts foo
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(ShadowedArgument);
