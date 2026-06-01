//! `Style/UnlessLogicalOperators` — flags logical operators in `unless` conditions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/UnlessLogicalOperators
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Two EnforcedStyle values: `forbid_mixed_logical_operators` (default) and
//!   `forbid_logical_operators`. No autocorrect. Offense range is the whole
//!   `unless` node. Detection uses token scanning within the condition range
//!   to handle parenthesized sub-expressions (which parse as `Unknown` in
//!   Murphy's AST). The `forbid_mixed_logical_operators` style fires when the
//!   condition mixes `and`-type and `or`-type operators, or mixes symbolic
//!   (`&&`/`||`) and keyword (`and`/`or`) forms of the same type.
//!   Only the condition of the `unless` is examined; logical operators in the
//!   body are not flagged.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, SourceTokenKind, cop};

const MSG_MIXED: &str = "Do not use mixed logical operators in an `unless`.";
const MSG_ANY: &str = "Do not use any logical operator in an `unless`.";

#[derive(Default)]
pub struct UnlessLogicalOperators;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "forbid_mixed_logical_operators")]
    ForbidMixedLogicalOperators,
    #[option(value = "forbid_logical_operators")]
    ForbidLogicalOperators,
}

#[derive(CopOptions)]
pub struct UnlessLogicalOperatorsOptions {
    #[option(
        name = "EnforcedStyle",
        default = "forbid_mixed_logical_operators",
        description = "When `forbid_mixed_logical_operators`, flags `unless` conditions that mix different logical operators. When `forbid_logical_operators`, flags any use of logical operators in `unless` conditions."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/UnlessLogicalOperators",
    description = "Do not use logical operators in `unless` conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = UnlessLogicalOperatorsOptions
)]
impl UnlessLogicalOperators {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.is_unless(node) {
            return;
        }
        let opts = cx.options_or_default::<UnlessLogicalOperatorsOptions>();
        let cond = match cx.kind(node) {
            NodeKind::If { cond, .. } => *cond,
            _ => return,
        };
        match opts.enforced_style {
            EnforcedStyle::ForbidMixedLogicalOperators => {
                if is_mixed_logical_operator(cond, cx) {
                    cx.emit_offense(cx.range(node), MSG_MIXED, None);
                }
            }
            EnforcedStyle::ForbidLogicalOperators => {
                if has_any_logical_operator(cond, cx) {
                    cx.emit_offense(cx.range(node), MSG_ANY, None);
                }
            }
        }
    }
}

/// Logical operator token classification.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpKind {
    /// `&&` — symbolic and
    SymbolicAnd,
    /// `and` — keyword and
    KeywordAnd,
    /// `||` — symbolic or
    SymbolicOr,
    /// `or` — keyword or
    KeywordOr,
}

impl OpKind {
    fn is_and(self) -> bool {
        matches!(self, OpKind::SymbolicAnd | OpKind::KeywordAnd)
    }
    fn is_or(self) -> bool {
        matches!(self, OpKind::SymbolicOr | OpKind::KeywordOr)
    }
}

/// Collects all logical operator tokens within the condition's source range.
///
/// We scan tokens because some sub-expressions (e.g. `(b && c)`) parse as
/// `Unknown` in Murphy's AST, making purely AST-based descendant walks miss
/// operators inside parenthesized groups.
fn collect_op_tokens_in_range(range_start: u32, range_end: u32, cx: &Cx<'_>) -> Vec<OpKind> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < range_start);

    let mut ops = Vec::new();
    for tok in &toks[idx..] {
        if tok.range.start >= range_end {
            break;
        }
        if tok.kind != SourceTokenKind::Other {
            continue;
        }
        let text = &source[tok.range.start as usize..tok.range.end as usize];
        let op = match text {
            b"&&" => OpKind::SymbolicAnd,
            b"and" => OpKind::KeywordAnd,
            b"||" => OpKind::SymbolicOr,
            b"or" => OpKind::KeywordOr,
            _ => continue,
        };
        // For `and`/`or` keywords, verify word boundary (they're not method
        // names or identifiers — the tokenizer already ensures this since they
        // are `Other` tokens, but double-check adjacent characters).
        if matches!(text, b"and" | b"or") {
            let before_ok =
                tok.range.start == 0 || !is_word_char(source[tok.range.start as usize - 1]);
            let after_ok = tok.range.end as usize >= source.len()
                || !is_word_char(source[tok.range.end as usize]);
            if !before_ok || !after_ok {
                continue;
            }
        }
        ops.push(op);
    }
    ops
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'?'
}

