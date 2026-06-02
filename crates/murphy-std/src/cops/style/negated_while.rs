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
//!   Murphy handles `while !foo`, `until !foo`, and `while (!expr)` (block and
//!   modifier form), and `not(expr)` style. Autocorrect swaps the keyword and
//!   replaces the condition with the receiver source. Parenthesized conditions
//!   (`(!x.even?)`) are now detected via NodeKind::Begin (murphy-imxw).
//!   Remaining gaps:
//!   - `while (not a_condition)` — `not` with space inside parens; the inner
//!     node is Send{not}, not supported yet.
//!   - `until (var = foo; !bar)` — multi-statement parenthesized condition
//!     with `!` on last stmt; not yet implemented.
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
//! Two edits:
//! 1. Replace the `while`/`until` keyword with its inverse.
//! 2. Replace the entire condition range with the receiver source string,
//!    which handles both `!expr` (removes `!`) and `not(expr)` (removes `not(…)`).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};
use crate::cops::util::is_parenthesized;

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
    if let NodeKind::Send { method: m, .. } = cx.kind(recv)
        && cx.symbol_str(*m) == "!" {
            return None;
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

    // Unwrap a parenthesized condition: `(!x.even?)` is now Begin([Send{!}]).
    // Try the inner node for the negation check; keep `cond` for range/autocorrect.
    let effective_cond = if is_parenthesized(cond, cx) {
        if let NodeKind::Begin(list) = cx.kind(cond) {
            let children = cx.list(*list);
            if children.len() == 1 { children[0] } else { cond }
        } else {
            cond
        }
    } else {
        cond
    };

    // Condition must be a single `!` negation.
    let Some(recv) = single_negative(effective_cond, cx) else {
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

    // Edit 2: replace the entire condition with the receiver source.
    // Using replace-whole-condition handles both `!expr` (strips `!`) and
    // `not(expr)` (strips `not(` and the closing `)`), matching RuboCop's
    // ConditionCorrector which does `replace(condition, condition.children.first.source)`.
    // For `while(!expr)` — no space before the `(` — prepend a space so the
    // keyword and the replacement don't run together.
    let cond_start = cx.range(cond).start;
    let source = cx.source().as_bytes();
    let needs_space = cond_start > 0
        && !source[(cond_start - 1) as usize].is_ascii_whitespace();
    let recv_src = cx.raw_source(cx.range(recv));
    let replacement = if needs_space {
        format!(" {recv_src}")
    } else {
        recv_src.to_owned()
    };
    cx.emit_edit(cx.range(cond), &replacement);
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

    // ----- not(expr) form regression -----

    #[test]
    fn flags_while_not_paren_form() {
        test::<NegatedWhile>().expect_correction(
            "some_method while not(a_condition)\n\
             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `until` over `while` for negative conditions.\n",
            "some_method until a_condition\n",
        );
    }

    // ----- parenthesized bang condition (murphy-imxw) -----

    #[test]
    fn flags_while_with_parenthesized_negation() {
        // `something while(!x.even?)` — condition was Unknown before murphy-imxw;
        // now Begin([Send{!}]).
        test::<NegatedWhile>().expect_correction(
            "something while(!x.even?)\n\
             ^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `until` over `while` for negative conditions.\n",
            "something until x.even?\n",
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
