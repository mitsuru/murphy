//! `Layout/EmptyLinesAroundClassBody` — keeps track of empty lines around
//! class (and singleton-class) bodies.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLinesAroundClassBody
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports RuboCop's `on_class`/`on_sclass` for the default
//!   `EnforcedStyle: no_empty_lines`. A blank line immediately after the
//!   class header (`adjusted_first_line` = the superclass's last line when
//!   present, else the `class`/`class <<` header line) or immediately before
//!   the closing `end` is flagged and removed. RuboCop's `&:empty?` blank
//!   test is literal — a whitespace-only line is NOT considered blank — and
//!   this matches that exactly. Single-line classes are skipped
//!   (`node.single_line?`). A class whose only inner line is blank
//!   (`class Foo\n\nend`) emits two offenses (beginning + end, different
//!   messages, matching RuboCop) but a single de-duplicated removal edit so
//!   the autocorrect edits never overlap.
//!   Messages:
//!     "Extra empty line detected at class body beginning."
//!     "Extra empty line detected at class body end."
//!   Gaps (documented, not bypassed):
//!     - Non-default `EnforcedStyle`s are not implemented:
//!       `empty_lines`, `empty_lines_except_namespace`, `empty_lines_special`,
//!       `beginning_only`, `ending_only`. These require the `empty_lines`
//!       (insert-a-blank) direction plus the namespace / special-case child
//!       analysis (`check_empty_lines_except_namespace` /
//!       `check_empty_lines_special`) and the `MSG_MISSING` / `MSG_DEFERRED`
//!       messages. Only `no_empty_lines` (the config default) is ported.
//! ```

use crate::cops::util::check_empty_lines_around_body_no_empty_lines;
use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLinesAroundClassBody;

#[cop(
    name = "Layout/EmptyLinesAroundClassBody",
    description = "Keeps track of empty lines around class bodies.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EmptyLinesAroundClassBody {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop: `first_line = node.parent_class.last_line if node.parent_class`.
        // When the class has a superclass, the header may span the superclass
        // expression; anchor the beginning check on the superclass's last line.
        let header_anchor = match *cx.kind(node) {
            NodeKind::Class { superclass, .. } => superclass
                .get()
                .map(|sc| cx.range(sc).end.saturating_sub(1))
                .unwrap_or(cx.range(node).start),
            _ => cx.range(node).start,
        };
        check_empty_lines_around_body_no_empty_lines(node, header_anchor, "class", cx);
    }

    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop: `on_sclass` calls `check(node, node.body)` with no adjusted
        // first line — the header is `class << expr`, anchored on the node start.
        check_empty_lines_around_body_no_empty_lines(node, cx.range(node).start, "class", cx);
    }
}

#[cfg(test)]
mod tests {
    use super::EmptyLinesAroundClassBody;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, test};

    #[test]
    fn accepts_class_without_surrounding_blank_lines() {
        test::<EmptyLinesAroundClassBody>().expect_no_offenses("class Foo\n  a = 1\nend\n");
    }

    #[test]
    fn flags_blank_line_at_beginning() {
        let src = "class Foo\n\n  a = 1\nend\n";
        let offenses = run_cop::<EmptyLinesAroundClassBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at class body beginning."
        );
    }

    #[test]
    fn flags_blank_line_at_end() {
        let src = "class Foo\n  a = 1\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundClassBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at class body end."
        );
    }

    #[test]
    fn flags_both_beginning_and_end() {
        let src = "class Foo\n\n  a = 1\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundClassBody>(src);
        assert_eq!(offenses.len(), 2, "expected 2 offenses, got {offenses:?}");
    }

    #[test]
    fn corrects_beginning_blank_line() {
        let src = "class Foo\n\n  a = 1\nend\n";
        let result = run_cop_with_edits::<EmptyLinesAroundClassBody>(src);
        assert_eq!(result.offenses.len(), 1);
        // The blank line is "\n" at byte 10..11 (after "class Foo\n").
        let edit = &result.edits[0];
        assert_eq!(edit.replacement, "");
        assert_eq!(edit.range.start, 10);
        assert_eq!(edit.range.end, 11);
    }

    #[test]
    fn single_blank_inner_line_emits_two_offenses_one_edit() {
        // `class Foo\n\nend` — the blank line is both beginning and end
        // candidate. RuboCop emits two offenses (different messages); the edit
        // is de-duplicated so corrections never overlap.
        let src = "class Foo\n\nend\n";
        let result = run_cop_with_edits::<EmptyLinesAroundClassBody>(src);
        assert_eq!(result.offenses.len(), 2, "{:?}", result.offenses);
        assert_eq!(result.edits.len(), 1, "edits must be de-duplicated");
    }

    #[test]
    fn accepts_single_line_class() {
        // `class Foo; end` is single-line — skipped.
        test::<EmptyLinesAroundClassBody>().expect_no_offenses("class Foo; end\n");
    }

    #[test]
    fn ignores_whitespace_only_line() {
        // RuboCop's `&:empty?` treats a whitespace-only line as NOT blank.
        test::<EmptyLinesAroundClassBody>().expect_no_offenses("class Foo\n  \n  a = 1\nend\n");
    }

    #[test]
    fn flags_blank_line_with_superclass() {
        let src = "class Foo < Bar\n\n  a = 1\nend\n";
        let offenses = run_cop::<EmptyLinesAroundClassBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at class body beginning."
        );
    }

    #[test]
    fn flags_blank_line_in_singleton_class() {
        let src = "class << self\n\n  def foo; end\nend\n";
        let offenses = run_cop::<EmptyLinesAroundClassBody>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at class body beginning."
        );
    }
}

murphy_plugin_api::submit_cop!(EmptyLinesAroundClassBody);
