//! `Style/EmptyLambdaParameter` — flags empty parentheses in stabby lambda params.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EmptyLambdaParameter
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `-> ()` (empty lambda parameter parens) and autocorrects to `->`.
//!   Only applies to stabby lambda literals (`->`), not `lambda { }` forms.
//!   Only `Block` nodes are checked — `Numblock`/`Itblock` forms inherently
//!   have parameters and are not relevant.
//!
//!   Detection is token-based because the AST for `-> () {}` and `-> {}`
//!   are identical: both produce `(block (lambda) (args) …)`. We scan for
//!   a `LeftParen` immediately followed by a `RightParen` between the `->`
//!   operator end and the block opener.
//!
//!   Autocorrect: delete any whitespace between `->` and `(` plus `()`,
//!   yielding `-> { }` in all forms.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! -> () { do_something }
//!
//! # good
//! -> { do_something }
//!
//! # good — has parameters
//! -> (arg) { do_something(arg) }
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Omit parentheses for the empty lambda parameters.";

/// Stateless unit struct.
#[derive(Default)]
pub struct EmptyLambdaParameter;

#[cop(
    name = "Style/EmptyLambdaParameter",
    description = "Omit parens for empty lambda parameters.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EmptyLambdaParameter {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only stabby lambda literals (`->`), not `lambda { }` form.
    if !cx.is_lambda_literal(node) {
        return;
    }

    let NodeKind::Block { call, args, .. } = *cx.kind(node) else {
        return;
    };

    // If the args node has any children, there are actual parameters — no offense.
    let has_params = match *cx.kind(args) {
        NodeKind::Args(list) => !cx.list(list).is_empty(),
        _ => false,
    };
    if has_params {
        return;
    }

    // AST cannot distinguish `-> ()` from `->` — both have empty `(args)`.
    // Scan tokens for an empty paren pair after `->` and before the block opener.
    let arrow_range = cx.range(call);
    let node_end = cx.range(node).end;

    let Some(empty_parens) = find_empty_parens(arrow_range.end, node_end, cx) else {
        return;
    };

    // Offense range: the `()`
    cx.emit_offense(empty_parens, MSG, None);

    // Autocorrect: remove any whitespace between `->` and `(` plus `()`.
    // `-> () {}` → `-> {}`, `->() {}` → `-> {}`.
    let src = cx.source().as_bytes();
    let removal_start = scan_whitespace_before(src, empty_parens.start, arrow_range.end);
    cx.emit_edit(
        Range {
            start: removal_start,
            end: empty_parens.end,
        },
        "",
    );
}

/// Scan backward from `pos` past any spaces/tabs, stopping at `floor`.
fn scan_whitespace_before(src: &[u8], pos: u32, floor: u32) -> u32 {
    let mut i = pos as usize;
    while i > floor as usize && (src[i - 1] == b' ' || src[i - 1] == b'\t') {
        i -= 1;
    }
    i as u32
}

/// Find empty `()` between `from` and `until_end`.
/// Returns the range from `(` start to `)` end, or None if not found or not empty.
fn find_empty_parens(from: u32, until_end: u32, cx: &Cx<'_>) -> Option<Range> {
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);

    // Find first LeftParen.
    let open_pos = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < until_end)
        .position(|t| t.kind == SourceTokenKind::LeftParen)?;
    let open = &toks[idx + open_pos];

    // The very next token must be RightParen (empty parens).
    let close = toks.get(idx + open_pos + 1)?;
    if close.kind != SourceTokenKind::RightParen {
        return None;
    }

    Some(Range {
        start: open.range.start,
        end: close.range.end,
    })
}

#[cfg(test)]
mod tests {
    use super::EmptyLambdaParameter;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_lambda_parens() {
        test::<EmptyLambdaParameter>().expect_offense(indoc! {"
            -> () { do_something }
               ^^ Omit parentheses for the empty lambda parameters.
        "});
    }

    #[test]
    fn flags_empty_lambda_parens_no_space() {
        test::<EmptyLambdaParameter>().expect_offense(indoc! {"
            ->() { do_something }
              ^^ Omit parentheses for the empty lambda parameters.
        "});
    }

    #[test]
    fn accepts_lambda_without_parens() {
        test::<EmptyLambdaParameter>().expect_no_offenses("-> { do_something }\n");
    }

    #[test]
    fn accepts_lambda_with_params() {
        test::<EmptyLambdaParameter>().expect_no_offenses("-> (arg) { do_something(arg) }\n");
    }

    #[test]
    fn accepts_lambda_method_form() {
        test::<EmptyLambdaParameter>().expect_no_offenses("lambda { do_something }\n");
    }

    #[test]
    fn corrects_empty_lambda_parens() {
        test::<EmptyLambdaParameter>().expect_correction(
            indoc! {"
                -> () { do_something }
                   ^^ Omit parentheses for the empty lambda parameters.
            "},
            "-> { do_something }\n",
        );
    }

    #[test]
    fn corrects_empty_lambda_parens_no_space() {
        test::<EmptyLambdaParameter>().expect_correction(
            indoc! {"
                ->() { do_something }
                  ^^ Omit parentheses for the empty lambda parameters.
            "},
            "-> { do_something }\n",
        );
    }
}

murphy_plugin_api::submit_cop!(EmptyLambdaParameter);
