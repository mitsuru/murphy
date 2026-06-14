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
//! gap_issues: [murphy-a2x8]
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
//!   Endless methods (`def foo =\n\n value`) use RuboCop's separate
//!   `offending_endless_method?` / `register_offense_for_endless_method` path
//!   keyed off `node.loc.assignment` (the `=` token). Murphy has no
//!   assignment-operator loc on `NodeLoc` for a `def`, so the `=` is recovered
//!   with a paren-depth-0 token scan between the signature and the body
//!   (`endless_assignment_loc`); a `=` inside a default-argument value is not
//!   matched. An offense fires when the body begins more than one line below
//!   `=` AND the line directly after `=` is blank, with the message "Extra
//!   empty line detected at method body beginning." The correction removes the
//!   full run of blank lines after `=` (one idempotent edit), reaching the same
//!   fixpoint RuboCop reaches by re-running its single-line removal. Verified
//!   against RuboCop 1.86.2 (TargetRubyVersion 3.0).
//!
//!   ABI note: `NodeLoc` exposes only `expression`/`name` ranges, so the
//!   adjusted first line is derived from the arguments node's `expression`
//!   range (zero-width ⇒ no args ⇒ def line), matching RuboCop's
//!   `arguments.source_range&.last_line`.
//!
//!   Remaining gap (tracked in murphy-a2x8): parenless multi-line argument
//!   lists do not contribute their last line to the `adjusted_first_line`
//!   computation (only the parameter-list closing `)` line is used), so a blank
//!   line after a parenless multi-line signature anchors on the method-name
//!   line rather than the signature's true last line.
//! ```

use crate::cops::util::{check_empty_lines_around_body_blank_run, physical_lines};
use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

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

    // `if node.endless? … else …`. RuboCop keys on `node.endless?`
    // (`loc.assignment` present), NOT on the absence of a trailing `end`: an
    // endless method whose RHS is itself an `if`/`case` (`def foo = if c … end`)
    // ends in an `end` keyword, so `end_keyword()` is non-zero even though the
    // method is endless. Detect the endless `=` operator directly instead.
    if is_endless(node, cx) {
        check_endless(node, cx);
        return;
    }

    // `adjusted_first_line = node.arguments.source_range&.last_line`.
    //
    // ABI note: Murphy's empty `Args` node carries the *whole def* range
    // rather than a parameter-list sub-range, so the args node's
    // `expression` cannot stand in for RuboCop's `arguments.source_range`.
    // Instead we derive the signature's last line from the parameter-list's
    // closing `)` token (when the def is parenthesized) or fall back to the
    // method-name line. This matches `arguments.source_range&.last_line` for
    // the common shapes; parenless multi-line argument lists are a documented
    // gap (murphy-a2x8).
    let first_line = adjusted_first_line(node, cx);

    let last_line = line_1based(range.end.saturating_sub(1).max(range.start), cx);
    check_empty_lines_around_body_blank_run(cx, "method", first_line, last_line);
}

/// RuboCop's `node.endless?` (`loc.assignment` present). Keyed on the endless
/// `=` operator, not the absence of a trailing `end`: `def foo = if c … end`
/// has an `end` (the conditional's) yet is endless. Reuses `endless_assignment_loc`,
/// whose scan is bounded to the gap between the signature and the body so a
/// default-argument `=` or the body's own `=` is never mistaken for it.
fn is_endless(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(body) = cx.def_body(node).get() else {
        return false;
    };
    endless_assignment_loc(node, body, cx).is_some()
}

