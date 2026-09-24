//! `Layout/HeredocArgumentClosingParenthesis` — checks the placement of the
//! closing parenthesis in a method call that passes a HEREDOC as an argument.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/HeredocArgumentClosingParenthesis
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `on_send`/`on_csend` detection: a call passing a HEREDOC
//!   argument whose closing `)` is **not** on the same line as the HEREDOC
//!   opening (`<<-SQL`) is flagged at the closing parenthesis of the outermost
//!   call on the HEREDOC opener line. The three RuboCop skip conditions are
//!   implemented:
//!     * an `end` keyword between the HEREDOC and the `)` (a `def`/`do`/`begin`
//!       ancestor), via `end_keyword_before_closing_parenthesis?`;
//!     * subsequent closing parens on the same line as the `)`
//!       (`subsequent_closing_parentheses_in_same_line?`);
//!     * a non-whitespace argument between the HEREDOC terminator and the `)`
//!       (`exist_argument_between_heredoc_end_and_closing_parentheses?`).
//!   The HEREDOC "opening line" is taken from the `HeredocStart` token's line
//!   (RuboCop's `heredoc.last_line`, which for parser-gem is the pointer/opener
//!   line, not the terminator line).
//!   Autocorrect relocates the `)` to directly after the last argument with
//!   RuboCop's internal/external trailing-comma juggling
//!   (`remove_internal_trailing_comma`, `fix_external_trailing_comma`): an
//!   internal `,\n` after the last argument is dropped, and an external `,`
//!   after the old `)` (up to 20 spaces out) is moved inside the new parens.
//!   RuboCop's `autocorrect_incompatible_with [Style::TrailingCommaInArguments]`
//!   has no Murphy equivalent and is not declared.
//!   Known gap versus RuboCop: `extract_heredoc` covers a direct HEREDOC
//!   argument and a HEREDOC nested as a hash value, plus the
//!   single-line-send-with-HEREDOC-receiver shape to the extent token ranges
//!   allow. Deeply chained HEREDOC receivers may be under-detected.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.";

#[derive(Default)]
pub struct HeredocArgumentClosingParenthesis;

#[cop(
    name = "Layout/HeredocArgumentClosingParenthesis",
    description = "Checks the closing parenthesis placement for calls with a HEREDOC argument.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl HeredocArgumentClosingParenthesis {
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
    // `heredoc_arg = extract_heredoc_argument(node)` — the first argument that
    // is (or contains) a HEREDOC. We need the HEREDOC node *and* its opener
    // line.
    let Some(heredoc_arg) = extract_heredoc_argument(node, cx) else {
        return;
    };
    let Some(opener_line) = heredoc_opener_line(heredoc_arg, cx) else {
        return;
    };

    // `outermost_send = outermost_send_on_same_line(heredoc_arg)`.
    let Some(outermost) = outermost_send_on_same_line(heredoc_arg, opener_line, cx) else {
        return;
    };

    // `return if end_keyword_before_closing_parenthesis?(node)`.
    if end_keyword_before_closing_parenthesis(node, cx) {
        return;
    }
    // `return if subsequent_closing_parentheses_in_same_line?(outermost_send)`.
    if subsequent_closing_parentheses_in_same_line(outermost, cx) {
        return;
    }
    // `return if exist_argument_between_heredoc_end_and_closing_parentheses?(node)`.
    if exist_argument_between_heredoc_end_and_closing_parens(node, cx) {
        return;
    }

    // `add_offense(outermost_send.loc.end)`.
    let close = call_close_paren(outermost, cx);
    if close == Range::ZERO {
        return;
    }
    cx.emit_offense(close, MSG, None);
    emit_correction(outermost, close, cx);
}

/// `extract_heredoc_argument`: the first argument of `node` that is, or
/// contains, a HEREDOC. Returns the *HEREDOC-bearing argument node*.
fn extract_heredoc_argument(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    cx.call_arguments(node)
        .iter()
        .copied()
        .find(|&arg| extract_heredoc(arg, cx).is_some())
}

