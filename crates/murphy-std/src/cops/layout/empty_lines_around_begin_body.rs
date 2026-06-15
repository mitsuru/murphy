//! `Layout/EmptyLinesAroundBeginBody` — keeps track of empty lines around
//! `begin ... end` bodies.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLinesAroundBeginBody
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports RuboCop's `on_kwbegin`, which calls `check(node, nil)` with the
//!   style hard-coded to `:no_empty_lines` (this cop has no `EnforcedStyle`
//!   config — `def style; :no_empty_lines; end`). Murphy's translator lowers
//!   keyword `begin ... end` to `NodeKind::Begin` (the `Kwbegin` variant is
//!   never produced), the same variant used by parenthesized expressions
//!   `(...)` and by the implicit begin a method-level `rescue` introduces. To
//!   fire only on RuboCop's `kwbegin`, the cop matches `kind = "begin"` and
//!   keeps only nodes whose first token is the `begin` keyword (`Other` ==
//!   "begin" at the node start) — the inverse of `is_parenthesized`'s
//!   LeftParen check. This excludes parenthesized expressions and implicit
//!   begins. A blank line immediately after the `begin` keyword line or
//!   immediately before the closing `end` is flagged and removed. RuboCop's
//!   `&:empty?` blank test is literal — a whitespace-only line is NOT blank —
//!   matched exactly. Single-line `begin`s are skipped. A `begin` whose only
//!   inner line is blank emits two offenses (beginning + end) and one
//!   de-duplicated edit.
//!   Messages:
//!     "Extra empty line detected at begin body beginning."
//!     "Extra empty line detected at begin body end."
//! ```

use crate::cops::util::check_empty_lines_around_body_no_empty_lines;
use murphy_plugin_api::{Cx, NoOptions, NodeId, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLinesAroundBeginBody;

#[cop(
    name = "Layout/EmptyLinesAroundBeginBody",
    description = "Keeps track of empty lines around begin-end bodies.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EmptyLinesAroundBeginBody {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        // Keep only keyword `begin ... end`. A `Begin` node is a keyword begin
        // iff its first token is the `begin` keyword at the node start; this
        // excludes parenthesized `(...)` (first token `(`) and the implicit
        // begin produced by a method-level `rescue` (range does not start at
        // a `begin` keyword).
        let range = cx.range(node);
        let start = range.start;
        let is_keyword_begin = cx.token_after(start).is_some_and(|t| {
            t.range.start == start
                && t.kind == SourceTokenKind::Other
                && cx.raw_source(t.range) == "begin"
        });
        if !is_keyword_begin {
            return;
        }
        // When a keyword `begin … end` is one of several statements in a body,
        // an implicit outer `(begin <kwbegin> <other-stmts>)` wrapper begins at
        // the *same* byte as the real `begin` keyword, so it also passes the
        // first-token check. Discriminate by the last token: a real begin … end
        // ends at the `end` keyword, whereas the wrapper ends at its last
        // statement. This prevents the wrapper from flagging the blank line that
        // follows the real `end`.
        let ends_with_end_keyword = cx.token_before(range.end).is_some_and(|t| {
            t.range.end == range.end
                && t.kind == SourceTokenKind::Other
                && cx.raw_source(t.range) == "end"
        });
        if !ends_with_end_keyword {
            return;
        }
        check_empty_lines_around_body_no_empty_lines(node, start, "begin", cx);
    }
}

