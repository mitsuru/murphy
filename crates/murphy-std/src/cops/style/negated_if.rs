//! `Style/NegatedIf` — flags `if` with a singly-negated condition
//! and suggests using `unless` instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NegatedIf
//! upstream_version_checked: 1.69.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy handles `if !foo` and `if not(expr)` (modifier and block
//!   form). Autocorrect replaces `if` with `unless` and replaces the condition
//!   with the receiver source. EnforcedStyle: both/prefix/postfix is supported.
//!   Parity gaps vs RuboCop:
//!   - `something if (!x.even?)` — parenthesized bang parses as Unknown
//!     in Murphy; the offense is silently skipped.
//!   - `if (not a_condition)` — space-`not` inside parens parses as Unknown;
//!     offense silently skipped.
//! ```
//!
//! ## Matched shapes
//!
//! `If` nodes (representing `if`) with:
//! - Source keyword is `if` (not `unless`/`elsif`)
//! - No `else` clause
//! - Not a ternary
//! - Condition is `Send { receiver: Some(x), method: "!", args: [] }`
//!   where `x` is not itself a `!` Send
//! - Style-check pass (prefix/postfix/both)
//!
//! ## Autocorrect
//!
//! Two edits:
//! 1. Replace the `if` keyword with `unless`.
//! 2. Replace the entire condition range with the receiver source string,
//!    which handles both `!expr` (removes `!`) and `not(expr)` (removes `not(…)`).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Favor `unless` over `if` for negative conditions.";

#[derive(Default)]
pub struct NegatedIf;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Flag both prefix (block) and postfix (modifier) forms.
    #[default]
    #[option(value = "both")]
    Both,
    /// Flag only prefix (block) form.
    #[option(value = "prefix")]
    Prefix,
    /// Flag only postfix (modifier) form.
    #[option(value = "postfix")]
    Postfix,
}

#[derive(CopOptions)]
pub struct NegatedIfOptions {
    #[option(
        name = "EnforcedStyle",
        default = "both",
        description = "Selects which form to flag: `both` flags prefix and postfix; `prefix` flags only block-form; `postfix` flags only modifier-form."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/NegatedIf",
    description = "Favor `unless` over `if` for negative conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = NegatedIfOptions,
)]
impl NegatedIf {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
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
    // Only `if`, not `unless` / `elsif` / ternary.
    if !cx.is_if(node) {
        return;
    }
    if cx.is_ternary(node) {
        return;
    }
    // `if...else...end` is not flagged here (NegatedIfElseCondition handles that).
    if cx.is_else(node) {
        return;
    }

    let opts = cx.options_or_default::<NegatedIfOptions>();
    let is_modifier = cx.is_modifier_form(node);

    // Apply EnforcedStyle filter.
    match opts.enforced_style {
        EnforcedStyle::Prefix if is_modifier => return,
        EnforcedStyle::Postfix if !is_modifier => return,
        _ => {}
    }

    // Extract condition.
    let NodeKind::If { cond, .. } = cx.kind(node) else {
        return;
    };
    let cond = *cond;

    // Condition must be a single `!` negation.
    let Some(recv) = single_negative(cond, cx) else {
        return;
    };

    let node_range = cx.range(node);

    // Offense range: first source line of the node.
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

    cx.emit_offense(offense_range, MSG, None);

    // Autocorrect — two surgical edits:

    // Edit 1: replace `if` keyword with `unless`.
    let kw_range = cx.if_keyword_loc(node);
    if kw_range != Range::ZERO {
        cx.emit_edit(kw_range, "unless");
    }

    // Edit 2: replace the entire condition with the receiver source.
    // Using replace-whole-condition handles both `!expr` (strips `!`) and
    // `not(expr)` (strips `not(` and the closing `)`), matching RuboCop's
    // ConditionCorrector which does `replace(condition, condition.children.first.source)`.
    let recv_src = cx.raw_source(cx.range(recv));
    cx.emit_edit(cx.range(cond), recv_src);
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, NegatedIf, NegatedIfOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Default style (both) -----