/// RuboCop's endless-method path:
///
/// ```ruby
/// if node.endless?
///   return unless offending_endless_method?(node)
///   register_offense_for_endless_method(node)
/// end
///
/// def offending_endless_method?(node)
///   node.body.first_line > node.loc.assignment.line + 1 &&
///     processed_source.lines[node.loc.assignment.line].empty?
/// end
/// ```
///
/// `node.loc.assignment` is the `=` token between the signature and the body.
/// Murphy has no assignment-operator loc on `NodeLoc` for a `def`, so the `=`
/// is located by a token scan between the method name (or parameter-list close)
/// and the body's start. An offense is registered when the body begins more
/// than one line below `=` AND the line directly after `=` is blank. The
/// correction removes that blank line; mirroring this cop's body-boundary
/// correction, the full run of consecutive blank lines is removed so a single
/// pass reaches a clean fixpoint.
fn check_endless(node: NodeId, cx: &Cx<'_>) {
    let Some(body) = cx.def_body(node).get() else {
        return;
    };
    let Some(assignment) = endless_assignment_loc(node, body, cx) else {
        return;
    };

    let assignment_line = line_1based(assignment.start, cx);
    let body_first_line = line_1based(cx.range(body).start, cx);

    // `node.body.first_line > node.loc.assignment.line + 1`
    if body_first_line <= assignment_line + 1 {
        return;
    }

    // `processed_source.lines[node.loc.assignment.line].empty?` — RuboCop's
    // 0-based `lines[assignment.line]` is the line directly after the 1-based
    // assignment line.
    let lines = physical_lines(cx.source());
    let after_idx = assignment_line; // 0-based index of the line after `=`
    let Some(&after_line) = lines.get(after_idx) else {
        return;
    };
    if !after_line.blank {
        return;
    }

    cx.emit_offense(
        Range {
            start: after_line.start,
            end: after_line.end,
        },
        "Extra empty line detected at method body beginning.",
        None,
    );

    // Remove the full run of consecutive blank lines after `=` (one idempotent
    // edit). RuboCop removes a single line per pass and re-runs to fixpoint;
    // removing the whole run reaches the same end state in one pass.
    let mut hi = after_idx;
    while hi + 1 < lines.len() && lines[hi + 1].blank {
        hi += 1;
    }
    cx.emit_edit(
        Range {
            start: after_line.start,
            end: lines[hi].end,
        },
        "",
    );
}

/// The `=` operator range of an endless method (`def foo = body`), located by
/// scanning for the first `=` token between the method signature and the body
/// (bounded by the body's start, keyed on a depth-0 `=`).
///
/// The scan starts **after the parameter list's closing `)`** when the signature
/// is parenthesized — Murphy's `def` name loc is 0, so a scan from the `def`
/// keyword would also see a parenthesized default `=` (`def foo(a = 1) = …`) and,
/// worse, an assignment-method selector `=` (`def foo=(x)`), both at depth 0
/// before the `(`. Anchoring past the `)` skips them. For a non-parenthesized
/// signature the scan starts at the `def` keyword.
///
/// A *parenless* default (`def foo a = 1`, `def foo= x`) has its `=` at depth 0
/// with no `)` to anchor past, so we bail early: a method with parenless
/// parameters is never endless (Ruby parses `name = expr` after a parenless
/// parameter as a default-argument optarg — `def foo x = x` is even a "circular
/// argument reference" error). Returns `None` when no endless `=` is found.
fn endless_assignment_loc(node: NodeId, body: NodeId, cx: &Cx<'_>) -> Option<Range> {
    if has_parenless_params(node, cx) {
        return None;
    }
    let body_start = cx.range(body).start;
    let scan_start =
        param_list_close_end(node, body_start, cx).unwrap_or_else(|| cx.range(node).start);

    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < scan_start);
    let mut depth = 0i32;
    for tok in toks[idx..]
        .iter()
        .take_while(|t| t.range.start < body_start)
    {
        match tok.kind {
            SourceTokenKind::LeftParen => depth += 1,
            SourceTokenKind::RightParen if depth > 0 => depth -= 1,
            SourceTokenKind::Other if depth == 0 && cx.raw_source(tok.range) == "=" => {
                return Some(tok.range);
            }
            _ => {}
        }
    }
    None
}

/// Byte offset just past the parameter list's closing `)`, or `None` when the
/// signature is not parenthesized. The parameter list is the **last** top-level
/// `(…)` group before the body: a singleton receiver contributes an earlier
/// group (`def (obj).foo=(x)` has `(obj)` then the parameter `(x)`), so returning
/// the first close would land on the receiver's `)` — before the method name —
/// and re-expose the selector `=`. Bounded by the body start so parentheses
/// inside the body are never mistaken for the parameter list.
fn param_list_close_end(node: NodeId, body_start: u32, cx: &Cx<'_>) -> Option<u32> {
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < cx.range(node).start);
    let mut depth = 0i32;
    let mut last_close = None;
    for tok in toks[idx..]
        .iter()
        .take_while(|t| t.range.start < body_start)
    {
        match tok.kind {
            SourceTokenKind::LeftParen => depth += 1,
            SourceTokenKind::RightParen if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    last_close = Some(tok.range.end);
                }
            }
            _ => {}
        }
    }
    last_close
}

/// True when the def has *parenless* parameters (`def foo a = 1`, `def foo a`).
/// Such a method is never endless, so its depth-0 default `=` must not be read
/// as the endless operator. Parenthesized parameter lists return `false` (their
/// default `=` is at depth > 0 and the scan handles it); a def with no
/// parameters also returns `false`.
///
/// Murphy's `def` name loc and the `Args` node's range are both unreliable here
/// (name loc is 0; the empty/whole-signature `Args` node spans the entire def),
/// so we key on the first parameter node: a parenthesized list has a `(`
/// immediately before its first parameter, a parenless one is preceded by the
/// method name.
fn has_parenless_params(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(args) = cx.def_arguments(node).get() else {
        return false;
    };
    let NodeKind::Args(list) = cx.kind(args) else {
        return false;
    };
    let Some(&first) = cx.list(*list).first() else {
        return false; // no parameters
    };
    !matches!(
        cx.token_before(cx.range(first).start).map(|t| t.kind),
        Some(SourceTokenKind::LeftParen)
    )
}

