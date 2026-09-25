//! `Layout/EmptyLineAfterGuardClause` — require a blank line after a guard
//! clause (`return`/`next`/`break`/`raise`/`fail` in modifier `if`/`unless`
//! form) before the following code.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLineAfterGuardClause
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports RuboCop's `on_if`. A guard clause is a modifier-form `if`/`unless`
//!   whose condition-true branch (`if_branch`) is a `return`/`break`/`next` or a
//!   `raise`/`fail` command. When the immediately following physical line is
//!   non-blank, the guard clause must be separated from it by a blank line.
//!
//!   `correct_style?` exclusions are ported:
//!     - the if-branch is not a guard clause;
//!     - the parent is nil / a `rescue` / an `ensure` (`next_line_rescue_or_ensure?`);
//!     - the next sibling's parent is an `if` with an `else`
//!       (`next_sibling_parent_empty_or_else?`);
//!     - the next sibling is nil or an `if` whose if-branch is itself a guard
//!       clause (`next_sibling_empty_or_guard_clause?`) — so consecutive guards
//!       fire only once, on the last.
//!   `multiple_statements_on_line?` (the guard shares a line with its sibling)
//!   is also ported.
//!
//!   Heredoc-argument guards (`raise <<~MSG ... MSG if cond`) follow RuboCop's
//!   heredoc branch (murphy-cipl): the last heredoc argument is found via
//!   `last_heredoc_argument`, the "next line" is the line after the heredoc
//!   terminator (paired by FIFO `HeredocStart`/`HeredocEnd` token index, label
//!   newline trimmed to match the parser gem's `heredoc_end`), the offense
//!   lands on the terminator label, and autocorrect inserts the blank line
//!   after the terminator (or after a trailing allowed directive comment).
//!
//!   The directive-comment allowance (`next_line_empty_or_allowed_directive_comment?`)
//!   is ported: a `rubocop:enable`/`murphy:enable` directive comment or a
//!   SimpleCov directive (`:nocov:` / `simplecov:disable` / `simplecov:enable`)
//!   immediately after the guard (or heredoc terminator) is accepted when it is
//!   itself followed by a blank line; otherwise the offense still fires but
//!   autocorrect inserts the blank line after the directive comment.
//!
//!   Offense location is the whole modifier-form `If` node (it has no `end`
//!   keyword), except for heredoc guards where it is the terminator label.
//!   Autocorrect inserts a `\n` after the guard clause's line (or terminator /
//!   directive line).
//! ```
//!
//! ## Matched shapes
//!
//! Modifier-form `if`/`unless` guard clauses (including heredoc-argument
//! guards) not followed by a blank line (or allowed directive + blank).

use crate::cops::util::{line_is_blank, line_of, nth_line_start};
use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, SourceTokenKind, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct EmptyLineAfterGuardClause;

const MSG: &str = "Add empty line after guard clause.";

#[cop(
    name = "Layout/EmptyLineAfterGuardClause",
    description = "Add empty line after guard clause.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyLineAfterGuardClause {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop: `return if correct_style?(node)`.
        if correct_style(node, cx) {
            return;
        }
        // RuboCop: `return if multiple_statements_on_line?(node)`.
        if multiple_statements_on_line(node, cx) {
            return;
        }

        // RuboCop: `if node.modifier_form? && (heredoc_node = last_heredoc_argument(node))`.
        // `correct_style?` already requires modifier form, so only check the
        // heredoc argument here.
        if let Some(heredoc) = last_heredoc_argument(node, cx) {
            let Some(term_range) = heredoc_end_label_range(heredoc, cx) else {
                return;
            };
            let term_line = line_of(term_range.start, cx);
            let next = term_line + 1;
            // RuboCop: `return if next_line_empty_or_allowed_directive_comment?(heredoc_line(...))`.
            if next_line_empty_or_allowed_directive_comment(next, cx) {
                return;
            }
            cx.emit_offense(term_range, MSG, None);
            autocorrect_at_next_line(next, cx);
            return;
        }

        // RuboCop: `return if next_line_empty_or_allowed_directive_comment?(node.last_line)`.
        let node_end = cx.range(node).end;
        let guard_last_line = line_of(node_end.saturating_sub(1), cx);
        let next = guard_last_line + 1;
        if next_line_empty_or_allowed_directive_comment(next, cx) {
            return;
        }

        cx.emit_offense(cx.range(node), MSG, None);
        autocorrect_at_next_line(next, cx);
    }
}