/// `extract_heredoc`: resolves a node to the HEREDOC it directly is, or that it
/// contains as a hash value. Returns the HEREDOC string node.
fn extract_heredoc(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    if is_heredoc_node(node, cx) {
        return Some(node);
    }
    // Hash argument: any value that is (or contains) a HEREDOC.
    if matches!(cx.kind(node), NodeKind::Hash(_)) {
        for pair in cx.hash_pairs(node) {
            // A pair's value is its second child.
            if let Some(&value) = cx.children(pair).get(1)
                && let Some(h) = extract_heredoc(value, cx)
            {
                return Some(h);
            }
        }
    }
    None
}

/// A string/xstring node whose source contains a `HeredocStart` token —
/// RuboCop's `node.heredoc?`.
fn is_heredoc_node(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Str(_) | NodeKind::Dstr(_) | NodeKind::Xstr(_)
    ) && heredoc_start_token(node, cx).is_some()
}

/// The `HeredocStart` token contained in `node`'s source range, if any.
fn heredoc_start_token(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    cx.tokens_in(cx.range(node))
        .iter()
        .find(|t| t.kind == SourceTokenKind::HeredocStart)
        .map(|t| t.range)
}

/// 1-based line of the HEREDOC opener (`<<-SQL`), via the `HeredocStart` token.
fn heredoc_opener_line(heredoc_arg: NodeId, cx: &Cx<'_>) -> Option<usize> {
    let heredoc = extract_heredoc(heredoc_arg, cx)?;
    let start = heredoc_start_token(heredoc, cx)?;
    Some(line_of(start.start, cx.source()))
}

/// `outermost_send_on_same_line(heredoc)`: walk up parents until reaching a
/// call node whose own argument list contains the previous node, that has a
/// `(`, and whose closing `)` is on a different line than the HEREDOC opener.
fn outermost_send_on_same_line(
    heredoc_arg: NodeId,
    opener_line: usize,
    cx: &Cx<'_>,
) -> Option<NodeId> {
    let mut previous = heredoc_arg;
    let mut current = cx.parent(previous).get()?;
    while !send_missing_closing_parens(current, previous, opener_line, cx) {
        previous = current;
        current = cx.parent(current).get()?;
    }
    Some(current)
}

/// `send_missing_closing_parens?(parent, child, heredoc)`.
fn send_missing_closing_parens(
    parent: NodeId,
    child: NodeId,
    opener_line: usize,
    cx: &Cx<'_>,
) -> bool {
    let is_call = matches!(
        cx.kind(parent),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    );
    if !is_call {
        return false;
    }
    if !cx.call_arguments(parent).contains(&child) {
        return false;
    }
    let close = call_close_paren(parent, cx);
    if close == Range::ZERO {
        return false; // `parent.loc.begin` / `parent.loc.end` missing.
    }
    line_of(close.start, cx.source()) != opener_line
}

/// `end_keyword_before_closing_parenthesis?`: any ancestor whose source ends
/// with an `end` keyword (a `def`/`class`/`module`/`do`-block/`begin`).
fn end_keyword_before_closing_parenthesis(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.ancestors(node)
        .any(|anc| cx.loc(anc).end_keyword() != Range::ZERO)
}

/// `subsequent_closing_parentheses_in_same_line?(outermost_send)`: the last
/// argument of the outermost call itself ends with a `)` on the same line and
/// one column before the outer `)` — i.e. `...))` with no gap.
fn subsequent_closing_parentheses_in_same_line(outermost: NodeId, cx: &Cx<'_>) -> bool {
    let Some(last_arg) = cx.last_argument(outermost).get() else {
        return false;
    };
    let inner_close = call_close_paren(last_arg, cx);
    if inner_close == Range::ZERO {
        return false;
    }
    let outer_close = call_close_paren(outermost, cx);
    if outer_close == Range::ZERO {
        return false;
    }
    let src = cx.source();
    same_line(outer_close.start, inner_close.start, src)
        && column_of(src, outer_close.start as usize)
            == column_of(src, inner_close.start as usize) + 1
}

/// `exist_argument_between_heredoc_end_and_closing_parentheses?`: between the
/// bottom-most HEREDOC terminator and the `)` there is non-whitespace text
/// (another argument). Returns `true` when there is no `)` at all.
fn exist_argument_between_heredoc_end_and_closing_parens(node: NodeId, cx: &Cx<'_>) -> bool {
    let close = call_close_paren(node, cx);
    if close == Range::ZERO {
        return true; // `return true unless node.loc.end`.
    }
    let Some(heredoc_end) = find_most_bottom_heredoc_end(node, cx) else {
        return false;
    };
    if heredoc_end >= close.start {
        return false;
    }
    let between = cx.raw_source(Range {
        start: heredoc_end,
        end: close.start,
    });
    !between.trim().is_empty()
}

