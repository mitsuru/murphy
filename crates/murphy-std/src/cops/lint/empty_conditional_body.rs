//! `Lint/EmptyConditionalBody` — flag `if`, `elsif`, and `unless` branches
//! without a body.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EmptyConditionalBody
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detection mirrors RuboCop's per-`if` handler, including single-line
//!   suppression and `AllowComments` (default true). The offense highlight is
//!   clamped to the node's first line, an accepted project-wide convention
//!   shared with `Lint/MissingSuper`; its start byte matches RuboCop's
//!   keyword-to-`else` range (murphy-4k23 resolved).
//!
//!   For an empty `if`/`unless` with a populated literal `else`, autocorrect
//!   replaces `else` with the inverse keyword and condition, then removes the
//!   empty branch using RuboCop's nested-branch vs whole-line range split.
//!   It preserves the `elsif` offense but skips RuboCop 1.87.0's malformed
//!   autocorrection for a multiline empty `elsif` with a final `else`; it also
//!   skips heredoc conditions whose upstream correction can leave invalid Ruby.
//!   Both safeguards leave the offense intact and avoid a destructive edit.
//! ```
//!
//! ## Autocorrect
//!
//! The contextual `flip_orphaned_else` rewrite applies only when the empty
//! branch has a populated `else`: flip `if x` to `unless x` (or vice versa),
//! replace the keyword, and remove the empty branch. Conditions containing a
//! heredoc and `elsif` nodes are not autocorrected.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

#[derive(Default)]
pub struct EmptyConditionalBody;

/// Cop options for [`EmptyConditionalBody`], read at dispatch time via
/// [`Cx::options_or_default`].
#[derive(CopOptions)]
pub struct Options {
    #[option(name = "AllowComments", 
        default = true,
        description = "When true, don't flag a branch whose body region contains only a comment."
    )]
    pub allow_comments: bool,
}

#[cop(
    name = "Lint/EmptyConditionalBody",
    description = "Flag if, elsif, and unless branches without a body.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl EmptyConditionalBody {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop: `return if node.body || same_line?(node.loc.begin, node.loc.end)`.
        // `node.body` is the branch that holds the actual statements: for
        // `if`/`elsif` it is the `then_` slot, but the translator's parser-gem
        // swap puts an `unless` body in the `else_` slot. Selecting the slot by
        // keyword distinguishes `unless cond; X; end` (body present) from
        // `if cond; else X; end` (empty if-branch with an orphaned else) — the
        // two parse to the identical `(if cond nil X)` shape. The `same_line?`
        // guard skips the single-line `if x then end` / `if x; end` forms.
        let body = if cx.is_unless(node) {
            cx.else_branch(node)
        } else {
            cx.if_branch(node)
        };
        if body.get().is_some() || is_single_line(node, cx) {
            return;
        }
        // RuboCop: `return if allow_comments?(node)`. The comment must be
        // *inside this empty branch's body region* — a comment in a sibling
        // `else`/`elsif` branch must not suppress the offense. The region runs
        // from the condition's end to the next `else`/`elsif` keyword (or the
        // node's end for a plain `if`/`unless` with no else).
        let opts = cx.options_or_default::<Options>();
        if opts.allow_comments
            && !cx.comments_in_range(empty_body_region(node, cx)).is_empty()
        {
            return;
        }
        let keyword = cx.if_keyword(node);
        cx.emit_offense(
            crate::cops::util::first_line_range(node, cx),
            &format!("Avoid `{keyword}` branches without a body."),
            None,
        );
        autocorrect_orphaned_else(node, cx);
    }
}

/// The branch selected when the condition holds. Prism stores an `unless`
/// body's statements in the `else_` slot to match parser-gem's AST.
fn conditional_body(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    if cx.is_unless(node) {
        cx.else_branch(node).get()
    } else {
        cx.if_branch(node).get()
    }
}

/// The branch selected when the condition does not hold, accounting for the
/// parser-gem slot swap used for `unless` nodes.
fn conditional_else_branch(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    if cx.is_unless(node) {
        cx.if_branch(node).get()
    } else {
        cx.else_branch(node).get()
    }
}

/// RuboCop's `empty_if_branch?`: a missing or nonconditional parent selects
/// the full branch-range deletion. For a parent conditional, it selects that
/// deletion only when the parent's semantic body is an empty nested `if`.
fn empty_if_branch(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !matches!(cx.kind(parent), NodeKind::If { .. }) {
        return true;
    }
    let Some(parent_body) = conditional_body(parent, cx) else {
        return true;
    };
    matches!(cx.kind(parent_body), NodeKind::If { .. })
        && conditional_body(parent_body, cx).is_none()
}

