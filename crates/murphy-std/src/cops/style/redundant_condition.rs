//! `Style/RedundantCondition` — flags unnecessary conditional expressions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantCondition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection covers four synonymous-branch patterns:
//!     1. condition == if_branch (by raw source): `b ? b : c` → `b || c`.
//!     2. Predicate condition + true if_branch: `a.nil? ? true : a` → `a.nil? || a`.
//!        AllowedMethods (default: ["infinite?", "nonzero?"]) suppresses these.
//!     3. Both branches are same-variable assignments: `if foo; @v = foo; else; @v = x; end`.
//!     4. Both branches are single-argument calls to the same method/receiver:
//!        `if foo; test.v = foo; else; test.v = x; end`.
//!   Guards:
//!     - Modifier-form if/unless: skipped.
//!     - elsif: skipped.
//!     - Else branch is an if node (nested else-if chain): skipped.
//!     - Else branch uses `[]=` hash key assignment: skipped.
//!     - Non-ternary where else_branch is multi-line: skipped.
//!   Autocorrect:
//!     - Skipped when the node's range contains any comment tokens.
//!     - Ternary `b ? b : c` → replaces `? b :` with `||`.
//!     - Predicate+true ternary `a.nil? ? true : a` → replaces `? true :` with `||`.
//!     - Block-form if: replaces whole node with `condition || else_branch_src`.
//!   Gaps (v1):
//!     - Autocorrect for branches_have_method (case 4): not implemented for non-ternary.
//!     - Autocorrect for branches_have_assignment (case 3): not implemented for non-ternary.
//!     - Arithmetic-operation branches: not implemented.
//!     - require_parentheses / require_braces / without_argument_parentheses wrapping.
//!     - Comment reinsertion in autocorrect output.
//! ```
//!
//! ## Matched shapes
//!
//! An `If` node where `synonymous_condition_and_branch?` is true, i.e. one of:
//! 1. condition and if_branch have the same raw source.
//! 2. condition is a predicate method call (ends with `?`), if_branch is `true`,
//!    else_branch is not `true`, and condition method is not in AllowedMethods.
//! 3. Both branches are same-variable assignments, condition == if_branch's expression.
//! 4. Both branches are single-argument calls to same method/receiver (not `[]=`),
//!    condition == if_branch's first argument.
//!
//! ## Autocorrect
//!
//! For ternary forms: replace the `? branch :` middle section with `||`.
//! For block forms: replace the whole node with `condition || else_branch`.
//! Skip autocorrect when comments appear in the node's range.

use murphy_plugin_api::{
    CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, SourceTokenKind, Symbol, cop,
};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantCondition;

/// Cop options for [`RedundantCondition`].
#[derive(CopOptions)]
pub struct RedundantConditionOptions {
    #[option(
        name = "AllowedMethods",
        default = ["infinite?", "nonzero?"],
        description = "Methods that are allowed even when their predicate result is used as a condition."
    )]
    pub allowed_methods: Vec<String>,
}

const MSG: &str = "Use double pipes `||` instead.";
const REDUNDANT_CONDITION: &str = "This condition is not needed.";

/// Assignment node types that can appear in both branches.
fn is_asgn_type(cx: &Cx<'_>, node: NodeId) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Lvasgn { .. }
            | NodeKind::Ivasgn { .. }
            | NodeKind::Cvasgn { .. }
            | NodeKind::Gvasgn { .. }
            | NodeKind::Casgn { .. }
    )
}

/// Get the variable name (Symbol) of an assignment node.
fn asgn_name(cx: &Cx<'_>, node: NodeId) -> Option<Symbol> {
    match *cx.kind(node) {
        NodeKind::Lvasgn { name, .. } => Some(name),
        NodeKind::Ivasgn { name, .. } => Some(name),
        NodeKind::Cvasgn { name, .. } => Some(name),
        NodeKind::Gvasgn { name, .. } => Some(name),
        _ => None,
    }
}

