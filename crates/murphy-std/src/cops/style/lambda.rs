//! `Style/Lambda` — enforces consistent lambda syntax.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Lambda
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Three EnforcedStyle values are implemented:
//!     line_count_dependent (default): single-line lambdas use `->`, multiline use `lambda`.
//!     lambda: all lambdas use the `lambda` method form.
//!     literal: all lambdas use the `->` stabby literal form.
//!   Detection is complete for all three styles.
//!   Autocorrect is implemented for brace-block lambdas only:
//!     - `lambda { |x| x }` -> `->(x) { x }` (method to literal, brace block)
//!     - `->(x) { x }` -> `lambda { |x| x }` (literal to method, brace block)
//!   Autocorrect for `do/end` blocks is skipped (parity gap) as it requires
//!   reconstructing the `do`/`end` keywords and form. The offense is still reported.
//!   Handles `block`, `numblock`, and `itblock` forms (mirrors RuboCop's aliases).
//!   Note on token kinds: the `{` in `-> { }` (stabby lambda body) is tokenized as
//!   `SourceTokenKind::Other` (not LeftBrace), matching the token-api.md note.
//! ```
//!
//! ## Examples
//!
//! ```ruby
//! # line_count_dependent (default)
//! # bad
//! f = lambda { |x| x }
//! f = ->(x) do
//!   x
//! end
//!
//! # good
//! f = ->(x) { x }
//! f = lambda do |x|
//!   x
//! end
//! ```

use murphy_plugin_api::{Cx, CopOptionEnum, CopOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const LITERAL_MSG_SINGLE: &str =
    "Use the `-> { ... }` lambda literal syntax for single line lambdas.";
const LITERAL_MSG_MULTI: &str =
    "Use the `-> { ... }` lambda literal syntax for multiline lambdas.";
const LITERAL_MSG_ALL: &str =
    "Use the `-> { ... }` lambda literal syntax for all lambdas.";
const METHOD_MSG_MULTI: &str = "Use the `lambda` method for multiline lambdas.";
const METHOD_MSG_ALL: &str = "Use the `lambda` method for all lambdas.";

/// Stateless unit struct.
#[derive(Default)]
pub struct Lambda;

/// Enforcement style for lambda syntax.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    /// Single-line lambdas use `->`, multiline use `lambda` (default).
    #[default]
    #[option(value = "line_count_dependent")]
    LineCountDependent,
    /// All lambdas use the `lambda` method form.
    #[option(value = "lambda")]
    LambdaMethod,
    /// All lambdas use the `->` stabby literal form.
    #[option(value = "literal")]
    Literal,
}

/// Cop options for Lambda.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "EnforcedStyle",
        default = "line_count_dependent",
        description = "Enforce lambda syntax style."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/Lambda",
    description = "Use consistent lambda syntax (`->` or `lambda`).",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl Lambda {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must be a lambda (either stabby or method form).
    if !cx.is_lambda(node) {
        return;
    }

    let is_literal = cx.is_lambda_literal(node);
    let is_multiline = cx.is_multiline(node);

    let opts = cx.options_or_default::<Options>();

    // Determine if this is an offense and what message to use.
    let (is_offense, msg) = match opts.enforced_style {
        EnforcedStyle::LineCountDependent => {
            if is_literal && is_multiline {
                // Stabby lambda used for multiline — should use `lambda do...end`
                (true, METHOD_MSG_MULTI)
            } else if !is_literal && !is_multiline {
                // Lambda method used for single line — should use `->`
                (true, LITERAL_MSG_SINGLE)
            } else {
                (false, "")
            }
        }
        EnforcedStyle::LambdaMethod => {
            if is_literal {
                // Any stabby lambda is bad
                let msg = if is_multiline { METHOD_MSG_MULTI } else { METHOD_MSG_ALL };
                (true, msg)
            } else {
                (false, "")
            }
        }
        EnforcedStyle::Literal => {
            if !is_literal {
                // Any lambda method form is bad
                let msg = if is_multiline { LITERAL_MSG_MULTI } else { LITERAL_MSG_ALL };
                (true, msg)
            } else {
                (false, "")
            }
        }
    };

    if !is_offense {
        return;
    }

    // Get the call node (Lambda marker or Send(:lambda)).
    let call = match *cx.kind(node) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => send,
        _ => return,
    };

    // Offense range:
    // - For stabby lambda: cx.range(call) = just `->` (the Lambda marker's operator_loc)
    // - For lambda method: cx.selector(call) = just `lambda` (loc.name)
    let offense_range = if is_literal {
        cx.range(call)
    } else {
        cx.selector(call)
    };

    cx.emit_offense(offense_range, msg, None);

    // Autocorrect: only for brace blocks.
    // Skip do/end blocks (they require `do`/`end` reconstruction which is more complex).
    if is_brace_block(node, cx) {
        if is_literal {
            // Convert `->` to `lambda` form.
            autocorrect_literal_to_method(node, call, cx);
        } else {
            // Convert `lambda` to `->` form.
            autocorrect_method_to_literal(node, call, cx);
        }
    }
}

