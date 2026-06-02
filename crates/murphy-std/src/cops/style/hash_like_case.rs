//! `Style/HashLikeCase` — checks for `case-when` that is a simple 1:1 mapping
//! and can be replaced with a hash lookup.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashLikeCase
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports RuboCop's Style/HashLikeCase exactly.
//!
//!   Conditions (within a single `when` branch): must be a single node of
//!   type `Str` or `Sym`. A `when` with multiple conditions (e.g. `when 'a', 'b'`)
//!   is not a 1:1 mapping and is skipped.
//!
//!   Body per branch: must be non-nil and `recursive_basic_literal?`
//!   (cx.is_recursive_basic_literal).
//!
//!   Uniformity: all conditions must share the same NodeKind discriminant
//!   (all Str or all Sym), and all bodies must share the same NodeKind
//!   discriminant. Mirrors RuboCop's `nodes_of_same_type?`.
//!
//!   No `else` clause: if the case has an `else_` body, no offense.
//!
//!   `MinBranchesCount` (default 3): the number of `when` branches must be
//!   >= MinBranchesCount for an offense.
//!
//!   Offense range: the `case` keyword token only — mirrors RuboCop's
//!   `add_offense(node)` anchored to the case keyword for annotation
//!   purposes.
//!
//!   No autocorrect: RuboCop's HashLikeCase has no AutoCorrector and the
//!   rewrite (building a constant hash, replacing the case with a lookup)
//!   is structural — cannot be expressed as non-overlapping surgical edits.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (MinBranchesCount: 3, default)
//! case country
//! when 'europe'
//!   'http://eu.example.com'
//! when 'america'
//!   'http://us.example.com'
//! when 'australia'
//!   'http://au.example.com'
//! end
//!
//! # good — use a hash lookup instead
//! SITES = {
//!   'europe'    => 'http://eu.example.com',
//!   'america'   => 'http://us.example.com',
//!   'australia' => 'http://au.example.com'
//! }
//! SITES[country]
//! ```

use std::mem::discriminant;

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct HashLikeCase;

#[derive(CopOptions)]
pub struct HashLikeCaseOptions {
    #[option(
        name = "MinBranchesCount",
        default = 3,
        description = "Minimum number of `when` branches to trigger this cop."
    )]
    pub min_branches_count: i64,
}

const MSG: &str = "Consider replacing `case-when` with a hash lookup.";

#[cop(
    name = "Style/HashLikeCase",
    description = "Checks for `case-when` that is a simple 1:1 mapping replaceable with a hash.",
    default_severity = "warning",
    default_enabled = true,
    options = HashLikeCaseOptions,
)]
impl HashLikeCase {
    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Case { subject: _, whens, else_ } = *cx.kind(node) else {
        return;
    };

    // Must have no else clause.
    if else_.get().is_some() {
        return;
    }

    let when_nodes = cx.list(whens);

    // Min branches count gate.
    let opts = cx.options_or_default::<HashLikeCaseOptions>();
    let min = opts.min_branches_count.max(0) as usize;
    if when_nodes.len() < min {
        return;
    }

    // Collect the condition and body from each when branch.
    let mut cond_nodes: Vec<NodeId> = Vec::with_capacity(when_nodes.len());
    let mut body_nodes: Vec<NodeId> = Vec::with_capacity(when_nodes.len());

    for &when_id in when_nodes {
        let NodeKind::When { conds, body } = *cx.kind(when_id) else {
            return;
        };

        // Exactly one condition.
        let cond_list = cx.list(conds);
        if cond_list.len() != 1 {
            return;
        }
        let cond = cond_list[0];

        // Condition must be Str or Sym.
        if !matches!(cx.kind(cond), NodeKind::Str(_) | NodeKind::Sym(_)) {
            return;
        }

        // Body must be non-nil and recursive_basic_literal.
        let Some(body_id) = body.get() else {
            return;
        };
        if !cx.is_recursive_basic_literal(body_id) {
            return;
        }

        cond_nodes.push(cond);
        body_nodes.push(body_id);
    }

    // All conditions must share the same NodeKind discriminant.
    if !nodes_of_same_discriminant(&cond_nodes, cx) {
        return;
    }
    // All bodies must share the same NodeKind discriminant.
    if !nodes_of_same_discriminant(&body_nodes, cx) {
        return;
    }

