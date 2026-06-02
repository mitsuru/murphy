//! `Style/LineEndConcatenation` — flag `+` or `<<` used to concatenate two
//! string literals across a line end; suggest `\` instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/LineEndConcatenation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `+` and `<<` when used to concatenate two string literals across a
//!   line break (the operator sits at the end of the line, the successor is on
//!   the next line).  Both the predecessor (rightmost-leaf of receiver) and the
//!   successor must be quote-delimited (`'` or `"`) Str or Dstr literals.
//!   `%`-literals and heredocs are not treated as string literals (matches
//!   RuboCop's `QUOTE_DELIMITERS`).  High-precedence follow-on operators
//!   (`*`, `%`, `.`, `[`) are excluded (matches RuboCop's
//!   `HIGH_PRECEDENCE_OP_TOKEN_TYPES`).
//!   Autocorrect replaces the operator and any trailing whitespace (up to but
//!   not including the newline) with `\`, guarding against double-backslash.
//!   Autocorrect is **unsafe**: `array << 'foo' <<\n 'bar'` is a documented
//!   false positive — `<<` on a non-String receiver would become a syntax
//!   error, but the cop cannot guarantee the receiver type.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! some_str = 'ala' +
//!            'bala'
//!
//! some_str = 'ala' <<
//!            'bala'
//!
//! # good
//! some_str = 'ala' \
//!            'bala'
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, Range, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct LineEndConcatenation;

#[cop(
    name = "Style/LineEndConcatenation",
    description = "Use `\\` instead of `+` or `<<` to concatenate multiline strings.",
    default_severity = "warning",
    default_enabled = true,
    safe_autocorrect = false,
    options = NoOptions,
)]
impl LineEndConcatenation {
    #[on_node(kind = "send", methods = ["+", "<<"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send {
        receiver,
        method: _,
        args,
    } = *cx.kind(node)
    else {
        return;
    };

    let Some(recv) = receiver.get() else {
        return;
    };

    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return;
    }
    let rhs = arg_list[0];

    // The successor (rhs) must be a quote-delimited string literal.
    if !is_quote_delimited_string(rhs, cx) {
        return;
    }

    // The rightmost leaf of the receiver must be a quote-delimited string literal.
    if !is_quote_delimited_string(rightmost_leaf(recv, cx), cx) {
        return;
    }

    // The node must be multiline.
    if !cx.is_multiline(node) {
        return;
    }

    // Find the operator token between receiver end and rhs start.
    // The operator must be at line-end: only spaces/tabs between op and newline.
    let recv_end = cx.range(recv).end;
    let rhs_start = cx.range(rhs).start;
    let src = cx.source().as_bytes();

    let search_range = Range {
        start: recv_end,
        end: rhs_start,
    };

    let op_tok = cx
        .tokens_in(search_range)
        .iter()
        .find(|t| {
            if t.kind != SourceTokenKind::Other {
                return false;
            }
            let s = &src[t.range.start as usize..t.range.end as usize];
            s == b"+" || s == b"<<"
        })
        .copied();

    let Some(op_tok) = op_tok else {
        return;
    };

    // Check operator is at line end: only spaces/tabs follow until the newline.
    let after_op = op_tok.range.end as usize;
    let between = &src[after_op..rhs_start as usize];
    // Get the bytes before the first newline.
    let before_newline: &[u8] = between
        .split(|&b| b == b'\n')
        .next()
        .unwrap_or(&[]);
    if !before_newline.iter().all(|&b| b == b' ' || b == b'\t') {
        return;
    }
    // Must have a newline at all.
    if !between.contains(&b'\n') {
        return;
    }

    // Check: token after rhs is not a high-precedence operator.
    // HIGH_PRECEDENCE_OP_TOKEN_TYPES: tSTAR2 (*), tPERCENT (%), tDOT (.), tLBRACK2 ([)
    if let Some(next_tok) = cx.token_after(cx.range(rhs).end)
        && next_tok.kind == SourceTokenKind::Other
        && matches!(
            &src[next_tok.range.start as usize..next_tok.range.end as usize],
            b"*" | b"%" | b"." | b"[" | b"&."
        )
    {
        return;
    }

    let op_src = &src[op_tok.range.start as usize..op_tok.range.end as usize];
    let op_str = std::str::from_utf8(op_src).unwrap_or("+");
    let offense_msg = format!(
        "Use `\\` instead of `{}` to concatenate multiline strings.",
        op_str
    );
    cx.emit_offense(op_tok.range, &offense_msg, None);

    // Autocorrect: replace the operator + any trailing whitespace (but not
    // the newline) with `\`.
    let after_op_range_end = op_tok.range.end as usize + before_newline.len();

    // Guard against double-backslash: if the char right after our replacement
    // range is `\`, include it so the result doesn't produce `\\`.
    let has_trailing_backslash = src
        .get(after_op_range_end)
        .copied()
        .map(|b| b == b'\\')
        .unwrap_or(false);

    let replace_end = if has_trailing_backslash {
        after_op_range_end + 1
    } else {
        after_op_range_end
    } as u32;

    cx.emit_edit(
        Range {
            start: op_tok.range.start,
            end: replace_end,
        },
        "\\",
    );
}

