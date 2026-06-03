//! `Style/TrailingCommaInArguments` — checks for trailing comma in argument lists.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TrailingCommaInArguments
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Supports all four EnforcedStyleForMultiline values: no_comma (default),
//!   comma, consistent_comma, diff_comma.  Single-line calls never permit a
//!   trailing comma regardless of style.  The `[]` bracket-access form is
//!   checked via the `method?(:[])` path (RuboCop parity).
//!   Heredoc-in-args interaction and the autocorrect incompatibility with
//!   Layout/HeredocArgumentClosingParenthesis are documented parity gaps
//!   (Murphy does not yet track heredoc presence in arg lists).
//!   Autocorrect is provided for both "avoid comma" and "put comma" paths;
//!   like RuboCop this cop is marked Unsafe because removing a trailing comma
//!   may change behaviour for some patterns.
//! ```
//!
//! ## Matched shapes
//!
//! `Send` / `Csend` nodes that:
//! - have at least one argument, AND
//! - are parenthesized (`cx.is_parenthesized`) OR use bracket access (`[]` method).
//!
//! ## Enforcement logic
//!
//! | Style              | Single-line        | Multiline                                   |
//! |--------------------|--------------------|---------------------------------------------|
//! | `no_comma`         | never allow comma  | never allow comma                           |
//! | `comma`            | never allow comma  | require when each arg + `)` on own line     |
//! | `consistent_comma` | never allow comma  | require when any line break in call         |
//! | `diff_comma`       | never allow comma  | require when last arg followed by newline   |
//!
//! ## Autocorrect
//!
//! - "avoid comma" path: delete the trailing comma token.
//! - "put comma" path: insert `,` immediately after the last argument's end.

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceToken, SourceTokenKind, cop,
};

/// Message for "avoid trailing comma" offenses.
const MSG_AVOID: &str = "Avoid comma after the last parameter of a method call";
/// Message for "put trailing comma" offenses.
const MSG_PUT: &str = "Put a comma after the last parameter of a multiline method call";

/// Stateless unit struct.
#[derive(Default)]
pub struct TrailingCommaInArguments;

/// Options for [`TrailingCommaInArguments`].
#[derive(CopOptions)]
pub struct TrailingCommaInArgumentsOptions {
    #[option(
        name = "EnforcedStyleForMultiline",
        default = "no_comma",
        description = "Controls when trailing commas are required or forbidden in multiline method calls."
    )]
    pub enforced_style_for_multiline: TrailingCommaStyle,
}

/// The trailing-comma style variants for argument lists.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrailingCommaStyle {
    /// `no_comma`: never allow a trailing comma (default).
    #[option(value = "no_comma")]
    #[default]
    NoComma,
    /// `comma`: require trailing comma when each arg is on its own line.
    #[option(value = "comma")]
    Comma,
    /// `consistent_comma`: require trailing comma for any multiline call.
    #[option(value = "consistent_comma")]
    ConsistentComma,
    /// `diff_comma`: require trailing comma when last arg is immediately
    /// followed by a newline.
    #[option(value = "diff_comma")]
    DiffComma,
}

#[cop(
    name = "Style/TrailingCommaInArguments",
    description = "Checks for trailing comma in argument lists.",
    default_severity = "warning",
    default_enabled = true,
    options = TrailingCommaInArgumentsOptions,
)]
impl TrailingCommaInArguments {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return;
    }

    // Only parenthesized calls or `[]` method calls are checked.
    // Note: `cx.is_parenthesized` is true for both `obj[...]` (closing `]` from Prism)
    // and `obj.[](...)` (closing `)`), so we cannot use it alone to distinguish
    // bracket syntax from parenthesized bracket calls.
    let is_bracket_method = cx.method_name(node) == Some("[]");
    if !cx.is_parenthesized(node) && !is_bracket_method {
        return;
    }

    let opts = cx.options_or_default::<TrailingCommaInArgumentsOptions>();
    let style = opts.enforced_style_for_multiline;

    let last_arg = *args.last().unwrap();
    let last_arg_end = cx.range(last_arg).end;

    // Find the closing delimiter.  For `[]` method calls, the delimiter may be `]`
    // (bracket-access syntax `obj[...]`) or `)` (explicit-dot form `obj.[](...)`)
    // — we accept either and return the first one found.  For regular calls it is `)`.
    let source = cx.source().as_bytes();
    let node_range = cx.range(node);
    let toks = cx.sorted_tokens();

    let Some(close_tok) = find_closing_delimiter(
        toks,
        source,
        last_arg_end,
        node_range.end,
        is_bracket_method,
    ) else {
        return;
    };

    // Look for a comma in the "after last arg" region: [last_arg_end, close_tok.start).
    let after_last_arg = Range {
        start: last_arg_end,
        end: close_tok.range.start,
    };
    let comma_tok = find_trailing_comma(toks, after_last_arg);

    // Is the call single-line?
    let is_single_line = cx.is_single_line(node);

    if let Some(comma_tok) = comma_tok {
        // There IS a trailing comma. Check whether it should be there.
        if is_single_line || !should_have_comma(style, node, last_arg, close_tok, cx) {
            let extra = extra_avoid_info(style);
            let msg = format!("{MSG_AVOID}{extra}.");
            cx.emit_offense(comma_tok.range, &msg, None);
            cx.emit_edit(comma_tok.range, "");
        }
    } else {
        // There is NO trailing comma.
        if !is_single_line && should_have_comma(style, node, last_arg, close_tok, cx) {
            // Skip if last arg is a block pass (`&blk`).
            if matches!(cx.kind(last_arg), NodeKind::BlockPass(_)) {
                return;
            }
            let insert_at = last_arg_end;
            let msg = format!("{MSG_PUT}.");
            cx.emit_offense(cx.range(last_arg), &msg, None);
            cx.emit_edit(
                Range {
                    start: insert_at,
                    end: insert_at,
                },
                ",",
            );
        }
    }
}

