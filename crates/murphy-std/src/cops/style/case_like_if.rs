//! `Style/CaseLikeIf` — flags `if-elsif` chains that can be converted to `case-when`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CaseLikeIf
//! upstream_version_checked: 1.86.2
//! version_added: "0.88"
//! safe: false
//! supports_autocorrect: false
//! status: partial
//! gap_issues: []
//! notes: >
//!   Core detection (equality operators, is_a?, ===, match/match?/=~ with regexp,
//!   Or fan-out, and Begin unwrapping) is implemented.
//!
//!   include?/cover? with range: parenthesized ranges `(1..5)` are represented
//!   as `NodeKind::Unknown` in Murphy v1 (Prism `ParenthesesNode` without a
//!   dedicated `RangeExpr` lowering in that position). The code paths for
//!   `include?` and `cover?` are present but will not fire for `(a..b)` receivers
//!   because the receiver is `Unknown`, not `RangeExpr`. False-negative (safe).
//!
//!   Autocorrect is intentionally omitted: the structural rewrite (inserting
//!   `case <target>`, replacing each branch-condition segment) is deferred to a
//!   follow-up.
//!
//!   Regexp named-capture guard: Murphy's `MatchWithLvasgn` node encodes "this
//!   =~ binds named captures", so `match_with_lvasgn` branches are correctly
//!   excluded. The `match`/`match?` named-capture guard (where a regexp argument
//!   has named groups) is not yet implemented; those shapes will not be flagged
//!   (false-negative, not false-positive).
//!
//!   class_reference guard: conditions of the form `x == Foo` (CamelCase const)
//!   are excluded because `case` uses `===` (class membership) while `==` is
//!   identity. ALL_CAPS constants (e.g. `CONST`) are considered non-class
//!   references and remain convertible, mirroring RuboCop's logic.
//! ```
//!
//! ## Matched shapes
//!
//! An `if-elsif` chain (at least `MinBranchesCount` branches, default 3) where
//! every branch condition reduces to a consistent target variable compared with
//! literals/constants via `==`, `eql?`, `equal?`, `===`, `is_a?`, range
//! `include?`/`cover?`, or regexp `match`/`match?`/`=~` — and branches may use
//! `||` to combine multiple conditions for the same target.
//!
//! ## No autocorrect
//!
//! The structural rewrite (inserting `case <target>`, replacing each
//! branch-condition segment) is deferred to a follow-up.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, Symbol, cop};

const MSG: &str = "Convert `if-elsif` to `case-when`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct CaseLikeIf;

#[derive(CopOptions)]
pub struct CaseLikeIfOptions {
    #[option(
        name = "MinBranchesCount",
        default = 3,
        description = "Minimum number of `if`/`elsif` branches to trigger this cop."
    )]
    pub min_branches_count: i64,
}

#[cop(
    name = "Style/CaseLikeIf",
    description = "Identifies places where `if-elsif` constructions can be replaced with `case-when`.",
    default_severity = "warning",
    default_enabled = true,
    options = CaseLikeIfOptions,
)]
impl CaseLikeIf {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !should_check(node, cx) {
        return;
    }

    let NodeKind::If { cond, .. } = *cx.kind(node) else {
        return;
    };
    let target = match find_target(cond, cx) {
        Some(t) => t,
        None => return,
    };

    let mut convertible = true;
    let mut branch_node = node;
    while let NodeKind::If { cond: branch_cond, else_: branch_else, .. } = *cx.kind(branch_node) {
        if regexp_with_working_captures(branch_cond, cx) {
            return;
        }

        let mut conditions: Vec<NodeId> = Vec::new();
        if !collect_conditions(branch_cond, target, &mut conditions, cx) {
            convertible = false;
            break;
        }

        match branch_else.get() {
            Some(else_id)
                if matches!(cx.kind(else_id), NodeKind::If { .. }) && cx.is_elsif(else_id) =>
            {
                branch_node = else_id;
            }
            _ => break,
        }
    }

    if !convertible {
        return;
    }

    cx.emit_offense(cx.range(node), MSG, None);
}

fn should_check(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_unless(node) || cx.is_elsif(node) || cx.is_modifier_form(node) || cx.is_ternary(node)
    {
        return false;
    }

    let NodeKind::If { else_, .. } = *cx.kind(node) else {
        return false;
    };
    let has_elsif = else_
        .get()
        .map(|e| matches!(cx.kind(e), NodeKind::If { .. }) && cx.is_elsif(e))
        .unwrap_or(false);
    if !has_elsif {
        return false;
    }

    let branch_count = count_if_branches(node, cx);
    let opts = cx.options_or_default::<CaseLikeIfOptions>();
    let min = opts.min_branches_count.max(0) as usize;
    branch_count >= min
}

