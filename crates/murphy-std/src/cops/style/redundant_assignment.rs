//! `Style/RedundantAssignment` — flags redundant local-variable assignments
//! immediately before the value is returned (used as the last expression).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantAssignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Checks for `lvasgn` + `lvar` pairs at the end of a branch that are
//!   redundant: the assignment value could be used directly.
//!   Pattern matching (`CaseMatch`/`InPattern`) branches are not checked
//!   (conservative; RuboCop recurses into them).
//!   Scope: only `on_def`/`on_defs` entry points. Top-level/block bodies
//!   are not checked (same as RuboCop).
//!   Ensure block body is not checked (RuboCop skips `ensure` branches;
//!   the ensure block is for cleanup, not return values).
//!   Autocorrect: replace the `lvasgn` node with just the RHS expression
//!   source, and delete the trailing `lvar` statement (including its newline).
//! ```
//!
//! ## Offense pattern
//!
//! The last two children of a `Begin` (or top-level method body) are:
//!
//! 1. `Lvasgn { name: N, value: expr }` — assigns a local variable.
//! 2. `Lvar(N)` — returns that same variable.
//!
//! The second node is redundant; the assignment can be replaced with the RHS.
//!
//! ## Recursion
//!
//! Murphy's `check_branch` descends into:
//!
//! - `Begin` / (kwbegin translates to `Begin` in Murphy)
//! - `If` (non-modifier, non-ternary) — then and else branches
//! - `Case` — each `When` body and the else branch
//! - `Rescue` — protected body and each `Resbody` body
//!
//! The offense is reported on the `Lvasgn` node.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, NodeList, OptNodeId, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantAssignment;

const MSG: &str = "Redundant assignment before returning detected.";

#[cop(
    name = "Style/RedundantAssignment",
    description = "Checks for redundant assignment before returning.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantAssignment {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let body_opt = cx.def_body(node);
        if let Some(body) = body_opt.get() {
            check_branch(body, cx);
        }
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        let body_opt = cx.def_body(node);
        if let Some(body) = body_opt.get() {
            check_branch(body, cx);
        }
    }
}

/// Recursively check branches for the redundant-assignment pattern.
fn check_branch(node: NodeId, cx: &Cx<'_>) {
    match cx.kind(node) {
        NodeKind::Begin(list) => {
            check_begin_node(*list, cx);
        }
        NodeKind::If { then_, else_, .. } => {
            check_if_node(*then_, *else_, node, cx);
        }
        NodeKind::Case { whens, else_, .. } => {
            check_case_node(*whens, *else_, cx);
        }
        NodeKind::Rescue { body, resbodies, .. } => {
            // Check the protected body.
            if let Some(b) = body.get() {
                check_branch(b, cx);
            }
            // Check each resbody.
            for &rb in cx.list(*resbodies) {
                check_branch(rb, cx);
            }
        }
        NodeKind::Resbody { body, .. } => {
            if let Some(b) = body.get() {
                check_branch(b, cx);
            }
        }
        // Ensure: not checked (ensure is cleanup, not the return value).
        _ => {}
    }
}

/// Check the last two statements of a `Begin` node for the pattern.
fn check_begin_node(list: NodeList, cx: &Cx<'_>) {
    let children = cx.list(list);

    // Need at least two children.
    if children.len() < 2 {
        // Even with only one child, recurse into it.
        if let Some(&last) = children.last() {
            check_branch(last, cx);
        }
        return;
    }

    let second_last = children[children.len() - 2];
    let last = children[children.len() - 1];

    // Pattern: Lvasgn{name, value} followed by Lvar(name).
    if let Some(asgn_value) = redundant_assignment_check(second_last, last, cx) {
        // Emit offense on the Lvasgn node.
        cx.emit_offense(cx.range(second_last), MSG, None);

        // Autocorrect:
        // 1. Replace the Lvasgn node with just the RHS expression source.
        let rhs_source = cx.raw_source(cx.range(asgn_value)).to_owned();
        cx.emit_edit(cx.range(second_last), &rhs_source);

        // 2. Delete the trailing Lvar statement (from end of assignment to end
        //    of lvar), which includes the "\n  x" suffix.
        let lvasgn_end = cx.range(second_last).end;
        let lvar_end = cx.range(last).end;
        let trailing_range = Range {
            start: lvasgn_end,
            end: lvar_end,
        };
        cx.emit_edit(trailing_range, "");
        return;
    }

    // No pattern match at this level — recurse into the last child.
    check_branch(last, cx);
}

