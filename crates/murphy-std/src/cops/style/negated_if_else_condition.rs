//! `Style/NegatedIfElseCondition` — flags `if-else` and ternary operators with
//! a negated condition that can be simplified by inverting the condition and
//! swapping the branches.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NegatedIfElseCondition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `if`/`unless`-else and ternary nodes whose condition is a negated
//!   expression (`!x`, `not x`, `x != y`, `x !~ y`).
//!   Autocorrects by inverting the condition and swapping the if and else branches.
//!   No options.
//!   Parity gaps vs RuboCop upstream:
//!   - Nested corrected nodes: RuboCop uses a corrected-node set (identity map)
//!     to skip re-correcting an already-corrected ancestor. Murphy's fixpoint
//!     loop re-runs after each pass; because each pass emits only one edit per
//!     node, the end result is the same but requires more passes for deeply
//!     nested negated-if-else chains. Practically identical output.
//!   - `begin`/`kwbegin` unwrapping for `!=`/`!~` conditions: Supported.
//!   - Offense range: for block-form `if-else`, limited to the first source line
//!     (matching RuboCop's `add_offense(node)` single-line highlight behavior).
//!     For ternaries, the full node range is used.
//! ```
//!
//! ## Matched shapes
//!
//! `If` nodes (covering both `if` and `unless`, and ternary `a ? b : c`) where:
//! - Not an `elsif` node itself.
//! - Has an `else` clause.
//! - The `else` clause is not an `elsif` (no `elsif` chains).
//! - The condition is a negated Send: `!x`, `not x`, `x != y`, or `x !~ y`.
//! - No double negation (`!!x`).
//! - The negated method has fewer than 2 arguments (excludes `foo.!=(bar, baz)`).
//! - Both branches are not simultaneously empty.
//! - The else branch is not empty (nil).
//!
//! ## Autocorrect
//!
//! For `!x` / `not x`: replace the condition with just the receiver source.
//! For `x != y`: replace with `x == y`.
//! For `x !~ y`: replace with `x =~ y`.
//! Then swap the if and else branch content.
//!
//! For ternaries: the full node is replaced with inverted condition and swapped
//! branch source ranges.
//! For block-form if-else: a whole-node replacement is emitted with the condition
//! patched and the two source regions swapped.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Offense message. `%type%` → "if-else" or "ternary".
const MSG: &str = "Invert the negated condition and swap the %type% branches.";

#[derive(Default)]
pub struct NegatedIfElseCondition;

#[cop(
    name = "Style/NegatedIfElseCondition",
    description = "Invert negated conditions and swap if-else/ternary branches.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl NegatedIfElseCondition {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Unwrap `begin`/`kwbegin` wrapper nodes (one or more layers).
///
/// Mirrors RuboCop's `unwrap_begin_nodes`: descends through `(begin …)` etc.
/// until a non-begin node is found. Returns `None` when the chain is empty.
fn unwrap_begin(mut node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    loop {
        match cx.kind(node) {
            NodeKind::Begin(list) | NodeKind::Kwbegin(list) => {
                let children = cx.list(*list);
                if children.is_empty() {
                    return None;
                }
                node = children[0];
            }
            _ => return Some(node),
        }
    }
}

/// Check whether `node` is a double negation (`!!x` / `not(not x)`).
fn is_double_negation(node: NodeId, cx: &Cx<'_>) -> bool {
    if !cx.is_negation_method(node) {
        return false;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    cx.is_negation_method(recv)
}

/// Returns true if `node` is a Send with a negated operator suitable for
/// branch-swap simplification.
///
/// Matches: `!x`, `not x`, `x != y`, `x !~ y`.
/// Excludes double negation and multi-argument forms.
fn is_negated_condition(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::Send { .. }) {
        return false;
    }
    // Double negation → not a simplifiable single negation.
    if is_double_negation(node, cx) {
        return false;
    }
    // `!` / `not` negation: receiver must exist, no extra arguments.
    if cx.is_negation_method(node) {
        return cx.call_arguments(node).is_empty();
    }
    // `!=` / `!~` inequality with exactly one argument.
    matches!(cx.method_name(node), Some("!=") | Some("!~"))
        && cx.call_arguments(node).len() == 1
}

