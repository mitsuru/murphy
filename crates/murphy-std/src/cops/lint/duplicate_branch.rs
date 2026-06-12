//! `Lint/DuplicateBranch` — flag repeated branch bodies within `if/unless`,
//! `case`/`case-match`, and `rescue` constructs.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DuplicateBranch
//! upstream_version_checked: 1.86.2
//! version_added: "1.3"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues:
//!   - murphy-ff1x
//! notes: >
//!   Covers if/elsif/else, case/when, case/in, ternary, and begin/rescue/else
//!   duplicate detection, plus IgnoreLiteralBranches / IgnoreConstantBranches /
//!   IgnoreDuplicateElseBranch options. Branch equality is compared via
//!   `raw_source` (whitespace-sensitive), a known divergence from RuboCop's
//!   structural AST `==`. `IgnoreLiteralBranches`' literal-descendant walk
//!   approximates RuboCop's `basic_literal?` / container check.
//! ```
//!
//! ## Matched shapes
//!
//! The branch bodies of `if`/`unless` (non-elsif entry), `case`, `case-match`,
//! and `rescue`. The first occurrence of an equal body is the baseline; every
//! later equal body is flagged.
//!
//! ## No autocorrect
//!
//! RuboCop ships no autocorrect — collapsing duplicate branches changes control
//! flow and the intended branch is ambiguous.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Duplicate branch body detected.";

#[derive(Default)]
pub struct DuplicateBranch;

#[derive(CopOptions)]
pub struct Options {
    #[option(default = false, description = "Ignore branches whose body is a literal.")]
    pub ignore_literal_branches: bool,
    #[option(default = false, description = "Ignore branches whose body is a constant.")]
    pub ignore_constant_branches: bool,
    #[option(default = false, description = "Ignore a duplicate else branch in a multi-branch construct.")]
    pub ignore_duplicate_else_branch: bool,
}

/// One branch: its body node, the range to flag if it is a duplicate, and
/// whether it is the trailing `else` branch.
struct Branch {
    body: NodeId,
    offense_range: Range,
    is_else: bool,
}

#[cop(
    name = "Lint/DuplicateBranch",
    description = "Flag repeated branch bodies in conditionals and rescues.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl DuplicateBranch {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // elsif nodes are handled as part of the parent `if`'s branch walk.
        if cx.is_elsif(node) {
            return;
        }
        let branches = if_branches(node, cx);
        flag_duplicates(node, &branches, cx);
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        let branches = case_branches(node, cx);
        flag_duplicates(node, &branches, cx);
    }

    #[on_node(kind = "case_match")]
    fn check_case_match(&self, node: NodeId, cx: &Cx<'_>) {
        let branches = case_match_branches(node, cx);
        flag_duplicates(node, &branches, cx);
    }

    #[on_node(kind = "rescue")]
    fn check_rescue(&self, node: NodeId, cx: &Cx<'_>) {
        let branches = rescue_branches(node, cx);
        flag_duplicates(node, &branches, cx);
    }
}

/// Dedup branch bodies by source text and emit an offense for every body whose
/// source already appeared in an earlier branch.
fn flag_duplicates(node: NodeId, branches: &[Branch], cx: &Cx<'_>) {
    let opts = cx.options_or_default::<Options>();
    let mut seen: Vec<&str> = Vec::with_capacity(branches.len());

    for (idx, branch) in branches.iter().enumerate() {
        if !consider_branch(node, branches, branch, idx, &opts, cx) {
            continue;
        }
        let src = cx.raw_source(cx.range(branch.body));
        if seen.contains(&src) {
            cx.emit_offense(branch.offense_range, MSG, None);
        } else {
            seen.push(src);
        }
    }
}

fn consider_branch(
    node: NodeId,
    branches: &[Branch],
    branch: &Branch,
    idx: usize,
    opts: &Options,
    cx: &Cx<'_>,
) -> bool {
    if opts.ignore_literal_branches && is_literal_branch(branch.body, opts, cx) {
        return false;
    }
    if opts.ignore_constant_branches && is_const_branch(branch.body, cx) {
        return false;
    }
    if opts.ignore_duplicate_else_branch && is_duplicate_else_branch(node, branches, branch, idx) {
        return false;
    }
    true
}