/// The largest `HeredocEnd` terminator end-position among the call's arguments.
fn find_most_bottom_heredoc_end(node: NodeId, cx: &Cx<'_>) -> Option<u32> {
    let close = call_close_paren(node, cx);
    let search_end = if close == Range::ZERO {
        cx.range(node).end
    } else {
        close.end
    };
    cx.sorted_tokens()
        .iter()
        .filter(|t| {
            t.kind == SourceTokenKind::HeredocEnd
                && t.range.start >= cx.range(node).start
                && t.range.end <= search_end
        })
        .map(|t| t.range.end)
        .max()
}

/// The closing `)` of a call node's own argument list, or `Range::ZERO`.
fn call_close_paren(node: NodeId, cx: &Cx<'_>) -> Range {
    if !matches!(
        cx.kind(node),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    ) {
        return Range::ZERO;
    }
    cx.loc(node).end()
}

/// RuboCop's `autocorrect`: relocate the closing `)` to directly after the
/// last argument, with internal/external trailing-comma juggling.
///
/// All byte arithmetic mirrors RuboCop's buffer-source indexing
/// (`node.source_range.end_pos`, `node.children.last.source_range.end_pos`,
/// `node.loc.end.begin_pos`); `)`/`,`/`\n`/` ` are ASCII so byte offsets
/// stay on UTF-8 boundaries.
fn emit_correction(outermost: NodeId, close: Range, cx: &Cx<'_>) {
    let Some(last_arg) = cx.last_argument(outermost).get() else {
        return;
    };
    let last_arg_end = cx.range(last_arg).end;
    // RuboCop's `node.source_range.end_pos`.
    let node_end = cx.range(outermost).end;
    if node_end != close.end || node_end == 0 {
        return;
    }
    let src = cx.source();
    let bytes = src.as_bytes();
    // The node must end with the `)` itself ...
    if bytes.get(node_end as usize - 1) != Some(&b')') {
        return;
    }
    // ... and the last argument must end before it.
    if last_arg_end >= close.start {
        return;
    }

    let external = external_trailing_comma_len(bytes, node_end);

    // `remove_incorrect_closing_paren`: drop the whole `)` line when it only
    // holds the paren (plus an immediately-adjacent `,`), else just the `)`.
    // `incorrect_parenthesis_removal_end` folds an adjacent external comma
    // into the same removal.
    let removal_begin = if line_is_bare_closing_paren(src, close.start) {
        line_start(bytes, close.start).saturating_sub(1)
    } else {
        close.start
    };
    let mut removal_end = node_end;
    if bytes.get(node_end as usize) == Some(&b',') {
        removal_end += 1;
    }
    if removal_begin < removal_end {
        cx.emit_edit(
            Range {
                start: removal_begin,
                end: removal_end,
            },
            "",
        );
    }

    // `add_correct_closing_paren` + `add_correct_external_trailing_comma` as
    // one combined insert (`)` or `),`) right after the last argument, so the
    // two zero-width inserts never depend on edit-apply order.
    cx.emit_edit(
        Range {
            start: last_arg_end,
            end: last_arg_end,
        },
        if external.is_some() { ")," } else { ")" },
    );

    // `remove_internal_trailing_comma`.
    if let Some(len) = internal_trailing_comma_len(src, last_arg_end, close.start) {
        cx.emit_edit(
            Range {
                start: last_arg_end,
                end: last_arg_end + len,
            },
            "",
        );
    }

    // `remove_incorrect_external_trailing_comma`, unless the incorrect-paren
    // removal above already consumed the adjacent comma.
    if let Some(len) = external
        && bytes.get(node_end as usize) != Some(&b',')
    {
        cx.emit_edit(
            Range {
                start: node_end,
                end: node_end + len,
            },
            "",
        );
    }
}