/// Locate the literal `else` token between a condition and its populated
/// else-branch. Token text is exact, so strings/comments containing `else`
/// cannot be mistaken for the keyword.
fn else_keyword_range(
    condition: NodeId,
    else_branch: NodeId,
    cx: &Cx<'_>,
) -> Option<Range> {
    let condition_end = cx.range(condition).end;
    let branch_start = cx.range(else_branch).start;
    if condition_end > branch_start {
        return None;
    }
    cx.tokens_in(Range {
        start: condition_end,
        end: branch_start,
    })
    .iter()
    .find(|token| cx.token_text(**token) == "else")
    .map(|token| token.range)
}

fn condition_contains_heredoc(condition: NodeId, cx: &Cx<'_>) -> bool {
    cx.tokens_in(cx.range(condition))
        .iter()
        .any(|token| token.kind == SourceTokenKind::HeredocStart)
}

/// Flip an empty `if`/`unless` with a populated literal `else`. Keep the
/// offense but avoid malformed corrections for upstream's `elsif` and heredoc
/// edge cases.
fn autocorrect_orphaned_else(node: NodeId, cx: &Cx<'_>) {
    let inverse_keyword = cx.if_inverse_keyword(node);
    if inverse_keyword.is_empty() {
        // RuboCop 1.87.0's correction can corrupt an empty `elsif` followed by
        // a final `else` because `inverse_keyword` is empty for `elsif`.
        return;
    }
    let Some(condition) = cx.if_condition(node).get() else {
        return;
    };
    if condition_contains_heredoc(condition, cx) {
        // The condition's source range excludes its heredoc body, so the
        // upstream whole-line deletion can strand the body and invalidate Ruby.
        return;
    }
    let Some(else_branch) = conditional_else_branch(node, cx) else {
        return;
    };
    let Some(else_range) = else_keyword_range(condition, else_branch, cx) else {
        return;
    };

    let condition_source = cx.raw_source(cx.range(condition));
    cx.emit_edit(
        else_range,
        &format!("{inverse_keyword} {condition_source}"),
    );

    let node_range = cx.range(node);
    let deletion_range = if empty_if_branch(node, cx)
        && !matches!(cx.kind(else_branch), NodeKind::If { .. })
    {
        Range {
            start: node_range.start,
            end: else_range.start,
        }
    } else {
        deletion_range_through_line_end(
            Range {
                start: node_range.start,
                end: cx.range(condition).end,
            },
            cx,
        )
    };
    cx.emit_edit(deletion_range, "");
}

/// Extend the condition-header range through the end of its physical line,
/// including the newline. This matches RuboCop's `deletion_range` behavior.
fn deletion_range_through_line_end(range: Range, cx: &Cx<'_>) -> Range {
    let source = cx.source().as_bytes();
    let mut end = range.end as usize;
    while end < source.len() && source[end] != b'\n' {
        end += 1;
    }
    if end < source.len() {
        end += 1;
    }
    Range {
        start: range.start,
        end: end as u32,
    }
}

/// Whether `node`'s source range fits on a single physical line — Murphy's
/// equivalent of RuboCop's `same_line?(node.loc.begin, node.loc.end)`, which
/// suppresses the one-line `if x then end` form.
fn is_single_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    let bytes = cx.source().as_bytes();
    // Defensive slice per `.claude/rules/safe-rust-patterns.md`: a node's own
    // range is well-formed, but `get` avoids any panic on a degenerate range
    // and treats a missing slice as "not single-line".
    bytes
        .get(range.start as usize..range.end as usize)
        .is_some_and(|slice| !slice.contains(&b'\n'))
}

/// The source region that would hold this empty branch's body: from the
/// condition's end to the first following `else`/`elsif` keyword (exclusive),
/// or to the node's end when there is none. A comment in this region belongs
/// to *this* branch; a comment past the `else`/`elsif` keyword belongs to a
/// sibling branch and must not suppress the offense. The slot-based bound is
/// deliberately avoided: the `else_` slot is `nil` precisely in the comment-only
/// sibling case, so only the keyword token is a robust boundary.
fn empty_body_region(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);
    let start = cx
        .if_condition(node)
        .get()
        .map_or(node_range.start, |c| cx.range(c).end);
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < start);
    let end = toks[idx..]
        .iter()
        .take_while(|t| t.range.end <= node_range.end)
        .find(|t| matches!(cx.token_text(**t), "else" | "elsif"))
        .map_or(node_range.end, |t| t.range.start);
    Range { start, end }
}

