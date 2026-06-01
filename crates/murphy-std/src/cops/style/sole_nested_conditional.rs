//! `Style/SoleNestedConditional` — flags nested conditionals that can be merged.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SoleNestedConditional
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection is fully implemented: flags any non-ternary, non-else, non-elsif
//!   outer conditional whose sole branch is a non-ternary, non-else inner
//!   conditional. The AllowModifier option (default false) is implemented.
//!   The offense is the keyword range of the inner conditional, matching RuboCop.
//!
//!   Autocorrect gaps (v1 partial):
//!   - The basic non-modifier `if a / if b` → `if a && b` autocorrect is
//!     implemented for the common case (non-unless, non-modifier outer and inner).
//!   - Modifier-form autocorrect (outer or inner in modifier form) is not
//!     implemented.
//!   - `unless` outer condition autocorrect (negating the condition) is not
//!     implemented.
//!   - Full `chainable_condition` parenthesisation (or-type, assignment-in-and,
//!     unparenthesised method calls, block receivers) is not implemented.
//!     Basic `||` / `or` wrapping is applied.
//!   - Guard for variable assignment in outer condition is not implemented.
//!   - Comment reinsertion above merged conditions is not implemented.
//! ```
//!
//! ## Matched shapes
//!
//! An outer `If` node where:
//! - Is not ternary, not `elsif`, has no `else` clause.
//! - The condition-true branch is a single `If` node that is:
//!   - Not ternary, has no `else` clause.
//!   - Not in modifier form when `AllowModifier: true`.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct SoleNestedConditional;

#[derive(CopOptions)]
pub struct SoleNestedConditionalOptions {
    #[option(
        name = "AllowModifier",
        default = false,
        description = "If true, allows modifier-form nested conditionals."
    )]
    pub allow_modifier: bool,
}

const MSG: &str = "Consider merging nested conditions into outer `%<keyword>s` conditions.";