/// Build the inverted-condition source string for the condition node.
///
/// - `!x` / `not x` → receiver source verbatim.
/// - `x != y` → `x == y`.
/// - `x !~ y` → `x =~ y`.
fn inverted_condition_source(cond: NodeId, cx: &Cx<'_>) -> String {
    if cx.is_negation_method(cond) {
        // `!x` / `not x` → just the receiver.
        let recv = cx
            .call_receiver(cond)
            .get()
            .expect("negation_method always has a receiver");
        return cx.raw_source(cx.range(recv)).to_string();
    }
    // `x != y` → `x == y`, `x !~ y` → `x =~ y`.
    let method_name = cx
        .method_name(cond)
        .expect("binary negation op is a Send");
    let inverted_op = method_name.replace('!', "=");
    let recv = cx
        .call_receiver(cond)
        .get()
        .expect("binary op always has a receiver");
    let args = cx.call_arguments(cond);
    let arg = args.first().expect("binary op has exactly one argument");
    format!(
        "{} {} {}",
        cx.raw_source(cx.range(recv)),
        inverted_op,
        cx.raw_source(cx.range(*arg))
    )
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Skip `elsif` nodes — the cop fires on `if`/`unless`/ternary only.
    if cx.is_elsif(node) {
        return;
    }

    // Must have an `else` branch.
    let Some(else_branch) = cx.if_else_branch(node).get() else {
        return;
    };
    // The `else` branch must not itself be an `elsif`.
    if cx.is_elsif(else_branch) {
        return;
    }
    // Empty else branch (`else\nend`) → no offense.
    if matches!(cx.kind(else_branch), NodeKind::Nil) {
        return;
    }

    // Get the condition, unwrapping any begin/kwbegin wrappers.
    let Some(raw_cond) = cx.if_condition(node).get() else {
        return;
    };
    let Some(inner_cond) = unwrap_begin(raw_cond, cx) else {
        return;
    };

    // The condition must be negated (and not double-negated, not multi-arg).
    if !is_negated_condition(inner_cond, cx) {
        return;
    }

    // Both branches empty → no offense.
    let then_empty = cx
        .if_then_branch(node)
        .get()
        .is_none_or(|t| matches!(cx.kind(t), NodeKind::Nil));
    if then_empty && matches!(cx.kind(else_branch), NodeKind::Nil) {
        return;
    }

    // Determine type and offense range.
    let is_ternary = cx.is_ternary(node);
    let type_str = if is_ternary { "ternary" } else { "if-else" };
    let msg = MSG.replace("%type%", type_str);

    // Offense range:
    // - Ternary: full node range (the entire `!x ? a : b` expression).
    // - Block-form: first source line only (matching RuboCop's single-line
    //   highlight for multi-line `if !x\n  ...\nelse\n  ...\nend`).
    let offense_range = if is_ternary {
        cx.range(node)
    } else {
        first_line_range(cx.range(node), cx)
    };

    cx.emit_offense(offense_range, &msg, None);

    // Autocorrect.
    let inverted = inverted_condition_source(inner_cond, cx);
    if is_ternary {
        autocorrect_ternary(node, raw_cond, &inverted, cx);
    } else {
        autocorrect_block_if(node, raw_cond, &inverted, cx);
    }
}

