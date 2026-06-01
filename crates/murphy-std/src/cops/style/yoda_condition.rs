//! `Style/YodaCondition` — forbids or enforces yoda conditions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/YodaCondition
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Four EnforcedStyle values are implemented (forbid_for_all_comparison_operators,
//!   forbid_for_equality_operators_only, require_for_all_comparison_operators,
//!   require_for_equality_operators_only).
//!   constant_portion? is implemented as: Int/Float/Str/Sym/True/False/Nil literals
//!   and Const nodes. RuboCop's recursive_literal? also covers Array/Hash/Range of
//!   literals; those are treated as non-constant here (v1 gap, benign: more permissive
//!   for the forbid styles, less permissive for the require styles).
//!   __FILE__ is parsed to (unknown) by Murphy, so the file-constant exception
//!   is implemented via raw-source text comparison.
//!   Regexp interpolation detection: any regexp with a non-Str child is treated
//!   as interpolated (matches RuboCop's node.interpolation?).
//! ```
//!
//! A yoda condition is a comparison where a literal (or constant) appears on the
//! left-hand side, e.g. `5 == x` instead of `x == 5`.
//!
//! ## Matched shapes
//!
//! `Send` nodes whose method is one of the comparison operators
//! (`==`, `!=`, `<`, `>`, `<=`, `>=`, `<=>`), excluding `===` (noncommutative).
//!
//! ## Autocorrect
//!
//! The operands are swapped and the operator is reversed where necessary
//! (`<` <-> `>`, `<=` <-> `>=`). Whole-node interpolation (structural rewrite,
//! not a simple delete + rename).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct YodaCondition;

#[derive(CopOptions)]
pub struct YodaConditionOptions {
    #[option(
        name = "EnforcedStyle",
        default = "forbid_for_all_comparison_operators",
        description = "Yoda condition enforcement style."
    )]
    pub enforced_style: YodaConditionStyle,
}

#[derive(Default, CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum YodaConditionStyle {
    #[option(value = "forbid_for_all_comparison_operators")]
    #[default]
    ForbidForAllComparisonOperators,
    #[option(value = "forbid_for_equality_operators_only")]
    ForbidForEqualityOperatorsOnly,
    #[option(value = "require_for_all_comparison_operators")]
    RequireForAllComparisonOperators,
    #[option(value = "require_for_equality_operators_only")]
    RequireForEqualityOperatorsOnly,
}

const EQUALITY_OPERATORS: &[&str] = &["==", "!="];

fn msg(src: &str) -> String {
    format!("Reverse the order of the operands `{src}`.")
}

#[cop(
    name = "Style/YodaCondition",
    description = "Forbid or enforce yoda conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = YodaConditionOptions,
)]
impl YodaCondition {
    #[on_node(kind = "send", methods = ["==", "!=", "<", ">", "<=", ">=", "<=>"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<YodaConditionOptions>();
        check(node, cx, &opts);
    }
}

fn check(node: NodeId, cx: &Cx<'_>, opts: &YodaConditionOptions) {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return;
    };

    let method_name = cx.symbol_str(method);

    // Equality-only styles: skip non-equality operators.
    let is_equality_only = matches!(
        opts.enforced_style,
        YodaConditionStyle::ForbidForEqualityOperatorsOnly
            | YodaConditionStyle::RequireForEqualityOperatorsOnly
    );
    if is_equality_only && !EQUALITY_OPERATORS.contains(&method_name) {
        return;
    }

    let Some(lhs_id) = receiver.get() else {
        return;
    };

    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return;
    }
    let rhs_id = arg_list[0];

    // File-constant exception: __FILE__ == $0 / $PROGRAM_NAME.
    // __FILE__ parses to (unknown); detect via raw source text.
    if is_file_constant_equal_program_name(lhs_id, rhs_id, cx, method_name) {
        return;
    }

    // valid_yoda? mirrors RuboCop's logic:
    //   - both operands are constant portions -> skip
    //   - neither operand is a constant portion -> skip
    //   - lhs contains string/regexp interpolation -> skip
    //   - for forbid styles: offense when lhs IS constant
    //   - for require styles: offense when lhs is NOT constant
    let lhs_const = is_constant_portion(lhs_id, cx);
    let rhs_const = is_constant_portion(rhs_id, cx);

    // Both or neither constant -> no offense.
    if lhs_const == rhs_const {
        return;
    }

    // LHS has string/regexp interpolation -> never flag.
    if has_interpolation(lhs_id, cx) {
        return;
    }

    let enforce_yoda = matches!(
        opts.enforced_style,
        YodaConditionStyle::RequireForAllComparisonOperators
            | YodaConditionStyle::RequireForEqualityOperatorsOnly
    );

    // enforce_yoda=false (forbid): offense when lhs_const=true (yoda ordering).
    // enforce_yoda=true  (require): offense when lhs_const=false (non-yoda).
    if enforce_yoda == lhs_const {
        return;
    }

    let node_range = cx.range(node);
    let src = cx.raw_source(node_range);
    cx.emit_offense(node_range, &msg(src), None);

    // `<=>` is non-commutative: swapping operands negates the result
    // (-1 becomes 1 and vice versa), so autocorrect cannot be applied safely.
    if method_name == "<=>" {
        return;
    }

    // Autocorrect: swap lhs/rhs and reverse relational operators.
    let lhs_src = cx.raw_source(cx.range(lhs_id)).to_owned();
    let rhs_src = cx.raw_source(cx.range(rhs_id)).to_owned();
    let flipped_op = reverse_operator(method_name);
    let replacement = format!("{rhs_src} {flipped_op} {lhs_src}");
    cx.emit_edit(node_range, &replacement);
}