    // Emit offense on the `case` keyword.
    cx.emit_offense(cx.loc(node).keyword(), MSG, None);
}

/// Returns true if all nodes in the slice have the same `NodeKind` discriminant.
fn nodes_of_same_discriminant(nodes: &[NodeId], cx: &Cx<'_>) -> bool {
    if nodes.is_empty() {
        return true;
    }
    let first_disc = discriminant(cx.kind(nodes[0]));
    nodes[1..].iter().all(|&n| discriminant(cx.kind(n)) == first_disc)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{HashLikeCase, HashLikeCaseOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_string_case() {
        test::<HashLikeCase>().expect_offense(indoc! {r#"
            case country
            ^^^^ Consider replacing `case-when` with a hash lookup.
            when 'europe'
              'http://eu.example.com'
            when 'america'
              'http://us.example.com'
            when 'australia'
              'http://au.example.com'
            end
        "#});
    }

    #[test]
    fn flags_symbol_case() {
        test::<HashLikeCase>().expect_offense(indoc! {r#"
            case country
            ^^^^ Consider replacing `case-when` with a hash lookup.
            when :europe
              :eu
            when :america
              :us
            when :australia
              :au
            end
        "#});
    }

    #[test]
    fn no_offense_with_else() {
        test::<HashLikeCase>().expect_no_offenses(indoc! {r#"
            case country
            when 'europe'
              'eu'
            when 'america'
              'us'
            when 'australia'
              'au'
            else
              'unknown'
            end
        "#});
    }

    #[test]
    fn no_offense_too_few_branches() {
        test::<HashLikeCase>().expect_no_offenses(indoc! {r#"
            case country
            when 'europe'
              'eu'
            when 'america'
              'us'
            end
        "#});
    }

    #[test]
    fn no_offense_multi_condition_when() {
        test::<HashLikeCase>().expect_no_offenses(indoc! {r#"
            case x
            when 'a', 'b'
              'ab'
            when 'c'
              'c'
            when 'd'
              'd'
            end
        "#});
    }

    #[test]
    fn no_offense_mixed_condition_types() {
        test::<HashLikeCase>().expect_no_offenses(indoc! {r#"
            case x
            when :foo
              'bar'
            when 'baz'
              'qux'
            when :quux
              'corge'
            end
        "#});
    }

    #[test]
    fn no_offense_non_literal_body() {
        test::<HashLikeCase>().expect_no_offenses(indoc! {r#"
            case x
            when 'a'
              some_method
            when 'b'
              other_method
            when 'c'
              yet_another
            end
        "#});
    }

    #[test]
    fn no_offense_mixed_body_types() {
        test::<HashLikeCase>().expect_no_offenses(indoc! {r#"
            case x
            when 'a'
              'string'
            when 'b'
              :symbol
            when 'c'
              'another_string'
            end
        "#});
    }

    #[test]
    fn custom_min_branches_count_2_flags_two_branches() {
        test::<HashLikeCase>()
            .with_options(&HashLikeCaseOptions {
                min_branches_count: 2,
            })
            .expect_offense(indoc! {r#"
                case country
                ^^^^ Consider replacing `case-when` with a hash lookup.
                when 'europe'
                  'eu'
                when 'america'
                  'us'
                end
            "#});
    }

    #[test]
    fn custom_min_branches_count_4_no_offense_on_three() {
        test::<HashLikeCase>()
            .with_options(&HashLikeCaseOptions {
                min_branches_count: 4,
            })
            .expect_no_offenses(indoc! {r#"
                case country
                when 'europe'
                  'eu'
                when 'america'
                  'us'
                when 'australia'
                  'au'
                end
            "#});
    }

    #[test]
    fn no_offense_integer_conditions() {
        test::<HashLikeCase>().expect_no_offenses(indoc! {r#"
            case x
            when 1
              'one'
            when 2
              'two'
            when 3
              'three'
            end
        "#});
    }

    #[test]
    fn no_offense_nil_body() {
        test::<HashLikeCase>().expect_no_offenses(indoc! {r#"
            case x
            when 'a'
            when 'b'
              'b'
            when 'c'
              'c'
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(HashLikeCase);
