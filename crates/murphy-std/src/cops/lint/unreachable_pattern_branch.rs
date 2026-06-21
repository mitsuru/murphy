//! `Lint/UnreachablePatternBranch` — flag `in` (and `else`) pattern-match
//! branches that can never be reached because an earlier unguarded catch-all
//! pattern already matches everything.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnreachablePatternBranch
//! upstream_version_checked: 1.87.0
//! version_added: "1.85"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues: [murphy-e7bz.19.1]
//! notes: >
//!   Faithful port of RuboCop's `on_case_match`: once an unguarded catch-all
//!   `in` pattern is seen, every later `in` branch is flagged, and a trailing
//!   `else` is flagged too. `catch_all_pattern?` is ported exactly — a bare
//!   `match_var` (any name, incl. `_`/`_foo`) is a catch-all; `match_as`
//!   recurses on its value pattern only (never the bind name); `match_alt`
//!   recurses on both alternatives; parenthesized patterns recurse through the
//!   `Begin` wrapper; everything else (array/hash/find/const/literal/pin) is
//!   not a catch-all. A catch-all only sets the flag when its `in` branch has
//!   no guard. Offense ranges follow Murphy's first-source-line convention
//!   (cf. `duplicate_branch`): the `in <pattern>` first line, or the `else`
//!   keyword — matching RuboCop's single-line carets for these spec cases.
//!
//!   KNOWN GAP (parser limitation, not subsumption logic): Murphy's parser
//!   lowers ANY parenthesized `in` pattern — `in (_)`, `in (_ | Integer)`,
//!   `in (_ | Integer) => y` — to `Begin([Unknown])`, discarding the inner
//!   pattern structure. The inner catch-all is therefore invisible, so the one
//!   RuboCop spec case `in (_ | Integer) => y` does not fire. Tracked by
//!   murphy-e7bz.19.1; the `catch_all_pattern?` recursion is already written to
//!   handle it once the parser preserves parenthesized pattern internals.
//! ```
//!
//! ## Matched shapes
//!
//! `case <subject>; in <pat>; ...; [else]; end`. The first unguarded catch-all
//! `in` arm makes every subsequent `in` arm — and any trailing `else` —
//! unreachable.
//!
//! ## No autocorrect
//!
//! RuboCop ships no autocorrect: removing an unreachable branch is a
//! behaviour-preserving cleanup, but the user may instead want to reorder the
//! branches, so the fix is ambiguous.

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Unreachable `in` pattern branch detected.";
const MSG_ELSE: &str = "Unreachable `else` branch detected.";

#[derive(Default)]
pub struct UnreachablePatternBranch;

#[cop(
    name = "Lint/UnreachablePatternBranch",
    description = "Checks for unreachable `in` pattern branches after an unconditional catch-all pattern.",
    default_severity = "warning",
    default_enabled = false
)]
impl UnreachablePatternBranch {
    #[on_node(kind = "case_match")]
    fn check_case_match(&self, node: NodeId, cx: &Cx<'_>) {
        let mut catch_all_found = false;

        for &in_pat in cx.in_pattern_branches(node) {
            if catch_all_found {
                let arm_start = cx.range(in_pat).start;
                let range = Range {
                    start: arm_start,
                    end: first_line_end(arm_start, cx),
                };
                cx.emit_offense(range, MSG, None);
                continue;
            }

            let Some(pattern) = cx.in_pattern_pattern(in_pat).get() else {
                continue;
            };
            let guarded = cx.in_pattern_guard(in_pat).get().is_some();
            if catch_all_pattern(pattern, cx) && !guarded {
                catch_all_found = true;
            }
        }

        if !catch_all_found {
            return;
        }

        let Some(else_body) = cx.case_match_else_branch(node).get() else {
            return;
        };
        let else_range = else_keyword_range(node, cx.range(else_body).start, cx)
            .unwrap_or_else(|| cx.range(else_body));
        cx.emit_offense(else_range, MSG_ELSE, None);
    }
}