/// RuboCop: `branches.size > 2 && branch is the last && parent has an else`.
fn is_duplicate_else_branch(
    _node: NodeId,
    branches: &[Branch],
    branch: &Branch,
    idx: usize,
) -> bool {
    branches.len() > 2 && idx == branches.len() - 1 && branch.is_else
}

/// RuboCop's `const_branch?`.
fn is_const_branch(body: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(body), NodeKind::Const { .. })
}

/// RuboCop's `literal_branch?`: a literal that is not an xstr; basic literals
/// pass directly, containers pass when every descendant is a basic literal, a
/// pair, or (when IgnoreConstantBranches) a const.
fn is_literal_branch(body: NodeId, opts: &Options, cx: &Cx<'_>) -> bool {
    if matches!(cx.kind(body), NodeKind::Xstr(..)) {
        return false;
    }
    if is_basic_literal(body, cx) {
        return true;
    }
    if !is_container_literal(body, cx) {
        return false;
    }
    cx.descendants(body).iter().all(|&n| {
        is_basic_literal(n, cx)
            || matches!(cx.kind(n), NodeKind::Pair { .. })
            || (matches!(cx.kind(n), NodeKind::Const { .. }) && opts.ignore_constant_branches)
    })
}

/// RuboCop's `basic_literal?` — scalar literals with no sub-expressions.
fn is_basic_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Int(..)
            | NodeKind::Float(..)
            | NodeKind::Rational(..)
            | NodeKind::Complex(..)
            | NodeKind::Str(..)
            | NodeKind::Sym(..)
            | NodeKind::True_
            | NodeKind::False_
            | NodeKind::Nil
    )
}

/// Container literals whose descendants are walked for `literal?`.
fn is_container_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Array(..) | NodeKind::Hash(..) | NodeKind::RangeExpr { .. }
    )
}

/// Build `if`/`unless`/elsif-chain branches. Mirrors rubocop-ast `IfNode#branches`
/// (flattened over elsif), with nil bodies dropped (RuboCop's `.compact`).
fn if_branches(node: NodeId, cx: &Cx<'_>) -> Vec<Branch> {
    let mut out = Vec::new();
    let ternary = cx.is_ternary(node);

    // Prism normalizes `unless foo; A; else; B; end` to `if foo; B; else; A`,
    // swapping the then/else bodies relative to source. RuboCop's `branches`
    // walks source order, so for an `unless` we read the true branch from
    // `if_else_branch` and the else branch from `if_then_branch`. `unless`
    // cannot carry `elsif`, so the chain is at most two bodies.
    let is_unless = cx.if_keyword(node) == "unless";
    let (then_branch, else_chain_start) = if is_unless {
        (cx.if_else_branch(node).get(), cx.if_then_branch(node).get())
    } else {
        (cx.if_then_branch(node).get(), cx.if_else_branch(node).get())
    };

    // The `if`/true branch — its label is the node itself (only flagged if it
    // duplicates a later branch, which the order-sensitive dedup prevents, but
    // we still record it as a baseline).
    if let Some(body) = then_branch {
        out.push(Branch {
            body,
            offense_range: cx.range(body),
            is_else: false,
        });
    }

    // Walk the else chain: each elsif contributes its body; the final plain
    // else contributes the else body.
    let mut current = else_chain_start;
    while let Some(id) = current {
        if matches!(cx.kind(id), NodeKind::If { .. }) && cx.is_elsif(id) {
            if let Some(body) = cx.if_then_branch(id).get() {
                out.push(Branch {
                    body,
                    offense_range: first_line_range(cx.range(id).start, cx),
                    is_else: false,
                });
            }
            current = cx.if_else_branch(id).get();
        } else {
            // Plain else body.
            let offense_range = if ternary {
                cx.range(id)
            } else {
                else_keyword_range(node, cx.range(id).start, cx).unwrap_or(cx.range(id))
            };
            out.push(Branch {
                body: id,
                offense_range,
                is_else: true,
            });
            break;
        }
    }

    out
}