/// RuboCop `correct_style?`.
fn correct_style(node: NodeId, cx: &Cx<'_>) -> bool {
    // `!node.if_branch&.guard_clause?`. rubocop-ast's `IfNode#if_branch` is the
    // condition-true branch. Murphy stores the raw parser-gem slots without the
    // unless inversion (`then_`/`else_` are swapped by the translator for
    // `unless`), so the condition-true branch is `else_` for `unless` and
    // `then_` otherwise.
    let is_guard = condition_true_branch(node, cx)
        .get()
        .is_some_and(|branch| cx.is_guard_clause(branch));
    if !is_guard {
        return true;
    }
    // Only modifier-form guards (`x if cond`) are guard clauses to separate.
    if !cx.is_modifier_form(node) {
        return true;
    }
    next_line_rescue_or_ensure(node, cx)
        || next_sibling_parent_empty_or_else(node, cx)
        || next_sibling_empty_or_guard_clause(node, cx)
}

/// rubocop-ast `IfNode#if_branch` — the branch run when the condition holds.
/// For `unless`, Murphy's translator swaps `then_`/`else_`, so the
/// condition-true branch is the `else_` slot.
fn condition_true_branch(node: NodeId, cx: &Cx<'_>) -> OptNodeId {
    if cx.is_unless(node) {
        cx.if_else_branch(node)
    } else {
        cx.if_then_branch(node)
    }
}

/// RuboCop `next_line_rescue_or_ensure?`: parent is nil / rescue / ensure.
fn next_line_rescue_or_ensure(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.parent(node).get() {
        None => true,
        Some(parent) => matches!(
            *cx.kind(parent),
            NodeKind::Rescue { .. } | NodeKind::Ensure { .. }
        ),
    }
}

/// RuboCop `next_sibling_parent_empty_or_else?`: the next sibling's parent is an
/// `if` with an `else`. (`true unless next_sibling is a node` — but a guard's
/// next sibling within a `begin` body is always a node when present.)
fn next_sibling_parent_empty_or_else(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(next_sibling) = cx.right_sibling(node).get() else {
        // RuboCop returns `true` when there is no next-sibling node.
        return true;
    };
    let Some(parent) = cx.parent(next_sibling).get() else {
        return false;
    };
    matches!(*cx.kind(parent), NodeKind::If { else_, .. } if else_.get().is_some())
}

/// RuboCop `next_sibling_empty_or_guard_clause?`: next sibling is nil, or an
/// `if` whose if-branch is itself a guard clause (consecutive guards).
fn next_sibling_empty_or_guard_clause(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(next_sibling) = cx.right_sibling(node).get() else {
        return true;
    };
    if !matches!(*cx.kind(next_sibling), NodeKind::If { .. }) {
        return false;
    }
    condition_true_branch(next_sibling, cx)
        .get()
        .is_some_and(|branch| cx.is_guard_clause(branch))
}

/// RuboCop `multiple_statements_on_line?`: the guard shares a source line with
/// its right sibling inside a `begin` body.
fn multiple_statements_on_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !matches!(*cx.kind(parent), NodeKind::Begin(_)) {
        return false;
    }
    let Some(sibling) = cx.right_sibling(node).get() else {
        return false;
    };
    same_line(node, sibling, cx)
}

/// True if two nodes' last/first lines coincide — here, whether the byte range
/// between the first node's end and the second's start contains no `\n`.
fn same_line(a: NodeId, b: NodeId, cx: &Cx<'_>) -> bool {
    let a_end = cx.range(a).end as usize;
    let b_start = cx.range(b).start as usize;
    let (lo, hi) = if a_end <= b_start {
        (a_end, b_start)
    } else {
        (b_start, a_end)
    };
    !cx.source().as_bytes()[lo..hi].contains(&b'\n')
}

/// RuboCop `next_line_empty_or_allowed_directive_comment?(line)`: 0-based
/// `line` (the physical line after the guard or heredoc terminator) is blank,
/// or it holds an allowed directive comment itself followed by a blank line.
///
/// Upstream mixes 0-based `processed_source[line]` with 1-based
/// `comment_at_line`, but both reference the same physical lines; in 0-based
/// terms: `blank(next) || (allowed(next) && blank(next + 1))`.
fn next_line_empty_or_allowed_directive_comment(line: u32, cx: &Cx<'_>) -> bool {
    if line_is_blank(cx, line) {
        return true;
    }
    is_allowed_directive_on_line(line, cx) && line_is_blank(cx, line + 1)
}