/// RuboCop's `catch_all_pattern?`. A pattern matches everything when it is a
/// bare variable binding, or recursively when it is a capture (`pat => name`,
/// recursing on `pat`), an alternation (`a | b`, recursing on each side), or a
/// parenthesized pattern (recursing through the `Begin` wrapper).
fn catch_all_pattern(pattern: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(pattern) {
        NodeKind::MatchVar(_) => true,
        // `pat => name`: only the captured pattern matters; `name` is always a
        // bind variable and would falsely read as a catch-all.
        NodeKind::MatchAs { value, .. } => catch_all_pattern(value, cx),
        NodeKind::MatchAlt { left, right } => {
            catch_all_pattern(left, cx) || catch_all_pattern(right, cx)
        }
        // Parenthesized pattern — recurse through the single wrapped child.
        // (Murphy currently lowers parenthesized patterns to `Begin([Unknown])`,
        // so this recursion is a no-op in practice; see the parity GAP note.)
        NodeKind::Begin(_) => {
            let inner = crate::cops::util::unwrap_parenthesized(pattern, cx);
            inner != pattern && catch_all_pattern(inner, cx)
        }
        _ => false,
    }
}

/// End of the source line containing `start` (exclusive of the newline).
/// Mirrors RuboCop's single-line offense carets for `in` arms (cf.
/// `duplicate_branch`'s `first_line_range`).
fn first_line_end(start: u32, cx: &Cx<'_>) -> u32 {
    let source = cx.source().as_bytes();
    let mut end = start as usize;
    while end < source.len() && source[end] != b'\n' {
        end += 1;
    }
    end as u32
}

/// Find the `else` keyword token belonging to `node` that immediately precedes
/// the else body starting at `body_start`.
fn else_keyword_range(node: NodeId, body_start: u32, cx: &Cx<'_>) -> Option<Range> {
    let node_range = cx.range(node);
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < node_range.start);

    let mut found = None;
    for tok in &toks[idx..] {
        if tok.range.start >= body_start {
            break;
        }
        if tok.kind == SourceTokenKind::Other && cx.token_text(*tok) == "else" {
            found = Some(tok.range);
        }
    }
    found
}

murphy_plugin_api::submit_cop!(UnreachablePatternBranch);

