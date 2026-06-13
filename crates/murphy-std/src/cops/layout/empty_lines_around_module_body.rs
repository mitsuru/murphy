//! `Layout/EmptyLinesAroundModuleBody` — flags empty lines at the very top or
//! bottom of a module body.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLinesAroundModuleBody
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-vf72]
//! notes: >
//!   Ports RuboCop's `EmptyLinesAroundBody` mixin (`KIND = 'module'`,
//!   `on_module`) for the default `EnforcedStyle: no_empty_lines`. The
//!   beginning boundary is the line after `module`; the ending boundary is the
//!   line before `end`. Each boundary fires independently, so a nil-body
//!   module with one blank line (`module Foo\n\nend`) emits two offenses,
//!   matching RuboCop. Autocorrect removes the full run of consecutive blank
//!   lines at each boundary (deduped when both boundaries hit the same run).
//!
//!   Gaps vs. upstream (tracked in murphy-vf72):
//!   - `EnforcedStyle: empty_lines` (insert missing blank lines) is not
//!     implemented — only `no_empty_lines` is honored.
//!   - `empty_lines_except_namespace` and `empty_lines_special` styles are not
//!     implemented.
//!   These need a config-time SupportedStyles surface; only the default style
//!   ships in this port.
//!
//!   ABI note: `NodeLoc` exposes only `expression`/`name` ranges (no `keyword`
//!   or `end` sub-ranges), so the cop works line-based off the module node's
//!   `expression` range, exactly as RuboCop's mixin does (it operates on
//!   `node.source_range.first_line`/`last_line`).
//! ```

use crate::cops::util::check_empty_lines_around_body_blank_run;
use murphy_plugin_api::{Cx, NoOptions, NodeId, cop};

#[derive(Default)]
pub struct EmptyLinesAroundModuleBody;

#[cop(
    name = "Layout/EmptyLinesAroundModuleBody",
    description = "Keeps track of empty lines around module bodies.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyLinesAroundModuleBody {
    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        let range = cx.range(node);
        // 1-based physical line numbers of the module node's source range.
        let first_line = line_1based(range.start, cx);
        let last_line = line_1based(range.end.saturating_sub(1).max(range.start), cx);
        check_empty_lines_around_body_blank_run(cx, "module", first_line, last_line);
    }
}

/// 1-based physical line of `offset` (number of `\n` strictly before `offset`,
/// plus one).
fn line_1based(offset: u32, cx: &Cx<'_>) -> usize {
    crate::cops::util::line_of(offset, cx) as usize + 1
}

murphy_plugin_api::submit_cop!(EmptyLinesAroundModuleBody);

#[cfg(test)]
mod tests {
    use super::EmptyLinesAroundModuleBody;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, test, CapturedEdit};

    /// Apply non-overlapping edits left-to-right.
    fn apply(source: &str, edits: &[CapturedEdit]) -> String {
        let mut sorted: Vec<&CapturedEdit> = edits.iter().collect();
        sorted.sort_by_key(|e| e.range.start);
        let mut out = String::new();
        let mut cursor = 0usize;
        for e in sorted {
            out.push_str(&source[cursor..e.range.start as usize]);
            out.push_str(&e.replacement);
            cursor = e.range.end as usize;
        }
        out.push_str(&source[cursor..]);
        out
    }

    #[test]
    fn accepts_no_empty_lines() {
        test::<EmptyLinesAroundModuleBody>().expect_no_offenses("module Foo\n  x = 1\nend\n");
    }

    #[test]
    fn accepts_single_line_module() {
        test::<EmptyLinesAroundModuleBody>().expect_no_offenses("module Foo; end\n");
    }

    #[test]
    fn accepts_comment_after_opener() {
        // A comment line is not blank, so it is acceptable.
        test::<EmptyLinesAroundModuleBody>()
            .expect_no_offenses("module Foo\n  # a comment\n  x = 1\nend\n");
    }

    #[test]
    fn flags_empty_line_at_beginning() {
        let src = "module Foo\n\n  x = 1\nend\n";
        let offenses = run_cop::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at module body beginning."
        );
    }

    #[test]
    fn flags_empty_line_at_end() {
        let src = "module Foo\n  x = 1\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at module body end."
        );
    }

    #[test]
    fn flags_both_beginning_and_end() {
        let src = "module Foo\n\n  x = 1\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(offenses.len(), 2, "got {offenses:?}");
    }

    /// Nil body with one blank line: RuboCop emits two offenses (beginning and
    /// end both see the single blank line) but the autocorrect must remove it
    /// only once.
    #[test]
    fn flags_nil_body_single_blank_twice() {
        let src = "module Foo\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(offenses.len(), 2, "got {offenses:?}");
    }

    #[test]
    fn corrects_beginning() {
        let src = "module Foo\n\n  x = 1\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(apply(src, &run.edits), "module Foo\n  x = 1\nend\n");
    }

    #[test]
    fn corrects_end() {
        let src = "module Foo\n  x = 1\n\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(apply(src, &run.edits), "module Foo\n  x = 1\nend\n");
    }

    #[test]
    fn corrects_multiple_blank_lines_at_beginning() {
        let src = "module Foo\n\n\n\n  x = 1\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(apply(src, &run.edits), "module Foo\n  x = 1\nend\n");
    }

    #[test]
    fn corrects_nil_body_without_overlap() {
        // Two offenses, one deduped edit; result removes the single blank line.
        let src = "module Foo\n\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundModuleBody>(src);
        assert_eq!(apply(src, &run.edits), "module Foo\nend\n");
    }

    #[test]
    fn correction_is_idempotent() {
        let src = "module Foo\n\n  x = 1\n\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundModuleBody>(src);
        let fixed = apply(src, &run.edits);
        // Second pass must find no offenses.
        assert!(
            run_cop::<EmptyLinesAroundModuleBody>(&fixed).is_empty(),
            "not idempotent: {fixed:?}"
        );
    }
}