murphy_plugin_api::submit_cop!(EmptyConditionalBody);

#[cfg(test)]
mod tests {
    use super::{EmptyConditionalBody, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_if_body() {
        test::<EmptyConditionalBody>().expect_offense(indoc! {r#"
            if condition
            ^^^^^^^^^^^^ Avoid `if` branches without a body.
            end
        "#});
    }

    #[test]
    fn flags_empty_unless_body() {
        test::<EmptyConditionalBody>().expect_offense(indoc! {r#"
            unless condition
            ^^^^^^^^^^^^^^^^ Avoid `unless` branches without a body.
            end
        "#});
    }

    #[test]
    fn flags_empty_elsif_body() {
        // The outer `if` has a body, so only the empty `elsif` is flagged.
        test::<EmptyConditionalBody>().expect_offense(indoc! {r#"
            if condition
              do_something
            elsif other_condition
            ^^^^^^^^^^^^^^^^^^^^^ Avoid `elsif` branches without a body.
            end
        "#});
    }

    #[test]
    fn accepts_if_with_body() {
        test::<EmptyConditionalBody>().expect_no_offenses(indoc! {r#"
            if condition
              do_something
            end
        "#});
    }

    #[test]
    fn accepts_unless_with_body() {
        test::<EmptyConditionalBody>().expect_no_offenses(indoc! {r#"
            unless condition
              do_something
            end
        "#});
    }

    #[test]
    fn accepts_elsif_with_body() {
        test::<EmptyConditionalBody>().expect_no_offenses(indoc! {r#"
            if condition
              do_something
            elsif other_condition
              do_something_else
            end
        "#});
    }

    #[test]
    fn accepts_single_line_if_then_end() {
        // RuboCop's `same_line?(loc.begin, loc.end)` guard skips this form.
        test::<EmptyConditionalBody>().expect_no_offenses("if condition then end\n");
    }

    #[test]
    fn allows_comment_only_branch_by_default() {
        // AllowComments default true: a comment-only body is not flagged.
        test::<EmptyConditionalBody>().expect_no_offenses(indoc! {r#"
            if condition
              do_something
            elsif other_condition
              # noop
            end
        "#});
    }

    #[test]
    fn flags_comment_only_branch_when_allow_comments_disabled() {
        // `AllowComments: false` is read live via `cx.options_or_default`, so a
        // comment-only branch is flagged (RuboCop's `AllowComments: false`).
        test::<EmptyConditionalBody>()
            .with_options(&Options {
                allow_comments: false,
            })
            .expect_offense(indoc! {r#"
                if condition
                  do_something
                elsif other_condition
                ^^^^^^^^^^^^^^^^^^^^^ Avoid `elsif` branches without a body.
                  # noop
                end
            "#});
    }

    #[test]
    fn flags_empty_elsif_when_comment_is_in_else_not_elsif() {
        // RuboCop spec: a comment in a sibling `else` branch must NOT suppress
        // the offense for the empty `elsif`. The comment region is scoped to
        // the empty branch's body, bounded by the next `else`/`elsif` keyword.
        test::<EmptyConditionalBody>().expect_offense(indoc! {r#"
            if condition
              do_something
            elsif other_condition
            ^^^^^^^^^^^^^^^^^^^^^ Avoid `elsif` branches without a body.
            else
              # noop
            end
        "#});
    }

    #[test]
    fn flags_empty_if_branch_with_else() {
        // `if cond; else X; end` parses to the same shape as `unless cond; X;
        // end`, but here the if-branch is empty with an orphaned else — must
        // be flagged. This is the case the keyword-aware body selection guards.
        test::<EmptyConditionalBody>().expect_offense(indoc! {r#"
            if condition
            ^^^^^^^^^^^^ Avoid `if` branches without a body.
            else
              do_something
            end
        "#});
    }

    #[test]
    fn accepts_single_line_unless_then_end() {
        // The single-line guard short-circuits the `unless` form too.
        test::<EmptyConditionalBody>().expect_no_offenses("unless condition then end\n");
    }

    #[test]
    fn accepts_empty_else_branch() {
        // Empty `else` is Style/EmptyElse's job, not this cop's. The `if`
        // here has a body, so nothing is flagged.
        test::<EmptyConditionalBody>().expect_no_offenses(indoc! {r#"
            if condition
              do_something
            else
            end
        "#});
    }

    #[test]
    fn autocorrects_empty_if_with_populated_else() {
        test::<EmptyConditionalBody>().expect_correction(
            indoc! {r#"
                if condition
                ^^^^^^^^^^^^ Avoid `if` branches without a body.
                else
                  do_something
                end
            "#},
            "unless condition\n  do_something\nend\n",
        );
    }

    #[test]
    fn autocorrects_empty_unless_with_populated_else() {
        test::<EmptyConditionalBody>().expect_correction(
            indoc! {r#"
                unless condition
                ^^^^^^^^^^^^^^^^ Avoid `unless` branches without a body.
                else
                  do_something
                end
            "#},
            "if condition\n  do_something\nend\n",
        );
    }

    #[test]
    fn autocorrects_nested_empty_if_branch() {
        test::<EmptyConditionalBody>().expect_correction(
            indoc! {r#"
                if outer
                  if condition
                  ^^^^^^^^^^^^ Avoid `if` branches without a body.
                  else
                    do_something
                  end
                end
            "#},
            "if outer\n  unless condition\n    do_something\n  end\nend\n",
        );
    }

    #[test]
    fn autocorrects_inline_comment_when_comments_are_disallowed() {
        test::<EmptyConditionalBody>()
            .with_options(&Options { allow_comments: false }).expect_correction(
            indoc! {r#"
                if condition # empty
                ^^^^^^^^^^^^^^^^^^^^ Avoid `if` branches without a body.
                else
                  do_something
                end
            "#},
            "unless condition\n  do_something\nend\n",
        );
    }

    #[test]
    fn autocorrects_else_if_branch() {
        test::<EmptyConditionalBody>().expect_correction(
            indoc! {r#"
                if condition
                ^^^^^^^^^^^^ Avoid `if` branches without a body.
                else
                  if other
                    do_other
                  end
                end
            "#},
            "unless condition\n  if other\n    do_other\n  end\nend\n",
        );
    }

    #[test]
    fn removes_empty_branch_lines_inside_method() {
        test::<EmptyConditionalBody>().expect_correction(
            indoc! {r#"
                def m
                  if condition
                  ^^^^^^^^^^^^ Avoid `if` branches without a body.

                  else
                    do_something
                  end
                end
            "#},
            "def m\n  unless condition\n    do_something\n  end\nend\n",
        );
    }

    #[test]
    fn removes_comment_only_empty_branch_inside_method_when_comments_disallowed() {
        test::<EmptyConditionalBody>()
            .with_options(&Options { allow_comments: false }).expect_correction(
            indoc! {r#"
                def m
                  if condition
                  ^^^^^^^^^^^^ Avoid `if` branches without a body.
                    # no body
                  else
                    do_something
                  end
                end
            "#},
            "def m\n  unless condition\n    do_something\n  end\nend\n",
        );
    }

    #[test]
    fn preserves_blank_line_before_else_if_inside_method() {
        test::<EmptyConditionalBody>().expect_correction(
            indoc! {r#"
                def m
                  if condition
                  ^^^^^^^^^^^^ Avoid `if` branches without a body.

                  else
                    if other
                      do_other
                    end
                  end
                end
            "#},
            "def m\n  \n  unless condition\n    if other\n      do_other\n    end\n  end\nend\n",
        );
    }

    #[test]
    fn leaves_empty_elsif_with_final_else_offense_uncorrected() {
        test::<EmptyConditionalBody>().expect_offense(indoc! {r#"
            if condition
              do_something
            elsif other
            ^^^^^^^^^^^ Avoid `elsif` branches without a body.
            else
              do_other
            end
        "#});
        test::<EmptyConditionalBody>().expect_no_corrections(
            "if condition\n  do_something\nelsif other\nelse\n  do_other\nend\n",
        );
    }

    #[test]
    fn leaves_heredoc_condition_offense_uncorrected() {
        test::<EmptyConditionalBody>().expect_offense(indoc! {r#"
            if <<~TEXT
            ^^^^^^^^^^ Avoid `if` branches without a body.
              condition
            TEXT
            else
              do_something
            end
        "#});
        test::<EmptyConditionalBody>().expect_no_corrections(
            "if <<~TEXT\n  condition\nTEXT\nelse\n  do_something\nend\n",
        );
    }

    #[test]
    fn does_not_correct_empty_branch_without_else() {
        test::<EmptyConditionalBody>().expect_no_corrections("if condition\nend\n");
    }

    #[test]
    fn offense_message_matches_rubocop_verbatim() {
        test::<EmptyConditionalBody>().expect_offense(indoc! {r#"
            if x
            ^^^^ Avoid `if` branches without a body.
            end
        "#});
    }
}