/// Returns `Some(value_node)` if `asgn` is `Lvasgn{name, value}` and `ret` is
/// `Lvar(name)` with the same name. Returns `None` otherwise.
///
/// Skips the `x = x` form (RHS is the same local variable as the LHS):
/// in Ruby, `x = x` assigns `nil` (x is not yet in scope on the RHS), so
/// autocorrecting `x = x; x` → `x` would change behavior from `nil` to a
/// method call.
fn redundant_assignment_check(asgn: NodeId, ret: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Lvasgn {
        name: asgn_name,
        value: asgn_value,
    } = *cx.kind(asgn)
    else {
        return None;
    };
    let Some(value_id) = asgn_value.get() else {
        return None;
    };
    let NodeKind::Lvar(ret_name) = *cx.kind(ret) else {
        return None;
    };
    if asgn_name != ret_name {
        return None;
    }
    // Skip `x = x`: the RHS references the same variable as the LHS.
    // This is a self-assignment that evaluates to nil in Ruby (the variable
    // is not yet in scope on the RHS side), so autocorrecting to `x` would
    // change semantics.
    if matches!(cx.kind(value_id), NodeKind::Lvar(rhs_name) if *rhs_name == asgn_name) {
        return None;
    }
    Some(value_id)
}

/// Check an `If` node's branches.
fn check_if_node(then_: OptNodeId, else_: OptNodeId, if_node: NodeId, cx: &Cx<'_>) {
    // Skip modifier and ternary forms. We detect these by checking whether
    // the first token at the node start is `if` or `unless`. If not, it's
    // a modifier (condition comes first) or ternary (uses `?`).
    if is_if_ternary_or_modifier(if_node, cx) {
        return;
    }
    if let Some(t) = then_.get() {
        check_branch(t, cx);
    }
    if let Some(e) = else_.get() {
        check_branch(e, cx);
    }
}

/// Returns `true` if the `If` node is a modifier or ternary form.
fn is_if_ternary_or_modifier(node: NodeId, cx: &Cx<'_>) -> bool {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    // Find the first token in this node range.
    let idx = toks.partition_point(|t| t.range.start < node_range.start);
    let first_tok = toks[idx..].iter().find(|t| t.range.start < node_range.end);
    let Some(tok) = first_tok else {
        return false;
    };

    let tok_src = &source[tok.range.start as usize..tok.range.end as usize];
    // Standard if/unless form starts with the keyword.
    !matches!(tok_src, b"if" | b"unless" | b"elsif")
}