/// Find the closing delimiter token (`)` or `]`) starting from `after` up to `end`.
///
/// When `is_bracket_method` is true (`[]` method call), we accept either `]` or `)` as
/// the closing delimiter: `obj[...]` uses `]` while `obj.[](...)` uses `)`.  The first
/// matching token in source order is returned.
fn find_closing_delimiter(
    toks: &[SourceToken],
    source: &[u8],
    after: u32,
    end: u32,
    is_bracket_method: bool,
) -> Option<SourceToken> {
    let lo = toks.partition_point(|t| t.range.start < after);
    toks[lo..]
        .iter()
        .take_while(|t| t.range.start < end)
        .find(|t| {
            if t.kind == SourceTokenKind::RightParen {
                return true;
            }
            if is_bracket_method
                && t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"]"
            {
                return true;
            }
            false
        })
        .copied()
}

/// Find the first comma token in `range`.
fn find_trailing_comma(toks: &[SourceToken], range: Range) -> Option<SourceToken> {
    let lo = toks.partition_point(|t| t.range.start < range.start);
    toks[lo..]
        .iter()
        .take_while(|t| t.range.start < range.end)
        .find(|t| t.kind == SourceTokenKind::Comma)
        .copied()
}

/// Determine whether the configured style requires a trailing comma for this
/// multiline call.
fn should_have_comma(
    style: TrailingCommaStyle,
    node: NodeId,
    last_arg: NodeId,
    close_tok: SourceToken,
    cx: &Cx<'_>,
) -> bool {
    match style {
        TrailingCommaStyle::NoComma => false,
        TrailingCommaStyle::Comma => {
            is_multiline_with_all_on_own_lines(node, last_arg, close_tok, cx)
        }
        TrailingCommaStyle::ConsistentComma => is_multiline_consistent(node, last_arg, cx),
        TrailingCommaStyle::DiffComma => last_arg_precedes_newline(last_arg, close_tok, cx),
    }
}

/// `comma` style: multiline AND no two consecutive elements on the same line,
/// AND the closing delimiter is on its own line.
fn is_multiline_with_all_on_own_lines(
    node: NodeId,
    last_arg: NodeId,
    close_tok: SourceToken,
    cx: &Cx<'_>,
) -> bool {
    if !cx.is_multiline(node) {
        return false;
    }
    let source = cx.source().as_bytes();
    let args = cx.call_arguments(node);

    // Special case: single argument — closing delimiter must be on a different
    // line from the argument end (mirrors RuboCop's `allowed_multiline_argument?`).
    if args.len() == 1 {
        let gap_start = cx.range(last_arg).end as usize;
        let gap_end = close_tok.range.start as usize;
        return source[gap_start..gap_end].contains(&b'\n');
    }

    // Check all consecutive pairs of args are on different lines.
    for pair in args.windows(2) {
        let gap_start = cx.range(pair[0]).end as usize;
        let gap_end = cx.range(pair[1]).start as usize;
        if !source[gap_start..gap_end].contains(&b'\n') {
            return false;
        }
    }

    // Check last arg and closing delimiter are on different lines.
    let gap_start = cx.range(last_arg).end as usize;
    let gap_end = close_tok.range.start as usize;
    source[gap_start..gap_end].contains(&b'\n')
}