/// RuboCop `next_line_allowed_directive_comment?(line)`: 0-based `line` holds
/// a `rubocop:enable`/`murphy:enable` directive or a SimpleCov directive.
fn is_allowed_directive_on_line(line: u32, cx: &Cx<'_>) -> bool {
    comment_text_on_line(line, cx).is_some_and(|text| {
        is_enable_directive(text) || is_simplecov_directive(text)
    })
}

/// Source text of a comment starting on 0-based `line`, if any. Mirrors
/// RuboCop's `processed_source.comment_at_line` (1-based) for the single
/// comment this cop cares about.
fn comment_text_on_line<'a>(line: u32, cx: &Cx<'a>) -> Option<&'a str> {
    cx.comments().iter().find_map(|comment| {
        if line_of(comment.range.start, cx) == line {
            Some(cx.raw_source(comment.range))
        } else {
            None
        }
    })
}

/// `DirectiveComment#enabled?` — the comment is a `rubocop:enable` or
/// `murphy:enable` directive. The cop-name list is irrelevant (any `enable`
/// counts), matching RuboCop's `mode == 'enable'`.
fn is_enable_directive(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('#') else {
        return false;
    };
    let rest = rest.trim_start();
    let Some(rest) = rest
        .strip_prefix("rubocop:")
        .or_else(|| rest.strip_prefix("murphy:"))
    else {
        return false;
    };
    let rest = rest.trim_start();
    if !rest.starts_with("enable") {
        return false;
    }
    // Word boundary (`\b` in the upstream directive regexp): `enable` alone,
    // `enable Foo`, `enable all` count; `enablefoo` does not.
    match rest.as_bytes().get("enable".len()) {
        None => true,
        Some(b) => !b.is_ascii_alphanumeric() && *b != b'_',
    }
}

/// SimpleCov directive comment (`murphy-cipl`): `# :nocov:` or
/// `# simplecov:disable` / `# simplecov:enable`.
///
/// Upstream: `/\A#\s*(?::nocov:|simplecov\s*:\s*(?:disable|enable)\b)/`.
fn is_simplecov_directive(text: &str) -> bool {
    let Some(rest) = text.strip_prefix('#') else {
        return false;
    };
    let rest = rest.trim_start();
    if rest.starts_with(":nocov:") {
        return true;
    }
    let Some(after) = rest.strip_prefix("simplecov") else {
        return false;
    };
    let after = after.trim_start();
    let Some(after) = after.strip_prefix(':') else {
        return false;
    };
    let after = after.trim_start();
    for word in ["disable", "enable"] {
        if let Some(tail) = after.strip_prefix(word) {
            // `\b` — end of comment or a non-word char.
            if tail.is_empty()
                || !tail
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return true;
            }
        }
    }
    false
}

/// RuboCop `autocorrect` (simplified to Murphy's single-edit model): insert a
/// blank line at the start of 0-based `next` (the line after the guard or
/// terminator). When that line holds an allowed directive comment, insert
/// after the directive line instead, preserving directive semantics.
fn autocorrect_at_next_line(next: u32, cx: &Cx<'_>) {
    let insert_line = if is_allowed_directive_on_line(next, cx) {
        next + 1
    } else {
        next
    };
    let Some(insert_at) = nth_line_start(cx, insert_line) else {
        return;
    };
    cx.emit_edit(
        Range {
            start: insert_at,
            end: insert_at,
        },
        "\n",
    );
}

// ── Heredoc-argument guards (murphy-cipl) ────────────────────────────────────

/// Whether `node` is a heredoc string literal (a `Str`/`Dstr`/`Xstr` whose
/// opener is a `<<` heredoc token).
fn is_heredoc_string(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(
        cx.kind(node),
        NodeKind::Str(_) | NodeKind::Dstr(_) | NodeKind::Xstr(_)
    ) {
        return false;
    }
    cx.token_after(cx.range(node).start)
        .is_some_and(|t| t.kind == SourceTokenKind::HeredocStart)
}