/// Returns `true` when the condition mixes logical operator types or
/// mixes symbolic and keyword forms.
///
/// Mirrors RuboCop's `mixed_logical_operator?`: gates on the top condition
/// being `And` or `Or` to avoid false positives on logical operators inside
/// method call arguments (e.g. `unless foo(a && b || c)`).
fn is_mixed_logical_operator(cond: NodeId, cx: &Cx<'_>) -> bool {
    let cond_range = cx.range(cond);
    match cx.kind(cond) {
        NodeKind::Or { .. } => {
            // or_with_and?: top is `or`; fire if any descendant is `and`-type.
            let ops = collect_op_tokens_in_range(cond_range.start, cond_range.end, cx);
            let has_and = ops.iter().any(|op| op.is_and());
            if has_and {
                return true; // cross-type: or + and
            }
            // mixed_precedence_or?: only `or`-type but mixing symbolic/keyword.
            let has_sym = ops.iter().any(|&op| op == OpKind::SymbolicOr);
            let has_kw = ops.iter().any(|&op| op == OpKind::KeywordOr);
            has_sym && has_kw
        }
        NodeKind::And { .. } => {
            // and_with_or?: top is `and`; fire if any descendant is `or`-type.
            let ops = collect_op_tokens_in_range(cond_range.start, cond_range.end, cx);
            let has_or = ops.iter().any(|op| op.is_or());
            if has_or {
                return true; // cross-type: and + or
            }
            // mixed_precedence_and?: only `and`-type but mixing symbolic/keyword.
            let has_sym = ops.iter().any(|&op| op == OpKind::SymbolicAnd);
            let has_kw = ops.iter().any(|&op| op == OpKind::KeywordAnd);
            has_sym && has_kw
        }
        // Top condition is not And/Or (e.g. Send, Unknown): not flagged.
        _ => false,
    }
}