/// `consistent_comma` style: any multiline call, UNLESS the method selector
/// and last argument end are on the same line.
fn is_multiline_consistent(node: NodeId, last_arg: NodeId, cx: &Cx<'_>) -> bool {
    if !cx.is_multiline(node) {
        return false;
    }
    let source = cx.source().as_bytes();
    let selector_range = cx.selector(node);
    // If the selector and the last arg end are on the same line, do not
    // require a trailing comma (mirrors RuboCop's
    // `method_name_and_arguments_on_same_line?`).
    // We check for a newline in the span from selector start to last arg end.
    let gap_start = selector_range.start as usize;
    let gap_end = cx.range(last_arg).end as usize;
    source[gap_start..gap_end].contains(&b'\n')
}

/// `diff_comma` style: last arg is immediately followed by a newline (after
/// optional whitespace and an optional inline comment).
fn last_arg_precedes_newline(last_arg: NodeId, close_tok: SourceToken, cx: &Cx<'_>) -> bool {
    let source = cx.source();
    let last_arg_end = cx.range(last_arg).end as usize;
    let close_start = close_tok.range.start as usize;

    let between = &source[last_arg_end..close_start];
    // Strip optional comma.
    let stripped = between.trim_start_matches(',');
    is_followed_by_newline_after_optional_comment(stripped)
}

fn is_followed_by_newline_after_optional_comment(s: &str) -> bool {
    // Skip leading whitespace (but not newline).
    let rest = s.trim_start_matches([' ', '\t']);
    // If there's a `#` comment, skip to end of line.
    let rest = if rest.starts_with('#') {
        match rest.find('\n') {
            Some(i) => &rest[i..],
            None => return false,
        }
    } else {
        rest
    };
    rest.starts_with('\n')
}

fn extra_avoid_info(style: TrailingCommaStyle) -> &'static str {
    match style {
        TrailingCommaStyle::Comma => ", unless each item is on its own line",
        TrailingCommaStyle::ConsistentComma => ", unless items are split onto multiple lines",
        TrailingCommaStyle::DiffComma => ", unless that item immediately precedes a newline",
        TrailingCommaStyle::NoComma => "",
    }
}

