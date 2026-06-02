//! `Style/IfInsideElse` — flags an `if` nested directly inside an `else` branch.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/IfInsideElse
//! upstream_version_checked: 1.69.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection is fully implemented: flags any non-ternary, non-unless outer
//!   `if` whose else branch is a plain `if` (not `unless`, not `elsif`). The
//!   AllowIfModifier option (default false) is supported.
//!
//!   Comments between `else` and the nested `if` suppress the offense, matching
//!   RuboCop's `comments_between_else_and_if?` guard.
//!
//!   Autocorrect gaps (v1 partial):
//!   - The standard `if...else...if` → `if...elsif...` conversion for the
//!     non-modifier case is implemented.
//!   - Modifier-form correction (else branch is `action if cond`) is not
//!     implemented.
//!   - `then`-form inner branch (IfThenCorrector equivalent) is not
//!     implemented.
//! ```
//!
//! ## Matched shapes
//!
//! An outer `If` node where:
//! - Outer keyword is `if` (not `unless`, not `elsif`, not ternary).
//! - The else branch is an `If` node with keyword `if` (not `unless`/`elsif`).
//! - No comments between the `else` token and the nested `if` keyword.
//! - `AllowIfModifier: false` (default), or the else branch is not in modifier form.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Convert `if` nested inside `else` to `elsif`.";

#[derive(Default)]
pub struct IfInsideElse;

#[derive(CopOptions)]
pub struct IfInsideElseOptions {
    #[option(
        name = "AllowIfModifier",
        default = false,
        description = "If true, allows a modifier `if` as the sole body of the `else` branch."
    )]
    pub allow_if_modifier: bool,
}

#[cop(
    name = "Style/IfInsideElse",
    description = "Finds `if` nodes inside `else`, which can be converted to `elsif`.",
    default_severity = "warning",
    default_enabled = true,
    options = IfInsideElseOptions,
)]
impl IfInsideElse {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Outer node must be `if` (not `unless`, not `elsif`, not ternary).
    if !cx.is_if(node) {
        return;
    }
    if cx.is_ternary(node) {
        return;
    }

    // The else branch must exist and be a plain `if` (not `unless`, not `elsif`).
    let else_branch_id = match cx.else_branch(node).get() {
        Some(id) => id,
        None => return,
    };

    if !matches!(cx.kind(else_branch_id), NodeKind::If { .. }) {
        return;
    }
    if !cx.is_if(else_branch_id) {
        return;
    }

    let opts = cx.options_or_default::<IfInsideElseOptions>();

    // AllowIfModifier: skip if the else branch is in modifier form.
    if opts.allow_if_modifier && cx.is_modifier_form(else_branch_id) {
        return;
    }

    // Skip if there are comments between the `else` token and the nested `if` keyword.
    if has_comments_between_else_and_if(node, else_branch_id, cx) {
        return;
    }

    // Offense: the keyword of the nested `if`.
    let inner_kw = cx.if_keyword_loc(else_branch_id);
    let offense_range = if inner_kw != Range::ZERO {
        inner_kw
    } else {
        cx.range(else_branch_id)
    };

    cx.emit_offense(offense_range, MSG, None);

    // Autocorrect: basic non-modifier, non-then form only.
    if !cx.is_modifier_form(else_branch_id) && !has_then_keyword(else_branch_id, cx) {
        autocorrect(node, else_branch_id, cx);
    }
}

/// Returns true if there are any comments between the `else` keyword of `outer`
/// and the `if` keyword of `inner`.
fn has_comments_between_else_and_if(outer: NodeId, inner: NodeId, cx: &Cx<'_>) -> bool {
    // Skip if the inner is in modifier form (no separate else keyword to scan).
    if cx.is_modifier_form(inner) {
        return false;
    }

    let Some(else_tok) = find_else_token(outer, cx) else {
        return false;
    };

    let inner_kw = cx.if_keyword_loc(inner);
    if inner_kw == Range::ZERO {
        return false;
    }

    let scan_range = Range {
        start: else_tok.end,
        end: inner_kw.start,
    };

    !cx.comments_in_range(scan_range).is_empty()
}

/// Finds the `else` keyword token directly within the outer `if` node,
/// excluding tokens inside the condition, then-branch, and else-branch
/// child ranges. Avoids heap allocation by matching on `NodeKind::If` directly.
fn find_else_token(outer: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let node_range = cx.range(outer);
    let (cond, then_, else_) = match cx.kind(outer) {
        NodeKind::If { cond, then_, else_ } => (*cond, then_.get(), else_.get()),
        _ => return None,
    };
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < node_range.start);

    for tok in &toks[idx..] {
        if tok.range.start >= node_range.end {
            break;
        }
        if tok.kind != SourceTokenKind::Other {
            continue;
        }
        let text = &source[tok.range.start as usize..tok.range.end as usize];
        if text != b"else" {
            continue;
        }
        // Exclude tokens inside the condition, then-branch, or else-branch.
        let inside_child = {
            let r_cond = cx.range(cond);
            if tok.range.start >= r_cond.start && tok.range.end <= r_cond.end {
                true
            } else if let Some(t) = then_ {
                let r = cx.range(t);
                tok.range.start >= r.start && tok.range.end <= r.end
            } else if let Some(e) = else_ {
                let r = cx.range(e);
                tok.range.start >= r.start && tok.range.end <= r.end
            } else {
                false
            }
        };
        if !inside_child {
            return Some(tok.range);
        }
    }
    None
}

