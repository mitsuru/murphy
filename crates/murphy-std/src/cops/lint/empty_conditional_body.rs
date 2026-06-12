//! `Lint/EmptyConditionalBody` — flag `if`, `elsif`, and `unless` branches
//! without a body.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EmptyConditionalBody
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-9cr.9
//!   - murphy-g2lu
//! notes: >
//!   Detection mirrors RuboCop's on_if: flags an `if`/`elsif`/`unless` whose
//!   body (`if_branch`) is empty, skipping the single-line `if x then end`
//!   form (RuboCop's `same_line?(loc.begin, loc.end)` guard) and, by default,
//!   comment-only branches (AllowComments). `elsif` is a nested `If` node, so
//!   the per-node handler fires once per branch (matching RuboCop's per-node
//!   on_if). Offense highlight is clamped to the node's first line (Murphy
//!   convention) vs RuboCop's keyword..else range; start position matches.
//!   `AllowComments` defaults to `true` (matching RuboCop) but the override is
//!   ABI-blocked until murphy-9cr.9; the default IS the live behavior. The
//!   `flip_orphaned_else` autocorrect is NOT ported (genuinely structural —
//!   condition flip + branch removal + line math); detection only.
//! ```
//!
//! ## Deferred: the `flip_orphaned_else` autocorrect
//!
//! RuboCop offers a contextual autocorrect that, when an empty `if` has an
//! orphaned `else`, flips the condition (`if x` → `unless x`) and removes the
//! empty branch. This is a structural rewrite with non-trivial line math; it
//! is intentionally not ported in this pass. Detection and message match
//! RuboCop; only the corrector is omitted.

use murphy_plugin_api::{CopOptions, Cx, NodeId, Range, cop};

#[derive(Default)]
pub struct EmptyConditionalBody;

/// Cop options for [`EmptyConditionalBody`]. v1: read from `Default` at
/// dispatch time (`murphy-9cr.9` will wire live overrides through `Cx`).
#[derive(CopOptions)]
pub struct Options {
    #[option(
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
        let opts = Options::default();
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
    }
}

/// Whether `node`'s source range fits on a single physical line — Murphy's
/// equivalent of RuboCop's `same_line?(node.loc.begin, node.loc.end)`, which
/// suppresses the one-line `if x then end` form.
fn is_single_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    let bytes = cx.source().as_bytes();
    !bytes[range.start as usize..range.end as usize].contains(&b'\n')
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
    use super::EmptyConditionalBody;
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
            ^^^^^^^^^^^^^^^^^^^^^^ Avoid `elsif` branches without a body.
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
    fn flags_empty_elsif_when_comment_is_in_else_not_elsif() {
        // RuboCop spec: a comment in a sibling `else` branch must NOT suppress
        // the offense for the empty `elsif`. The comment region is scoped to
        // the empty branch's body, bounded by the next `else`/`elsif` keyword.
        test::<EmptyConditionalBody>().expect_offense(indoc! {r#"
            if condition
              do_something
            elsif other_condition
            ^^^^^^^^^^^^^^^^^^^^^^ Avoid `elsif` branches without a body.
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
    fn offense_message_matches_rubocop_verbatim() {
        test::<EmptyConditionalBody>().expect_offense(indoc! {r#"
            if x
            ^^^^ Avoid `if` branches without a body.
            end
        "#});
    }
}