/// Get the expression (rhs) of an assignment node.
fn asgn_expression(cx: &Cx<'_>, node: NodeId) -> OptNodeId {
    match *cx.kind(node) {
        NodeKind::Lvasgn { value, .. } => value,
        NodeKind::Ivasgn { value, .. } => value,
        NodeKind::Cvasgn { value, .. } => value,
        NodeKind::Gvasgn { value, .. } => value,
        _ => OptNodeId::NONE,
    }
}

/// Check if `a.nil? ? true : a` — predicate condition with `true` if_branch.
/// Mirrors RuboCop's `if_branch_is_true_type_and_else_is_not?`.
fn if_branch_is_true_type_and_else_is_not(node: NodeId, cx: &Cx<'_>, opts: &RedundantConditionOptions) -> bool {
    // Must be ternary or block if.
    if !cx.is_ternary(node) && !cx.is_if(node) {
        return false;
    }
    let cond = match cx.if_condition(node).get() {
        Some(c) => c,
        None => return false,
    };
    // Condition must be a Send (call_type).
    if !matches!(cx.kind(cond), NodeKind::Send { .. }) {
        return false;
    }
    // Condition must be a predicate method (ends with `?`).
    if !cx.is_predicate_method(cond) {
        return false;
    }
    // Condition method must not be in AllowedMethods.
    if let Some(method_name) = cx.method_name(cond) {
        if opts.allowed_methods.iter().any(|m| m == method_name) {
            return false;
        }
    }
    // if_branch must be True_.
    let if_branch = match cx.if_then_branch(node).get() {
        Some(b) => b,
        None => return false,
    };
    if !matches!(cx.kind(if_branch), NodeKind::True_) {
        return false;
    }
    // else_branch must exist and not be True_.
    let else_branch = match cx.if_else_branch(node).get() {
        Some(b) => b,
        None => return false,
    };
    !matches!(cx.kind(else_branch), NodeKind::True_)
}

/// Check if both branches are same-variable assignments, and condition == if_branch's expression.
/// Mirrors RuboCop's `branches_have_assignment?` + condition check.
fn branches_have_assignment_with_cond(node: NodeId, cx: &Cx<'_>) -> bool {
    let cond = match cx.if_condition(node).get() {
        Some(c) => c,
        None => return false,
    };
    let if_branch = match cx.if_then_branch(node).get() {
        Some(b) => b,
        None => return false,
    };
    let else_branch = match cx.if_else_branch(node).get() {
        Some(b) => b,
        None => return false,
    };

    if !is_asgn_type(cx, if_branch) || !is_asgn_type(cx, else_branch) {
        return false;
    }

    // Both branches must assign to the same variable.
    let if_name = match asgn_name(cx, if_branch) {
        Some(n) => n,
        None => return false,
    };
    let else_name = match asgn_name(cx, else_branch) {
        Some(n) => n,
        None => return false,
    };
    if if_name != else_name {
        return false;
    }

    // Condition must equal the expression of the if_branch assignment.
    let if_expr = match asgn_expression(cx, if_branch).get() {
        Some(e) => e,
        None => return false,
    };
    cx.raw_source(cx.range(cond)) == cx.raw_source(cx.range(if_expr))
}

