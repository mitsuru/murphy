//! `Lint/DuplicateMatchPattern` — flag repeated `in` patterns within a
//! single `case`/`in` expression.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DuplicateMatchPattern
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Dispatches on `case_match` and dedups the `in` branches by a structural
//!   identity string, mirroring RuboCop's `pattern_identity`. For `hash_pattern`
//!   and `match_alt` patterns the children's source is sorted so that
//!   order-independent forms compare equal (`{foo: a, bar: b}` == `{bar: b, foo: a}`,
//!   `0 | 1` == `1 | 0`). RuboCop relies on parser-gem's n-ary `match_alt`;
//!   Murphy's AST nests `MatchAlt` binarily, so the alternation is flattened to
//!   its leaf source strings before sorting (`0 | 1 | 2` == `2 | 1 | 0`). The
//!   guard source (`if`/`unless` condition) is appended to the identity, so the
//!   same pattern with different guards stays distinct. The offense is emitted on
//!   the pattern node, matching RuboCop's `add_offense(pattern)`.
//! ```

use std::collections::HashSet;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct DuplicateMatchPattern;

#[cop(
    name = "Lint/DuplicateMatchPattern",
    description = "Flag repeated `in` patterns in a `case`/`in` expression.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicateMatchPattern {
    #[on_node(kind = "case_match")]
    fn check_case_match(&self, node: NodeId, cx: &Cx<'_>) {
        let mut seen: HashSet<String> = HashSet::new();
        for &branch in cx.in_pattern_branches(node) {
            let Some(pattern) = cx.in_pattern_pattern(branch).get() else {
                continue;
            };
            let identity = pattern_identity(cx, branch, pattern);
            if !seen.insert(identity) {
                cx.emit_offense(cx.range(pattern), "Duplicate `in` pattern detected.", None);
            }
        }
    }
}

/// Structural identity of an `in` pattern, matching RuboCop's
/// `pattern_identity`. Hash patterns and alternation patterns are order
/// independent, so their child sources are sorted. The guard source (if any)
/// is appended so that identical patterns with different guards stay distinct.
fn pattern_identity(cx: &Cx<'_>, branch: NodeId, pattern: NodeId) -> String {
    let mut identity = match *cx.kind(pattern) {
        NodeKind::HashPattern(list) => {
            let mut parts: Vec<&str> = cx
                .list(list)
                .iter()
                .map(|&child| cx.raw_source(cx.range(child)))
                .collect();
            parts.sort_unstable();
            parts.join("\u{0}")
        }
        NodeKind::MatchAlt { .. } => {
            // parser-gem's `match_alt` is n-ary, but Murphy nests `MatchAlt`
            // binarily. Flatten to the leaf source strings so that the order
            // of the alternatives doesn't affect the identity.
            let mut parts: Vec<&str> = Vec::new();
            flatten_match_alt(cx, pattern, &mut parts);
            parts.sort_unstable();
            parts.join("\u{0}")
        }
        _ => cx.raw_source(cx.range(pattern)).to_string(),
    };

    if let Some(guard) = cx.in_pattern_guard(branch).get() {
        // Separator distinguishes a guarded pattern from an unguarded one
        // whose source happens to equal `pattern + guard`.
        identity.push('\u{1}');
        identity.push_str(cx.raw_source(cx.range(guard)));
    }

    identity
}

/// Collect the source strings of every non-`MatchAlt` leaf in a left-nested
/// `MatchAlt` tree, in left-to-right order (order is normalized by the caller's
/// sort).
fn flatten_match_alt<'a>(cx: &Cx<'a>, node: NodeId, out: &mut Vec<&'a str>) {
    match *cx.kind(node) {
        NodeKind::MatchAlt { left, right } => {
            flatten_match_alt(cx, left, out);
            flatten_match_alt(cx, right, out);
        }
        _ => out.push(cx.raw_source(cx.range(node))),
    }
}

murphy_plugin_api::submit_cop!(DuplicateMatchPattern);

#[cfg(test)]
mod tests {
    use super::DuplicateMatchPattern;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_simple_pattern() {
        test::<DuplicateMatchPattern>().expect_offense(indoc! {r#"
            case x
            in 'first'
              do_something
            in 'first'
               ^^^^^^^ Duplicate `in` pattern detected.
              do_something_else
            end
        "#});
    }

    #[test]
    fn allows_distinct_patterns() {
        test::<DuplicateMatchPattern>().expect_no_offenses(indoc! {r#"
            case x
            in 'first'
              do_something
            in 'second'
              do_something_else
            end
        "#});
    }

    #[test]
    fn flags_reordered_alternation() {
        test::<DuplicateMatchPattern>().expect_offense(indoc! {r#"
            case x
            in 0 | 1
              first_method
            in 1 | 0
               ^^^^^ Duplicate `in` pattern detected.
              second_method
            end
        "#});
    }

    #[test]
    fn flags_reordered_three_way_alternation() {
        test::<DuplicateMatchPattern>().expect_offense(indoc! {r#"
            case x
            in 0 | 1 | 2
              first_method
            in 2 | 1 | 0
               ^^^^^^^^^ Duplicate `in` pattern detected.
              second_method
            end
        "#});
    }

    #[test]
    fn allows_distinct_alternation() {
        test::<DuplicateMatchPattern>().expect_no_offenses(indoc! {r#"
            case x
            in 0 | 1
              first_method
            in 2 | 3
              second_method
            end
        "#});
    }

    #[test]
    fn flags_reordered_hash_pattern() {
        test::<DuplicateMatchPattern>().expect_offense(indoc! {r#"
            case x
            in foo: a, bar: b
              first_method
            in bar: b, foo: a
               ^^^^^^^^^^^^^^ Duplicate `in` pattern detected.
              second_method
            end
        "#});
    }

    #[test]
    fn allows_distinct_hash_pattern() {
        test::<DuplicateMatchPattern>().expect_no_offenses(indoc! {r#"
            case x
            in foo: a, bar: b
              first_method
            in bar: b, baz: c
              second_method
            end
        "#});
    }

    #[test]
    fn array_pattern_order_matters() {
        test::<DuplicateMatchPattern>().expect_no_offenses(indoc! {r#"
            case x
            in [foo, bar]
              first_method
            in [bar, foo]
              second_method
            end
        "#});
    }

    #[test]
    fn flags_identical_array_pattern() {
        test::<DuplicateMatchPattern>().expect_offense(indoc! {r#"
            case x
            in [foo, bar]
              first_method
            in [foo, bar]
               ^^^^^^^^^^ Duplicate `in` pattern detected.
              second_method
            end
        "#});
    }

    #[test]
    fn flags_same_pattern_and_guard() {
        test::<DuplicateMatchPattern>().expect_offense(indoc! {r#"
            case x
            in foo if bar
              first_method
            in foo if bar
               ^^^ Duplicate `in` pattern detected.
              second_method
            end
        "#});
    }

    #[test]
    fn allows_same_pattern_different_guard() {
        test::<DuplicateMatchPattern>().expect_no_offenses(indoc! {r#"
            case x
            in foo if bar
              first_method
            in foo if baz
              second_method
            end
        "#});
    }

    #[test]
    fn if_guard_and_unless_guard_with_same_condition_are_distinct() {
        // `if bar` and `unless bar` are opposite conditions. The guard range
        // covers the keyword (`if bar` vs `unless bar`), so the identities differ
        // and no duplicate is reported.
        test::<DuplicateMatchPattern>().expect_no_offenses(indoc! {r#"
            case x
            in foo if bar
              first_method
            in foo unless bar
              second_method
            end
        "#});
    }
}
