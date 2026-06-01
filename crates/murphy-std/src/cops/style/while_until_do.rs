//! `Style/WhileUntilDo` — flags redundant `do` in multi-line `while`/`until`
//! statements.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/WhileUntilDo
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detects multi-line while/until loops that contain a redundant `do`
//!   keyword and autocorrects by removing `do` along with the leading space.
//!   Post-condition loops (begin..end while c) are skipped since Murphy
//!   folds them into While{post:true} nodes.
//! ```
//!
//! ## Matched shapes
//!
//! `While` and `Until` nodes that:
//! - Are NOT post-condition loops (`While{post:false}` / `Until{post:false}`)
//! - Are multi-line
//! - Have a `do` keyword token in the gap between condition end and body start
//!
//! ## Autocorrect
//!
//! Removes `[condition.end, do_token.end)` — deletes the space + `do`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Do not use `do` with multi-line `%<keyword>s`.";

#[derive(Default)]
pub struct WhileUntilDo;

#[cop(
    name = "Style/WhileUntilDo",
    description = "Checks for redundant `do` after `while` or `until`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl WhileUntilDo {
    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx, "while");
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx, "until");
    }
}

fn check(node: NodeId, cx: &Cx<'_>, keyword: &str) {
    // Skip post-condition loops (begin..end while c).
    if cx.is_post_condition_loop(node) {
        return;
    }

    // Skip modifier-form loops (x while cond).
    if cx.is_modifier_form(node) {
        return;
    }

    // Must be multiline.
    if !cx.is_multiline(node) {
        return;
    }

    // Extract cond from node kind.
    let cond = match *cx.kind(node) {
        NodeKind::While { cond, .. } => cond,
        NodeKind::Until { cond, .. } => cond,
        _ => return,
    };

    let cond_end = cx.range(cond).end;
    let node_end = cx.range(node).end;

    // Find `do` token in the gap [cond_end, node_end).
    let Some(do_range) = find_do_in_gap(cx, cond_end, node_end) else {
        return;
    };

    let message = MSG.replacen("%<keyword>s", keyword, 1);
    cx.emit_offense(do_range, &message, None);

    // Autocorrect: remove from cond_end to do_range.end (space + `do`).
    let remove_range = Range {
        start: cond_end,
        end: do_range.end,
    };
    cx.emit_edit(remove_range, "");
}

/// Scan tokens in `[from, to)` for a `do` keyword (`Other` token with text
/// `b"do"`). Returns the range of the first one found, or `None`.
fn find_do_in_gap(cx: &Cx<'_>, from: u32, to: u32) -> Option<Range> {
    if from >= to {
        return None;
    }
    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        if tok.kind == SourceTokenKind::Other
            && &src[tok.range.start as usize..tok.range.end as usize] == b"do"
        {
            return Some(tok.range);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::WhileUntilDo;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_while_do_multiline() {
        test::<WhileUntilDo>().expect_correction(
            indoc! {"
                while x.any? do
                             ^^ Do not use `do` with multi-line `while`.
                  do_something(x.pop)
                end
            "},
            "while x.any?\n  do_something(x.pop)\nend\n",
        );
    }

    #[test]
    fn flags_until_do_multiline() {
        test::<WhileUntilDo>().expect_correction(
            indoc! {"
                until x.empty? do
                               ^^ Do not use `do` with multi-line `until`.
                  do_something(x.pop)
                end
            "},
            "until x.empty?\n  do_something(x.pop)\nend\n",
        );
    }

    #[test]
    fn accepts_while_without_do() {
        test::<WhileUntilDo>().expect_no_offenses(indoc! {"
            while x.any?
              do_something(x.pop)
            end
        "});
    }

    #[test]
    fn accepts_until_without_do() {
        test::<WhileUntilDo>().expect_no_offenses(indoc! {"
            until x.empty?
              do_something(x.pop)
            end
        "});
    }

    #[test]
    fn accepts_single_line_while_do() {
        // Single-line is not flagged (not multiline).
        test::<WhileUntilDo>().expect_no_offenses("while x do y end\n");
    }

    #[test]
    fn accepts_modifier_while() {
        // Modifier form has no `do`.
        test::<WhileUntilDo>().expect_no_offenses("x += 1 while x < 10\n");
    }

    #[test]
    fn accepts_post_condition_while() {
        // begin..end while c — post-condition, no `do` to flag.
        test::<WhileUntilDo>().expect_no_offenses(indoc! {"
            begin
              do_something
            end while condition
        "});
    }
}