/// Check if both branches are single-argument method calls to the same method/receiver.
/// The condition must == the if_branch's first_argument.
/// Mirrors RuboCop's `branches_have_method?` + condition check.
fn branches_have_method_with_cond(node: NodeId, cx: &Cx<'_>) -> bool {
    let cond = match cx.if_condition(node).get() {
        Some(c) => c,
        None => return false,
    };
    let if_branch = match cx.if_then_branch(node).get() {
        Some(b) => b,
        None => return false,
    };
    let else_branch = match cx.if_else_branch(node).get() {
        Some(b) => b,
        None => return false,
    };

    // Both branches must be Send nodes.
    let NodeKind::Send {
        receiver: if_recv,
        method: if_method,
        args: if_args,
    } = *cx.kind(if_branch)
    else {
        return false;
    };
    let NodeKind::Send {
        receiver: else_recv,
        method: else_method,
        args: else_args,
    } = *cx.kind(else_branch)
    else {
        return false;
    };

    // Both must have exactly one argument.
    let if_arg_list = cx.list(if_args);
    let else_arg_list = cx.list(else_args);
    if if_arg_list.len() != 1 || else_arg_list.len() != 1 {
        return false;
    }

    // Must not be hash key access `[]`.
    if if_method == else_method {
        // Check method name != `[]`
        if let Some(name) = cx.method_name(if_branch) {
            if name == "[]" {
                return false;
            }
        }
    }

    // Must be the same method name.
    if if_method != else_method {
        return false;
    }

    // Must have the same receiver (by raw source).
    let recv_match = match (if_recv.get(), else_recv.get()) {
        (Some(ir), Some(er)) => cx.raw_source(cx.range(ir)) == cx.raw_source(cx.range(er)),
        (None, None) => true,
        _ => false,
    };
    if !recv_match {
        return false;
    }

    // Condition must == if_branch's first argument.
    let if_first_arg = if_arg_list[0];
    cx.raw_source(cx.range(cond)) == cx.raw_source(cx.range(if_first_arg))
}

/// Check if the condition and if_branch have the same raw source.
fn condition_equals_if_branch(node: NodeId, cx: &Cx<'_>) -> bool {
    let cond = match cx.if_condition(node).get() {
        Some(c) => c,
        None => return false,
    };
    let if_branch = match cx.if_then_branch(node).get() {
        Some(b) => b,
        None => return false,
    };
    cx.raw_source(cx.range(cond)) == cx.raw_source(cx.range(if_branch))
}

/// Returns true if the node contains any comment tokens in its range.
fn has_comments_in_range(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    cx.tokens_in(range)
        .iter()
        .any(|tok| tok.kind == SourceTokenKind::Comment)
}

/// Returns `true` if this `If` node has a redundant condition.
/// Mirrors RuboCop's `redundant_condition?`.
fn is_redundant_condition(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.is_modifier_form(node) || cx.if_else_branch(node).get().is_none()
}

/// Check if this `If` node is an offense.
fn is_offense(node: NodeId, cx: &Cx<'_>, opts: &RedundantConditionOptions) -> bool {
    // Skip modifier form.
    if cx.is_modifier_form(node) {
        return false;
    }
    // Skip elsif.
    if cx.is_elsif(node) {
        return false;
    }

    let else_branch = match cx.if_else_branch(node).get() {
        Some(b) => b,
        None => return false,
    };

    // Skip if else_branch is an if node (nested elsif/else-if).
    if matches!(cx.kind(else_branch), NodeKind::If { .. }) {
        return false;
    }

    // Skip if else_branch is a `[]=` hash key assignment send.
    if let NodeKind::Send { method, .. } = *cx.kind(else_branch) {
        if cx.symbol_str(method) == "[]=" {
            return false;
        }
    }

    // Check synonymous_condition_and_branch?
    let synonymous = condition_equals_if_branch(node, cx)
        || if_branch_is_true_type_and_else_is_not(node, cx, opts)
        || branches_have_assignment_with_cond(node, cx)
        || branches_have_method_with_cond(node, cx);

    if !synonymous {
        return false;
    }

    // For non-ternary, else_branch must be single-line.
    if !cx.is_ternary(node) {
        let else_src = cx.raw_source(cx.range(else_branch));
        if else_src.contains('\n') {
            return false;
        }
    }

    true
}

#[cop(
    name = "Style/RedundantCondition",
    description = "Checks for unnecessary conditional expressions.",
    default_severity = "warning",
    default_enabled = true,
    options = RedundantConditionOptions,
)]
impl RedundantCondition {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<RedundantConditionOptions>();
        check(node, cx, &opts);
    }
}