#[cfg(test)]
mod tests {
    use super::EmptyLinesAroundBeginBody;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, test};

    #[test]
    fn accepts_begin_without_surrounding_blank_lines() {
        test::<EmptyLinesAroundBeginBody>().expect_no_offenses("begin\n  foo\nend\n");
    }

    #[test]
    fn flags_blank_line_at_beginning() {
        let src = "begin\n\n  foo\nend\n";
        let offenses = run_cop::<EmptyLinesAroundBeginBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at begin body beginning."
        );
    }

    #[test]
    fn flags_blank_line_at_end() {
        let src = "begin\n  foo\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundBeginBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at begin body end."
        );
    }

    #[test]
    fn flags_both_beginning_and_end() {
        let src = "begin\n\n  foo\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundBeginBody>(src);
        assert_eq!(offenses.len(), 2, "expected 2 offenses, got {offenses:?}");
    }

    #[test]
    fn flags_multi_statement_begin_once_at_beginning() {
        // The inner `Begin([a, b])` wrapper must not double-fire — only the
        // outer keyword-begin node starts with the `begin` token.
        let src = "begin\n\n  a\n  b\nend\n";
        let offenses = run_cop::<EmptyLinesAroundBeginBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at begin body beginning."
        );
    }

    #[test]
    fn corrects_beginning_blank_line() {
        // "begin\n" = bytes 0..6; blank "\n" = byte 6..7.
        let src = "begin\n\n  foo\nend\n";
        let result = run_cop_with_edits::<EmptyLinesAroundBeginBody>(src);
        assert_eq!(result.offenses.len(), 1);
        let edit = &result.edits[0];
        assert_eq!(edit.replacement, "");
        assert_eq!(edit.range.start, 6);
        assert_eq!(edit.range.end, 7);
    }

    #[test]
    fn no_offense_for_parenthesized_expression() {
        // A parenthesized multi-line expression is `Begin` with a LeftParen
        // first token — NOT a keyword begin. No offense.
        test::<EmptyLinesAroundBeginBody>().expect_no_offenses("x = (\n\n  a\n)\n");
    }

    #[test]
    fn no_offense_for_implicit_begin_from_def_rescue() {
        // `def foo; ...; rescue; ...; end` has an implicit begin whose range
        // does not start with a `begin` keyword. No begin-body offense.
        test::<EmptyLinesAroundBeginBody>()
            .expect_no_offenses("def foo\n\n  bar\nrescue\n  baz\nend\n");
    }

    #[test]
    fn flags_begin_rescue_blank_at_beginning() {
        // An explicit keyword `begin` with a `rescue` clause: the beginning
        // blank line is still flagged.
        let src = "begin\n\n  foo\nrescue\n  bar\nend\n";
        let offenses = run_cop::<EmptyLinesAroundBeginBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at begin body beginning."
        );
    }

    #[test]
    fn ignores_whitespace_only_line() {
        test::<EmptyLinesAroundBeginBody>().expect_no_offenses("begin\n  \n  foo\nend\n");
    }

    /// Mastodon FP: a `begin … rescue … end` that is one of several statements
    /// in a method body has an implicit outer `(begin <kwbegin> <stmts>)`
    /// wrapper starting at the *same* byte as the real `begin` keyword. The
    /// first-token heuristic is true for both, so the wrapper (whose range runs
    /// to the last statement) treats the blank line after the real `end` as a
    /// "body end" blank and flags it. Requiring the node's last token to be the
    /// `end` keyword discriminates the real kwbegin from the wrapper.
    #[test]
    fn no_offense_for_blank_after_begin_among_statements() {
        test::<EmptyLinesAroundBeginBody>().expect_no_offenses(concat!(
            "def f(u)\n",
            "  begin\n",
            "    x = parse(u)\n",
            "  rescue StandardError\n",
            "    return false\n",
            "  end\n",
            "\n",
            "  other(x)\n",
            "end\n",
        ));
    }

    #[test]
    fn single_blank_inner_line_emits_two_offenses_one_edit() {
        let src = "begin\n\nend\n";
        let result = run_cop_with_edits::<EmptyLinesAroundBeginBody>(src);
        assert_eq!(result.offenses.len(), 2, "{:?}", result.offenses);
        assert_eq!(result.edits.len(), 1, "edits must be de-duplicated");
    }
}

murphy_plugin_api::submit_cop!(EmptyLinesAroundBeginBody);