/// RuboCop `last_heredoc_argument(node)` for a modifier-form guard `If`:
/// the heredoc string carried by the guard branch (or condition), or `None`.
fn last_heredoc_argument(if_node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let branch = condition_true_branch(if_node, cx).get()?;
    let cond = cx.if_condition(if_node).get()?;

    // `if node.if_branch.and_type?` → `node.if_branch.children.first`.
    if let NodeKind::And { lhs, .. } = *cx.kind(branch) {
        return find_heredoc_in(lhs, cx);
    }
    // `elsif use_heredoc_in_condition?(node.condition)` → `node.condition`.
    if node_or_descendants_is_heredoc(cond, cx) {
        return find_heredoc_in(cond, cx);
    }
    // `else node.if_branch.children.last`.
    let last = cx.children(branch).last().copied()?;
    find_heredoc_in(last, cx)
}

/// Whether `node` itself or any descendant is a heredoc string. Mirrors
/// RuboCop's `use_heredoc_in_condition?` (`condition.descendants.any?` +
/// the condition itself).
fn node_or_descendants_is_heredoc(node: NodeId, cx: &Cx<'_>) -> bool {
    if is_heredoc_string(node, cx) {
        return true;
    }
    cx.descendants(node)
        .iter()
        .any(|&d| is_heredoc_string(d, cx))
}

/// Recursive `last_heredoc_argument` search starting from `node`:
/// unwrap `begin` (parenthesized) nodes, accept a heredoc string, else search
/// call arguments in order then the receiver. Faithful to RuboCop, which only
/// recurses through `arguments`/`receiver` (other containers are not heredoc
/// carriers for this cop).
fn find_heredoc_in(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let mut cur = node;
    loop {
        // `n = n.children.first while n.begin_type?`.
        while matches!(cx.kind(cur), NodeKind::Begin(_)) {
            let kids = cx.children(cur);
            let &first = kids.first()?;
            cur = first;
        }
        if is_heredoc_string(cur, cx) {
            return Some(cur);
        }
        if !matches!(
            cx.kind(cur),
            NodeKind::Send { .. } | NodeKind::Csend { .. }
        ) {
            return None;
        }
        for &arg in cx.call_arguments(cur) {
            if let Some(found) = find_heredoc_in(arg, cx) {
                return Some(found);
            }
        }
        match cx.call_receiver(cur).get() {
            Some(recv) => {
                // Recurse into the receiver chain (`node.children.first`).
                // Avoid infinite loops on malformed cycles by only looping
                // when the receiver is itself a call; otherwise search once.
                if matches!(
                    cx.kind(recv),
                    NodeKind::Send { .. } | NodeKind::Csend { .. }
                ) {
                    cur = recv;
                    continue;
                }
                return find_heredoc_in(recv, cx);
            }
            None => return None,
        }
    }
}

/// RuboCop's `heredoc.loc.heredoc_end` (label only, no trailing newline):
/// the `HeredocEnd` token paired by FIFO opener index, trimmed of the prism
/// token's trailing `\r\n`. Same pairing as `Layout/BlockEndNewline`.
fn heredoc_end_label_range(heredoc: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let range = cx.range(heredoc);
    let toks = cx.sorted_tokens();
    let opener = toks
        .iter()
        .find(|t| {
            t.kind == SourceTokenKind::HeredocStart
                && t.range.start >= range.start
                && t.range.end <= range.end
        })?
        .range;
    let index = toks
        .iter()
        .filter(|t| t.kind == SourceTokenKind::HeredocStart)
        .take_while(|t| t.range.start < opener.start)
        .count();
    let term = toks
        .iter()
        .filter(|t| t.kind == SourceTokenKind::HeredocEnd)
        .nth(index)?
        .range;
    let bytes = cx.source().as_bytes();
    let mut end = term.end as usize;
    while end > term.start as usize && matches!(bytes.get(end - 1), Some(b'\n' | b'\r')) {
        end -= 1;
    }
    Some(Range {
        start: term.start,
        end: end as u32,
    })
}

murphy_plugin_api::submit_cop!(EmptyLineAfterGuardClause);

#[cfg(test)]
mod tests {
    use super::EmptyLineAfterGuardClause;
    use murphy_plugin_api::test_support::{indoc, run_cop, run_cop_with_edits};