/// Returns `true` if the node is a "constant portion": a simple literal
/// (Int, Float, Str, Sym, True, False, Nil) or a Const node.
fn is_constant_portion(id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(id),
        NodeKind::Int(_)
            | NodeKind::Float(_)
            | NodeKind::Str(_)
            | NodeKind::Sym(_)
            | NodeKind::True_
            | NodeKind::False_
            | NodeKind::Nil
            | NodeKind::Const { .. }
    )
}

/// Returns `true` if the node is a string interpolation (Dstr) or an
/// interpolated regexp (Regexp with a non-Str child).
fn has_interpolation(id: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(id) {
        NodeKind::Dstr(_) => true,
        NodeKind::Regexp { parts, .. } => cx
            .list(*parts)
            .iter()
            .any(|&child| !matches!(cx.kind(child), NodeKind::Str(_))),
        _ => false,
    }
}

/// File-constant exception: `__FILE__ == $0` / `__FILE__ != $PROGRAM_NAME` etc.
fn is_file_constant_equal_program_name(
    lhs_id: NodeId,
    rhs_id: NodeId,
    cx: &Cx<'_>,
    method_name: &str,
) -> bool {
    if !matches!(method_name, "==" | "!=") {
        return false;
    }
    let program_name_gvars: &[&str] = &["$0", "$PROGRAM_NAME"];
    let lhs_src = cx.raw_source(cx.range(lhs_id));
    let rhs_src = cx.raw_source(cx.range(rhs_id));
    (lhs_src == "__FILE__" && is_gvar_in(rhs_id, cx, program_name_gvars))
        || (rhs_src == "__FILE__" && is_gvar_in(lhs_id, cx, program_name_gvars))
}

fn is_gvar_in(id: NodeId, cx: &Cx<'_>, names: &[&str]) -> bool {
    if let NodeKind::Gvar(sym) = cx.kind(id) {
        return names.contains(&cx.symbol_str(*sym));
    }
    false
}