fn case_branches(node: NodeId, cx: &Cx<'_>) -> Vec<Branch> {
    let mut out = Vec::new();
    for &when in cx.case_when_branches(node) {
        if let Some(body) = cx.when_body(when).get() {
            out.push(Branch {
                body,
                offense_range: first_line_range(cx.range(when).start, cx),
                is_else: false,
            });
        }
    }
    if let Some(else_body) = cx.case_else_branch(node).get() {
        out.push(Branch {
            body: else_body,
            offense_range: else_keyword_range(node, cx.range(else_body).start, cx)
                .unwrap_or(cx.range(else_body)),
            is_else: true,
        });
    }
    out
}

fn case_match_branches(node: NodeId, cx: &Cx<'_>) -> Vec<Branch> {
    let mut out = Vec::new();
    for &in_pat in cx.in_pattern_branches(node) {
        if let Some(body) = cx.in_pattern_body(in_pat).get() {
            out.push(Branch {
                body,
                offense_range: first_line_range(cx.range(in_pat).start, cx),
                is_else: false,
            });
        }
    }
    if let Some(else_body) = cx.case_match_else_branch(node).get() {
        out.push(Branch {
            body: else_body,
            offense_range: else_keyword_range(node, cx.range(else_body).start, cx)
                .unwrap_or(cx.range(else_body)),
            is_else: true,
        });
    }
    out
}

fn rescue_branches(node: NodeId, cx: &Cx<'_>) -> Vec<Branch> {
    let NodeKind::Rescue {
        resbodies, else_, ..
    } = *cx.kind(node)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for &resbody in cx.list(resbodies) {
        let NodeKind::Resbody { body, .. } = *cx.kind(resbody) else {
            continue;
        };
        if let Some(body) = body.get() {
            out.push(Branch {
                body,
                offense_range: first_line_range(cx.range(resbody).start, cx),
                is_else: false,
            });
        }
    }
    if let Some(else_body) = else_.get() {
        out.push(Branch {
            body: else_body,
            offense_range: else_keyword_range(node, cx.range(else_body).start, cx)
                .unwrap_or(cx.range(else_body)),
            is_else: true,
        });
    }
    out
}

/// Range from `start` to the end of its source line (mirrors RuboCop showing
/// only the first line of a multi-line `parent.source_range`).
fn first_line_range(start: u32, cx: &Cx<'_>) -> Range {
    let source = cx.source().as_bytes();
    let mut end = start as usize;
    while end < source.len() && source[end] != b'\n' {
        end += 1;
    }
    Range {
        start,
        end: end as u32,
    }
}

/// Find the `else` keyword token belonging to `node` that immediately precedes
/// the else body starting at `body_start`.
fn else_keyword_range(node: NodeId, body_start: u32, cx: &Cx<'_>) -> Option<Range> {
    let node_range = cx.range(node);
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < node_range.start);

    let mut found = None;
    for tok in &toks[idx..] {
        if tok.range.start >= body_start {
            break;
        }
        if tok.kind == SourceTokenKind::Other && cx.token_text(*tok) == "else" {
            found = Some(tok.range);
        }
    }
    found
}

murphy_plugin_api::submit_cop!(DuplicateBranch);

