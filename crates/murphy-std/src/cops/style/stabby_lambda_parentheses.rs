//! `Style/StabbyLambdaParentheses` — checks for parentheses around stabby
//! lambda arguments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/StabbyLambdaParentheses
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Two EnforcedStyle values are implemented:
//!     require_parentheses (default): flag stabby lambdas whose args lack parens.
//!     require_no_parentheses: flag stabby lambdas whose args have parens.
//!   Zero-arg lambdas (-> {} and ->() {}) are never flagged — consistent with
//!   RuboCop's arguments? guard. Empty-parens ->() {} is not flagged under
//!   either style because args is empty.
//!   Parentheses presence is detected by token-scanning: the first token after
//!   the -> marker is LeftParen iff parens are present.
//!   Autocorrect:
//!     require_parentheses: wraps the args range with ( and ).
//!     require_no_parentheses: removes the ( and ) tokens.
//! ```
//!
//! ## Examples
//!
//! ```ruby
//! # require_parentheses (default)
//! # bad
//! ->a,b,c { a + b + c }
//! # good
//! ->(a,b,c) { a + b + c }
//!
//! # require_no_parentheses
//! # bad
//! ->(a,b,c) { a + b + c }
//! # good
//! ->a,b,c { a + b + c }
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG_REQUIRE: &str = "Wrap stabby lambda arguments with parentheses.";
const MSG_NO_REQUIRE: &str = "Do not wrap stabby lambda arguments with parentheses.";

/// Stateless unit struct.
#[derive(Default)]
pub struct StabbyLambdaParentheses;

/// Enforcement style for stabby lambda parentheses.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "require_parentheses")]
    RequireParentheses,
    #[option(value = "require_no_parentheses")]
    RequireNoParentheses,
}

/// Cop options for StabbyLambdaParentheses.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "require_parentheses",
        description = "Whether stabby lambda arguments must or must not be wrapped in parentheses."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/StabbyLambdaParentheses",
    description = "Enforce consistent parentheses around stabby lambda arguments.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl StabbyLambdaParentheses {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must be a stabby lambda (->).
    if !cx.is_lambda_literal(node) {
        return;
    }

    let NodeKind::Block { call, args, .. } = *cx.kind(node) else {
        return;
    };

    // Only flag when the lambda has arguments (matching RuboCop's arguments? guard).
    let args_children = match *cx.kind(args) {
        NodeKind::Args(list) => cx.list(list),
        _ => return,
    };
    if args_children.is_empty() {
        return;
    }

    let opts = cx.options_or_default::<Options>();

    // The args node range covers the whole lambda block (Murphy assigns the
    // block range to the args node in translate_parameters). Use the individual
    // arg children's ranges instead to find the actual arg span.
    let first_arg = args_children[0];
    let last_arg = *args_children.last().unwrap();
    let content_start = cx.range(first_arg).start;
    let content_end = cx.range(last_arg).end;

    // Detect parentheses: scan tokens from `->` end to `first_arg.start`.
    // The `(` token (if present) must be between `->` and the first arg.
    let lambda_end = cx.range(call).end;
    // The `(` must be in [lambda_end, content_start].
    let has_parens = has_paren_after(lambda_end, content_start + 1, cx);

    match opts.enforced_style {
        EnforcedStyle::RequireParentheses => {
            if !has_parens {
                // Offense range = first_arg.start .. last_arg.end
                let offense_range = Range {
                    start: content_start,
                    end: content_end,
                };
                cx.emit_offense(offense_range, MSG_REQUIRE, None);
                // Autocorrect: insert ( before first arg, ) after last arg.
                cx.emit_edit(Range { start: content_start, end: content_start }, "(");
                cx.emit_edit(Range { start: content_end, end: content_end }, ")");
            }
        }
        EnforcedStyle::RequireNoParentheses => {
            if has_parens {
                // Find the ( just before first arg and ) just after last arg.
                let open = paren_open_range(lambda_end, content_start + 1, cx);
                // Search for ) after last arg's end.
                let close = open.and_then(|o| paren_close_range_from(o, content_end, cx));

                #[cfg(test)]
                eprintln!("DEBUG: open={:?}, close={:?}", open, close);

                if let (Some(open), Some(close)) = (open, close) {
                    let offense_range = Range {
                        start: open.start,
                        end: close.end,
                    };
                    cx.emit_offense(offense_range, MSG_NO_REQUIRE, None);
                    // Autocorrect: remove ( and ).
                    cx.emit_edit(open, "");
                    cx.emit_edit(close, "");
                }
            }
        }
    }
}

/// Returns `true` if the first token at or after `from` (before `until_end`)
/// is a `(`. Handles both `LeftParen` and `Other` with text `(` since the
/// lambda argument list opener `->(` may be tokenized as `Other`.
fn has_paren_after(from: u32, until_end: u32, cx: &Cx<'_>) -> bool {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    toks[idx..]
        .iter()
        .find(|t| t.range.start < until_end)
        .map(|t| is_open_paren(t.range, t.kind, source))
        .unwrap_or(false)
}