/// Returns `true` if the block uses `{`/`}` delimiters.
///
/// Note: The `{` in `-> { }` (stabby lambda body) is tokenized as `Other`, not `LeftBrace`.
/// Regular brace blocks use `LeftBrace`. We check for both.
fn is_brace_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let toks = cx.tokens_in(node_range);
    toks.iter().any(|t| {
        t.kind == SourceTokenKind::LeftBrace
            || (t.kind == SourceTokenKind::Other
                && t.range.end - t.range.start == 1
                && source[t.range.start as usize] == b'{')
    })
}

/// Find the `{` token (either LeftBrace or Other `{`) after `from` and before `until_end`.
fn find_brace_after(from: u32, until_end: u32, cx: &Cx<'_>) -> Option<Range> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < until_end)
        .find(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other
                    && t.range.end - t.range.start == 1
                    && source[t.range.start as usize] == b'{')
        })
        .map(|t| t.range)
}

/// Autocorrect: convert `lambda { |x, y| body }` => `->(x, y) { body }`.
///
/// Steps:
/// 1. Replace `lambda` (selector range) with `->`.
/// 2. If args present: insert `(x, y)` immediately after `->`.
/// 3. Remove the `|x, y|` (including surrounding space) from inside `{`.
fn autocorrect_method_to_literal(node: NodeId, call: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { args, .. } = *cx.kind(node) else {
        return;
    };

    // The selector range is just `lambda`.
    let selector = cx.selector(call);
    if selector == Range::ZERO {
        return;
    }

    // Step 1: replace `lambda` with `->`
    cx.emit_edit(selector, "->");

    // Find the block opener `{` after `lambda`.
    let brace = match find_brace_after(selector.end, cx.range(node).end, cx) {
        Some(r) => r,
        None => return,
    };

    // Get the list of arg children.
    let arg_ids = match *cx.kind(args) {
        NodeKind::Args(list) => cx.list(list),
        _ => return,
    };

    if arg_ids.is_empty() {
        // No args: nothing more to do.
        return;
    }

    // Step 2: build `(x, y)` from the arg sources and insert after `->`.
    let arg_src: Vec<&str> = arg_ids
        .iter()
        .map(|&id| cx.raw_source(cx.range(id)))
        .collect();
    let args_str = format!("({})", arg_src.join(", "));
    cx.emit_edit(Range { start: selector.end, end: selector.end }, &args_str);

    // Step 3: remove `| x, y |` (the `|`...`|` block params with surrounding space)
    // The params are inside `{`. Find the pipe range.
    if let Some(pipe_range) = find_block_params_range(brace.end, cx.range(node).end, cx) {
        cx.emit_edit(pipe_range, "");
    }
}

