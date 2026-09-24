//! `Lint/ElseLayout` — flag odd code arrangement in an `else` block, where a
//! statement shares the `else` keyword's line (a likely missed `elsif`).
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ElseLayout
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detection mirrors RuboCop's on_if + check_else: for an `if`/`elsif` with a
//!   real `else` keyword whose else-branch is a multi-statement begin, flag the
//!   first else statement when it sits on the same line as the `else` keyword.
//!   The on_if guards (ternary, then-without-begin-else, single-line) are
//!   replicated. RuboCop's `check` recurses through elsif chains from the top
//!   `if`; Murphy fires `on_node(kind="if")` per nested `If`, so the per-node
//!   handler reaches every `elsif` exactly once WITHOUT recursion — replicating
//!   the recursion would double-count. The `else` keyword is located in the gap
//!   between the then-branch end and the else-branch start so a nested `else` in
//!   the then-branch cannot be matched. The offense highlight is clamped to the
//!   first statement's first line — an accepted project-wide rendering
//!   convention (shared with verified `Lint/MissingSuper`,
//!   `crate::cops::util::first_line_range`) vs RuboCop's full `first_else` node
//!   range; the start byte matches, so line/column is faithful (murphy-4k23
//!   resolved). Autocorrect ports RuboCop's `insert_after(else, "\n")` +
//!   `replace(else.end...first_else.begin, indentation(node))` as a single
//!   gap replacement with `"\n" + indentation` (murphy-4m6o); `indentation`
//!   is the `Alignment` mixin's `offset(node) + width` where `offset` is the
//!   owning `if`/`elsif` keyword column and `width` is the run-wide
//!   `Layout/IndentationWidth` (`cx.indentation_width()`, default 2).
//! ```
//!
//! ## Autocorrect
//!
//! Replaces the gap between `else` and the first else statement with
//! `"\n" + indentation`, where `indentation` is the owning conditional's
//! column plus `Layout/IndentationWidth`. Idempotent: the corrected layout
//! puts the first else statement on its own line, so `same_line?` no longer
//! fires.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct ElseLayout;

#[cop(
    name = "Lint/ElseLayout",
    description = "Flag odd code arrangement in an else block.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ElseLayout {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop on_if guards.
        if is_ternary(node, cx) {
            return;
        }
        // `return if node.then? && !node.else_branch&.begin_type?`
        let else_branch = cx.else_branch(node);
        if cx.is_then(node)
            && !else_branch
                .get()
                .is_some_and(|b| matches!(cx.kind(b), NodeKind::Begin(_)))
        {
            return;
        }
        // `return if node.single_line?`
        if is_single_line(node, cx) {
            return;
        }

        // RuboCop `check`: only act when this node has a real `else` keyword
        // (not an `elsif`). Murphy reaches every `elsif` node directly via the
        // per-node dispatch, so no recursion into the else-branch is needed.
        let Some(else_id) = else_branch.get() else {
            return;
        };
        let Some(else_kw) = else_keyword_range(node, cx) else {
            return;
        };
        check_else(node, else_id, else_kw, cx);
    }
}

/// RuboCop `check_else`: the first statement of the else block is an offense if
/// it sits on the same line as the `else` keyword.
fn check_else(if_node: NodeId, else_branch: NodeId, else_kw: Range, cx: &Cx<'_>) {
    // A true `elsif` carries no `else` keyword at this level, so the caller's
    // `else_keyword_range` already returned `None` and never reaches here. When
    // it *does* reach here the else slot may still be a nested `If` — that is
    // `else if … end` (written with a space), which RuboCop flags — so the
    // nested `If` must be treated as the first else statement, not skipped.
    let first_else = match cx.kind(else_branch) {
        NodeKind::Begin(list) => match cx.list(*list).first() {
            Some(&first) => first,
            None => return,
        },
        _ => else_branch,
    };
    let first_range = cx.range(first_else);
    // RuboCop `same_line?(first_else, node.loc.else)`: the first else statement
    // starts on the same line the `else` keyword ends on.
    if !same_line(else_kw.end, first_range.start, cx.source()) {
        return;
    }
    cx.emit_offense(
        crate::cops::util::first_line_range(first_else, cx),
        "Odd `else` layout detected. Did you mean to use `elsif`?",
        None,
    );
    // RuboCop `autocorrect`: `insert_after(else, "\n")` +
    // `replace(else.end...first_else.begin, indentation(node))`. The two edits
    // combine to `gap -> "\n" + indentation`, so Murphy emits a single gap
    // replacement (avoids overlapping edits).
    if else_kw.end < first_range.start {
        let replacement = format!("\n{}", indentation(if_node, cx));
        cx.emit_edit(
            Range {
                start: else_kw.end,
                end: first_range.start,
            },
            &replacement,
        );
    }
}