/// Check a `Case` node's when branches and else branch.
fn check_case_node(whens: NodeList, else_: OptNodeId, cx: &Cx<'_>) {
    for &when_node in cx.list(whens) {
        let NodeKind::When { body, .. } = *cx.kind(when_node) else {
            continue;
        };
        if let Some(b) = body.get() {
            check_branch(b, cx);
        }
    }
    if let Some(e) = else_.get() {
        check_branch(e, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::RedundantAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Basic offense case (simple method body) ---

    #[test]
    fn flags_simple_redundant_assignment() {
        test::<RedundantAssignment>().expect_offense(indoc! {r#"
            def func
              some_preceding_statements
              x = something
              ^^^^^^^^^^^^^ Redundant assignment before returning detected.
              x
            end
        "#});
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_simple_redundant_assignment() {
        test::<RedundantAssignment>().expect_correction(
            indoc! {r#"
                def func
                  some_preceding_statements
                  x = something
                  ^^^^^^^^^^^^^ Redundant assignment before returning detected.
                  x
                end
            "#},
            indoc! {r#"
                def func
                  some_preceding_statements
                  something
                end
            "#},
        );
    }

    // --- Inside begin-end body ---

    #[test]
    fn flags_inside_begin_end_body() {
        test::<RedundantAssignment>().expect_offense(indoc! {r#"
            def func
              some_preceding_statements
              begin
                x = something
                ^^^^^^^^^^^^^ Redundant assignment before returning detected.
                x
              end
            end
        "#});
    }

    // --- Inside if-branch ---

    #[test]
    fn flags_inside_if_branch() {
        test::<RedundantAssignment>().expect_offense(indoc! {r#"
            def func
              some_preceding_statements
              if x
                z = 1
                ^^^^^ Redundant assignment before returning detected.
                z
              elsif y
                2
              else
                z = 3
                ^^^^^ Redundant assignment before returning detected.
                z
              end
            end
        "#});
    }

    // --- Inside when-branch ---

    #[test]
    fn flags_inside_when_branch() {
        test::<RedundantAssignment>().expect_offense(indoc! {r#"
            def func
              some_preceding_statements
              case x
              when y
                res = 1
                ^^^^^^^ Redundant assignment before returning detected.
                res
              when z
                2
              else
                res = 3
                ^^^^^^^ Redundant assignment before returning detected.
                res
              end
            end
        "#});
    }

    // --- Inside rescue block ---

    #[test]
    fn flags_inside_rescue_block() {
        test::<RedundantAssignment>().expect_offense(indoc! {r#"
            def func
              1
              x = 2
              ^^^^^ Redundant assignment before returning detected.
              x
            rescue SomeException
              3
              x = 4
              ^^^^^ Redundant assignment before returning detected.
              x
            rescue AnotherException
              5
            end
        "#});
    }

    // --- No-offense cases ---

    #[test]
    fn no_offense_empty_body() {
        test::<RedundantAssignment>().expect_no_offenses(indoc! {r#"
            def func
            end
        "#});
    }

    #[test]
    fn no_offense_empty_if_body() {
        test::<RedundantAssignment>().expect_no_offenses(indoc! {r#"
            def func
              if x
              elsif y
              else
              end
            end
        "#});
    }

    #[test]
    fn no_offense_different_variable() {
        test::<RedundantAssignment>().expect_no_offenses(indoc! {r#"
            def func
              x = 1
              y
            end
        "#});
    }

    #[test]
    fn no_offense_not_last_two_stmts() {
        test::<RedundantAssignment>().expect_no_offenses(indoc! {r#"
            def func
              x = 1
              x
              y
            end
        "#});
    }

    #[test]
    fn no_offense_ensure_block() {
        test::<RedundantAssignment>().expect_no_offenses(indoc! {r#"
            def func
              1
              x = 2
              x
            ensure
              3
            end
        "#});
    }

    #[test]
    fn no_offense_single_statement() {
        test::<RedundantAssignment>().expect_no_offenses(indoc! {r#"
            def func
              x = 1
            end
        "#});
    }

    // --- self-referential assignment guard ---

    #[test]
    fn no_offense_self_referential_assignment() {
        // `x = x; x` would autocorrect to just `x`, but `x = x` in Ruby
        // means x is nil (variable not yet in scope on RHS), so we skip this.
        test::<RedundantAssignment>().expect_no_offenses(indoc! {r#"
            def func
              x = x
              x
            end
        "#});
    }

    // --- defs ---

    #[test]
    fn flags_defs() {
        test::<RedundantAssignment>().expect_offense(indoc! {r#"
            def self.func
              x = something
              ^^^^^^^^^^^^^ Redundant assignment before returning detected.
              x
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(RedundantAssignment);