/// Returns true if the `if` node has a `then` keyword (e.g., `if cond then`).
fn has_then_keyword(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::If { cond, .. } = cx.kind(node) else {
        return false;
    };
    let cond_end = cx.range(*cond).end;
    let node_end = cx.range(node).end;
    let source = cx.source().as_bytes();

    // Look for `then` keyword on the same header line as the `if`.
    let scan_end = source[cond_end as usize..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(node_end, |pos| cond_end + pos as u32);

    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < cond_end);
    for tok in &toks[idx..] {
        if tok.range.start >= scan_end {
            break;
        }
        if tok.kind != SourceTokenKind::Other {
            continue;
        }
        let text = &source[tok.range.start as usize..tok.range.end as usize];
        if text == b"then" {
            return true;
        }
    }
    false
}

/// Autocorrect: convert `if a; ...; else; if b; ...; end; end` to
/// `if a; ...; elsif b; ...; end`.
///
/// Strategy (whole-node replacement):
/// 1. Replace from `else` token start to inner condition end with `elsif <condition>`.
/// 2. Remove the inner `end` line.
fn autocorrect(outer: NodeId, inner: NodeId, cx: &Cx<'_>) {
    let Some(else_tok) = find_else_token(outer, cx) else {
        return;
    };

    let inner_kw = cx.if_keyword_loc(inner);
    if inner_kw == Range::ZERO {
        return;
    }

    let NodeKind::If { cond: inner_cond, .. } = cx.kind(inner) else {
        return;
    };

    let inner_cond_range = cx.range(*inner_cond);
    let inner_cond_src = cx.raw_source(inner_cond_range);

    // Edit 1: replace `else\n  if <cond>` with `elsif <cond>`.
    let replace_range = Range {
        start: else_tok.start,
        end: inner_cond_range.end,
    };
    cx.emit_edit(replace_range, &format!("elsif {inner_cond_src}"));

    // Edit 2: remove the inner `end` line.
    let inner_end_loc = cx.loc(inner).end_keyword();
    if inner_end_loc == Range::ZERO {
        return;
    }
    let inner_end_line = cx.range_by_whole_lines(inner_end_loc, true);
    cx.emit_edit(inner_end_line, "");
}

#[cfg(test)]
mod tests {
    use super::{IfInsideElse, IfInsideElseOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Basic detection ---

    #[test]
    fn flags_if_inside_else() {
        test::<IfInsideElse>().expect_offense(indoc! {"
            if condition_a
              action_a
            else
              if condition_b
              ^^ Convert `if` nested inside `else` to `elsif`.
                action_b
              end
            end
        "});
    }

    #[test]
    fn flags_modifier_if_inside_else_by_default() {
        test::<IfInsideElse>().expect_offense(indoc! {"
            if condition_a
              action_a
            else
              action_b if condition_b
                       ^^ Convert `if` nested inside `else` to `elsif`.
            end
        "});
    }

    // --- AllowIfModifier option ---

    #[test]
    fn allow_if_modifier_accepts_modifier_if_in_else() {
        test::<IfInsideElse>()
            .with_options(&IfInsideElseOptions { allow_if_modifier: true })
            .expect_no_offenses(indoc! {"
                if condition_a
                  action_a
                else
                  action_b if condition_b
                end
            "});
    }

    #[test]
    fn allow_if_modifier_still_flags_block_if_in_else() {
        test::<IfInsideElse>()
            .with_options(&IfInsideElseOptions { allow_if_modifier: true })
            .expect_offense(indoc! {"
                if condition_a
                  action_a
                else
                  if condition_b
                  ^^ Convert `if` nested inside `else` to `elsif`.
                    action_b
                  end
                end
            "});
    }

    // --- No offense cases ---

    #[test]
    fn accepts_unless_outer() {
        test::<IfInsideElse>().expect_no_offenses(indoc! {"
            unless condition_a
              action_a
            else
              if condition_b
                action_b
              end
            end
        "});
    }

    #[test]
    fn accepts_unless_inner_in_else() {
        test::<IfInsideElse>().expect_no_offenses(indoc! {"
            if condition_a
              action_a
            else
              unless condition_b
                action_b
              end
            end
        "});
    }

    #[test]
    fn accepts_ternary_outer() {
        test::<IfInsideElse>().expect_no_offenses("a ? b : c\n");
    }

    #[test]
    fn accepts_if_without_else() {
        test::<IfInsideElse>().expect_no_offenses(indoc! {"
            if condition_a
              action_a
            end
        "});
    }

    #[test]
    fn accepts_non_if_in_else() {
        test::<IfInsideElse>().expect_no_offenses(indoc! {"
            if condition_a
              action_a
            else
              action_b
            end
        "});
    }

    #[test]
    fn accepts_if_with_comment_between_else_and_if() {
        test::<IfInsideElse>().expect_no_offenses(indoc! {"
            if condition_a
              action_a
            else
              # some comment
              if condition_b
                action_b
              end
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(IfInsideElse);