fn check(node: NodeId, cx: &Cx<'_>, opts: &RedundantConditionOptions) {
    if !is_offense(node, cx, opts) {
        return;
    }

    let msg = if is_redundant_condition(node, cx) {
        REDUNDANT_CONDITION
    } else {
        MSG
    };

    let offense_range = range_of_offense(node, cx);
    cx.emit_offense(offense_range, msg, None);

    // Autocorrect.
    autocorrect(node, cx, opts);
}

/// Range of the offense.
///
/// - Ternary: `? if_branch :` — the redundant middle section.
/// - Block form: `if <condition>` — keyword through condition end (single-line, mirrors the
///   visible offense position that users fix in their editor).
fn range_of_offense(node: NodeId, cx: &Cx<'_>) -> Range {
    if !cx.is_ternary(node) {
        // Use the `if`/`unless` keyword through the end of the condition.
        // This gives a single-line offense marker matching `if <cond>`.
        let keyword_loc = cx.if_keyword_loc(node);
        let cond_end = cx
            .if_condition(node)
            .get()
            .map(|c| cx.range(c).end)
            .unwrap_or_else(|| cx.range(node).end);
        let start = if keyword_loc != Range::ZERO {
            keyword_loc.start
        } else {
            cx.range(node).start
        };
        return Range { start, end: cond_end };
    }

    // For ternary `b ? b : c`, offense is `? b :` — from question start to colon end.
    // Mirrors RuboCop's range_between(node.loc.question.begin_pos, node.loc.colon.end_pos).
    let question_loc = cx.ternary_question_loc(node);
    let colon_loc = cx.ternary_colon_loc(node);

    if question_loc == Range::ZERO || colon_loc == Range::ZERO {
        return cx.range(node);
    }

    Range {
        start: question_loc.start,
        end: colon_loc.end,
    }
}

/// Autocorrect the node.
fn autocorrect(node: NodeId, cx: &Cx<'_>, opts: &RedundantConditionOptions) {
    // Skip if there are comments in the range (cannot determine intent).
    if has_comments_in_range(node, cx) {
        return;
    }

    if cx.is_ternary(node)
        && !branches_have_method_with_cond(node, cx)
        && !branches_have_assignment_with_cond(node, cx)
    {
        autocorrect_ternary(node, cx);
    } else if is_redundant_condition(node, cx) {
        // modifier form or no else branch — replace with if_branch source.
        if let Some(if_branch) = cx.if_then_branch(node).get() {
            let replacement = cx.raw_source(cx.range(if_branch)).to_owned();
            cx.emit_edit(cx.range(node), &replacement);
        }
    } else {
        autocorrect_block_form(node, cx, opts);
    }
}

/// Autocorrect ternary: `b ? b : c` → replace `? b :` with `||`.
fn autocorrect_ternary(node: NodeId, cx: &Cx<'_>) {
    let question_loc = cx.ternary_question_loc(node);
    let colon_loc = cx.ternary_colon_loc(node);
    if question_loc == Range::ZERO || colon_loc == Range::ZERO {
        return;
    }

    // Replace from `?` start to `:` end with `||`.
    let replace_range = Range {
        start: question_loc.start,
        end: colon_loc.end,
    };
    cx.emit_edit(replace_range, "||");
}

/// Returns `true` when the condition contains low-precedence keyword operators
/// (`not`, `and`, `or`) that would change meaning when embedded in `cond || else_val`
/// without parentheses.
///
/// Checks raw source tokens for these keywords. Using tokens avoids false positives
/// from identifiers like `android` that contain `and`.
fn condition_has_low_precedence_keyword(cond: NodeId, cx: &Cx<'_>) -> bool {
    cx.tokens_in(cx.range(cond)).iter().any(|tok| {
        matches!(
            cx.token_text(*tok),
            "not" | "and" | "or"
        )
    })
}