fn count_if_branches(node: NodeId, cx: &Cx<'_>) -> usize {
    let mut count = 0;
    let mut cur = Some(node);
    while let Some(n) = cur {
        if !matches!(cx.kind(n), NodeKind::If { .. }) || cx.is_ternary(n) {
            break;
        }
        count += 1;
        let NodeKind::If { else_, .. } = *cx.kind(n) else {
            break;
        };
        cur = else_.get().filter(|&e| cx.is_elsif(e));
    }
    count
}

fn deparenthesize(node: NodeId, cx: &Cx<'_>) -> NodeId {
    let mut n = node;
    while let NodeKind::Begin(list) = cx.kind(n) {
        let children = cx.list(*list);
        if children.len() == 1 {
            n = children[0];
        } else {
            break;
        }
    }
    n
}

fn is_literal_or_const_ref(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.is_literal(node) || is_const_reference(node, cx)
}

fn is_const_reference(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Const { name, .. } = cx.kind(node) else {
        return false;
    };
    let name_str = cx.symbol_str(*name);
    name_str.len() > 1 && !name_str.chars().any(|c| c.is_ascii_lowercase())
}

fn is_class_reference(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Const { name, .. } = cx.kind(node) else {
        return false;
    };
    let name_str = cx.symbol_str(*name);
    name_str.chars().any(|c| c.is_ascii_lowercase())
}

fn find_target(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match cx.kind(node) {
        NodeKind::Begin(list) => {
            let children = cx.list(*list);
            if children.is_empty() { None } else { find_target(children[0], cx) }
        }
        NodeKind::Or { lhs, .. } => find_target(*lhs, cx),
        NodeKind::MatchWithLvasgn { .. } => None,
        NodeKind::Send { receiver, method, args } => {
            find_target_in_send(*receiver, *method, *args, cx)
        }
        _ => None,
    }
}

fn find_target_in_send(
    receiver: OptNodeId,
    method: Symbol,
    args: murphy_plugin_api::NodeList,
    cx: &Cx<'_>,
) -> Option<NodeId> {
    let method_name = cx.symbol_str(method);
    let arg_list = cx.list(args);
    let first_arg = arg_list.first().copied();

    match method_name {
        "is_a?" => receiver.get(),
        "==" | "eql?" | "equal?" => {
            let recv = receiver.get()?;
            let arg = first_arg?;
            find_target_in_equality(recv, arg, cx)
        }
        "===" => first_arg,
        "include?" | "cover?" => {
            let recv = receiver.get()?;
            let recv_inner = deparenthesize(recv, cx);
            if matches!(cx.kind(recv_inner), NodeKind::RangeExpr { .. }) {
                first_arg
            } else {
                None
            }
        }
        "match" | "match?" | "=~" => {
            let recv = receiver.get()?;
            let arg = first_arg?;
            find_target_in_match(recv, arg, cx)
        }
        _ => None,
    }
}

fn find_target_in_equality(recv: NodeId, arg: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    if is_literal_or_const_ref(arg, cx) {
        Some(recv)
    } else if is_literal_or_const_ref(recv, cx) {
        Some(arg)
    } else {
        None
    }
}

fn find_target_in_match(recv: NodeId, arg: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    if matches!(cx.kind(recv), NodeKind::Regexp { .. }) {
        Some(arg)
    } else if matches!(cx.kind(arg), NodeKind::Regexp { .. }) {
        Some(recv)
    } else {
        None
    }
}