/// Returns `true` when the condition's top node is `And` or `Or`.
///
/// Mirrors RuboCop's `logical_operator?` which only matches when the condition
/// itself is `and`/`or`, not when logical operators appear inside method args.
fn has_any_logical_operator(cond: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(cond), NodeKind::And { .. } | NodeKind::Or { .. })
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, UnlessLogicalOperators, UnlessLogicalOperatorsOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- forbid_mixed_logical_operators (default) ----

    #[test]
    fn flags_mixed_and_or() {
        test::<UnlessLogicalOperators>().expect_offense(indoc! {r#"
            return unless a && b || c
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use mixed logical operators in an `unless`.
        "#});
    }

    #[test]
    fn flags_mixed_or_and() {
        test::<UnlessLogicalOperators>().expect_offense(indoc! {r#"
            return unless a || b && c
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use mixed logical operators in an `unless`.
        "#});
    }

    #[test]
    fn flags_mixed_and_keyword_and() {
        test::<UnlessLogicalOperators>().expect_offense(indoc! {r#"
            return unless a && b and c
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use mixed logical operators in an `unless`.
        "#});
    }

    #[test]
    fn flags_mixed_keyword_and_and() {
        test::<UnlessLogicalOperators>().expect_offense(indoc! {r#"
            return unless a and b && c
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use mixed logical operators in an `unless`.
        "#});
    }

    #[test]
    fn flags_mixed_and_or_keyword() {
        test::<UnlessLogicalOperators>().expect_offense(indoc! {r#"
            return unless a && b or c
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use mixed logical operators in an `unless`.
        "#});
    }

    #[test]
    fn flags_mixed_or_keyword_and() {
        test::<UnlessLogicalOperators>().expect_offense(indoc! {r#"
            return unless a or b && c
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use mixed logical operators in an `unless`.
        "#});
    }

    #[test]
    fn flags_mixed_or_or_keyword() {
        test::<UnlessLogicalOperators>().expect_offense(indoc! {r#"
            return unless a || b or c
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use mixed logical operators in an `unless`.
        "#});
    }

    #[test]
    fn flags_mixed_or_keyword_or() {
        test::<UnlessLogicalOperators>().expect_offense(indoc! {r#"
            return unless a or b || c
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use mixed logical operators in an `unless`.
        "#});
    }

    #[test]
    fn flags_mixed_or_keyword_and_symbolic() {
        test::<UnlessLogicalOperators>().expect_offense(indoc! {r#"
            return unless a || b and c
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use mixed logical operators in an `unless`.
        "#});
    }

    #[test]
    fn flags_mixed_and_keyword_or_symbolic() {
        test::<UnlessLogicalOperators>().expect_offense(indoc! {r#"
            return unless a and b || c
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use mixed logical operators in an `unless`.
        "#});
    }

    #[test]
    fn flags_mixed_parenthesized() {
        test::<UnlessLogicalOperators>().expect_offense(indoc! {r#"
            return unless a || (b && c) || d
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use mixed logical operators in an `unless`.
        "#});
    }

    #[test]
    fn accepts_only_symbolic_and() {
        test::<UnlessLogicalOperators>().expect_no_offenses("return unless a && b && c\n");
    }

    #[test]
    fn accepts_only_symbolic_or() {
        test::<UnlessLogicalOperators>().expect_no_offenses("return unless a || b || c\n");
    }

    #[test]
    fn accepts_only_keyword_and() {
        test::<UnlessLogicalOperators>().expect_no_offenses("return unless a and b and c\n");
    }

    #[test]
    fn accepts_only_keyword_or() {
        test::<UnlessLogicalOperators>().expect_no_offenses("return unless a or b or c\n");
    }

    #[test]
    fn accepts_if_with_mixed_operators() {
        test::<UnlessLogicalOperators>().expect_no_offenses("return if a || b && c || d\n");
    }

    #[test]
    fn accepts_logical_operators_outside_unless() {
        test::<UnlessLogicalOperators>().expect_no_offenses(indoc! {"
            def condition?
              a or b && c || d
            end
        "});
    }

    #[test]
    fn accepts_no_logical_operator() {
        test::<UnlessLogicalOperators>().expect_no_offenses("return unless a?\n");
    }

    #[test]
    fn accepts_operators_in_unless_body_not_condition() {
        // Logical operators inside the body of the unless should not be flagged.
        test::<UnlessLogicalOperators>().expect_no_offenses(indoc! {"
            unless condition
              includes_or_in_the_name

              foo || bar
            end
        "});
    }

    #[test]
    fn accepts_keyword_and_operators_in_unless_body() {
        test::<UnlessLogicalOperators>().expect_no_offenses(indoc! {"
            unless condition
              includes_and_in_the_name

              foo && bar
            end
        "});
    }

    // ---- forbid_logical_operators ----

    #[test]
    fn forbid_any_flags_single_symbolic_and() {
        test::<UnlessLogicalOperators>()
            .with_options(&UnlessLogicalOperatorsOptions {
                enforced_style: EnforcedStyle::ForbidLogicalOperators,
            })
            .expect_offense(indoc! {r#"
                return unless a && b
                ^^^^^^^^^^^^^^^^^^^^ Do not use any logical operator in an `unless`.
            "#});
    }

    #[test]
    fn forbid_any_flags_single_symbolic_or() {
        test::<UnlessLogicalOperators>()
            .with_options(&UnlessLogicalOperatorsOptions {
                enforced_style: EnforcedStyle::ForbidLogicalOperators,
            })
            .expect_offense(indoc! {r#"
                return unless a || b
                ^^^^^^^^^^^^^^^^^^^^ Do not use any logical operator in an `unless`.
            "#});
    }

    #[test]
    fn forbid_any_flags_single_keyword_and() {
        test::<UnlessLogicalOperators>()
            .with_options(&UnlessLogicalOperatorsOptions {
                enforced_style: EnforcedStyle::ForbidLogicalOperators,
            })
            .expect_offense(indoc! {r#"
                return unless a and b
                ^^^^^^^^^^^^^^^^^^^^^ Do not use any logical operator in an `unless`.
            "#});
    }

    #[test]
    fn forbid_any_flags_single_keyword_or() {
        test::<UnlessLogicalOperators>()
            .with_options(&UnlessLogicalOperatorsOptions {
                enforced_style: EnforcedStyle::ForbidLogicalOperators,
            })
            .expect_offense(indoc! {r#"
                return unless a or b
                ^^^^^^^^^^^^^^^^^^^^ Do not use any logical operator in an `unless`.
            "#});
    }

    #[test]
    fn forbid_any_flags_mixed_too() {
        test::<UnlessLogicalOperators>()
            .with_options(&UnlessLogicalOperatorsOptions {
                enforced_style: EnforcedStyle::ForbidLogicalOperators,
            })
            .expect_offense(indoc! {r#"
                return unless a && b || c
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use any logical operator in an `unless`.
            "#});
    }

    #[test]
    fn forbid_any_accepts_if() {
        test::<UnlessLogicalOperators>()
            .with_options(&UnlessLogicalOperatorsOptions {
                enforced_style: EnforcedStyle::ForbidLogicalOperators,
            })
            .expect_no_offenses("return if a || b\n");
    }

    #[test]
    fn forbid_any_accepts_outside_unless() {
        test::<UnlessLogicalOperators>()
            .with_options(&UnlessLogicalOperatorsOptions {
                enforced_style: EnforcedStyle::ForbidLogicalOperators,
            })
            .expect_no_offenses(indoc! {"
                def condition?
                  a || b
                end
            "});
    }

    #[test]
    fn forbid_any_accepts_no_logical_operator() {
        test::<UnlessLogicalOperators>()
            .with_options(&UnlessLogicalOperatorsOptions {
                enforced_style: EnforcedStyle::ForbidLogicalOperators,
            })
            .expect_no_offenses("return unless a?\n");
    }

    #[test]
    fn default_options_are_forbid_mixed() {
        let opts = UnlessLogicalOperatorsOptions::default();
        assert_eq!(
            opts.enforced_style,
            EnforcedStyle::ForbidMixedLogicalOperators
        );
    }

    #[test]
    fn forbid_any_does_not_flag_logical_inside_method_arg() {
        // `foo(a || b)` — the condition is a method call, not a logical operator.
        // RuboCop's `logical_operator?` pattern only matches when the condition
        // itself (direct child of `if`) is `and`/`or`.
        test::<UnlessLogicalOperators>()
            .with_options(&UnlessLogicalOperatorsOptions {
                enforced_style: EnforcedStyle::ForbidLogicalOperators,
            })
            .expect_no_offenses("return unless foo(a || b)\n");
    }

    #[test]
    fn forbid_mixed_does_not_flag_logical_inside_method_arg() {
        // Mixed operators inside a method arg — condition is Send, not And/Or.
        test::<UnlessLogicalOperators>().expect_no_offenses("return unless foo(a && b || c)\n");
    }
}

murphy_plugin_api::submit_cop!(UnlessLogicalOperators);