#[cfg(test)]
mod tests {
    use super::{DuplicateBranch, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_else_in_if() {
        test::<DuplicateBranch>().expect_offense(indoc! {r#"
            if foo
              do_foo
            else
            ^^^^ Duplicate branch body detected.
              do_foo
            end
        "#});
    }

    #[test]
    fn flags_duplicate_elsif() {
        test::<DuplicateBranch>().expect_offense(indoc! {r#"
            if foo
              do_foo
            elsif bar
            ^^^^^^^^^ Duplicate branch body detected.
              do_foo
            end
        "#});
    }

    #[test]
    fn flags_multiple_duplicate_elsif() {
        test::<DuplicateBranch>().expect_offense(indoc! {r#"
            if foo
              do_foo
            elsif bar
              do_bar
            elsif baz
            ^^^^^^^^^ Duplicate branch body detected.
              do_foo
            elsif quux
            ^^^^^^^^^^ Duplicate branch body detected.
              do_bar
            end
        "#});
    }

    #[test]
    fn flags_duplicate_unless_else() {
        test::<DuplicateBranch>().expect_offense(indoc! {r#"
            unless foo
              do_bar
            else
            ^^^^ Duplicate branch body detected.
              do_bar
            end
        "#});
    }

    #[test]
    fn flags_duplicate_when() {
        test::<DuplicateBranch>().expect_offense(indoc! {r#"
            case x
            when foo
              do_foo
            when bar
            ^^^^^^^^ Duplicate branch body detected.
              do_foo
            end
        "#});
    }

    #[test]
    fn flags_duplicate_case_else() {
        test::<DuplicateBranch>().expect_offense(indoc! {r#"
            case x
            when foo
              do_foo
            else
            ^^^^ Duplicate branch body detected.
              do_foo
            end
        "#});
    }

    #[test]
    fn flags_duplicate_rescue() {
        test::<DuplicateBranch>().expect_offense(indoc! {r#"
            begin
              do_something
            rescue FooError
              handle_error(x)
            rescue BarError
            ^^^^^^^^^^^^^^^ Duplicate branch body detected.
              handle_error(x)
            end
        "#});
    }

    #[test]
    fn flags_duplicate_rescue_else() {
        test::<DuplicateBranch>().expect_offense(indoc! {r#"
            begin
              do_something
            rescue FooError
              handle_error(x)
            else
            ^^^^ Duplicate branch body detected.
              handle_error(x)
            end
        "#});
    }

    #[test]
    fn flags_ternary_duplicate() {
        test::<DuplicateBranch>().expect_offense(indoc! {r#"
            res = foo ? do_foo : do_foo
                                 ^^^^^^ Duplicate branch body detected.
        "#});
    }

    #[test]
    fn flags_duplicate_case_in() {
        test::<DuplicateBranch>().expect_offense(indoc! {r#"
            case x
            in 1
              do_foo
            in 2
            ^^^^ Duplicate branch body detected.
              do_foo
            end
        "#});
    }

    #[test]
    fn flags_duplicate_case_in_else() {
        test::<DuplicateBranch>().expect_offense(indoc! {r#"
            case x
            in 1
              do_foo
            else
            ^^^^ Duplicate branch body detected.
              do_foo
            end
        "#});
    }

    #[test]
    fn does_not_flag_distinct_branches() {
        test::<DuplicateBranch>().expect_no_offenses(indoc! {r#"
            if foo
              do_foo
            elsif bar
              do_bar
            else
              do_baz
            end
        "#});
    }

    #[test]
    fn ignore_literal_branches_allows_literals() {
        test::<DuplicateBranch>()
            .with_options(&Options {
                ignore_literal_branches: true,
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                if foo
                  5
                else
                  5
                end
            "#});
    }

    #[test]
    fn ignore_literal_branches_still_flags_method_calls() {
        test::<DuplicateBranch>()
            .with_options(&Options {
                ignore_literal_branches: true,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                if foo
                  do_foo
                else
                ^^^^ Duplicate branch body detected.
                  do_foo
                end
            "#});
    }

    #[test]
    fn ignore_constant_branches_allows_constants() {
        test::<DuplicateBranch>()
            .with_options(&Options {
                ignore_constant_branches: true,
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                if foo
                  CONST
                else
                  CONST
                end
            "#});
    }

    #[test]
    fn ignore_duplicate_else_branch_allows_multi_branch_else() {
        test::<DuplicateBranch>()
            .with_options(&Options {
                ignore_duplicate_else_branch: true,
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                if foo
                  do_foo
                elsif bar
                  do_bar
                else
                  do_foo
                end
            "#});
    }

    #[test]
    fn ignore_duplicate_else_branch_still_flags_single_branch_else() {
        test::<DuplicateBranch>()
            .with_options(&Options {
                ignore_duplicate_else_branch: true,
                ..Default::default()
            })
            .expect_offense(indoc! {r#"
                if foo
                  do_foo
                else
                ^^^^ Duplicate branch body detected.
                  do_foo
                end
            "#});
    }
}
