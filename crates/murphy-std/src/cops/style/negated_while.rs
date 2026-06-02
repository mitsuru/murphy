//! `Style/NegatedWhile` — flags `while`/`until` with a singly-negated
//! condition and suggests using the inverse keyword.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NegatedWhile
//! upstream_version_checked: 1.69.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy handles the common case: a `while`/`until` whose condition is a
//!   single `!` method call (e.g. `while !foo`). Autocorrect replaces the
//!   keyword with its inverse and deletes the `!` prefix.
//!   Parity gaps vs RuboCop:
//!   - `while (not a_condition)` — parens around `not` parse as Unknown in
//!     Murphy; the offense is silently skipped.
//!   - `until (var = foo; !bar)` — a begin-sequence condition also parses as
//!     Unknown; silently skipped.
//!   - `something while(!x.even?)` — parenthesized bang parses as Unknown;
//!     silently skipped.
//! ```
//!
//! ## Matched shapes
//!
//! `While` and `Until` nodes (both block-form and modifier-form, NOT do-while)
//! whose condition is `Send { receiver: Some(x), method: "!", args: [] }`
//! where `x` is not itself a `!` Send (single negation only).
//!
//! ## Autocorrect
//!
//! Two surgical edits:
//! 1. Replace the `while`/`until` keyword with its inverse.
//! 2. Delete the `!` token (range from condition start to receiver start).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Favor `%s` over `%s` for negative conditions.";

#[derive(Default)]
pub struct NegatedWhile;

#[cop(
    name = "Style/NegatedWhile",
    description = "Favor `until` over `while` (and vice-versa) for negative conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl NegatedWhile {
    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns `Some(receiver)` if `cond` is a single `!` Send whose receiver