/// Returns the range from node start to end of first line (before `\n`).
fn first_line_range(node_range: Range, cx: &Cx<'_>) -> Range {
    let source = cx.source().as_bytes();
    let node_start = node_range.start as usize;
    let first_line_end = source[node_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(node_range.end as usize, |pos| node_start + pos);
    Range {
        start: node_range.start,
        end: first_line_end as u32,
    }
}

/// Autocorrect a ternary: `!x ? then : else` → `x ? else : then`.
fn autocorrect_ternary(node: NodeId, raw_cond: NodeId, inverted: &str, cx: &Cx<'_>) {
    let Some(then_id) = cx.if_then_branch(node).get() else {
        return;
    };
    let Some(else_id) = cx.if_else_branch(node).get() else {
        return;
    };

    let then_src = cx.raw_source(cx.range(then_id)).to_string();
    let else_src = cx.raw_source(cx.range(else_id)).to_string();

    let question_loc = cx.ternary_question_loc(node);
    let colon_loc = cx.ternary_colon_loc(node);

    // Preserve whitespace around operators and branches.
    let before_q = cx.raw_source(Range {
        start: cx.range(raw_cond).end,
        end: question_loc.start,
    });
    let after_q = cx.raw_source(Range {
        start: question_loc.end,
        end: cx.range(then_id).start,
    });
    let before_colon = cx.raw_source(Range {
        start: cx.range(then_id).end,
        end: colon_loc.start,
    });
    let after_colon = cx.raw_source(Range {
        start: colon_loc.end,
        end: cx.range(else_id).start,
    });

    // Swap branches: `<inverted><ws>?<ws><else_src><ws>:<ws><then_src>`
    let replacement = format!(
        "{inverted}{before_q}?{after_q}{else_src}{before_colon}:{after_colon}{then_src}"
    );
    cx.emit_edit(cx.range(node), &replacement);
}

/// Find the `else` keyword range within this `if` node (not inside children).
fn find_else_token_range(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let node_range = cx.range(node);
    let children = cx.children(node);
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
        // Exclude tokens inside child nodes.
        let inside = children.iter().any(|&c| {
            let r = cx.range(c);
            tok.range.start >= r.start && tok.range.end <= r.end
        });
        if !inside {
            return Some(tok.range);
        }
    }
    None
}

/// Autocorrect a block-form `if !x ... else ... end`.
///
/// Strategy (mirrors RuboCop's `swap_branches` for block form):
/// - if_range: raw_cond.end to else_tok.start (condition end → else keyword)
/// - else_range: else_tok.end to end_keyword.start (else keyword end → end)
/// Swap these two chunks and patch the condition in place.
///
/// When the then-branch is empty (`if !cond; else foo; end`):
/// RuboCop removes the else keyword entirely, leaving `if cond; foo; end`.
fn autocorrect_block_if(node: NodeId, raw_cond: NodeId, inverted: &str, cx: &Cx<'_>) {
    let keyword_loc = cx.if_keyword_loc(node);
    if keyword_loc == Range::ZERO {
        return;
    }
    let else_tok = match find_else_token_range(node, cx) {
        Some(r) => r,
        None => return,
    };
    let end_range = cx.loc(node).end_keyword();
    if end_range == Range::ZERO {
        return;
    }

    // Gap between `if` keyword and the start of the condition expression.
    let kw_cond_gap = cx.raw_source(Range {
        start: keyword_loc.end,
        end: cx.range(raw_cond).start,
    });

    // Check if the then-branch is empty.
    let then_empty = cx
        .if_then_branch(node)
        .get()
        .is_none_or(|t| matches!(cx.kind(t), NodeKind::Nil));

    if then_empty {
        // `if !cond\nelse\n  body\nend` → `if cond\n  body\nend`
        // else_chunk: from else_tok.end to end_range.start
        let else_chunk = cx.raw_source(Range {
            start: else_tok.end,
            end: end_range.start,
        });
        let replacement = format!("if{kw_cond_gap}{inverted}{else_chunk}end");
        cx.emit_edit(cx.range(node), &replacement);
        return;
    }

    // Normal: both branches have content.
    // if_chunk: from raw_cond.end to else_tok.start (includes then body)
    let if_chunk = cx.raw_source(Range {
        start: cx.range(raw_cond).end,
        end: else_tok.start,
    });
    // else_chunk: from else_tok.end to end_range.start (includes else body)
    let else_chunk = cx.raw_source(Range {
        start: else_tok.end,
        end: end_range.start,
    });

    // `if<gap><inverted><else_chunk>else<if_chunk>end`
    let replacement = format!("if{kw_cond_gap}{inverted}{else_chunk}else{if_chunk}end");
    cx.emit_edit(cx.range(node), &replacement);
}

#[cfg(test)]
mod tests {
    use super::NegatedIfElseCondition;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- `!` negation in if-else -----