#[cfg(test)]
mod tests {
    use super::{TrailingCommaInArguments, TrailingCommaInArgumentsOptions, TrailingCommaStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn comma_opts() -> TrailingCommaInArgumentsOptions {
        TrailingCommaInArgumentsOptions {
            enforced_style_for_multiline: TrailingCommaStyle::Comma,
        }
    }

    fn consistent_opts() -> TrailingCommaInArgumentsOptions {
        TrailingCommaInArgumentsOptions {
            enforced_style_for_multiline: TrailingCommaStyle::ConsistentComma,
        }
    }

    fn diff_opts() -> TrailingCommaInArgumentsOptions {
        TrailingCommaInArgumentsOptions {
            enforced_style_for_multiline: TrailingCommaStyle::DiffComma,
        }
    }

    // ===== no_comma (default) =====

    #[test]
    fn no_comma_flags_single_line_trailing_comma() {
        // method(1, 2,) — trailing comma at position 11
        test::<TrailingCommaInArguments>().expect_correction(
            indoc! {"
                method(1, 2,)
                           ^ Avoid comma after the last parameter of a method call.
            "},
            "method(1, 2)\n",
        );
    }

    #[test]
    fn no_comma_flags_bracket_call_trailing_comma() {
        // object[1, 2,] — trailing comma at position 11
        test::<TrailingCommaInArguments>().expect_correction(
            indoc! {"
                object[1, 2,]
                           ^ Avoid comma after the last parameter of a method call.
            "},
            "object[1, 2]\n",
        );
    }

    #[test]
    fn no_comma_flags_explicit_dot_bracket_call_trailing_comma() {
        // obj.[](1, 2,) — explicit dot + parens form of bracket access, trailing comma at position 11
        test::<TrailingCommaInArguments>().expect_correction(
            indoc! {"
                obj.[](1, 2,)
                           ^ Avoid comma after the last parameter of a method call.
            "},
            "obj.[](1, 2)\n",
        );
    }

    #[test]
    fn no_comma_accepts_no_trailing_comma() {
        test::<TrailingCommaInArguments>().expect_no_offenses("method(1, 2)\n");
    }

    #[test]
    fn no_comma_accepts_no_args() {
        test::<TrailingCommaInArguments>().expect_no_offenses("method()\n");
    }

    #[test]
    fn no_comma_accepts_unparenthesized() {
        test::<TrailingCommaInArguments>().expect_no_offenses("method 1, 2\n");
    }

    #[test]
    fn no_comma_flags_multiline_trailing_comma() {
        // The trailing comma is in `  2,` at position 3 (0-indexed in that line).
        test::<TrailingCommaInArguments>().expect_correction(
            indoc! {"
                method(
                  1,
                  2,
                   ^ Avoid comma after the last parameter of a method call.
                )
            "},
            "method(\n  1,\n  2\n)\n",
        );
    }

    #[test]
    fn no_comma_accepts_multiline_no_trailing_comma() {
        test::<TrailingCommaInArguments>().expect_no_offenses(indoc! {"
            method(
              1,
              2
            )
        "});
    }

    // ===== comma style =====

    #[test]
    fn comma_flags_single_line_trailing_comma() {
        test::<TrailingCommaInArguments>()
            .with_options(&comma_opts())
            .expect_correction(
                indoc! {"
                    method(1, 2,)
                               ^ Avoid comma after the last parameter of a method call, unless each item is on its own line.
                "},
                "method(1, 2)\n",
            );
    }

    #[test]
    fn comma_accepts_no_trailing_comma_single_line() {
        test::<TrailingCommaInArguments>()
            .with_options(&comma_opts())
            .expect_no_offenses("method(1, 2)\n");
    }

    #[test]
    fn comma_accepts_trailing_comma_when_each_on_own_line() {
        test::<TrailingCommaInArguments>()
            .with_options(&comma_opts())
            .expect_no_offenses(indoc! {"
                method(
                  1,
                  2,
                )
            "});
    }

    #[test]
    fn comma_flags_trailing_comma_when_not_each_on_own_line() {
        // method(\n  1, 2,\n  3,\n) — `3,` has comma at position 3 (in `  3,`)
        test::<TrailingCommaInArguments>()
            .with_options(&comma_opts())
            .expect_correction(
                indoc! {"
                    method(
                      1, 2,
                      3,
                       ^ Avoid comma after the last parameter of a method call, unless each item is on its own line.
                    )
                "},
                "method(\n  1, 2,\n  3\n)\n",
            );
    }

    #[test]
    fn comma_requires_trailing_comma_when_each_on_own_line() {
        // `2` is last arg; offense range = the `2` node = 1 char at position 2 in `  2`
        test::<TrailingCommaInArguments>()
            .with_options(&comma_opts())
            .expect_correction(
                indoc! {"
                    method(
                      1,
                      2
                      ^ Put a comma after the last parameter of a multiline method call.
                    )
                "},
                "method(\n  1,\n  2,\n)\n",
            );
    }

    // ===== consistent_comma style =====

    #[test]
    fn consistent_flags_single_line_trailing_comma() {
        test::<TrailingCommaInArguments>()
            .with_options(&consistent_opts())
            .expect_correction(
                indoc! {"
                    method(1, 2,)
                               ^ Avoid comma after the last parameter of a method call, unless items are split onto multiple lines.
                "},
                "method(1, 2)\n",
            );
    }

    #[test]
    fn consistent_accepts_no_trailing_comma_single_line() {
        test::<TrailingCommaInArguments>()
            .with_options(&consistent_opts())
            .expect_no_offenses("method(1, 2)\n");
    }

    #[test]
    fn consistent_requires_trailing_comma_multiline() {
        test::<TrailingCommaInArguments>()
            .with_options(&consistent_opts())
            .expect_correction(
                indoc! {"
                    method(
                      1,
                      2
                      ^ Put a comma after the last parameter of a multiline method call.
                    )
                "},
                "method(\n  1,\n  2,\n)\n",
            );
    }

    #[test]
    fn consistent_accepts_trailing_comma_multiline() {
        test::<TrailingCommaInArguments>()
            .with_options(&consistent_opts())
            .expect_no_offenses(indoc! {"
                method(
                  1,
                  2,
                )
            "});
    }

    // ===== diff_comma style =====

    #[test]
    fn diff_flags_single_line_trailing_comma() {
        test::<TrailingCommaInArguments>()
            .with_options(&diff_opts())
            .expect_correction(
                indoc! {"
                    method(1, 2,)
                               ^ Avoid comma after the last parameter of a method call, unless that item immediately precedes a newline.
                "},
                "method(1, 2)\n",
            );
    }

    #[test]
    fn diff_accepts_trailing_comma_when_last_arg_precedes_newline() {
        test::<TrailingCommaInArguments>()
            .with_options(&diff_opts())
            .expect_no_offenses(indoc! {"
                method(
                  1,
                  2,
                )
            "});
    }

    #[test]
    fn diff_flags_trailing_comma_when_close_paren_on_same_line() {
        // The trailing comma in `],)` is at position 1 in that line.
        test::<TrailingCommaInArguments>()
            .with_options(&diff_opts())
            .expect_correction(
                indoc! {"
                    method(1, [
                      2,
                    ],)
                     ^ Avoid comma after the last parameter of a method call, unless that item immediately precedes a newline.
                "},
                "method(1, [\n  2,\n])\n",
            );
    }
}
murphy_plugin_api::submit_cop!(TrailingCommaInArguments);