/// Returns `true` if a token is an opening `(` — either `LeftParen` or
/// `Other` with source text `(` (the lambda arg list opener uses the latter).
fn is_open_paren(range: Range, kind: SourceTokenKind, source: &[u8]) -> bool {
    kind == SourceTokenKind::LeftParen
        || (kind == SourceTokenKind::Other
            && range.end - range.start == 1
            && source[range.start as usize] == b'(')
}

/// Returns `true` if a token is a closing `)` — either `RightParen` or
/// `Other` with source text `)` (the lambda arg list closer may use the latter).
fn is_close_paren(range: Range, kind: SourceTokenKind, source: &[u8]) -> bool {
    kind == SourceTokenKind::RightParen
        || (kind == SourceTokenKind::Other
            && range.end - range.start == 1
            && source[range.start as usize] == b')')
}

/// Returns the range of the `(` token at or after `from` (before `until_end`).
fn paren_open_range(from: u32, until_end: u32, cx: &Cx<'_>) -> Option<Range> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < until_end)
        .find(|t| is_open_paren(t.range, t.kind, source))
        .map(|t| t.range)
}

/// Returns the matching `)` range starting from `open` (the `(` range), using
/// nesting depth tracking. Handles both `RightParen` and `Other` with text `)`.
fn paren_close_range_from(open: Range, _args_content_end: u32, cx: &Cx<'_>) -> Option<Range> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    // Start scanning AT the ( token itself (use strict-less-than so we land on `(`).
    let start_idx = toks.partition_point(|t| t.range.start < open.start);
    let mut d: i32 = 0;
    for tok in &toks[start_idx..] {
        if is_open_paren(tok.range, tok.kind, source) {
            d += 1;
        } else if is_close_paren(tok.range, tok.kind, source) {
            d -= 1;
            if d == 0 {
                return Some(tok.range);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- require_parentheses (default) ---

    #[test]
    fn flags_missing_parens() {
        test::<StabbyLambdaParentheses>().expect_offense(indoc! {"
            ->a,b,c { a + b + c }
              ^^^^^ Wrap stabby lambda arguments with parentheses.
        "});
    }

    #[test]
    fn corrects_missing_parens() {
        test::<StabbyLambdaParentheses>().expect_correction(
            indoc! {"
                ->a,b,c { a + b + c }
                  ^^^^^ Wrap stabby lambda arguments with parentheses.
            "},
            "->(a,b,c) { a + b + c }\n",
        );
    }

    #[test]
    fn accepts_parens_present() {
        test::<StabbyLambdaParentheses>().expect_no_offenses("->(a,b,c) { a + b + c }\n");
    }

    #[test]
    fn accepts_no_args_no_parens() {
        test::<StabbyLambdaParentheses>().expect_no_offenses("-> { a + b + c }\n");
    }

    #[test]
    fn accepts_empty_parens() {
        test::<StabbyLambdaParentheses>().expect_no_offenses("->() { a + b + c }\n");
    }

    #[test]
    fn accepts_single_arg_with_parens() {
        test::<StabbyLambdaParentheses>().expect_no_offenses("->(x) { x }\n");
    }

    #[test]
    fn flags_single_arg_without_parens() {
        test::<StabbyLambdaParentheses>().expect_offense(indoc! {"
            ->x { x }
              ^ Wrap stabby lambda arguments with parentheses.
        "});
    }

    // --- require_no_parentheses style ---

    fn no_parens_opts() -> Options {
        Options {
            enforced_style: EnforcedStyle::RequireNoParentheses,
        }
    }


    #[test]
    fn flags_unwanted_parens() {
        test::<StabbyLambdaParentheses>()
            .with_options(&no_parens_opts())
            .expect_offense(indoc! {"
                ->(a,b,c) { a + b + c }
                  ^^^^^^^ Do not wrap stabby lambda arguments with parentheses.
            "});
    }

    #[test]
    fn corrects_unwanted_parens() {
        test::<StabbyLambdaParentheses>()
            .with_options(&no_parens_opts())
            .expect_correction(
                indoc! {"
                    ->(a,b,c) { a + b + c }
                      ^^^^^^^ Do not wrap stabby lambda arguments with parentheses.
                "},
                "->a,b,c { a + b + c }\n",
            );
    }

    #[test]
    fn accepts_no_parens_in_no_parens_mode() {
        test::<StabbyLambdaParentheses>()
            .with_options(&no_parens_opts())
            .expect_no_offenses("->a,b,c { a + b + c }\n");
    }

    #[test]
    fn accepts_no_args_in_no_parens_mode() {
        test::<StabbyLambdaParentheses>()
            .with_options(&no_parens_opts())
            .expect_no_offenses("-> { a + b + c }\n");
    }

    #[test]
    fn accepts_empty_parens_in_no_parens_mode() {
        // ->() with no args: not flagged (args is empty regardless of parens)
        test::<StabbyLambdaParentheses>()
            .with_options(&no_parens_opts())
            .expect_no_offenses("->() { a + b + c }\n");
    }
}
murphy_plugin_api::submit_cop!(StabbyLambdaParentheses);