#[cop(
    name = "Style/SoleNestedConditional",
    description = "Finds sole nested conditional nodes which can be merged into outer conditional node.",
    default_severity = "warning",
    default_enabled = true,
    options = SoleNestedConditionalOptions,
)]
impl SoleNestedConditional {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(outer: NodeId, cx: &Cx<'_>) {
    // Skip ternary, elsif, and nodes with an else clause.
    if cx.is_ternary(outer) {
        return;
    }
    if cx.is_elsif(outer) {
        return;
    }
    if cx.is_else(outer) {
        return;
    }

    // Get the "if_branch": the body of the outer conditional.
    // For `if a; body; end`: then_ = body.
    // For `unless a; body; end`: then_ = None, else_ = body (translator swap).
    let if_branch_opt = if cx.is_unless(outer) {
        cx.if_else_branch(outer)
    } else {
        cx.if_then_branch(outer)
    };

    let Some(if_branch) = if_branch_opt.get() else {
        return;
    };

    // The branch must be a plain If node (not Begin, not other node kinds).
    if !matches!(cx.kind(if_branch), NodeKind::If { .. }) {
        return;
    }

    // The inner branch must not have an else clause.
    if cx.is_else(if_branch) {
        return;
    }

    // The inner branch must not be ternary.
    if cx.is_ternary(if_branch) {
        return;
    }

    let opts = cx.options_or_default::<SoleNestedConditionalOptions>();

    // If AllowModifier is true, skip when either outer or inner is in modifier form.
    if opts.allow_modifier
        && (cx.is_modifier_form(outer) || cx.is_modifier_form(if_branch))
    {
        return;
    }

    // Offense: the keyword range of the inner conditional.
    let inner_keyword_loc = cx.if_keyword_loc(if_branch);
    let offense_range = if inner_keyword_loc != Range::ZERO {
        inner_keyword_loc
    } else {
        cx.range(if_branch)
    };

    let outer_keyword = cx.if_keyword(outer);
    let msg = MSG.replace("%<keyword>s", outer_keyword);

    cx.emit_offense(offense_range, &msg, None);

    // Autocorrect: basic case only (non-modifier, non-unless outer, simple conditions).
    // Deferred for modifier and unless outer forms.
    if !cx.is_modifier_form(outer)
        && !cx.is_modifier_form(if_branch)
        && !cx.is_unless(outer)
        && !cx.is_unless(if_branch)
    {
        autocorrect_basic(outer, if_branch, cx);
    }
}

/// Basic autocorrect: `if a\n  if b\n    body\n  end\nend` → `if a && b\n  body\nend`.
///
/// Two non-overlapping edits:
/// 1. Replace the region from `outer_cond.end` to `inner_cond.start` with ` && `.
///    This removes the `\n  if ` gap text (newline + indent + "if " keyword).
///    The inner condition text itself stays unchanged.
/// 2. Remove the outer `end` line.
fn autocorrect_basic(outer: NodeId, inner: NodeId, cx: &Cx<'_>) {
    let NodeKind::If { cond: outer_cond, .. } = cx.kind(outer) else {
        return;
    };
    let NodeKind::If { cond: inner_cond, .. } = cx.kind(inner) else {
        return;
    };

    let outer_cond_range = cx.range(*outer_cond);
    let inner_cond_range = cx.range(*inner_cond);

    // Sanity: gap must be non-empty and follow the outer condition.
    if inner_cond_range.start <= outer_cond_range.end {
        return;
    }

    // Edit 1: replace the gap (from outer_cond.end to inner_cond.start) with ` && `.
    // After this edit the source looks like:
    //   `if outer_cond && inner_cond\n    body\n  end\nend`
    // inner_cond text is untouched (it was not in the gap range).
    let gap = Range {
        start: outer_cond_range.end,
        end: inner_cond_range.start,
    };

    // Wrap outer or inner condition in parens if either contains `||` / `or`,
    // so that `a || b / if c` becomes `(a || b) && c` (not `a || b && c`).
    let outer_cond_src = cx.raw_source(outer_cond_range);
    let inner_cond_src = cx.raw_source(inner_cond_range);

    cx.emit_edit(gap, " && ");

    if needs_parens_for_merge(outer_cond_src) {
        let wrapped = format!("({outer_cond_src})");
        cx.emit_edit(outer_cond_range, &wrapped);
    }
    if needs_parens_for_merge(inner_cond_src) {
        let wrapped = format!("({inner_cond_src})");
        cx.emit_edit(inner_cond_range, &wrapped);
    }

    // Edit 2: remove the outer `end` line (including its leading newline/spaces
    // and trailing newline), leaving the inner `end` as the only closing keyword.
    let outer_end_loc = cx.loc(outer).end_keyword();
    if outer_end_loc == Range::ZERO {
        return;
    }

    let source = cx.source().as_bytes();
    let end_start = outer_end_loc.start as usize;

    // Find the start of the outer `end` line (beginning of the line).
    let line_start = source[..end_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);

    // Find the end of the outer `end` line (including the trailing newline).
    let line_end = source[outer_end_loc.end as usize..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len(), |p| outer_end_loc.end as usize + p + 1);

    let remove_range = Range {
        start: line_start as u32,
        end: line_end as u32,
    };

    // Guard: ensure the outer end line is outside the gap range (no overlap).
    if remove_range.start >= gap.end {
        cx.emit_edit(remove_range, "");
    }
}

/// Returns `true` if the condition source needs parentheses when joined with `&&`.
/// Basic heuristic: wrap `||` / `or` conditions.
fn needs_parens_for_merge(src: &str) -> bool {
    src.contains(" or ") || src.contains(" || ")
}

#[cfg(test)]
mod tests {
    use super::{SoleNestedConditional, SoleNestedConditionalOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- basic detection ---

    #[test]
    fn flags_nested_if() {
        test::<SoleNestedConditional>().expect_offense(indoc! {"
            if condition_a
              if condition_b
              ^^ Consider merging nested conditions into outer `if` conditions.
                do_something
              end
            end
        "});
    }

    #[test]
    fn flags_nested_if_with_autocorrect() {
        test::<SoleNestedConditional>().expect_correction(
            indoc! {"
                if condition_a
                  if condition_b
                  ^^ Consider merging nested conditions into outer `if` conditions.
                    do_something
                  end
                end
            "},
            indoc! {"
                if condition_a && condition_b
                    do_something
                  end
            "},
        );
    }

    #[test]
    fn flags_nested_unless_inner() {
        test::<SoleNestedConditional>().expect_offense(indoc! {"
            if condition_a
              unless condition_b
              ^^^^^^ Consider merging nested conditions into outer `if` conditions.
                do_something
              end
            end
        "});
    }

    // --- no offense cases ---

    #[test]
    fn accepts_no_nesting() {
        test::<SoleNestedConditional>().expect_no_offenses(indoc! {"
            if condition_a
              do_something
            end
        "});
    }

    #[test]
    fn accepts_outer_with_else() {
        test::<SoleNestedConditional>().expect_no_offenses(indoc! {"
            if condition_a
              if condition_b
                do_something
              end
            else
              do_other
            end
        "});
    }

    #[test]
    fn accepts_inner_with_else() {
        test::<SoleNestedConditional>().expect_no_offenses(indoc! {"
            if condition_a
              if condition_b
                do_something
              else
                do_other
              end
            end
        "});
    }

    #[test]
    fn accepts_multi_statement_body() {
        test::<SoleNestedConditional>().expect_no_offenses(indoc! {"
            if condition_a
              x
              if condition_b
                do_something
              end
            end
        "});
    }

    #[test]
    fn accepts_ternary_outer() {
        test::<SoleNestedConditional>().expect_no_offenses("a ? b : c\n");
    }

    #[test]
    fn wraps_outer_or_condition_in_parens() {
        // `if a || b / if c` → `if (a || b) && c` (not `a || b && c`)
        test::<SoleNestedConditional>().expect_correction(
            indoc! {"
                if a || b
                  if c
                  ^^ Consider merging nested conditions into outer `if` conditions.
                    do_something
                  end
                end
            "},
            indoc! {"
                if (a || b) && c
                    do_something
                  end
            "},
        );
    }

    // --- AllowModifier option ---

    #[test]
    fn default_flags_modifier_inner() {
        // The offense range is the `if` keyword of the modifier form.
        test::<SoleNestedConditional>().expect_offense(indoc! {"
            if condition_a
              do_something if condition_b
                           ^^ Consider merging nested conditions into outer `if` conditions.
            end
        "});
    }

    #[test]
    fn allow_modifier_accepts_modifier_inner() {
        test::<SoleNestedConditional>()
            .with_options(&SoleNestedConditionalOptions { allow_modifier: true })
            .expect_no_offenses(indoc! {"
                if condition_a
                  do_something if condition_b
                end
            "});
    }

    // --- unless outer ---

    #[test]
    fn flags_unless_outer_with_nested_if() {
        test::<SoleNestedConditional>().expect_offense(indoc! {"
            unless condition_a
              if condition_b
              ^^ Consider merging nested conditions into outer `unless` conditions.
                do_something
              end
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(SoleNestedConditional);