fn collect_conditions(node: NodeId, target: NodeId, out: &mut Vec<NodeId>, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Begin(list) => {
            let children = cx.list(*list);
            if children.is_empty() {
                return false;
            }
            collect_conditions(children[0], target, out, cx)
        }
        NodeKind::Or { lhs, rhs } => {
            let (lhs, rhs) = (*lhs, *rhs);
            collect_conditions(lhs, target, out, cx) && collect_conditions(rhs, target, out, cx)
        }
        NodeKind::MatchWithLvasgn { .. } => false,
        NodeKind::Send { receiver, method, args } => {
            let cond = condition_from_send(*receiver, *method, *args, target, cx);
            if let Some(c) = cond {
                out.push(c);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn condition_from_send(
    receiver: OptNodeId,
    method: Symbol,
    args: murphy_plugin_api::NodeList,
    target: NodeId,
    cx: &Cx<'_>,
) -> Option<NodeId> {
    let method_name = cx.symbol_str(method);
    let arg_list = cx.list(args);
    let first_arg = arg_list.first().copied();
    let recv = receiver.get();

    match method_name {
        "is_a?" => {
            let r = recv?;
            if source_eq(r, target, cx) { first_arg } else { None }
        }
        "==" | "eql?" | "equal?" => {
            let r = recv?;
            let arg = first_arg?;
            let cond = condition_from_binary_op(r, arg, target, cx)?;
            if is_class_reference(cond, cx) { None } else { Some(cond) }
        }
        "=~" | "match" | "match?" => {
            let r = recv?;
            let arg = first_arg?;
            condition_from_binary_op(r, arg, target, cx)
        }
        "===" => {
            let r = recv?;
            let arg = first_arg?;
            if source_eq(arg, target, cx) { Some(r) } else { None }
        }
        "include?" | "cover?" => {
            let r = recv?;
            let r_inner = deparenthesize(r, cx);
            let arg = first_arg?;
            if matches!(cx.kind(r_inner), NodeKind::RangeExpr { .. }) && source_eq(arg, target, cx)
            {
                Some(r_inner)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn condition_from_binary_op(
    lhs: NodeId,
    rhs: NodeId,
    target: NodeId,
    cx: &Cx<'_>,
) -> Option<NodeId> {
    let lhs_inner = deparenthesize(lhs, cx);
    let rhs_inner = deparenthesize(rhs, cx);
    if source_eq(lhs_inner, target, cx) {
        Some(rhs_inner)
    } else if source_eq(rhs_inner, target, cx) {
        Some(lhs_inner)
    } else {
        None
    }
}

fn source_eq(a: NodeId, b: NodeId, cx: &Cx<'_>) -> bool {
    cx.raw_source(cx.range(a)) == cx.raw_source(cx.range(b))
}

fn regexp_with_working_captures(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::MatchWithLvasgn { .. })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{CaseLikeIf, CaseLikeIfOptions};
    use murphy_plugin_api::test_support::{indoc, run_cop, run_cop_with_options, test};

    fn hits(src: &str) -> usize {
        run_cop::<CaseLikeIf>(src).len()
    }

    // ---- Positive cases (offense) ----------------------------------------

    #[test]
    fn flags_three_branch_if_elsif_with_equality() {
        let src = indoc! {r#"
            if status == :active
              perform_action
            elsif status == :inactive || status == :hibernating
              check_timeout
            elsif status == :invalid
              report_invalid
            else
              final_action
            end
        "#};
        assert_eq!(hits(src), 1);
    }

    #[test]
    fn flags_three_branch_if_elsif_with_eql() {
        let src = indoc! {r#"
            if x.eql?(:a)
              :a
            elsif x.eql?(:b)
              :b
            elsif x.eql?(:c)
              :c
            end
        "#};
        assert_eq!(hits(src), 1);
    }

    #[test]
    fn flags_triple_elsif_with_is_a() {
        let src = indoc! {r#"
            if x.is_a?(Integer)
              :int
            elsif x.is_a?(String)
              :str
            elsif x.is_a?(Symbol)
              :sym
            end
        "#};
        assert_eq!(hits(src), 1);
    }

    #[test]
    fn flags_triple_branch_with_triple_equals() {
        let src = indoc! {r#"
            if Integer === x
              :int
            elsif String === x
              :str
            elsif Symbol === x
              :sym
            end
        "#};
        assert_eq!(hits(src), 1);
    }

    #[test]
    fn flags_or_combined_conditions() {
        let src = indoc! {r#"
            if x == :a || x == :b
              :ab
            elsif x == :c
              :c
            elsif x == :d
              :d
            end
        "#};
        assert_eq!(hits(src), 1);
    }

    #[test]
    fn range_include_with_parenthesized_range_is_v1_gap() {
        // `(1..5)` is a `NodeKind::Unknown` in Murphy v1 (parenthesized range
        // without a dedicated lowering). The include?/cover? range path
        // silently produces a false-negative — this is documented in the
        // murphy-parity block as a known v1 limitation.
        let src = indoc! {r#"
            if (1..5).include?(x)
              :low
            elsif (6..10).include?(x)
              :mid
            elsif (11..20).include?(x)
              :high
            end
        "#};
        assert_eq!(hits(src), 0, "known v1 gap: parenthesized range (a..b) is Unknown");
    }

    #[test]
    fn flags_regexp_match_branches() {
        let src = indoc! {r#"
            if x.match?(/\Afoo\z/)
              :foo
            elsif x.match?(/\Abar\z/)
              :bar
            elsif x.match?(/\Abaz\z/)
              :baz
            end
        "#};
        assert_eq!(hits(src), 1);
    }

    #[test]
    fn flags_all_caps_const_as_non_class_reference() {
        let src = indoc! {r#"
            if status == ACTIVE
              :active
            elsif status == INACTIVE
              :inactive
            elsif status == INVALID
              :invalid
            end
        "#};
        assert_eq!(hits(src), 1);
    }

    #[test]
    fn offense_message_is_correct() {
        let src = indoc! {r#"
            if x == :a
              :a
            elsif x == :b
              :b
            elsif x == :c
              :c
            end
        "#};
        let offenses = run_cop::<CaseLikeIf>(src);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Convert `if-elsif` to `case-when`.");
    }

    // ---- Negative cases (no offense) -------------------------------------

    #[test]
    fn no_offense_only_two_branches() {
        test::<CaseLikeIf>().expect_no_offenses(indoc! {r#"
            if status == :active
              perform_action
            elsif status == :inactive
              check_timeout
            end
        "#});
    }

    #[test]
    fn no_offense_plain_if_else_no_elsif() {
        test::<CaseLikeIf>().expect_no_offenses(indoc! {r#"
            if x == 1
              :one
            else
              :other
            end
        "#});
    }

    #[test]
    fn no_offense_unless() {
        test::<CaseLikeIf>().expect_no_offenses(indoc! {r#"
            unless x == 1
              :not_one
            end
        "#});
    }

    #[test]
    fn no_offense_modifier_form() {
        test::<CaseLikeIf>().expect_no_offenses("do_something if x == 1\n");
    }

    #[test]
    fn no_offense_ternary() {
        test::<CaseLikeIf>().expect_no_offenses("x == 1 ? :one : :other\n");
    }

    #[test]
    fn no_offense_mixed_targets() {
        test::<CaseLikeIf>().expect_no_offenses(indoc! {r#"
            if x == 1
              :one
            elsif y == 2
              :two
            elsif z == 3
              :three
            end
        "#});
    }

    #[test]
    fn no_offense_class_reference_in_equality() {
        // `x == Foo` is a class/module ref (CamelCase) — unsafe to use in `when`
        // because `case` uses `===` (class membership) not `==` (identity).
        test::<CaseLikeIf>().expect_no_offenses(indoc! {r#"
            if x == Foo
              :foo
            elsif x == Bar
              :bar
            elsif x == Baz
              :baz
            end
        "#});
    }

    #[test]
    fn no_offense_named_capture_match_with_lvasgn() {
        // `/(?<n>...)/ =~ str` binds named captures — converting to `case` would lose them.
        test::<CaseLikeIf>().expect_no_offenses(indoc! {r#"
            if /(?<name>\w+)/ =~ x
              name
            elsif /(?<age>\d+)/ =~ x
              age.to_i
            elsif /(?<date>\d{4})/ =~ x
              date
            end
        "#});
    }

    #[test]
    fn no_offense_non_matching_method() {
        test::<CaseLikeIf>().expect_no_offenses(indoc! {r#"
            if x.start_with?("a")
              :a
            elsif x.start_with?("b")
              :b
            elsif x.start_with?("c")
              :c
            end
        "#});
    }

    // ---- Option: MinBranchesCount ----------------------------------------

    #[test]
    fn custom_min_branches_count_two_flags_on_two_branches() {
        let opts = CaseLikeIfOptions { min_branches_count: 2 };
        let src = indoc! {r#"
            if x == :a
              :a
            elsif x == :b
              :b
            end
        "#};
        let offenses = run_cop_with_options::<CaseLikeIf>(src, &opts);
        assert_eq!(offenses.len(), 1);
    }

    #[test]
    fn custom_min_branches_count_four_no_offense_on_three() {
        test::<CaseLikeIf>()
            .with_options(&CaseLikeIfOptions {
                min_branches_count: 4,
            })
            .expect_no_offenses(indoc! {r#"
                if x == :a
                  :a
                elsif x == :b
                  :b
                elsif x == :c
                  :c
                end
            "#});
    }

    // ---- No double-firing on elsif node ----------------------------------

    #[test]
    fn emits_exactly_one_offense_not_per_elsif() {
        // The walker fires on each nested elsif `If` node too — the cop must
        // emit at most one offense (on the outer `if`).
        let src = indoc! {r#"
            if x == :a
              :a
            elsif x == :b
              :b
            elsif x == :c
              :c
            end
        "#};
        let offenses = run_cop::<CaseLikeIf>(src);
        assert_eq!(offenses.len(), 1, "should emit exactly one offense for the outer if");
    }
}

murphy_plugin_api::submit_cop!(CaseLikeIf);