/// `internal_trailing_comma_offset_from_last_arg`: byte length of the span
/// from the last argument's end up to and including the first comma, when a
/// comma precedes the first newline (`<<-SQL,\n`); `None` otherwise.
fn internal_trailing_comma_len(src: &str, from: u32, to: u32) -> Option<u32> {
    if from > to {
        return None;
    }
    let slice = src.get(from as usize..to as usize)?;
    let comma = slice.find(',')?;
    let newline = slice.find('\n')?;
    if comma > newline {
        return None;
    }
    u32::try_from(comma + 1).ok()
}

/// `external_trailing_comma_offset_from_loc_end`: byte length of the span from
/// the node's end over spaces (max 20, literal spaces only per RuboCop's
/// `space?`) up to and including the comma; `None` when there is none.
fn external_trailing_comma_len(bytes: &[u8], node_end: u32) -> Option<u32> {
    let base = node_end as usize;
    let mut offset = 0_usize;
    while offset < 20 && bytes.get(base + offset) == Some(&b' ') {
        offset += 1;
    }
    if bytes.get(base + offset) == Some(&b',') {
        u32::try_from(offset + 1).ok()
    } else {
        None
    }
}

/// `safe_to_remove_line_containing_closing_paren?`: the `)` line holds only
/// the paren, spaces (at most 20 between `)` and `,`), and an optional comma.
/// Checked against the full file line, mirroring
/// `processed_source[node.loc.end.line - 1]` with `/^ *\) {0,20},{0,1} *$/`.
fn line_is_bare_closing_paren(src: &str, offset: u32) -> bool {
    let bytes = src.as_bytes();
    let start = line_start(bytes, offset) as usize;
    let end = bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |p| start + p);
    let line = &bytes[start..end];
    let mut i = 0;
    while i < line.len() && line[i] == b' ' {
        i += 1;
    }
    if line.get(i) != Some(&b')') {
        return false;
    }
    i += 1;
    let mut spaces = 0;
    while i < line.len() && line[i] == b' ' && spaces < 20 {
        i += 1;
        spaces += 1;
    }
    if line.get(i) == Some(&b',') {
        i += 1;
    }
    while i < line.len() && line[i] == b' ' {
        i += 1;
    }
    i == line.len()
}

/// Byte offset just after the previous `\n` — the start of the line
/// containing `offset`.
fn line_start(bytes: &[u8], offset: u32) -> u32 {
    let end = (offset as usize).min(bytes.len());
    bytes[..end]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p as u32 + 1)
}

