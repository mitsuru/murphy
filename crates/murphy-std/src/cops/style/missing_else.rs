//! `Style/MissingElse` — require `if`/`case` expressions to have an `else` branch.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MissingElse
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Disabled by default (matches RuboCop's default).
//!
//!   EnforcedStyle:
//!   - `both` (default) — requires `else` for both `if`/`unless` and `case`
//!   - `if` — requires `else` only for `if`/`unless` expressions
//!   - `case` — requires `else` only for `case` expressions
//!
//!   Modifier `if`/`unless` (one-line form) is always skipped — they cannot
//!   have an `else` branch. Detection: `loc.end_keyword()` is ZERO for
//!   modifier forms.
//!
//!   `unless` with an explicit `else` keyword is skipped unconditionally to
//!   avoid double-flagging when Style/UnlessElse is also active. `unless`
//!   without `else` is flagged normally (same as `if` without `else`).
//!
//!   `elsif` branches are skipped — they are nested `if` nodes that appear
//!   in the `else_` slot of a parent `if`. Only top-level `if` nodes are
//!   checked. An `if…elsif…end` chain (no final `else`) IS flagged.
//!
//!   Pattern matching (`case`/`in`) is not checked — same as upstream.
//!
//!   No autocorrect — Murphy does not implement the `else; nil; ` / `else; `
//!   insertion (upstream's correction requires Style/EmptyElse cross-config
//!   awareness). This is a known gap.
//!
//!   Gaps:
//!   - No autocorrect (upstream inserts `else; nil; ` or `else; `).
//!   - `unless` with `else` present is unconditionally skipped rather than
//!     skipped only when Style/UnlessElse is enabled.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "both")]
    Both,
    #[option(value = "if")]
    If,
    #[option(value = "case")]
    Case,
}

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "both",
        description = "Which expression types require an else clause: `both`, `if`, or `case`."
    )]
    pub enforced_style: EnforcedStyle,
}

const IF_MSG: &str = "'if' condition requires an `else`-clause.";
const CASE_MSG: &str = "'case' condition requires an `else`-clause.";

/// Stateless unit struct.
#[derive(Default)]
pub struct MissingElse;

#[cop(
    name = "Style/MissingElse",
    description = "Require `if`/`case` expressions to have an `else` branch.",
    default_severity = "warning",
    default_enabled = false,
    options = Options,
)]
impl MissingElse {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        if matches!(opts.enforced_style, EnforcedStyle::Case) {
            return;
        }
        check_if_node(node, cx);
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        if matches!(opts.enforced_style, EnforcedStyle::If) {
            return;
        }
        check_case_node(node, cx);
    }
}

fn check_if_node(node: NodeId, cx: &Cx<'_>) {
    // Skip modifier-form `if`/`unless` (no `end` keyword).
    if cx.loc(node).end_keyword() == Range::ZERO {
        return;
    }

    // Skip `elsif` branches — they are nested `if` nodes in the `else_` slot
    // of a parent `if` node.
    if cx.is_elsif(node) {
        return;
    }

    // If there is an explicit `else` keyword on this node, skip:
    // - Normal `if/else` — already satisfied.
    // - `unless/else` — skip to avoid conflict with Style/UnlessElse.
    if has_else_keyword(node, cx) {
        return;
    }

    let offense_range = first_line_range(node, cx);
    cx.emit_offense(offense_range, IF_MSG, None);
}

fn check_case_node(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Case { else_, .. } = *cx.kind(node) else {
        return;
    };

    // If `else_` is present (non-nil), the branch exists — no offense.
    if else_.get().is_some() {
        return;
    }

    let offense_range = first_line_range(node, cx);
    cx.emit_offense(offense_range, CASE_MSG, None);
}

/// Returns `true` when the node has an explicit `else` keyword in its source
/// (i.e. not just `elsif`). Scans tokens within the node range, excluding
/// tokens that belong to child nodes.
fn has_else_keyword(node: NodeId, cx: &Cx<'_>) -> bool {
    let node_range = cx.range(node);
    let children = cx.children(node);
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
        let inside_child = children.iter().any(|&child| {
            let r = cx.range(child);
            tok.range.start >= r.start && tok.range.end <= r.end
        });
        if !inside_child {
            return true;
        }
    }
    false
}

/// Returns the range of the first source line of the node (up to the first newline).
fn first_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let node_start = node_range.start as usize;
    let first_line_end = source[node_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(node_range.end as usize, |pos| node_start + pos);
    Range {
        start: node_range.start,
        end: first_line_end as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, MissingElse, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_if_without_else() {
        test::<MissingElse>().expect_offense(indoc! {"
            if x
            ^^^^ 'if' condition requires an `else`-clause.
              1
            end
        "});
    }

    #[test]
    fn accepts_if_with_else() {
        test::<MissingElse>().expect_no_offenses(indoc! {"
            if x
              1
            else
              2
            end
        "});
    }

    #[test]
    fn flags_case_without_else() {
        test::<MissingElse>().expect_offense(indoc! {"
            case x
            ^^^^^^ 'case' condition requires an `else`-clause.
            when 1
              :a
            end
        "});
    }

    #[test]
    fn accepts_case_with_else() {
        test::<MissingElse>().expect_no_offenses(indoc! {"
            case x
            when 1
              :a
            else
              :b
            end
        "});
    }

    #[test]
    fn accepts_modifier_if() {
        test::<MissingElse>().expect_no_offenses(indoc! {"
            a = 1 if x
        "});
    }

    #[test]
    fn accepts_modifier_unless() {
        test::<MissingElse>().expect_no_offenses(indoc! {"
            a = 1 unless x
        "});
    }

    #[test]
    fn flags_unless_without_else() {
        test::<MissingElse>().expect_offense(indoc! {"
            unless x
            ^^^^^^^^ 'if' condition requires an `else`-clause.
              1
            end
        "});
    }

    #[test]
    fn accepts_unless_with_else() {
        // unless...else is skipped to avoid conflict with Style/UnlessElse
        test::<MissingElse>().expect_no_offenses(indoc! {"
            unless x
              1
            else
              2
            end
        "});
    }

    #[test]
    fn flags_if_with_elsif_no_final_else() {
        // The top-level `if` is flagged; `elsif` branches are not.
        test::<MissingElse>().expect_offense(indoc! {"
            if x
            ^^^^ 'if' condition requires an `else`-clause.
              1
            elsif y
              2
            end
        "});
    }

    #[test]
    fn enforced_style_if_only_skips_case() {
        test::<MissingElse>()
            .with_options(&Options { enforced_style: EnforcedStyle::If })
            .expect_no_offenses(indoc! {"
                case x
                when 1
                  :a
                end
            "});
    }

    #[test]
    fn enforced_style_if_flags_if() {
        test::<MissingElse>()
            .with_options(&Options { enforced_style: EnforcedStyle::If })
            .expect_offense(indoc! {"
                if x
                ^^^^ 'if' condition requires an `else`-clause.
                  1
                end
            "});
    }

    #[test]
    fn enforced_style_case_only_skips_if() {
        test::<MissingElse>()
            .with_options(&Options { enforced_style: EnforcedStyle::Case })
            .expect_no_offenses(indoc! {"
                if x
                  1
                end
            "});
    }

    #[test]
    fn enforced_style_case_flags_case() {
        test::<MissingElse>()
            .with_options(&Options { enforced_style: EnforcedStyle::Case })
            .expect_offense(indoc! {"
                case x
                ^^^^^^ 'case' condition requires an `else`-clause.
                when 1
                  :a
                end
            "});
    }
}

murphy_plugin_api::submit_cop!(MissingElse);