/// is not itself a `!` Send. Returns `None` otherwise.
fn single_negative(cond: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Send { receiver, method, args } = cx.kind(cond) else {
        return None;
    };
    if cx.symbol_str(*method) != "!" {
        return None;
    }
    if !cx.list(*args).is_empty() {
        return None;
    }
    let recv = receiver.get()?;
    // Exclude double negation: receiver must not itself be a `!` Send.
    if let NodeKind::Send { method: m, .. } = cx.kind(recv) {
        if cx.symbol_str(*m) == "!" {
            return None;
        }
    }
    Some(recv)
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Do-while (`begin..end while cond`) is not flagged.
    let (cond, inverse_kw, current_kw) = match cx.kind(node) {
        NodeKind::While { cond, post: true, .. } | NodeKind::Until { cond, post: true, .. } => {
            let _ = cond;
            return;
        }
        NodeKind::While { cond, .. } => (*cond, "until", "while"),
        NodeKind::Until { cond, .. } => (*cond, "while", "until"),
        _ => return,
    };

    // Condition must be a single `!` negation.
    let Some(recv) = single_negative(cond, cx) else {
        return;
    };

    let node_range = cx.range(node);
    let message = MSG
        .replacen("%s", inverse_kw, 1)
        .replacen("%s", current_kw, 1);

    // Offense range: first source line of the node (matches RuboCop's caret
    // display for both block-form and modifier-form).
    let source = cx.source().as_bytes();
    let node_start = node_range.start as usize;
    let first_line_end = source[node_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(node_range.end as usize, |pos| node_start + pos);
    let offense_range = Range {
        start: node_range.start,
        end: first_line_end as u32,
    };

    cx.emit_offense(offense_range, &message, None);

    // Autocorrect — two surgical edits:

    // Edit 1: replace keyword with inverse.
    let kw_range = cx.loc(node).keyword();
    if kw_range == Range::ZERO {
        // Modifier form has no loc.keyword(); find keyword token by searching
        // backward from the condition for the while/until token.
        if let Some(kw_range) = find_modifier_keyword(node, cond, cx) {
            cx.emit_edit(kw_range, inverse_kw);
        }
    } else {
        cx.emit_edit(kw_range, inverse_kw);
    }

    // Edit 2: delete the `!` prefix (from cond start to recv start).
    let bang_range = Range {
        start: cx.range(cond).start,
        end: cx.range(recv).start,
    };
    if bang_range.start < bang_range.end {
        cx.emit_edit(bang_range, "");
    }
}

/// For modifier-form while/until (`body while/until cond`), `cx.loc().keyword()`
/// returns ZERO because the keyword is not at the node start. Find the
/// `while`/`until` token that sits between the body and the condition.
fn find_modifier_keyword(node: NodeId, cond: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let cond_start = cx.range(cond).start;
    let node_start = cx.range(node).start;
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < cond_start);
    for tok in toks[..idx].iter().rev() {
        if tok.range.start < node_start {
            break;
        }
        let text = &source[tok.range.start as usize..tok.range.end as usize];
        if text == b"while" || text == b"until" {
            return Some(tok.range);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::NegatedWhile;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- `while !cond` cases -----

    #[test]
    fn flags_block_while_with_negated_condition() {
        test::<NegatedWhile>().expect_correction(
            indoc! {"
                while !a_condition
                ^^^^^^^^^^^^^^^^^^ Favor `until` over `while` for negative conditions.
                  some_method
                end
            "},
            indoc! {"
                until a_condition
                  some_method
                end
            "},
        );
    }

    #[test]
    fn flags_modifier_while_with_negated_condition() {
        test::<NegatedWhile>().expect_correction(
            "some_method while !a_condition\n\
             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `until` over `while` for negative conditions.\n",
            "some_method until a_condition\n",
        );
    }

    // ----- `until !cond` cases -----

    #[test]
    fn flags_block_until_with_negated_condition() {
        test::<NegatedWhile>().expect_correction(
            indoc! {"
                until !a_condition
                ^^^^^^^^^^^^^^^^^^ Favor `while` over `until` for negative conditions.
                  some_method
                end
            "},
            indoc! {"
                while a_condition
                  some_method
                end
            "},
        );
    }

    #[test]
    fn flags_modifier_until_with_negated_condition() {
        test::<NegatedWhile>().expect_correction(
            "some_method until !a_condition\n\
             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `while` over `until` for negative conditions.\n",
            "some_method while a_condition\n",
        );
    }

    // ----- Method chain negation -----

    #[test]
    fn flags_while_with_negated_method_chain() {
        test::<NegatedWhile>().expect_correction(
            "something while !x.even?\n\
             ^^^^^^^^^^^^^^^^^^^^^^^^ Favor `until` over `while` for negative conditions.\n",
            "something until x.even?\n",
        );
    }

    #[test]
    fn flags_until_with_negated_method_chain() {
        test::<NegatedWhile>().expect_correction(
            "something until !x.even?\n\
             ^^^^^^^^^^^^^^^^^^^^^^^^ Favor `while` over `until` for negative conditions.\n",
            "something while x.even?\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_while_with_compound_negated_condition() {
        test::<NegatedWhile>().expect_no_offenses(indoc! {"
            while !a_condition && another_condition
              some_method
            end
        "});
    }

    #[test]
    fn accepts_while_with_doubly_negated_condition() {
        test::<NegatedWhile>().expect_no_offenses(indoc! {"
            while !!a_condition
              some_method
            end
        "});
    }

    #[test]
    fn accepts_modifier_while_with_doubly_negated_condition() {
        test::<NegatedWhile>().expect_no_offenses("some_method while !!a_condition\n");
    }

    #[test]
    fn accepts_do_while_post_condition() {
        test::<NegatedWhile>().expect_no_offenses(indoc! {"
            begin
              some_method
            end while !a_condition
        "});
    }

    #[test]
    fn accepts_do_until_post_condition() {
        test::<NegatedWhile>().expect_no_offenses(indoc! {"
            begin
              some_method
            end until !a_condition
        "});
    }

    #[test]
    fn accepts_empty_while_condition() {
        test::<NegatedWhile>().expect_no_offenses(indoc! {"
            while ()
            end
        "});
    }

    #[test]
    fn accepts_empty_until_condition() {
        test::<NegatedWhile>().expect_no_offenses(indoc! {"
            until ()
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(NegatedWhile);
