//! `Layout/EmptyLinesAroundMethodBody` — flags empty lines at the very top or
//! bottom of a method body.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLinesAroundMethodBody
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-p8b2]
//! notes: >
//!   Ports RuboCop's `EmptyLinesAroundBody` mixin (`KIND = 'method'`,
//!   `on_def`/`on_defs`). This cop has no `EnforcedStyle` — it always enforces
//!   `no_empty_lines`. The beginning boundary uses the *adjusted first line* =
//!   the arguments' last line (`node.arguments.source_range&.last_line`), so a
//!   multi-line method signature anchors the "beginning" check at the line
//!   after the closing paren rather than the `def` line. When the method has
//!   no arguments the arguments node has no source range, and RuboCop falls
//!   back to `node.source_range.first_line` (the `def` line).
//!
//!   Gaps vs. upstream (tracked in murphy-p8b2):
//!   - Endless methods (`def foo =\n\n value`) use a separate
//!     `offending_endless_method?` path keyed off `node.loc.assignment`. Murphy
//!     has no assignment-operator loc on `NodeLoc`, so the endless-method path
//!     is not ported. Single-line endless methods never trip the body checks
//!     (they are `single_line?`), so the practical gap is only multi-line
//!     endless defs with a blank line after `=`.
//!
//!   ABI note: `NodeLoc` exposes only `expression`/`name` ranges, so the
//!   adjusted first line is derived from the arguments node's `expression`
//!   range (zero-width ⇒ no args ⇒ def line), matching RuboCop's
//!   `arguments.source_range&.last_line`.
//! ```

use crate::cops::util::check_empty_lines_around_body_blank_run;
use murphy_plugin_api::{Cx, NoOptions, NodeId, SourceTokenKind, cop};

#[derive(Default)]
pub struct EmptyLinesAroundMethodBody;

#[cop(
    name = "Layout/EmptyLinesAroundMethodBody",
    description = "Keeps track of empty lines around method bodies.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyLinesAroundMethodBody {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let range = cx.range(node);

    // `adjusted_first_line = node.arguments.source_range&.last_line`.
    //
    // ABI note: Murphy's empty `Args` node carries the *whole def* range
    // rather than a parameter-list sub-range, so the args node's
    // `expression` cannot stand in for RuboCop's `arguments.source_range`.
    // Instead we derive the signature's last line from the parameter-list's
    // closing `)` token (when the def is parenthesized) or fall back to the
    // method-name line. This matches `arguments.source_range&.last_line` for
    // the common shapes; parenless multi-line argument lists are a documented
    // gap (murphy-a12x).
    let first_line = adjusted_first_line(node, cx);

    let last_line = line_1based(range.end.saturating_sub(1).max(range.start), cx);
    check_empty_lines_around_body_blank_run(cx, "method", first_line, last_line);
}

/// The 1-based physical line where the method signature ends — RuboCop's
/// `adjusted_first_line`. Uses the parameter-list closing `)` line when the
/// def has parenthesized parameters; otherwise the method-name line (which is
/// the `def` line for the common single-line `def foo` / `def foo arg` shapes).
fn adjusted_first_line(node: NodeId, cx: &Cx<'_>) -> usize {
    let name_range = cx.loc(node).name;
    let name_line = line_1based(name_range.end.max(cx.range(node).start), cx);

    // Bound the paren scan by the body's start so parentheses inside the body
    // are never mistaken for the parameter list.
    let body_start = cx
        .def_body(node)
        .get()
        .map(|b| cx.range(b).start)
        .unwrap_or(cx.range(node).end);

    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < name_range.end);
    let mut depth = 0i32;
    let mut close_line: Option<usize> = None;
    for tok in toks[idx..]
        .iter()
        .take_while(|t| t.range.start < body_start)
    {
        match tok.kind {
            SourceTokenKind::LeftParen => depth += 1,
            SourceTokenKind::RightParen if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    // Closing paren of the (outermost) parameter list. The
                    // `depth > 0` guard prevents an unmatched `)` (invalid /
                    // incomplete syntax) from driving depth negative and then
                    // triggering a false early return on the next `(`.
                    close_line = Some(line_1based(tok.range.start, cx));
                    break;
                }
            }
            _ => {}
        }
    }

    close_line.unwrap_or(name_line)
}

/// 1-based physical line of `offset`.
fn line_1based(offset: u32, cx: &Cx<'_>) -> usize {
    crate::cops::util::line_of(offset, cx) as usize + 1
}

murphy_plugin_api::submit_cop!(EmptyLinesAroundMethodBody);

#[cfg(test)]
mod tests {
    use super::EmptyLinesAroundMethodBody;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, test, CapturedEdit};

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
        test::<EmptyLinesAroundMethodBody>().expect_no_offenses("def foo\n  x = 1\nend\n");
    }

    #[test]
    fn accepts_single_line_method() {
        test::<EmptyLinesAroundMethodBody>().expect_no_offenses("def foo; x; end\n");
    }

    #[test]
    fn accepts_endless_method() {
        test::<EmptyLinesAroundMethodBody>().expect_no_offenses("def foo = 42\n");
    }

    #[test]
    fn accepts_comment_after_opener() {
        test::<EmptyLinesAroundMethodBody>()
            .expect_no_offenses("def foo\n  # comment\n  x = 1\nend\n");
    }

    #[test]
    fn flags_empty_line_at_beginning() {
        let src = "def foo\n\n  x = 1\nend\n";
        let offenses = run_cop::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at method body beginning."
        );
    }

    #[test]
    fn flags_empty_line_at_end() {
        let src = "def foo\n  x = 1\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at method body end."
        );
    }

    /// Multi-line signature: the beginning anchor is the args' last line (the
    /// `b)` line), so a blank line after the closing paren is flagged.
    #[test]
    fn flags_empty_line_after_multiline_signature() {
        let src = "def foo(a,\n        b)\n\n  x = 1\nend\n";
        let offenses = run_cop::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at method body beginning."
        );
    }

    /// A blank line *inside* a multi-line signature (before the closing paren)
    /// is not a body boundary, so the cop does not fire there.
    #[test]
    fn accepts_no_body_blank_with_multiline_signature() {
        let src = "def foo(a,\n        b)\n  x = 1\nend\n";
        test::<EmptyLinesAroundMethodBody>().expect_no_offenses(src);
    }

    #[test]
    fn flags_nil_body_single_blank_twice() {
        let src = "def foo\n\nend\n";
        let offenses = run_cop::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(offenses.len(), 2, "got {offenses:?}");
    }

    #[test]
    fn corrects_beginning() {
        let src = "def foo\n\n  x = 1\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(apply(src, &run.edits), "def foo\n  x = 1\nend\n");
    }

    #[test]
    fn corrects_end() {
        let src = "def foo\n  x = 1\n\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(apply(src, &run.edits), "def foo\n  x = 1\nend\n");
    }

    #[test]
    fn corrects_nil_body_without_overlap() {
        let src = "def foo\n\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(apply(src, &run.edits), "def foo\nend\n");
    }

    #[test]
    fn correction_is_idempotent() {
        let src = "def foo\n\n  x = 1\n\nend\n";
        let run = run_cop_with_edits::<EmptyLinesAroundMethodBody>(src);
        let fixed = apply(src, &run.edits);
        assert!(
            run_cop::<EmptyLinesAroundMethodBody>(&fixed).is_empty(),
            "not idempotent: {fixed:?}"
        );
    }

    #[test]
    fn handles_singleton_method() {
        let src = "def self.foo\n\n  x = 1\nend\n";
        let offenses = run_cop::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at method body beginning."
        );
    }
}