    fn apply(source: &str, edits: &[murphy_plugin_api::test_support::CapturedEdit]) -> String {
        assert_eq!(edits.len(), 1, "expected exactly one insert edit");
        let edit = &edits[0];
        let mut out = String::with_capacity(source.len() + edit.replacement.len());
        out.push_str(&source[..edit.range.start as usize]);
        out.push_str(&edit.replacement);
        out.push_str(&source[edit.range.end as usize..]);
        out
    }

    // ── Clean ────────────────────────────────────────────────────────────────

    #[test]
    fn accepts_blank_line_after_guard() {
        let offenses = run_cop::<EmptyLineAfterGuardClause>(indoc! {r#"
            def foo
              return if x

              bar
            end
        "#});
        assert!(offenses.is_empty(), "unexpected offenses: {offenses:?}");
    }

    #[test]
    fn accepts_guard_as_last_statement() {
        // The line after the guard is `end`, not real code → no offense.
        let offenses = run_cop::<EmptyLineAfterGuardClause>(indoc! {r#"
            def foo
              return if x
            end
        "#});
        assert!(offenses.is_empty(), "unexpected offenses: {offenses:?}");
    }

    #[test]
    fn accepts_consecutive_guards_fires_only_once() {
        // Two consecutive guards then code: exactly one offense, on the second.
        let offenses = run_cop::<EmptyLineAfterGuardClause>(indoc! {r#"
            def foo
              return if a
              return if b

              bar
            end
        "#});
        assert!(offenses.is_empty(), "aligned consecutive guards: {offenses:?}");
    }

    #[test]
    fn accepts_non_guard_modifier_if() {
        // `foo if x` is a modifier if but not a guard clause.
        let offenses = run_cop::<EmptyLineAfterGuardClause>(indoc! {r#"
            def foo
              do_thing if x
              bar
            end
        "#});
        assert!(offenses.is_empty(), "non-guard modifier if: {offenses:?}");
    }

    #[test]
    fn accepts_block_form_if() {
        // A full `if ... end` is not a guard clause.
        let offenses = run_cop::<EmptyLineAfterGuardClause>(indoc! {r#"
            def foo
              if x
                return
              end
              bar
            end
        "#});
        assert!(offenses.is_empty(), "block-form if: {offenses:?}");
    }

    #[test]
    fn accepts_enable_directive_then_blank() {
        let offenses = run_cop::<EmptyLineAfterGuardClause>(
            "def foo\n  return if x\n  # rubocop:enable Foo\n\n  bar\nend\n",
        );
        assert!(offenses.is_empty(), "enable directive + blank: {offenses:?}");
    }

    #[test]
    fn accepts_simplecov_nocov_then_blank() {
        let offenses = run_cop::<EmptyLineAfterGuardClause>(
            "def foo\n  return if x\n  # :nocov:\n\n  bar\nend\n",
        );
        assert!(offenses.is_empty(), "simplecov + blank: {offenses:?}");
    }

    #[test]
    fn accepts_heredoc_guard_with_blank_after_terminator() {
        let src = "def foo\n  raise <<~MSG if cond\n    hello\n  MSG\n\n  bar\nend\n";
        let offenses = run_cop::<EmptyLineAfterGuardClause>(src);
        assert!(offenses.is_empty(), "heredoc + blank: {offenses:?}");
    }

    #[test]
    fn accepts_heredoc_guard_with_directive_then_blank() {
        let src = "def foo\n  raise <<~MSG if cond\n    hello\n  MSG\n  # rubocop:enable Foo\n\n  bar\nend\n";
        let offenses = run_cop::<EmptyLineAfterGuardClause>(src);
        assert!(offenses.is_empty(), "heredoc + directive + blank: {offenses:?}");
    }

    // ── Offenses ─────────────────────────────────────────────────────────────

    #[test]
    fn flags_missing_blank_line_after_return_guard() {
        let offenses = run_cop::<EmptyLineAfterGuardClause>(indoc! {r#"
            def foo
              return if x
              bar
            end
        "#});
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
        assert_eq!(offenses[0].message, super::MSG);
    }

    #[test]
    fn flags_missing_blank_line_after_raise_guard() {
        let offenses = run_cop::<EmptyLineAfterGuardClause>(indoc! {r#"
            def foo
              raise "e" if x
              bar
            end
        "#});
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
    }

    #[test]
    fn flags_missing_blank_line_after_unless_guard() {
        // `return unless x` is a guard clause (branch swap handled by if_branch).
        let offenses = run_cop::<EmptyLineAfterGuardClause>(indoc! {r#"
            def foo
              return unless x
              bar
            end
        "#});
        assert_eq!(offenses.len(), 1, "unless guard should fire: {offenses:?}");
    }

    #[test]
    fn flags_consecutive_guards_on_last_only() {
        // Missing blank between the second guard and code → one offense.
        let src = "def foo\n  return if a\n  return if b\n  bar\nend\n";
        let offenses = run_cop::<EmptyLineAfterGuardClause>(src);
        assert_eq!(offenses.len(), 1, "got {offenses:?}");
    }

    #[test]
    fn flags_enable_directive_without_blank() {
        let src = "def foo\n  return if x\n  # rubocop:enable Foo\n  bar\nend\n";
        let offenses = run_cop::<EmptyLineAfterGuardClause>(src);
        assert_eq!(offenses.len(), 1, "directive without blank should fire: {offenses:?}");
    }

    #[test]
    fn flags_disable_directive_even_with_blank() {
        // Only `enable` (and SimpleCov) are allowed; `disable` still fires.
        let src = "def foo\n  return if x\n  # rubocop:disable Foo\n\n  bar\nend\n";
        let offenses = run_cop::<EmptyLineAfterGuardClause>(src);
        assert_eq!(offenses.len(), 1, "disable directive should fire: {offenses:?}");
    }

    #[test]
    fn flags_plain_comment_even_with_blank() {
        let src = "def foo\n  return if x\n  # hello\n\n  bar\nend\n";
        let offenses = run_cop::<EmptyLineAfterGuardClause>(src);
        assert_eq!(offenses.len(), 1, "plain comment should fire: {offenses:?}");
    }

    #[test]
    fn flags_heredoc_guard_without_blank() {
        let src = "def foo\n  raise <<~MSG if cond\n    hello\n  MSG\n  bar\nend\n";
        let offenses = run_cop::<EmptyLineAfterGuardClause>(src);
        assert_eq!(offenses.len(), 1, "heredoc guard should fire: {offenses:?}");
        // Offense lands on the heredoc terminator label, like RuboCop.
        assert!(
            src[offenses[0].range.start as usize..offenses[0].range.end as usize].contains("MSG"),
            "heredoc offense range should cover terminator: {:?}",
            offenses[0].range
        );
    }

    #[test]
    fn flags_heredoc_guard_with_directive_without_blank() {
        let src = "def foo\n  raise <<~MSG if cond\n    hello\n  MSG\n  # rubocop:enable Foo\n  bar\nend\n";
        let offenses = run_cop::<EmptyLineAfterGuardClause>(src);
        assert_eq!(offenses.len(), 1, "heredoc + directive without blank: {offenses:?}");
    }

    #[test]
    fn corrects_missing_blank_line() {
        let src = "def foo\n  return if x\n  bar\nend\n";
        let run = run_cop_with_edits::<EmptyLineAfterGuardClause>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            apply(src, &run.edits),
            "def foo\n  return if x\n\n  bar\nend\n"
        );
    }

    #[test]
    fn corrects_after_enable_directive() {
        let src = "def foo\n  return if x\n  # rubocop:enable Foo\n  bar\nend\n";
        let run = run_cop_with_edits::<EmptyLineAfterGuardClause>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            apply(src, &run.edits),
            "def foo\n  return if x\n  # rubocop:enable Foo\n\n  bar\nend\n"
        );
    }

    #[test]
    fn corrects_heredoc_guard() {
        let src = "def foo\n  raise <<~MSG if cond\n    hello\n  MSG\n  bar\nend\n";
        let run = run_cop_with_edits::<EmptyLineAfterGuardClause>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            apply(src, &run.edits),
            "def foo\n  raise <<~MSG if cond\n    hello\n  MSG\n\n  bar\nend\n"
        );
    }

    #[test]
    fn corrects_heredoc_guard_after_directive() {
        let src = "def foo\n  raise <<~MSG if cond\n    hello\n  MSG\n  # rubocop:enable Foo\n  bar\nend\n";
        let run = run_cop_with_edits::<EmptyLineAfterGuardClause>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            apply(src, &run.edits),
            "def foo\n  raise <<~MSG if cond\n    hello\n  MSG\n  # rubocop:enable Foo\n\n  bar\nend\n"
        );
    }
}