/// Reverse a relational operator for autocorrect.
fn reverse_operator(op: &str) -> &str {
    match op {
        "<" => ">",
        ">" => "<",
        "<=" => ">=",
        ">=" => "<=",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::{YodaCondition, YodaConditionOptions, YodaConditionStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- forbid_for_all_comparison_operators (default) -----

    #[test]
    fn flags_yoda_eq() {
        test::<YodaCondition>().expect_correction(
            indoc! {"
                99 == foo
                ^^^^^^^^^ Reverse the order of the operands `99 == foo`.
            "},
            "foo == 99\n",
        );
    }

    #[test]
    fn flags_yoda_neq() {
        test::<YodaCondition>().expect_correction(
            indoc! {r#"
                "bar" != foo
                ^^^^^^^^^^^^ Reverse the order of the operands `"bar" != foo`.
            "#},
            "foo != \"bar\"\n",
        );
    }

    #[test]
    fn flags_yoda_ge() {
        test::<YodaCondition>().expect_correction(
            indoc! {"
                42 >= foo
                ^^^^^^^^^ Reverse the order of the operands `42 >= foo`.
            "},
            "foo <= 42\n",
        );
    }

    #[test]
    fn flags_yoda_lt() {
        test::<YodaCondition>().expect_correction(
            indoc! {"
                10 < bar
                ^^^^^^^^ Reverse the order of the operands `10 < bar`.
            "},
            "bar > 10\n",
        );
    }

    #[test]
    fn flags_yoda_le() {
        test::<YodaCondition>().expect_correction(
            indoc! {"
                5 <= x
                ^^^^^^ Reverse the order of the operands `5 <= x`.
            "},
            "x >= 5\n",
        );
    }

    #[test]
    fn flags_yoda_gt() {
        test::<YodaCondition>().expect_correction(
            indoc! {"
                5 > x
                ^^^^^ Reverse the order of the operands `5 > x`.
            "},
            "x < 5\n",
        );
    }

    #[test]
    fn flags_yoda_spaceship() {
        test::<YodaCondition>().expect_offense(indoc! {"
            1 <=> x
            ^^^^^^^ Reverse the order of the operands `1 <=> x`.
        "});
    }

    #[test]
    fn spaceship_has_no_autocorrect() {
        // `<=>` is non-commutative: swapping negates the result, so no edit.
        test::<YodaCondition>().expect_no_corrections(
            "1 <=> x
",
        );
    }

    // ----- valid cases for default forbid style -----

    #[test]
    fn accepts_normal_order_eq() {
        test::<YodaCondition>().expect_no_offenses("foo == 99\n");
    }

    #[test]
    fn accepts_normal_order_neq() {
        test::<YodaCondition>().expect_no_offenses("foo == \"bar\"\n");
    }

    #[test]
    fn accepts_both_constants() {
        // both sides constant -> neither fires
        test::<YodaCondition>().expect_no_offenses("99 == CONST\n");
    }

    #[test]
    fn accepts_both_variables() {
        // neither side constant -> skip
        test::<YodaCondition>().expect_no_offenses("foo == bar\n");
    }

    #[test]
    fn accepts_interpolated_string_lhs() {
        test::<YodaCondition>().expect_no_offenses("\"#{a}\" == foo\n");
    }

    #[test]
    fn accepts_interpolated_regexp_lhs() {
        test::<YodaCondition>().expect_no_offenses("/#{pattern}/ == foo\n");
    }

    #[test]
    fn accepts_const_on_right() {
        // CONST == 99: const on right (lhs is non-const), no offense in forbid style
        test::<YodaCondition>().expect_no_offenses("CONST == 99\n");
    }

    #[test]
    fn accepts_triple_eq() {
        // === is noncommutative -> never flagged by on_node filter
        test::<YodaCondition>().expect_no_offenses("1 === x\n");
    }

    // ----- forbid_for_equality_operators_only -----

    #[test]
    fn forbid_equality_only_flags_eq() {
        test::<YodaCondition>()
            .with_options(&YodaConditionOptions {
                enforced_style: YodaConditionStyle::ForbidForEqualityOperatorsOnly,
            })
            .expect_correction(
                indoc! {"
                    99 == foo
                    ^^^^^^^^^ Reverse the order of the operands `99 == foo`.
                "},
                "foo == 99\n",
            );
    }

    #[test]
    fn forbid_equality_only_accepts_relational() {
        test::<YodaCondition>()
            .with_options(&YodaConditionOptions {
                enforced_style: YodaConditionStyle::ForbidForEqualityOperatorsOnly,
            })
            .expect_no_offenses("99 >= foo\n");
    }

    // ----- require_for_all_comparison_operators -----

    #[test]
    fn require_all_flags_normal_order_eq() {
        test::<YodaCondition>()
            .with_options(&YodaConditionOptions {
                enforced_style: YodaConditionStyle::RequireForAllComparisonOperators,
            })
            .expect_correction(
                indoc! {"
                    foo == 99
                    ^^^^^^^^^ Reverse the order of the operands `foo == 99`.
                "},
                "99 == foo\n",
            );
    }

    #[test]
    fn require_all_flags_normal_order_lt() {
        test::<YodaCondition>()
            .with_options(&YodaConditionOptions {
                enforced_style: YodaConditionStyle::RequireForAllComparisonOperators,
            })
            .expect_correction(
                indoc! {"
                    bar > 10
                    ^^^^^^^^ Reverse the order of the operands `bar > 10`.
                "},
                "10 < bar\n",
            );
    }

    #[test]
    fn require_all_accepts_yoda_order() {
        test::<YodaCondition>()
            .with_options(&YodaConditionOptions {
                enforced_style: YodaConditionStyle::RequireForAllComparisonOperators,
            })
            .expect_no_offenses("99 == foo\n");
    }

    // ----- require_for_equality_operators_only -----

    #[test]
    fn require_equality_only_flags_normal_eq() {
        test::<YodaCondition>()
            .with_options(&YodaConditionOptions {
                enforced_style: YodaConditionStyle::RequireForEqualityOperatorsOnly,
            })
            .expect_offense(indoc! {"
                foo == 99
                ^^^^^^^^^ Reverse the order of the operands `foo == 99`.
            "});
    }

    #[test]
    fn require_equality_only_accepts_relational() {
        test::<YodaCondition>()
            .with_options(&YodaConditionOptions {
                enforced_style: YodaConditionStyle::RequireForEqualityOperatorsOnly,
            })
            .expect_no_offenses("foo > 99\n");
    }

    // ----- sym and bool literals -----

    #[test]
    fn flags_sym_lhs() {
        test::<YodaCondition>().expect_offense(indoc! {"
            :foo == x
            ^^^^^^^^^ Reverse the order of the operands `:foo == x`.
        "});
    }

    #[test]
    fn flags_true_lhs() {
        test::<YodaCondition>().expect_offense(indoc! {"
            true == x
            ^^^^^^^^^ Reverse the order of the operands `true == x`.
        "});
    }
}