/// Autocorrect block-form: `if b; b; else; c; end` → `b || c`.
fn autocorrect_block_form(node: NodeId, cx: &Cx<'_>, opts: &RedundantConditionOptions) {
    let cond = match cx.if_condition(node).get() {
        Some(c) => c,
        None => return,
    };

    // Skip autocorrect when the condition contains an assignment or
    // low-precedence keyword operators. Rewriting these without parentheses
    // would change semantics:
    //   - Assignment (`a = b`): `a = b || else` has different precedence than
    //     intended (`a` gets `b || else` not just `b`).
    //   - `not`: `not foo || bar` parses as `not (foo || bar)`, not
    //     `(not foo) || bar` — opposite of the original meaning.
    //   - `and`/`or`: similar low-precedence issues.
    if is_asgn_type(cx, cond) || condition_has_low_precedence_keyword(cond, cx) {
        return;
    }

    let else_branch = match cx.if_else_branch(node).get() {
        Some(b) => b,
        None => return,
    };

    let cond_src = cx.raw_source(cx.range(cond)).to_owned();
    let else_src = cx.raw_source(cx.range(else_branch)).to_owned();

    // Build replacement: `condition || else_src`
    // For predicate+true case, use just `condition || else_src`.
    let replacement = if if_branch_is_true_type_and_else_is_not(node, cx, opts) {
        // `a.nil? ? true : a` → `a.nil? || a`
        format!("{} || {}", cond_src, else_src)
    } else if branches_have_assignment_with_cond(node, cx) {
        // `if foo; @v = foo; else; @v = x; end`
        // → `@v = foo || x` — but this requires getting the assignment variable
        // and the else_branch expression. Too complex for v1; skip autocorrect.
        return;
    } else if branches_have_method_with_cond(node, cx) {
        // Complex method-call form — skip autocorrect for v1.
        return;
    } else {
        // Simple `condition || else_branch` form.
        format!("{} || {}", cond_src, else_src)
    };

    cx.emit_edit(cx.range(node), &replacement);
}