    #[test]
    fn flags_block_if_with_negated_condition() {
        test::<NegatedIf>().expect_correction(
            indoc! {"
                if !a_condition
                ^^^^^^^^^^^^^^^ Favor `unless` over `if` for negative conditions.
                  some_method
                end
            "},
            indoc! {"
                unless a_condition
                  some_method
                end
            "},
        );
    }

    #[test]
    fn flags_modifier_if_with_negated_condition() {
        test::<NegatedIf>().expect_correction(
            "some_method if !a_condition\n\
             ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `unless` over `if` for negative conditions.\n",
            "some_method unless a_condition\n",
        );
    }

    // ----- prefix style -----

    #[test]
    fn prefix_style_flags_block_form() {
        test::<NegatedIf>()
            .with_options(&NegatedIfOptions { enforced_style: EnforcedStyle::Prefix })
            .expect_correction(
                indoc! {"
                    if !foo
                    ^^^^^^^ Favor `unless` over `if` for negative conditions.
                    end
                "},
                "unless foo\nend\n",
            );
    }

    #[test]
    fn prefix_style_accepts_modifier_form() {
        test::<NegatedIf>()
            .with_options(&NegatedIfOptions { enforced_style: EnforcedStyle::Prefix })
            .expect_no_offenses("foo if !bar\n");
    }

    // ----- postfix style -----

    #[test]
    fn postfix_style_flags_modifier_form() {
        test::<NegatedIf>()
            .with_options(&NegatedIfOptions { enforced_style: EnforcedStyle::Postfix })
            .expect_correction(
                "foo if !bar\n\
                 ^^^^^^^^^^^ Favor `unless` over `if` for negative conditions.\n",
                "foo unless bar\n",
            );
    }

    #[test]
    fn postfix_style_accepts_block_form() {
        test::<NegatedIf>()
            .with_options(&NegatedIfOptions { enforced_style: EnforcedStyle::Postfix })
            .expect_no_offenses(indoc! {"
                if !foo
                end
            "});
    }

    // ----- not(expr) form regression -----

    #[test]
    fn flags_if_not_paren_form() {
        test::<NegatedIf>().expect_correction(
            "something if not(a_condition)\n\
             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `unless` over `if` for negative conditions.\n",
            "something unless a_condition\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_if_else_with_negated_condition() {
        test::<NegatedIf>().expect_no_offenses(indoc! {"
            if !a_condition
              some_method
            else
              something_else
            end
        "});
    }

    #[test]
    fn accepts_unless_with_negated_condition() {
        // NegatedUnless handles this, not NegatedIf
        test::<NegatedIf>().expect_no_offenses(indoc! {"
            unless !a_condition
              some_method
            end
        "});
    }

    #[test]
    fn accepts_if_with_compound_negated_condition() {
        test::<NegatedIf>().expect_no_offenses(indoc! {"
            if !condition && another_condition
              some_method
            end
        "});
    }

    #[test]
    fn accepts_if_with_doubly_negated_condition() {
        test::<NegatedIf>().expect_no_offenses(indoc! {"
            if !!condition
              some_method
            end
        "});
    }

    #[test]
    fn accepts_modifier_if_with_doubly_negated_condition() {
        test::<NegatedIf>().expect_no_offenses("some_method if !!condition\n");
    }

    #[test]
    fn accepts_ternary() {
        test::<NegatedIf>().expect_no_offenses("a ? b : c\n");
    }

    #[test]
    fn accepts_elsif() {
        test::<NegatedIf>().expect_no_offenses(indoc! {"
            if x
              a
            elsif !y
              b
            end
        "});
    }

    #[test]
    fn accepts_empty_if_condition() {
        test::<NegatedIf>().expect_no_offenses(indoc! {"
            if ()
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(NegatedIf);
