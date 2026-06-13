//! `Layout/EmptyLineAfterGuardClause` — require a blank line after a guard
//! clause (`return`/`next`/`break`/`raise`/`fail` in modifier `if`/`unless`
//! form) before the following code.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLineAfterGuardClause
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-cipl]
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
//!   Offense location is the whole modifier-form `If` node (it has no `end`
//!   keyword). Autocorrect inserts a `\n` after the guard clause's line.
//!
//!   Documented gaps (filed as murphy-cipl):
//!     - Heredoc-argument guard clauses (`raise <<~MSG ... MSG if cond`) need
//!       the heredoc-body / heredoc-end source locations, which are not
//!       available across the single-surface ABI; such guards are not checked.
//!     - The SimpleCov / directive-comment allowance
//!       (`next_line_allowed_directive_comment?`) is not modelled; a directive
//!       comment immediately after a guard clause is treated like ordinary
//!       code, so it still fires.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

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

        // RuboCop's heredoc branch is a documented gap (murphy-cipl): without
        // heredoc-end locations it cannot be modelled, so only the non-heredoc
        // path runs here.

        // RuboCop: `return if next_line_empty_or_allowed_directive_comment?(node.last_line)`.
        if next_line_empty(node, cx) {
            return;
        }

        cx.emit_offense(cx.range(node), MSG, None);
        autocorrect(node, cx);
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

/// RuboCop `next_line_empty?(node.last_line)`: the physical line immediately
/// following the guard clause's last line is blank (or there is no such line).
fn next_line_empty(node: NodeId, cx: &Cx<'_>) -> bool {
    let src = cx.source().as_bytes();
    let node_end = cx.range(node).end as usize;
    // Skip to the newline that ends the guard clause's last line.
    let Some(rel) = src[node_end..].iter().position(|&b| b == b'\n') else {
        // Guard clause is the last line of the file — no next line.
        return true;
    };
    let next_line_start = node_end + rel + 1;
    if next_line_start >= src.len() {
        return true;
    }
    let next_line_end = src[next_line_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(src.len(), |i| next_line_start + i);
    src[next_line_start..next_line_end]
        .iter()
        .all(|&b| crate::cops::util::is_ruby_blank_byte(b))
}

/// RuboCop `autocorrect`: insert `\n` after the guard clause's whole line.
fn autocorrect(node: NodeId, cx: &Cx<'_>) {
    let src = cx.source().as_bytes();
    let node_end = cx.range(node).end as usize;
    // Insert immediately after the newline terminating the guard's last line.
    let Some(rel) = src[node_end..].iter().position(|&b| b == b'\n') else {
        return;
    };
    let insert_at = (node_end + rel + 1) as u32;
    let anchor = Range {
        start: insert_at,
        end: insert_at,
    };
    cx.emit_edit(anchor, "\n");
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
    fn corrects_missing_blank_line() {
        let src = "def foo\n  return if x\n  bar\nend\n";
        let run = run_cop_with_edits::<EmptyLineAfterGuardClause>(src);
        assert_eq!(run.offenses.len(), 1);
        assert_eq!(
            apply(src, &run.edits),
            "def foo\n  return if x\n\n  bar\nend\n"
        );
    }
}