#[cfg(test)]
mod tests {
    use super::UnreachablePatternBranch;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_in_branch_after_bare_variable_catch_all() {
        test::<UnreachablePatternBranch>().expect_offense(indoc! {r#"
            case value
            in x
              handle_other
            in Integer
            ^^^^^^^^^^ Unreachable `in` pattern branch detected.
              handle_integer
            end
        "#});
    }

    #[test]
    fn flags_in_branch_after_underscore_catch_all() {
        test::<UnreachablePatternBranch>().expect_offense(indoc! {r#"
            case value
            in _
              handle_other
            in Integer
            ^^^^^^^^^^ Unreachable `in` pattern branch detected.
              handle_integer
            end
        "#});
    }

    #[test]
    fn flags_multiple_in_branches_after_catch_all() {
        test::<UnreachablePatternBranch>().expect_offense(indoc! {r#"
            case value
            in Integer
              handle_integer
            in x
              handle_other
            in String
            ^^^^^^^^^ Unreachable `in` pattern branch detected.
              handle_string
            in Symbol
            ^^^^^^^^^ Unreachable `in` pattern branch detected.
              handle_symbol
            end
        "#});
    }

    #[test]
    fn flags_unreachable_else_branch() {
        test::<UnreachablePatternBranch>().expect_offense(indoc! {r#"
            case value
            in Integer
              handle_integer
            in _
              handle_other
            else
            ^^^^ Unreachable `else` branch detected.
              handle_else
            end
        "#});
    }

    #[test]
    fn flags_both_unreachable_in_and_else_branches() {
        test::<UnreachablePatternBranch>().expect_offense(indoc! {r#"
            case value
            in x
              handle_other
            in Integer
            ^^^^^^^^^^ Unreachable `in` pattern branch detected.
              handle_integer
            else
            ^^^^ Unreachable `else` branch detected.
              handle_else
            end
        "#});
    }

    #[test]
    fn flags_single_catch_all_with_else() {
        test::<UnreachablePatternBranch>().expect_offense(indoc! {r#"
            case value
            in x
              handle_any
            else
            ^^^^ Unreachable `else` branch detected.
              handle_else
            end
        "#});
    }

    #[test]
    fn flags_match_as_alias_with_underscore() {
        test::<UnreachablePatternBranch>().expect_offense(indoc! {r#"
            case value
            in _ => y
              handle_other
            in Integer
            ^^^^^^^^^^ Unreachable `in` pattern branch detected.
              handle_integer
            end
        "#});
    }

    #[test]
    fn flags_match_as_alias_with_variable() {
        test::<UnreachablePatternBranch>().expect_offense(indoc! {r#"
            case value
            in x => y
              handle_other
            in Integer
            ^^^^^^^^^^ Unreachable `in` pattern branch detected.
              handle_integer
            end
        "#});
    }

    #[test]
    fn flags_alternation_with_catch_all_on_left() {
        test::<UnreachablePatternBranch>().expect_offense(indoc! {r#"
            case value
            in _ | Integer
              handle_other
            in String
            ^^^^^^^^^ Unreachable `in` pattern branch detected.
              handle_string
            end
        "#});
    }

    #[test]
    fn flags_alternation_with_catch_all_on_right() {
        test::<UnreachablePatternBranch>().expect_offense(indoc! {r#"
            case value
            in Integer | _
              handle_other
            in String
            ^^^^^^^^^ Unreachable `in` pattern branch detected.
              handle_string
            end
        "#});
    }

    #[test]
    fn flags_branch_after_unguarded_catch_all_even_with_guarded_catch_all_before() {
        test::<UnreachablePatternBranch>().expect_offense(indoc! {r#"
            case value
            in x if x.positive?
              handle_positive
            in y
              handle_other
            in Integer
            ^^^^^^^^^^ Unreachable `in` pattern branch detected.
              handle_integer
            end
        "#});
    }

    // ── No-offense cases ────────────────────────────────────────────────

    #[test]
    fn accepts_catch_all_as_final_branch() {
        test::<UnreachablePatternBranch>().expect_no_offenses(indoc! {r#"
            case value
            in Integer
              handle_integer
            in String
              handle_string
            in x
              handle_other
            end
        "#});
    }

    #[test]
    fn accepts_only_specific_patterns() {
        test::<UnreachablePatternBranch>().expect_no_offenses(indoc! {r#"
            case value
            in Integer
              handle_integer
            in String
              handle_string
            else
              handle_other
            end
        "#});
    }

    #[test]
    fn accepts_array_pattern_first() {
        test::<UnreachablePatternBranch>().expect_no_offenses(indoc! {r#"
            case value
            in [*]
              handle_array
            in Integer
              handle_integer
            end
        "#});
    }

    #[test]
    fn accepts_hash_pattern_first() {
        test::<UnreachablePatternBranch>().expect_no_offenses(indoc! {r#"
            case value
            in **rest
              handle_hash
            in Integer
              handle_integer
            end
        "#});
    }

    #[test]
    fn accepts_literal_patterns() {
        test::<UnreachablePatternBranch>().expect_no_offenses(indoc! {r#"
            case value
            in 1
              handle_one
            in 2
              handle_two
            end
        "#});
    }

    #[test]
    fn accepts_find_pattern_first() {
        test::<UnreachablePatternBranch>().expect_no_offenses(indoc! {r#"
            case value
            in [*, 1, *]
              handle_contains_one
            in Integer
              handle_integer
            end
        "#});
    }

    #[test]
    fn accepts_catch_all_with_if_guard() {
        test::<UnreachablePatternBranch>().expect_no_offenses(indoc! {r#"
            case value
            in x if x.positive?
              handle_positive
            in Integer
              handle_integer
            end
        "#});
    }

    #[test]
    fn accepts_catch_all_with_unless_guard() {
        test::<UnreachablePatternBranch>().expect_no_offenses(indoc! {r#"
            case value
            in x unless x.nil?
              handle_not_nil
            in Integer
              handle_integer
            end
        "#});
    }

    #[test]
    fn accepts_match_as_wrapping_non_catch_all() {
        test::<UnreachablePatternBranch>().expect_no_offenses(indoc! {r#"
            case value
            in Integer => y
              handle_integer
            in String
              handle_string
            end
        "#});
    }

    #[test]
    fn accepts_alternation_of_non_catch_all() {
        test::<UnreachablePatternBranch>().expect_no_offenses(indoc! {r#"
            case value
            in Integer | String
              handle_int_or_string
            in Symbol
              handle_symbol
            end
        "#});
    }

    #[test]
    fn accepts_single_in_branch() {
        test::<UnreachablePatternBranch>().expect_no_offenses(indoc! {r#"
            case value
            in x
              handle_any
            end
        "#});
    }
}
