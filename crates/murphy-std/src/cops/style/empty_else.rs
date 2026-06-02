//! `Style/EmptyElse` — flags empty `else`-clauses and `else`-clauses with only `nil`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EmptyElse
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   EnforcedStyle:
//!   - `both` (default) — flags both empty `else` and `else; nil`.
//!   - `empty` — flags only empty `else` (no branch).
//!   - `nil` — flags only `else; nil`.
//!
//!   AllowComments (default false): if true, an `else` with a comment is skipped.
//!   Not yet implemented — always treated as false (comments not checked).
//!
//!   Offense range: the `else` keyword token.
//!
//!   Detection for empty-else uses token scanning (same approach as
//!   `Style/MissingElse::has_else_keyword`) because the AST for
//!   `if x; 1; else; end` and `if x; 1; end` are identical (both have
//!   else_branch = None).
//!
//!   Ternary (`x ? a : b`) and modifier-form (`a if b`) are guarded.
//!
//!   Autocorrect: deletes from `else` keyword start to `end` keyword start
//!   (for if/unless) or from `else` keyword start to `end` keyword start
//!   (for case). For case nodes, removes `else` through to end.
//!
//!   Gaps:
//!   - `AllowComments` option is not implemented (always false).
//!   - `Style/MissingElse` cross-config check (`autocorrect_forbidden?`) is
//!     not implemented — Murphy always corrects.
//!   - The elsif `base_node` walk for finding the `end` keyword of an if/elsif
//!     chain is simplified: the end keyword is taken from the root node.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (both styles)
//! if condition
//!   result
//! else
//! end
//!
//! if condition
//!   result
//! else
//!   nil
//! end
//!
//! # good
//! if condition
//!   result
//! end
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Redundant `else`-clause.";

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "both")]
    Both,
    #[option(value = "empty")]
    Empty,
    #[option(value = "nil")]
    Nil,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "both",
        description = "Which else-clause styles to flag: `both`, `empty`, or `nil`."
    )]
    pub enforced_style: EnforcedStyle,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct EmptyElse;

#[cop(
    name = "Style/EmptyElse",
    description = "Avoid empty else-clauses.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl EmptyElse {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip ternary and modifier forms.
        if cx.is_ternary(node) || cx.is_modifier_form(node) {
            return;
        }
        let opts = cx.options_or_default::<Options>();
        let style = opts.enforced_style;
        if matches!(style, EnforcedStyle::Both | EnforcedStyle::Empty) {
            check_empty_else(node, cx);
        }
        if matches!(style, EnforcedStyle::Both | EnforcedStyle::Nil) {
            check_nil_else(node, cx);
        }
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        let style = opts.enforced_style;
        if matches!(style, EnforcedStyle::Both | EnforcedStyle::Empty) {
            check_empty_else(node, cx);
        }
        if matches!(style, EnforcedStyle::Both | EnforcedStyle::Nil) {
            check_nil_else(node, cx);
        }
    }
}

/// Check for an empty `else` (else keyword present but branch is absent).
fn check_empty_else(node: NodeId, cx: &Cx<'_>) {
    let else_branch_present = match *cx.kind(node) {
        NodeKind::If { .. } => cx.if_else_branch(node).get().is_some(),
        NodeKind::Case { else_, .. } => else_.get().is_some(),
        _ => return,
    };

    // An empty else has no branch node.
    if else_branch_present {
        return;
    }

    // Scan for `else` keyword token — distinguishes empty-else from no-else.
    let Some(else_range) = find_else_token(node, cx) else {
        return;
    };

    cx.emit_offense(else_range, MSG, None);
    autocorrect(node, else_range, cx);
}

/// Check for a `nil` else (`else; nil; end`).
fn check_nil_else(node: NodeId, cx: &Cx<'_>) {
    let else_branch = match *cx.kind(node) {
        NodeKind::If { .. } => cx.if_else_branch(node).get(),
        NodeKind::Case { else_, .. } => else_.get(),
        _ => return,
    };

    let Some(branch_id) = else_branch else {
        return;
    };

    // Branch must be a `nil` literal node.
    if !matches!(cx.kind(branch_id), NodeKind::Nil) {
        return;
    }

    // Find the `else` keyword for the offense range.
    let Some(else_range) = find_else_token(node, cx) else {
        return;
    };

    cx.emit_offense(else_range, MSG, None);
    autocorrect(node, else_range, cx);
}

/// Find the `else` keyword token directly belonging to `node` (not inside a child).
fn find_else_token(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    // Build direct child ranges to exclude `else` inside children.
    let child_ranges: Vec<Range> = direct_child_ranges(node, cx);

    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < node_range.start);

    for tok in &toks[idx..] {
        if tok.range.start >= node_range.end {
            break;
        }
        if tok.kind != SourceTokenKind::Other {
            continue;
        }
        let text = &source[tok.range.start as usize..tok.range.end as usize];
        if text != b"else" {
            continue;
        }
        // Make sure it's not inside a child node.
        let inside_child = child_ranges.iter().any(|r| {
            tok.range.start >= r.start && tok.range.end <= r.end
        });
        if !inside_child {
            return Some(tok.range);
        }
    }
    None
}