#[cfg(test)]
mod tests {
    use super::{RedundantCondition, RedundantConditionOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Offense: condition == if_branch (ternary) ---

    #[test]
    fn flags_ternary_synonymous_condition_and_branch() {
        test::<RedundantCondition>().expect_offense(indoc! {"
            a = b ? b : c
                  ^^^^^ Use double pipes `||` instead.
        "});
    }

    #[test]
    fn autocorrects_ternary_synonymous() {
        test::<RedundantCondition>().expect_correction(
            indoc! {"
                a = b ? b : c
                      ^^^^^ Use double pipes `||` instead.
            "},
            "a = b || c\n",
        );
    }

    // --- Offense: condition == if_branch (block form) ---

    #[test]
    fn flags_block_form_synonymous() {
        test::<RedundantCondition>().expect_offense(indoc! {"
            if b
            ^^^^ Use double pipes `||` instead.
              b
            else
              c
            end
        "});
    }

    #[test]
    fn autocorrects_block_form_synonymous() {
        test::<RedundantCondition>().expect_correction(
            indoc! {"
                if b
                ^^^^ Use double pipes `||` instead.
                  b
                else
                  c
                end
            "},
            "b || c\n",
        );
    }

    // --- Offense: a.nil? ? true : a ---

    #[test]
    fn flags_predicate_ternary() {
        test::<RedundantCondition>().expect_offense(indoc! {"
            a.nil? ? true : a
                   ^^^^^^^^ Use double pipes `||` instead.
        "});
    }

    #[test]
    fn autocorrects_predicate_ternary() {
        test::<RedundantCondition>().expect_correction(
            indoc! {"
                a.nil? ? true : a
                       ^^^^^^^^ Use double pipes `||` instead.
            "},
            "a.nil? || a\n",
        );
    }

    #[test]
    fn flags_predicate_block_form() {
        test::<RedundantCondition>().expect_offense(indoc! {"
            if a.nil?
            ^^^^^^^^^ Use double pipes `||` instead.
              true
            else
              a
            end
        "});
    }

    #[test]
    fn autocorrects_predicate_block_form() {
        test::<RedundantCondition>().expect_correction(
            indoc! {"
                if a.nil?
                ^^^^^^^^^ Use double pipes `||` instead.
                  true
                else
                  a
                end
            "},
            "a.nil? || a\n",
        );
    }

    // --- AllowedMethods ---

    #[test]
    fn no_offense_infinite_default_allowed() {
        test::<RedundantCondition>().expect_no_offenses("num.infinite? ? true : false\n");
    }

    #[test]
    fn no_offense_nonzero_default_allowed() {
        test::<RedundantCondition>().expect_no_offenses("num.nonzero? ? true : 0\n");
    }

    #[test]
    fn flags_predicate_when_not_in_allowed_methods() {
        test::<RedundantCondition>()
            .with_options(&RedundantConditionOptions {
                allowed_methods: vec!["infinite?".to_string()],
            })
            .expect_offense(indoc! {"
                num.nonzero? ? true : 0
                             ^^^^^^^^ Use double pipes `||` instead.
            "});
    }

    // --- No-offense cases ---

    #[test]
    fn no_offense_modifier_form() {
        // Modifier-form is skipped.
        test::<RedundantCondition>().expect_no_offenses("b if condition\n");
    }

    #[test]
    fn no_offense_elsif() {
        test::<RedundantCondition>().expect_no_offenses(indoc! {"
            if b
              b
            elsif cond
              c
            end
        "});
    }

    #[test]
    fn no_offense_no_else_branch() {
        test::<RedundantCondition>().expect_no_offenses(indoc! {"
            if b
              b
            end
        "});
    }

    #[test]
    fn no_offense_different_condition_and_branch() {
        test::<RedundantCondition>().expect_no_offenses("a = b ? c : d\n");
    }

    #[test]
    fn no_offense_else_is_nested_if() {
        test::<RedundantCondition>().expect_no_offenses(indoc! {"
            if b
              b
            else
              if c
                c
              end
            end
        "});
    }

    // --- Assignment branch case ---

    #[test]
    fn flags_assignment_branches() {
        test::<RedundantCondition>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use double pipes `||` instead.
              @value = foo
            else
              @value = another
            end
        "});
    }

    // --- Method call branch case ---

    #[test]
    fn flags_method_call_branches() {
        test::<RedundantCondition>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use double pipes `||` instead.
              test.value = foo
            else
              test.value = another
            end
        "});
    }

    // --- Skip autocorrect when comments present ---

    #[test]
    fn no_autocorrect_with_comment_in_range() {
        // Has a comment in the if body: autocorrect skipped, but offense still fires.
        test::<RedundantCondition>().expect_offense(indoc! {"
            if b
            ^^^^ Use double pipes `||` instead.
              # Important note.
              b
            else
              c
            end
        "});
    }

    // --- No autocorrect for low-precedence keyword conditions ---

    #[test]
    fn flags_not_condition_no_autocorrect() {
        // `not foo || bar` != `(not foo) || bar` due to `not` low precedence.
        // Offense is detected but autocorrect is skipped.
        test::<RedundantCondition>().expect_offense(indoc! {"
            if not foo
            ^^^^^^^^^^ Use double pipes `||` instead.
              not foo
            else
              bar
            end
        "});
    }

    // --- No autocorrect for ternary assignment branches (would corrupt code) ---

    #[test]
    fn flags_ternary_assignment_branches_no_autocorrect() {
        // `foo ? @value = foo : @value = another` detects offense but must NOT
        // autocorrect (doing so would produce invalid code).
        // The offense is reported (on the `? @value = foo :` range) but no edit is emitted.
        test::<RedundantCondition>().expect_offense(indoc! {"
            foo ? @value = foo : @value = another
                ^^^^^^^^^^^^^^^^ Use double pipes `||` instead.
        "});
    }
}

murphy_plugin_api::submit_cop!(RedundantCondition);