/// The 1-based physical line where the method signature ends — RuboCop's
/// `adjusted_first_line`. Uses the parameter-list closing `)` line when the
/// def has parenthesized parameters; otherwise the method-name line (which is
/// the `def` line for the common single-line `def foo` / `def foo arg` shapes).
///
/// The parameter-list close is located via [`param_list_close_end`], which
/// anchors its scan at `cx.range(node).start` and takes the *last* top-level
/// `)` before the body. Anchoring at the def's own start is load-bearing: a
/// `def`'s name loc is `{0,0}` in Murphy's ABI, so a scan keyed off
/// `name_range.end` would begin at byte 0 of the file and — for a method with a
/// preceding sibling (both wrapped in a top-level `begin`) — walk into the
/// earlier sibling's body and latch onto an inner call's `)` (murphy-1kiw).
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

    // `close_end` is the offset just past the `)`; `-1` lands on the `)` itself
    // (a single byte) so the line maps to the closing-paren line.
    match param_list_close_end(node, body_start, cx) {
        Some(close_end) => line_1based(close_end.saturating_sub(1), cx),
        None => name_line,
    }
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

    /// Parity pin (Codex #387): an endless method whose RHS is itself an
    /// `if`/`case` expression ending in `end` must be detected as endless by the
    /// assignment operator, not by the absence of a trailing `end` (the body's
    /// own `end` makes `end_keyword()` non-zero). Here the body (`if`) starts on
    /// the assignment line, so RuboCop's `offending_endless_method?` is false and
    /// the blank line is NOT a method-body-beginning offense.
    #[test]
    fn accepts_blank_after_endless_def_with_conditional_body() {
        let offenses =
            run_cop::<EmptyLinesAroundMethodBody>("def foo = if cond\n\n  value\nend\n");
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    /// Parity pin (Codex #387): a method with *parenless* default arguments
    /// (`def foo a = 1`) is a regular method, not endless — Ruby parses the
    /// parenless `a = 1` as a default-argument optarg. The endless-`=` scan must
    /// not mistake that depth-0 `=` for the endless operator; otherwise the
    /// normal body-boundary check is skipped and a blank line before `end` goes
    /// unreported. RuboCop 1.87 flags it as a method-body-end offense.
    #[test]
    fn flags_blank_before_end_in_parenless_optarg_method() {
        let offenses = run_cop::<EmptyLinesAroundMethodBody>("def foo a = 1\n  body\n\nend\n");
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at method body end."
        );
    }

    /// Discriminator (Codex #387): the parenless-params exclusion must NOT
    /// over-narrow. A genuinely endless method with *parenthesized* optargs
    /// (`def foo(a = 1) = body`) is still endless — the default `=` is at paren
    /// depth > 0 and the endless `=` follows the `)`. A blank line after the
    /// endless `=` is a method-body-beginning offense, as RuboCop 1.87 reports.
    #[test]
    fn flags_blank_after_eq_in_paren_optarg_endless_method() {
        let offenses = run_cop::<EmptyLinesAroundMethodBody>("def foo(a = 1) =\n\n  body\n");
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at method body beginning."
        );
    }

    /// Parity pin (Codex #387): an assignment method (`def foo=(x)`) is a regular
    /// method, not endless — but its selector `=` sits at paren depth 0 before the
    /// `(` (and the def name loc is 0), so a scan from the `def` keyword mistakes
    /// it for the endless operator. Anchoring the scan past the parameter list's
    /// `)` skips the selector, so the normal body-boundary check runs and the
    /// blank line before `end` is reported, as RuboCop 1.87 does.
    #[test]
    fn flags_blank_before_end_in_setter_method() {
        let offenses = run_cop::<EmptyLinesAroundMethodBody>("def foo=(x)\n  body\n\nend\n");
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at method body end."
        );
    }

    /// Parity pin (Codex #387): a singleton setter with a *parenthesized
    /// receiver* (`def (obj).foo=(x)`) is still a regular method. `param_list_close_end`
    /// must skip the receiver `(obj)` and anchor past the parameter `(x)` (the
    /// last top-level paren group) — anchoring on the receiver's `)` would resume
    /// the scan before `foo=`. The blank line before `end` is reported, matching
    /// RuboCop 1.87.
    #[test]
    fn flags_blank_before_end_in_singleton_setter_with_paren_receiver() {
        let offenses =
            run_cop::<EmptyLinesAroundMethodBody>("def (obj).foo=(x)\n  body\n\nend\n");
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at method body end."
        );
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

    /// Regression (murphy-1kiw): a blank line in the *middle* of a method body
    /// must not be reported as "method body beginning". A `def`'s name loc is
    /// `{0,0}` in Murphy's ABI, so `adjusted_first_line`'s paren scan anchored
    /// at `name_range.end` started at byte 0 of the file. With a sibling
    /// top-level method present, prism wraps both defs in a `begin`; processing
    /// the *second* def, the byte-0-anchored scan walked into the *first* def's
    /// body and picked up the `)` of an inner call (`bar(baz)`), anchoring the
    /// "beginning" at an unrelated mid-body blank line.
    #[test]
    fn accepts_blank_mid_body_with_sibling_method() {
        let src = "def a\n  x = bar(baz)\n\n  if w\n    q\n  end\nend\n\ndef b\n  c\nend\n";
        let offenses = run_cop::<EmptyLinesAroundMethodBody>(src);
        assert!(offenses.is_empty(), "got {offenses:?}");
    }

    /// Discriminator (murphy-1kiw): the byte-0-anchor fix must NOT over-suppress.
    /// A genuine blank line at the *beginning* of the second method's body is
    /// still an offense, even though the first method contains a paren call that
    /// the old buggy scan would have latched onto. Without the fix the scan
    /// mis-anchors and this real offense is mislocated/lost.
    #[test]
    fn flags_real_beginning_blank_in_sibling_method() {
        // `def b`'s body-beginning blank is the lone `\n` at byte 32 (line 6);
        // `def a`'s `bar(baz)` `)` sits at byte 19 — the offset the old buggy
        // scan would have latched onto.
        let src = "def a\n  x = bar(baz)\nend\n\ndef b\n\n  c\nend\n";
        let offenses = run_cop::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at method body beginning."
        );
        assert_eq!(offenses[0].range.start, 32, "got {offenses:?}");
    }

    // ---- endless methods (`offending_endless_method?`) ----

    /// A multi-line endless method with a blank line directly after `=` is an
    /// offense. RuboCop 1.86.2 (TargetRubyVersion 3.0): 1 offense at the body
    /// beginning, autocorrect removes the blank line.
    #[test]
    fn flags_blank_after_endless_assignment() {
        let src = "def foo =\n\n  value\n";
        let offenses = run_cop::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at method body beginning."
        );
    }

    #[test]
    fn corrects_blank_after_endless_assignment() {
        let src = "def foo =\n\n  value\n";
        let run = run_cop_with_edits::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(apply(src, &run.edits), "def foo =\n  value\n");
    }

    /// Endless method with parameters and a blank after `=`.
    #[test]
    fn flags_blank_after_endless_assignment_with_params() {
        let src = "def foo(a) =\n\n  a\n";
        let offenses = run_cop::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
    }

    /// `defs` (singleton) endless method with a blank after `=`.
    #[test]
    fn flags_blank_after_endless_singleton_assignment() {
        let src = "def self.foo =\n\n  value\n";
        let offenses = run_cop::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
    }

    /// An endless method with a default-argument `=` (`def foo(a = 1) =`): the
    /// endless-assignment `=` must be located past the default-arg `=` (which is
    /// at paren depth 1), so the blank after the real `=` is flagged exactly
    /// once. Verified against RuboCop 1.86.2 (TargetRubyVersion 3.0).
    #[test]
    fn flags_blank_after_endless_assignment_with_default_arg() {
        let src = "def foo(a = 1) =\n\n  value\n";
        let offenses = run_cop::<EmptyLinesAroundMethodBody>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(
            offenses[0].message,
            "Extra empty line detected at method body beginning."
        );
    }

    /// No blank after `=` — multi-line endless method is clean.
    #[test]
    fn accepts_endless_assignment_no_blank() {
        test::<EmptyLinesAroundMethodBody>().expect_no_offenses("def foo =\n  value\n");
    }

    /// Two blank lines after `=` are collapsed in one offense's correction.
    #[test]
    fn corrects_multiple_blanks_after_endless_assignment() {
        let src = "def foo =\n\n\n  value\n";
        let run = run_cop_with_edits::<EmptyLinesAroundMethodBody>(src);
        let fixed = apply(src, &run.edits);
        assert!(
            run_cop::<EmptyLinesAroundMethodBody>(&fixed).is_empty(),
            "not idempotent: {fixed:?}"
        );
    }
}