/// Returns the rightmost leaf of a node when descending through `+` and `<<`
/// send chains.  For any other node kind, `node` itself is the leaf.
fn rightmost_leaf(node: NodeId, cx: &Cx<'_>) -> NodeId {
    // Only descend into `+` and `<<` send nodes.
    if !matches!(cx.method_name(node), Some("+" | "<<")) {
        return node;
    }

    let NodeKind::Send {
        receiver: _,
        method: _,
        args,
    } = *cx.kind(node)
    else {
        return node;
    };

    let arg_list = cx.list(args);
    if arg_list.len() == 1 {
        // Recurse into the rightmost argument.
        rightmost_leaf(arg_list[0], cx)
    } else {
        node
    }
}

/// Returns true iff the node is a quote-delimited string literal (`'` or `"`).
/// Excludes `%`-literals and heredocs.
fn is_quote_delimited_string(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Str(_) | NodeKind::Dstr(_) => {
            let src = cx.raw_source(cx.range(node));
            let trimmed = src.trim_start();
            trimmed.starts_with('\'') || trimmed.starts_with('"')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::LineEndConcatenation;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offense detection ---

    #[test]
    fn flags_plus_at_line_end() {
        test::<LineEndConcatenation>().expect_offense(indoc! {r#"
            some_str = 'ala' +
                             ^ Use `\` instead of `+` to concatenate multiline strings.
                       'bala'
        "#});
    }

    #[test]
    fn flags_lshift_at_line_end() {
        test::<LineEndConcatenation>().expect_offense(indoc! {r#"
            some_str = 'ala' <<
                             ^^ Use `\` instead of `<<` to concatenate multiline strings.
                       'bala'
        "#});
    }

    #[test]
    fn flags_double_quoted_strings() {
        test::<LineEndConcatenation>().expect_offense(indoc! {r#"
            some_str = "ala" +
                             ^ Use `\` instead of `+` to concatenate multiline strings.
                       "bala"
        "#});
    }

    #[test]
    fn accepts_backslash_continuation() {
        // Already using backslash — no offense.
        test::<LineEndConcatenation>()
            .expect_no_offenses("some_str = 'ala' \\\n           'bala'\n");
    }

    #[test]
    fn accepts_same_line_concatenation() {
        // Single-line + is handled by Style/StringConcatenation, not this cop.
        test::<LineEndConcatenation>()
            .expect_no_offenses("some_str = 'ala' + 'bala'\n");
    }

    #[test]
    fn accepts_non_string_successor() {
        // rhs is a variable, not a string literal.
        test::<LineEndConcatenation>()
            .expect_no_offenses("str = 'foo' +\n  bar\n");
    }

    #[test]
    fn accepts_non_string_predecessor() {
        // Receiver is a method call, not a string literal.
        test::<LineEndConcatenation>()
            .expect_no_offenses("str = foo +\n  'bar'\n");
    }

    // --- autocorrect ---

    #[test]
    fn autocorrects_plus_to_backslash() {
        test::<LineEndConcatenation>().expect_correction(
            indoc! {r#"
                some_str = 'ala' +
                                 ^ Use `\` instead of `+` to concatenate multiline strings.
                           'bala'
            "#},
            "some_str = 'ala' \\\n           'bala'\n",
        );
    }

    #[test]
    fn autocorrects_lshift_to_backslash() {
        test::<LineEndConcatenation>().expect_correction(
            indoc! {r#"
                some_str = 'ala' <<
                                 ^^ Use `\` instead of `<<` to concatenate multiline strings.
                           'bala'
            "#},
            "some_str = 'ala' \\\n           'bala'\n",
        );
    }
}
murphy_plugin_api::submit_cop!(LineEndConcatenation);