/// 1-based source line number containing byte `offset`.
fn line_of(offset: u32, src: &str) -> usize {
    src.as_bytes()[..offset as usize]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// Whether two byte offsets are on the same source line.
fn same_line(a: u32, b: u32, src: &str) -> bool {
    line_of(a, src) == line_of(b, src)
}

/// 0-based column (char count) of `offset` within its source line.
fn column_of(src: &str, offset: usize) -> usize {
    let bytes = src.as_bytes();
    let start = bytes[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    src[start..offset].chars().count()
}

murphy_plugin_api::submit_cop!(HeredocArgumentClosingParenthesis);

#[cfg(test)]
mod tests {
    use super::HeredocArgumentClosingParenthesis;
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- bad cases: `)` not on the HEREDOC opener line ----

    #[test]
    fn flags_closing_paren_on_own_line() {
        test::<HeredocArgumentClosingParenthesis>().expect_offense(indoc! {r#"
            foo(<<-SQL
              bar
            SQL
            )
            ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
        "#});
    }

    #[test]
    fn flags_multiple_heredoc_args() {
        test::<HeredocArgumentClosingParenthesis>().expect_offense(indoc! {r#"
            foo(<<-SQL, 123, <<-NOSQL,
              bar
            SQL
              baz
            NOSQL
            )
            ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
        "#});
    }

    // ---- good cases: `)` on the HEREDOC opener line ----

    #[test]
    fn accepts_closing_paren_on_opener_line() {
        test::<HeredocArgumentClosingParenthesis>().expect_no_offenses(indoc! {r#"
            foo(<<-SQL)
              bar
            SQL
        "#});
    }

    #[test]
    fn accepts_multiple_heredoc_args_on_opener_line() {
        test::<HeredocArgumentClosingParenthesis>().expect_no_offenses(indoc! {r#"
            foo(<<-SQL, 123, <<-NOSQL)
              bar
            SQL
              baz
            NOSQL
        "#});
    }

    #[test]
    fn accepts_no_heredoc_argument() {
        test::<HeredocArgumentClosingParenthesis>().expect_no_offenses(indoc! {r#"
            foo(
              bar,
              123,
            )
        "#});
    }

    #[test]
    fn accepts_correct_case_with_other_param_after() {
        // RuboCop spec "accepts correct case with other param after".
        test::<HeredocArgumentClosingParenthesis>().expect_no_offenses(indoc! {r#"
            foo.bar(<<-SQL, 123)
              foo
            SQL
        "#});
    }

    #[test]
    fn accepts_correct_case_with_other_param_before() {
        // RuboCop spec "accepts correct case with other param before".
        test::<HeredocArgumentClosingParenthesis>().expect_no_offenses(indoc! {r#"
            foo.bar(123, <<-SQL)
              foo
            SQL
        "#});
    }

    #[test]
    fn accepts_argument_between_heredoc_and_closing_paren() {
        // RuboCop spec "accepts when there is an argument between a heredoc
        // argument and the closing parentheses". The `)` is on its own line,
        // but `some_arg: {...}` sits between the heredoc terminator and `)`.
        test::<HeredocArgumentClosingParenthesis>().expect_no_offenses(indoc! {r#"
            foo.bar(<<~TEXT,
                Lots of
                Lovely
                Text
              TEXT
              some_arg: { foo: "bar" }
            )
        "#});
    }

    #[test]
    fn flags_nested_inner_heredoc_call() {
        // RuboCop spec "nested incorrect case": the outer `foo(` closing paren
        // on its own line is flagged; the inner `bar(<<-SQL)` is correctly
        // positioned. `foo`'s argument is `foo.bar(<<-SQL)`, a single-line send
        // whose HEREDOC receiver extends past the call — so `foo` does have a
        // HEREDOC argument and is itself the outermost offending send.
        test::<HeredocArgumentClosingParenthesis>().expect_offense(indoc! {r#"
            foo(foo.bar(<<-SQL)
              foo
            SQL
            )
            ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
        "#});
    }

    // ---- autocorrect: relocate `)` after the last argument ----

    #[test]
    fn corrects_closing_paren_on_own_line() {
        test::<HeredocArgumentClosingParenthesis>()
            .expect_offense(indoc! {r#"
                foo(<<-SQL
                  bar
                SQL
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#})
            .expect_correction(
                indoc! {r#"
                    foo(<<-SQL
                      bar
                    SQL
                    )
                    ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
                "#},
                indoc! {r#"
                    foo(<<-SQL)
                      bar
                    SQL
                "#},
            )
            .expect_no_offenses(indoc! {r#"
                foo(<<-SQL)
                  bar
                SQL
            "#});
    }

    #[test]
    fn corrects_internal_trailing_comma() {
        // RuboCop spec "simple incorrect case comma": the `,` after the
        // opener is dropped while the `)` moves up.
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                foo(<<-SQL,
                  foo
                SQL
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                foo(<<-SQL)
                  foo
                SQL
            "#},
        );
    }

    #[test]
    fn corrects_internal_trailing_comma_with_spaces() {
        // RuboCop spec "simple incorrect case comma with spaces".
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                foo(<<-SQL    ,
                  foo
                SQL
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                foo(<<-SQL)
                  foo
                SQL
            "#},
        );
    }

    #[test]
    fn corrects_hash_heredoc_value() {
        // RuboCop spec "simple incorrect case hash".
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                foo.bar(foo: <<-SQL
                  foo
                SQL
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                foo.bar(foo: <<-SQL)
                  foo
                SQL
            "#},
        );
    }

    #[test]
    fn corrects_nested_inner_heredoc_call() {
        // RuboCop spec "nested incorrect case".
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                foo(foo.bar(<<-SQL)
                  foo
                SQL
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                foo(foo.bar(<<-SQL))
                  foo
                SQL
            "#},
        );
    }

    #[test]
    fn corrects_other_param_after_heredoc() {
        // RuboCop spec "incorrect case with other param after".
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                foo.bar(<<-SQL, 123
                  foo
                SQL
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                foo.bar(<<-SQL, 123)
                  foo
                SQL
            "#},
        );
    }

    #[test]
    fn corrects_other_param_before_heredoc() {
        // RuboCop spec "incorrect case with other param before".
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                foo.bar(123, <<-SQL
                  foo
                SQL
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                foo.bar(123, <<-SQL)
                  foo
                SQL
            "#},
        );
    }

    #[test]
    fn corrects_double_heredoc_args() {
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                foo(<<-SQL, 123, <<-NOSQL,
                  bar
                SQL
                  baz
                NOSQL
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                foo(<<-SQL, 123, <<-NOSQL)
                  bar
                SQL
                  baz
                NOSQL
            "#},
        );
    }

    #[test]
    fn corrects_call_after_closing_paren() {
        // RuboCop spec "simple incorrect case with call after": the `)` line
        // holds trailing `.baz`, so only the `)` itself is removed.
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                foo.bar(<<~SQL
                  foo
                SQL
                ).baz
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                foo.bar(<<~SQL)
                  foo
                SQL
                .baz
            "#},
        );
    }

    #[test]
    fn corrects_external_trailing_comma() {
        // RuboCop spec "incorrect case nested method call with comma" (which
        // needs `loop: false` there too): the external `,` after the old `)`
        // moves inside the new parens. Like RuboCop, the single-pass result
        // still flags the outer call — a second pass finishes the job (see
        // `corrects_outer_call_on_second_pass`).
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                bar(
                  foo.bar(123, <<-SQL
                    foo
                  SQL
                  ),
                  ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
                  456,
                  789,
                )
            "#},
            indoc! {r#"
                bar(
                  foo.bar(123, <<-SQL),
                    foo
                  SQL
                  456,
                  789,
                )
            "#},
        );
    }

    #[test]
    fn corrects_outer_call_on_second_pass() {
        // Second pass over `corrects_external_trailing_comma`'s output: the
        // outer `)` relocates after `789` (dropping its internal `,`).
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                bar(
                  foo.bar(123, <<-SQL),
                    foo
                  SQL
                  456,
                  789,
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                bar(
                  foo.bar(123, <<-SQL),
                    foo
                  SQL
                  456,
                  789)
            "#},
        );
    }

    #[test]
    fn third_pass_is_a_stable_no_op() {
        // Idempotency: once the outer `)` sits right after the last argument,
        // the correction removes and re-inserts the same `)` — the source is
        // unchanged (the remaining offense itself matches RuboCop, whose
        // `exist_argument_between...` skip also stays false here).
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                bar(
                  foo.bar(123, <<-SQL),
                    foo
                  SQL
                  456,
                  789)
                     ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                bar(
                  foo.bar(123, <<-SQL),
                    foo
                  SQL
                  456,
                  789)
            "#},
        );
    }

    #[test]
    fn corrects_csend_receiver() {
        // `&.` variant of the simple incorrect case.
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                foo&.bar(<<-SQL
                  foo
                SQL
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                foo&.bar(<<-SQL)
                  foo
                SQL
            "#},
        );
    }

    #[test]
    fn keeps_comma_inside_heredoc_body() {
        // RuboCop spec "simple incorrect case comma with spaces and comma in
        // heredoc": only the opener's trailing comma goes; the body comma
        // (past the first newline) survives.
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                foo(<<-SQL    ,
                  foo,
                SQL
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                foo(<<-SQL)
                  foo,
                SQL
            "#},
        );
    }

    #[test]
    fn corrects_double_heredoc_on_next_line() {
        // RuboCop spec "double case new line".
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                foo(
                  <<-SQL, <<-NOSQL
                  foo
                SQL
                  bar
                NOSQL
                )
                ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
            "#},
            indoc! {r#"
                foo(
                  <<-SQL, <<-NOSQL)
                  foo
                SQL
                  bar
                NOSQL
            "#},
        );
    }

    #[test]
    fn corrects_spaced_external_trailing_comma() {
        // RuboCop spec "incorrect case in array with spaced out comma".
        test::<HeredocArgumentClosingParenthesis>().expect_correction(
            indoc! {r#"
                [
                  foo.bar(123, <<-SQL
                    foo
                  SQL
                  )      ,
                  ^ Put the closing parenthesis for a method call with a HEREDOC parameter on the same line as the HEREDOC opening.
                  456,
                  789,
                ]
            "#},
            indoc! {r#"
                [
                  foo.bar(123, <<-SQL),
                    foo
                  SQL
                  456,
                  789,
                ]
            "#},
        );
    }
}