/// Returns the ranges of direct child nodes of `node`.
fn direct_child_ranges(node: NodeId, cx: &Cx<'_>) -> Vec<Range> {
    let mut ranges = Vec::new();
    match *cx.kind(node) {
        NodeKind::If { .. } => {
            if let Some(id) = cx.if_condition(node).get() {
                ranges.push(cx.range(id));
            }
            if let Some(id) = cx.if_then_branch(node).get() {
                ranges.push(cx.range(id));
            }
            if let Some(id) = cx.if_else_branch(node).get() {
                ranges.push(cx.range(id));
            }
        }
        NodeKind::Case { subject, else_, .. } => {
            if let Some(id) = subject.get() {
                ranges.push(cx.range(id));
            }
            if let Some(id) = else_.get() {
                ranges.push(cx.range(id));
            }
            // We don't need to add `when` children explicitly since they
            // won't contain a top-level `else` keyword.
        }
        _ => {}
    }
    ranges
}

/// Autocorrect: remove from `else` keyword start to the `end` keyword start.
fn autocorrect(node: NodeId, else_range: Range, cx: &Cx<'_>) {
    let end_kw = cx.loc(node).end_keyword();
    if end_kw == Range::ZERO {
        // No `end` keyword found — skip autocorrect.
        return;
    }
    cx.emit_edit(
        Range {
            start: else_range.start,
            end: end_kw.start,
        },
        "",
    );
}

#[cfg(test)]
mod tests {
    use super::{EmptyElse, EnforcedStyle, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- both style (default) ---

    #[test]
    fn flags_empty_else_in_if() {
        test::<EmptyElse>().expect_offense(indoc! {"
            if x
              1
            else
            ^^^^ Redundant `else`-clause.
            end
        "});
    }

    #[test]
    fn flags_nil_else_in_if() {
        test::<EmptyElse>().expect_offense(indoc! {"
            if x
              1
            else
            ^^^^ Redundant `else`-clause.
              nil
            end
        "});
    }

    #[test]
    fn flags_empty_else_in_case() {
        test::<EmptyElse>().expect_offense(indoc! {"
            case x
            when 1
              :a
            else
            ^^^^ Redundant `else`-clause.
            end
        "});
    }

    #[test]
    fn flags_nil_else_in_case() {
        test::<EmptyElse>().expect_offense(indoc! {"
            case x
            when 1
              :a
            else
            ^^^^ Redundant `else`-clause.
              nil
            end
        "});
    }

    #[test]
    fn accepts_if_without_else() {
        test::<EmptyElse>().expect_no_offenses(indoc! {"
            if x
              1
            end
        "});
    }

    #[test]
    fn accepts_if_with_non_empty_else() {
        test::<EmptyElse>().expect_no_offenses(indoc! {"
            if x
              1
            else
              2
            end
        "});
    }

    #[test]
    fn accepts_ternary() {
        test::<EmptyElse>().expect_no_offenses("x ? 1 : 2\n");
    }

    #[test]
    fn accepts_modifier_if() {
        test::<EmptyElse>().expect_no_offenses("a = 1 if x\n");
    }

    // --- empty style ---

    #[test]
    fn empty_style_flags_empty_else() {
        test::<EmptyElse>()
            .with_options(&Options { enforced_style: EnforcedStyle::Empty })
            .expect_offense(indoc! {"
                if x
                  1
                else
                ^^^^ Redundant `else`-clause.
                end
            "});
    }

    #[test]
    fn empty_style_accepts_nil_else() {
        test::<EmptyElse>()
            .with_options(&Options { enforced_style: EnforcedStyle::Empty })
            .expect_no_offenses(indoc! {"
                if x
                  1
                else
                  nil
                end
            "});
    }

    // --- nil style ---

    #[test]
    fn nil_style_flags_nil_else() {
        test::<EmptyElse>()
            .with_options(&Options { enforced_style: EnforcedStyle::Nil })
            .expect_offense(indoc! {"
                if x
                  1
                else
                ^^^^ Redundant `else`-clause.
                  nil
                end
            "});
    }

    #[test]
    fn nil_style_accepts_empty_else() {
        test::<EmptyElse>()
            .with_options(&Options { enforced_style: EnforcedStyle::Nil })
            .expect_no_offenses(indoc! {"
                if x
                  1
                else
                end
            "});
    }

    // --- autocorrect ---

    #[test]
    fn corrects_empty_else_in_if() {
        test::<EmptyElse>().expect_correction(
            indoc! {"
                if x
                  1
                else
                ^^^^ Redundant `else`-clause.
                end
            "},
            "if x\n  1\nend\n",
        );
    }

    #[test]
    fn corrects_nil_else_in_if() {
        test::<EmptyElse>().expect_correction(
            indoc! {"
                if x
                  1
                else
                ^^^^ Redundant `else`-clause.
                  nil
                end
            "},
            "if x\n  1\nend\n",
        );
    }

    #[test]
    fn corrects_empty_else_in_case() {
        test::<EmptyElse>().expect_correction(
            indoc! {"
                case x
                when 1
                  :a
                else
                ^^^^ Redundant `else`-clause.
                end
            "},
            "case x\nwhen 1\n  :a\nend\n",
        );
    }
}

murphy_plugin_api::submit_cop!(EmptyElse);