    #[test]
    fn flags_and_corrects_bang_negation_if_else() {
        test::<NegatedIfElseCondition>().expect_correction(
            indoc! {"
                if !x
                ^^^^^ Invert the negated condition and swap the if-else branches.
                  do_something
                else
                  do_something_else
                end
            "},
            indoc! {"
                if x
                  do_something_else
                else
                  do_something
                end
            "},
        );
    }

    // ----- `not` negation in if-else -----

    #[test]
    fn flags_and_corrects_not_negation_if_else() {
        test::<NegatedIfElseCondition>().expect_correction(
            indoc! {"
                if not x
                ^^^^^^^^ Invert the negated condition and swap the if-else branches.
                  do_something
                else
                  do_something_else
                end
            "},
            indoc! {"
                if x
                  do_something_else
                else
                  do_something
                end
            "},
        );
    }

    // ----- `!` negation in ternary -----

    #[test]
    fn flags_and_corrects_bang_negation_ternary() {
        test::<NegatedIfElseCondition>().expect_correction(
            indoc! {"
                !x ? do_something : do_something_else
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Invert the negated condition and swap the ternary branches.
            "},
            indoc! {"
                x ? do_something_else : do_something
            "},
        );
    }

    // ----- `!=` in if-else -----

    #[test]
    fn flags_and_corrects_neq_if_else() {
        test::<NegatedIfElseCondition>().expect_correction(
            indoc! {"
                if x != y
                ^^^^^^^^^ Invert the negated condition and swap the if-else branches.
                  do_something
                else
                  do_something_else
                end
            "},
            indoc! {"
                if x == y
                  do_something_else
                else
                  do_something
                end
            "},
        );
    }

    // ----- `!~` in if-else -----

    #[test]
    fn flags_and_corrects_not_match_if_else() {
        test::<NegatedIfElseCondition>().expect_correction(
            indoc! {"
                if x !~ y
                ^^^^^^^^^ Invert the negated condition and swap the if-else branches.
                  do_something
                else
                  do_something_else
                end
            "},
            indoc! {"
                if x =~ y
                  do_something_else
                else
                  do_something
                end
            "},
        );
    }

    // ----- `!=` in ternary -----

    #[test]
    fn flags_and_corrects_neq_ternary() {
        test::<NegatedIfElseCondition>().expect_correction(
            indoc! {"
                x != y ? do_something : do_something_else
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Invert the negated condition and swap the ternary branches.
            "},
            indoc! {"
                x == y ? do_something_else : do_something
            "},
        );
    }

    // ----- empty if_branch -----

    #[test]
    fn flags_and_corrects_empty_if_branch() {
        test::<NegatedIfElseCondition>().expect_correction(
            indoc! {"
                if !condition.nil?
                ^^^^^^^^^^^^^^^^^^ Invert the negated condition and swap the if-else branches.
                else
                  foo = 42
                end
            "},
            indoc! {"
                if condition.nil?
                  foo = 42
                end
            "},
        );
    }

    // ----- no offense cases -----

    #[test]
    fn no_offense_empty_else_branch() {
        test::<NegatedIfElseCondition>().expect_no_offenses(indoc! {"
            if !condition.nil?
              foo = 42
            else
            end
        "});
    }

    #[test]
    fn no_offense_both_branches_empty() {
        test::<NegatedIfElseCondition>().expect_no_offenses(indoc! {"
            if !condition.nil?
            else
            end
        "});
    }

    #[test]
    fn no_offense_elsif_chain() {
        test::<NegatedIfElseCondition>().expect_no_offenses(indoc! {"
            if !x
              do_something
            elsif !y
              do_something_else
            else
              do_another_thing
            end
        "});
    }

    #[test]
    fn no_offense_partially_negated() {
        test::<NegatedIfElseCondition>().expect_no_offenses(indoc! {"
            if !x && y
              do_something
            else
              do_another_thing
            end
        "});
    }

    #[test]
    fn no_offense_double_negation() {
        test::<NegatedIfElseCondition>().expect_no_offenses(indoc! {"
            if !!x
              do_something
            else
              do_another_thing
            end
        "});
    }

    #[test]
    fn no_offense_no_else() {
        test::<NegatedIfElseCondition>().expect_no_offenses(indoc! {"
            if !x
              do_something
            end
        "});
    }

    #[test]
    fn no_offense_neq_multiple_args() {
        test::<NegatedIfElseCondition>().expect_no_offenses(indoc! {"
            if foo.!=(bar, baz)
              do_a
            else
              do_c
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(NegatedIfElseCondition);