/// Autocorrect: convert `->(x, y) { body }` => `lambda { |x, y| body }`.
///
/// Steps:
/// 1. Replace `->` with `lambda`.
/// 2. Remove `(x, y)` after `->` (if present).
/// 3. Insert ` |x, y|` after `{`.
fn autocorrect_literal_to_method(node: NodeId, call: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { args, .. } = *cx.kind(node) else {
        return;
    };

    // `cx.range(call)` for Lambda marker = just `->`.
    let arrow_range = cx.range(call);

    // Step 1: replace `->` with `lambda`.
    cx.emit_edit(arrow_range, "lambda");

    // Get the block opener `{` (either LeftBrace or Other `{` for `-> {`).
    let brace = match find_brace_after(arrow_range.end, cx.range(node).end, cx) {
        Some(r) => r,
        None => return,
    };

    // Get the list of arg children.
    let arg_ids = match *cx.kind(args) {
        NodeKind::Args(list) => cx.list(list),
        _ => return,
    };

    if arg_ids.is_empty() {
        // No args: just remove any `()` after `->` if present.
        if let Some(pr) = find_parens_range(arrow_range.end, brace.start, cx) {
            cx.emit_edit(pr, "");
        }
        return;
    }

    // Step 2: remove `(x, y)` after `->`.
    if let Some(pr) = find_parens_range(arrow_range.end, brace.start, cx) {
        cx.emit_edit(pr, "");
    }

    // Step 3: insert ` |x, y|` just after `{`.
    let arg_src: Vec<&str> = arg_ids
        .iter()
        .map(|&id| cx.raw_source(cx.range(id)))
        .collect();
    let pipe_str = format!(" |{}|", arg_src.join(", "));
    cx.emit_edit(Range { start: brace.end, end: brace.end }, &pipe_str);
}

/// Find the range of ` |x, y|` block params inside a brace block.
/// Returns range from `brace_end` to closing-pipe end (removes everything including
/// the opening `|`, params, and closing `|`).
fn find_block_params_range(brace_end: u32, node_end: u32, cx: &Cx<'_>) -> Option<Range> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < brace_end);

    // Find the first `|` token.
    let first_pipe = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && source[t.range.start as usize..t.range.end as usize] == [b'|']
        })?;

    // Find the second `|` token (closing pipe).
    let second_pipe_idx = toks.partition_point(|t| t.range.start <= first_pipe.range.start);
    let second_pipe = toks[second_pipe_idx..]
        .iter()
        .take_while(|t| t.range.start < node_end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && source[t.range.start as usize..t.range.end as usize] == [b'|']
        })?;

    // Range to delete: from `brace_end` to closing pipe end.
    // This deletes e.g. ` |x, y|` (space + pipe + args + pipe).
    Some(Range {
        start: brace_end,
        end: second_pipe.range.end,
    })
}

/// Find the `(...)` range for lambda params between `from` and `until_end`.
fn find_parens_range(from: u32, until_end: u32, cx: &Cx<'_>) -> Option<Range> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);

    // Find open paren `(`.
    let open = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < until_end)
        .find(|t| {
            t.kind == SourceTokenKind::LeftParen
                || (t.kind == SourceTokenKind::Other
                    && t.range.end - t.range.start == 1
                    && source[t.range.start as usize] == b'(')
        })?;

    // Find matching close paren `)`.
    let close_idx = toks.partition_point(|t| t.range.start <= open.range.start);
    // We need to find a close paren AFTER the open paren.
    // Use a larger search bound to handle default args.
    let close = toks[close_idx..]
        .iter()
        .take_while(|t| t.range.start < until_end + 20)
        .find(|t| {
            t.kind == SourceTokenKind::RightParen
                || (t.kind == SourceTokenKind::Other
                    && t.range.end - t.range.start == 1
                    && source[t.range.start as usize] == b')')
        })?;

    Some(Range {
        start: open.range.start,
        end: close.range.end,
    })
}