/// RuboCop `Alignment#indentation(node)`: `offset(node)` (spaces matching the
/// owning `if`/`elsif` keyword column) plus one indentation width
/// (`Layout/IndentationWidth`, default 2).
fn indentation(if_node: NodeId, cx: &Cx<'_>) -> String {
    let keyword = cx.if_keyword_loc(if_node);
    let base = if keyword != Range::ZERO {
        keyword.start
    } else {
        cx.range(if_node).start
    };
    let width = cx.indentation_width().max(0) as usize;
    " ".repeat(column_of(base, cx) + width)
}

/// 0-based byte column of `offset` (distance from its line start). Indentation
/// is ASCII whitespace, so byte and character columns coincide here.
fn column_of(offset: u32, cx: &Cx<'_>) -> usize {
    let line = crate::cops::util::line_of(offset, cx);
    let line_start = crate::cops::util::nth_line_start(cx, line).unwrap_or(0);
    (offset - line_start) as usize
}

/// The `else` keyword token range for this `If` node, searched only in the gap
/// between the then-branch's end and the else-branch's start so a nested `else`
/// inside the then-branch cannot be matched. `None` if there is no literal
/// `else` keyword (e.g. an `elsif` chain or a no-else `if`).
fn else_keyword_range(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let else_branch = cx.else_branch(node).get()?;
    let then_end = cx
        .if_branch(node)
        .get()
        .map_or(cx.range(node).start, |b| cx.range(b).end);
    let else_start = cx.range(else_branch).start;
    // Guard against parser-recovery ranges where the then-branch end overruns
    // the else-branch start, which would make `gap.start > gap.end`.
    if then_end > else_start {
        return None;
    }
    let gap = Range {
        start: then_end,
        end: else_start,
    };
    cx.tokens_in(gap)
        .iter()
        .find(|&&tok| cx.token_text(tok) == "else")
        .map(|tok| tok.range)
}

fn is_ternary(node: NodeId, cx: &Cx<'_>) -> bool {
    // A ternary `c ? a : b` has a `?` token in the gap between the condition
    // and the then-branch and no `if`/`unless`/`elsif` keyword.
    cx.if_keyword(node).is_empty()
}

fn is_single_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    // Defensive slice per `.claude/rules/safe-rust-patterns.md`: a node's own
    // range is well-formed, but `get` avoids any panic on a degenerate range
    // and treats a missing slice as "not single-line".
    cx.source()
        .as_bytes()
        .get(range.start as usize..range.end as usize)
        .is_some_and(|slice| !slice.contains(&b'\n'))
}

fn same_line(lhs_end: u32, rhs_start: u32, source: &str) -> bool {
    let start = lhs_end as usize;
    let end = rhs_start as usize;
    // Guard against parser-recovery ranges where `lhs_end > rhs_start` (e.g. a
    // recovered `0..0` first-else range against a positive keyword offset),
    // which would panic on the slice.
    if start >= end {
        return false;
    }
    !source.as_bytes()[start..end].contains(&b'\n')
}

murphy_plugin_api::submit_cop!(ElseLayout);

