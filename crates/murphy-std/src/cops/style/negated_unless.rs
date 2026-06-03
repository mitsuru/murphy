//! `Style/NegatedUnless` — flags `unless` with a singly-negated condition
//! and suggests using `if` instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NegatedUnless
//! upstream_version_checked: 1.69.0
//! status: partial
//! gap_issues:
//!   - murphy-imxw
//! notes: >
//!   Murphy handles `unless !foo` and `unless not(expr)` (modifier and block
//!   form). Autocorrect replaces `unless` with `if` and replaces the condition
//!   with the receiver source. EnforcedStyle: both/prefix/postfix is supported.
//!   Parity gaps vs RuboCop:
//!   - `something unless (!x.even?)` — parenthesized bang parses as Unknown
//!     in Murphy; the offense is silently skipped.
//!   - `unless (not a_condition)` — space-`not` inside parens parses as Unknown;
//!     offense silently skipped.
//! ```
//!
//! ## Matched shapes
//!
//! `If` nodes (representing `unless`) with:
//! - Source keyword is `unless` (not `if`/`elsif`)
//! - No `else` clause
//! - Not a ternary
//! - Condition is `Send { receiver: Some(x), method: "!", args: [] }`
//!   where `x` is not itself a `!` Send
//! - Style-check pass (prefix/postfix/both)
//!
//! ## Autocorrect
//!
//! Two edits:
//! 1. Replace the `unless` keyword with `if`.
//! 2. Replace the entire condition range with the receiver source string,
//!    which handles both `!expr` (removes `!`) and `not(expr)` (removes `not(…)`).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Favor `if` over `unless` for negative conditions.";

#[derive(Default)]
pub struct NegatedUnless;

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
pub struct NegatedUnlessOptions {
    #[option(
        name = "EnforcedStyle",
        default = "both",
        description = "Selects which form to flag: `both` flags prefix and postfix; `prefix` flags only block-form; `postfix` flags only modifier-form."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/NegatedUnless",
    description = "Favor `if` over `unless` for negative conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = NegatedUnlessOptions,
)]
impl NegatedUnless {
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
    if let NodeKind::Send { method: m, .. } = cx.kind(recv)
        && cx.symbol_str(*m) == "!" {
            return None;
        }
    Some(recv)
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only `unless`, not `if` / `elsif` / ternary.
    if !cx.is_unless(node) {
        return;
    }
    if cx.is_ternary(node) {
        return;
    }
    // `unless...else...end` is handled by Style/UnlessElse, not here.
    if cx.is_else(node) {
        return;
    }

    let opts = cx.options_or_default::<NegatedUnlessOptions>();
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

    // Edit 1: replace `unless` keyword with `if`.
    let kw_range = cx.if_keyword_loc(node);
    if kw_range != Range::ZERO {
        cx.emit_edit(kw_range, "if");
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
    use super::{EnforcedStyle, NegatedUnless, NegatedUnlessOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Default style (both) -----

    #[test]
    fn flags_block_unless_with_negated_condition() {
        test::<NegatedUnless>().expect_correction(
            indoc! {"
                unless !a_condition
                ^^^^^^^^^^^^^^^^^^^ Favor `if` over `unless` for negative conditions.
                  some_method
                end
            "},
            indoc! {"
                if a_condition
                  some_method
                end
            "},
        );
    }

    #[test]
    fn flags_modifier_unless_with_negated_condition() {
        test::<NegatedUnless>().expect_correction(
            "some_method unless !a_condition\n\
             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `if` over `unless` for negative conditions.\n",
            "some_method if a_condition\n",
        );
    }

    // ----- prefix style -----

    #[test]
    fn prefix_style_flags_block_form() {
        test::<NegatedUnless>()
            .with_options(&NegatedUnlessOptions { enforced_style: EnforcedStyle::Prefix })
            .expect_correction(
                indoc! {"
                    unless !foo
                    ^^^^^^^^^^^ Favor `if` over `unless` for negative conditions.
                    end
                "},
                "if foo\nend\n",
            );
    }

    #[test]
    fn prefix_style_accepts_modifier_form() {
        test::<NegatedUnless>()
            .with_options(&NegatedUnlessOptions { enforced_style: EnforcedStyle::Prefix })
            .expect_no_offenses("foo unless !bar\n");
    }

    // ----- postfix style -----

    #[test]
    fn postfix_style_flags_modifier_form() {
        test::<NegatedUnless>()
            .with_options(&NegatedUnlessOptions { enforced_style: EnforcedStyle::Postfix })
            .expect_correction(
                "foo unless !bar\n\
                 ^^^^^^^^^^^^^^^ Favor `if` over `unless` for negative conditions.\n",
                "foo if bar\n",
            );
    }

    #[test]
    fn postfix_style_accepts_block_form() {
        test::<NegatedUnless>()
            .with_options(&NegatedUnlessOptions { enforced_style: EnforcedStyle::Postfix })
            .expect_no_offenses(indoc! {"
                unless !foo
                end
            "});
    }

    // ----- not(expr) form regression -----

    #[test]
    fn flags_unless_not_paren_form() {
        test::<NegatedUnless>().expect_correction(
            "something unless not(a_condition)\n\
             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `if` over `unless` for negative conditions.\n",
            "something if a_condition\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_unless_else_with_negated_condition() {
        test::<NegatedUnless>().expect_no_offenses(indoc! {"
            unless !a_condition
              some_method
            else
              something_else
            end
        "});
    }

    #[test]
    fn accepts_unless_with_compound_negated_condition() {
        test::<NegatedUnless>().expect_no_offenses(indoc! {"
            unless !condition && another_condition
              some_method
            end
        "});
    }

    #[test]
    fn accepts_unless_with_doubly_negated_condition() {
        test::<NegatedUnless>().expect_no_offenses(indoc! {"
            unless !!condition
              some_method
            end
        "});
    }

    #[test]
    fn accepts_modifier_unless_with_doubly_negated_condition() {
        test::<NegatedUnless>().expect_no_offenses("some_method unless !!condition\n");
    }

    #[test]
    fn accepts_ternary() {
        test::<NegatedUnless>().expect_no_offenses("a ? b : c\n");
    }

    #[test]
    fn accepts_empty_unless_condition() {
        test::<NegatedUnless>().expect_no_offenses(indoc! {"
            unless ()
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(NegatedUnless);