#[cfg(test)]
mod tests {
    use super::{EnforcedStyle, Lambda, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- line_count_dependent (default) ---

    #[test]
    fn flags_single_line_lambda_method() {
        test::<Lambda>().expect_offense(indoc! {"
            f = lambda { |x| x }
                ^^^^^^ Use the `-> { ... }` lambda literal syntax for single line lambdas.
        "});
    }

    #[test]
    fn corrects_single_line_lambda_method_no_args() {
        test::<Lambda>().expect_correction(
            indoc! {"
                f = lambda { 1 }
                    ^^^^^^ Use the `-> { ... }` lambda literal syntax for single line lambdas.
            "},
            "f = -> { 1 }\n",
        );
    }

    #[test]
    fn corrects_single_line_lambda_method_with_args() {
        test::<Lambda>().expect_correction(
            indoc! {"
                f = lambda { |x| x }
                    ^^^^^^ Use the `-> { ... }` lambda literal syntax for single line lambdas.
            "},
            "f = ->(x) { x }\n",
        );
    }

    #[test]
    fn corrects_single_line_lambda_method_multiple_args() {
        test::<Lambda>().expect_correction(
            indoc! {"
                f = lambda { |x, y| x + y }
                    ^^^^^^ Use the `-> { ... }` lambda literal syntax for single line lambdas.
            "},
            "f = ->(x, y) { x + y }\n",
        );
    }

    #[test]
    fn flags_multiline_stabby_lambda() {
        test::<Lambda>().expect_offense(indoc! {"
            f = ->(x) do
                ^^ Use the `lambda` method for multiline lambdas.
              x
            end
        "});
    }

    #[test]
    fn accepts_single_line_stabby_lambda() {
        test::<Lambda>().expect_no_offenses("f = ->(x) { x }\n");
    }

    #[test]
    fn accepts_multiline_lambda_method() {
        test::<Lambda>().expect_no_offenses(indoc! {"
            f = lambda do |x|
              x
            end
        "});
    }

    // --- literal style ---

    fn literal_opts() -> Options {
        Options {
            enforced_style: EnforcedStyle::Literal,
        }
    }

    #[test]
    fn flags_lambda_method_single_line_literal_style() {
        test::<Lambda>()
            .with_options(&literal_opts())
            .expect_offense(indoc! {"
                f = lambda { |x| x }
                    ^^^^^^ Use the `-> { ... }` lambda literal syntax for all lambdas.
            "});
    }

    #[test]
    fn corrects_lambda_method_single_line_literal_style() {
        test::<Lambda>()
            .with_options(&literal_opts())
            .expect_correction(
                indoc! {"
                    f = lambda { |x| x }
                        ^^^^^^ Use the `-> { ... }` lambda literal syntax for all lambdas.
                "},
                "f = ->(x) { x }\n",
            );
    }

    #[test]
    fn accepts_stabby_lambda_literal_style() {
        test::<Lambda>()
            .with_options(&literal_opts())
            .expect_no_offenses("f = ->(x) { x }\n");
    }

    // --- lambda method style ---

    fn lambda_method_opts() -> Options {
        Options {
            enforced_style: EnforcedStyle::LambdaMethod,
        }
    }

    #[test]
    fn flags_stabby_lambda_single_line_lambda_style() {
        test::<Lambda>()
            .with_options(&lambda_method_opts())
            .expect_offense(indoc! {"
                f = ->(x) { x }
                    ^^ Use the `lambda` method for all lambdas.
            "});
    }

    #[test]
    fn corrects_stabby_lambda_to_lambda_method() {
        test::<Lambda>()
            .with_options(&lambda_method_opts())
            .expect_correction(
                indoc! {"
                    f = ->(x) { x }
                        ^^ Use the `lambda` method for all lambdas.
                "},
                "f = lambda { |x| x }\n",
            );
    }

    #[test]
    fn corrects_stabby_lambda_no_args_to_lambda_method() {
        test::<Lambda>()
            .with_options(&lambda_method_opts())
            .expect_correction(
                indoc! {"
                    f = -> { x }
                        ^^ Use the `lambda` method for all lambdas.
                "},
                "f = lambda { x }\n",
            );
    }

    #[test]
    fn corrects_stabby_lambda_empty_parens_to_lambda_method() {
        test::<Lambda>()
            .with_options(&lambda_method_opts())
            .expect_correction(
                indoc! {"
                    f = ->() { x }
                        ^^ Use the `lambda` method for all lambdas.
                "},
                "f = lambda { x }\n",
            );
    }

    #[test]
    fn accepts_lambda_method_lambda_style() {
        test::<Lambda>()
            .with_options(&lambda_method_opts())
            .expect_no_offenses("f = lambda { |x| x }\n");
    }

    // --- not a lambda ---

    #[test]
    fn accepts_regular_block() {
        test::<Lambda>().expect_no_offenses("[1].each { |x| x }\n");
    }

    // --- idempotency ---

    #[test]
    fn corrected_stabby_lambda_is_idempotent() {
        test::<Lambda>().expect_no_offenses("f = ->(x) { x }\n");
    }

    #[test]
    fn corrected_lambda_method_is_idempotent() {
        test::<Lambda>()
            .with_options(&lambda_method_opts())
            .expect_no_offenses("f = lambda { |x| x }\n");
    }
}

murphy_plugin_api::submit_cop!(Lambda);