#[cfg(test)]
mod tests {
    use super::ElseLayout;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_statement_on_else_line() {
        test::<ElseLayout>().expect_offense(indoc! {r#"
            if something
              test
            else something_else
                 ^^^^^^^^^^^^^^ Odd `else` layout detected. Did you mean to use `elsif`?
              test2
            end
        "#});
    }

    #[test]
    fn accepts_normal_else_layout() {
        test::<ElseLayout>().expect_no_offenses(indoc! {r#"
            if something
              test
            else
              something_else
              test2
            end
        "#});
    }

    #[test]
    fn accepts_else_with_single_statement_on_own_line() {
        test::<ElseLayout>().expect_no_offenses(indoc! {r#"
            if something
              test
            else
              something_else
            end
        "#});
    }

    #[test]
    fn flags_single_statement_else_on_same_line_without_then() {
        // No `then` keyword, single-statement else on the else line — RuboCop
        // flags this (the `then? && !begin_type?` guard does not apply).
        test::<ElseLayout>().expect_offense(indoc! {r#"
            if something
              test
            else something_else
                 ^^^^^^^^^^^^^^ Odd `else` layout detected. Did you mean to use `elsif`?
            end
        "#});
    }

    #[test]
    fn accepts_single_statement_else_on_same_line_with_then() {
        // `if ... then ...` with a single-statement else: RuboCop's
        // `then? && !else_branch.begin_type?` guard exempts it.
        test::<ElseLayout>().expect_no_offenses(indoc! {r#"
            if something then test
            else something_else
            end
        "#});
    }

    #[test]
    fn accepts_if_without_else() {
        test::<ElseLayout>().expect_no_offenses(indoc! {r#"
            if something
              test
            end
        "#});
    }

    #[test]
    fn does_not_double_count_elsif_chain() {
        // The `else` body is laid out badly; there must be exactly one offense
        // even though the chain contains an `elsif` (a nested `If` visited
        // separately by the per-node dispatch).
        test::<ElseLayout>().expect_offense(indoc! {r#"
            if a
              test
            elsif b
              test2
            else foo
                 ^^^ Odd `else` layout detected. Did you mean to use `elsif`?
              bar
            end
        "#});
    }

    #[test]
    fn flags_partial_else_body_on_same_line_with_then() {
        // `then` present, but the else-branch is a multi-statement begin, so the
        // `then? && !begin_type?` guard does NOT apply and the first else
        // statement on the else line is flagged.
        test::<ElseLayout>().expect_offense(indoc! {r#"
            if something then test
            else something_else
                 ^^^^^^^^^^^^^^ Odd `else` layout detected. Did you mean to use `elsif`?
              other
            end
        "#});
    }

    #[test]
    fn accepts_elsif_with_no_body() {
        test::<ElseLayout>().expect_no_offenses(indoc! {r#"
            if something
              foo
            elsif something_else
            end
        "#});
    }

    #[test]
    fn accepts_single_line_if_then_else_end() {
        // RuboCop's `single_line?` guard skips this.
        test::<ElseLayout>().expect_no_offenses("if a then b else c end\n");
    }

    #[test]
    fn accepts_ternary() {
        test::<ElseLayout>().expect_no_offenses("x = a ? b : c\n");
    }

    #[test]
    fn flags_else_if_on_same_line() {
        // `else if … end` (a nested `if` in the else slot, written with a space)
        // carries a real `else` keyword, and the nested `if` sits on the `else`
        // line — RuboCop flags it ("Did you mean to use `elsif`?"). Only a true
        // `elsif` (no `else` keyword → `else_keyword_range` returns `None`) is
        // exempt, so the nested-`If` early return that used to skip this case
        // produced a false negative.
        test::<ElseLayout>().expect_offense(indoc! {r#"
            if something
              test
            else if other
                 ^^^^^^^^ Odd `else` layout detected. Did you mean to use `elsif`?
              bar
            end
            end
        "#});
    }

    // === autocorrect (murphy-4m6o) ===

    #[test]
    fn corrects_else_body_onto_new_line() {
        // RuboCop: `insert_after(else, "\\n")` + `replace(gap, indentation)`.
        // `if` at column 0 + default width 2 → `"  "`.
        test::<ElseLayout>().expect_correction(
            indoc! {r#"
                if something
                  test
                else something_else
                     ^^^^^^^^^^^^^^ Odd `else` layout detected. Did you mean to use `elsif`?
                  test2
                end
            "#},
            indoc! {r#"
                if something
                  test
                else
                  something_else
                  test2
                end
            "#},
        );
    }

    #[test]
    fn corrects_indented_if_with_outer_column() {
        // Owning `if` at column 2 → indent is 2 + 2 = 4 spaces (verified
        // against standalone rubocop 1.87.0).
        test::<ElseLayout>().expect_correction(
            indoc! {r#"
                def foo
                  if something
                    test
                  else something_else
                       ^^^^^^^^^^^^^^ Odd `else` layout detected. Did you mean to use `elsif`?
                    test2
                  end
                end
            "#},
            indoc! {r#"
                def foo
                  if something
                    test
                  else
                    something_else
                    test2
                  end
                end
            "#},
        );
    }

    #[test]
    fn corrects_elsif_chain_else_with_elsif_column() {
        // The offense owns to the `elsif` node (column 0) → 2-space indent.
        test::<ElseLayout>().expect_correction(
            indoc! {r#"
                if a
                  test
                elsif b
                  test2
                else foo
                     ^^^ Odd `else` layout detected. Did you mean to use `elsif`?
                  bar
                end
            "#},
            indoc! {r#"
                if a
                  test
                elsif b
                  test2
                else
                  foo
                  bar
                end
            "#},
        );
    }

    #[test]
    fn corrects_else_if_nested_if() {
        // `else if` (space): the outer `if` owns the `else`; the nested `if`
        // moves to its own line at the outer indent + width. The rest of the
        // body is untouched — matching RuboCop (verified standalone).
        test::<ElseLayout>().expect_correction(
            indoc! {r#"
                if something
                  test
                else if other
                     ^^^^^^^^ Odd `else` layout detected. Did you mean to use `elsif`?
                  bar
                end
                end
            "#},
            indoc! {r#"
                if something
                  test
                else
                  if other
                  bar
                end
                end
            "#},
        );
    }

    #[test]
    fn correction_is_idempotent() {
        // Re-feeding the corrected output yields no offenses and no edits.
        test::<ElseLayout>().expect_no_offenses(indoc! {r#"
            if something
              test
            else
              something_else
              test2
            end
        "#});
        test::<ElseLayout>().expect_no_corrections(indoc! {r#"
            if something
              test
            else
              something_else
              test2
            end
        "#});
    }
}
