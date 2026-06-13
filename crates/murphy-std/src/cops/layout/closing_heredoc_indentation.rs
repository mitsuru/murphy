//! `Layout/ClosingHeredocIndentation` — checks the indentation of here-document
//! closings (the terminator label line).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/ClosingHeredocIndentation
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Direct port of RuboCop's `on_heredoc`. Murphy's AST hides only the heredoc
//!   *sigil*, not the node: a heredoc is a `Str`/`Dstr`/`Xstr` node whose source
//!   range contains a `HeredocStart` token (parser-gem shape, where the node's
//!   `source_range` begins at the `<<~LABEL` opener). The cop is hooked on those
//!   three node kinds and self-filters to heredoc nodes.
//!
//!   For each heredoc:
//!     * `heredoc_type == SIMPLE_HEREDOC` (`<<`, no `~`/`-`) is skipped — only
//!       `<<~` and `<<-` are checked.
//!     * `opening_indentation` = `node.source_range.source_line[/\A */].length`,
//!       the count of leading **spaces** (RuboCop's `indent_level` is spaces-only;
//!       a tab-indented line has indent 0).
//!     * `closing_indentation` = same metric on the terminator line.
//!     * Skip when `opening == closing`.
//!     * Skip when `argument_indentation_correct?`: when the node is an
//!       `argument?` or `chained?`, the opener indent of the outermost
//!       enclosing `send` (`find_node_used_heredoc_argument`, walking up while the
//!       parent is a `send`) equals the closing indent — i.e. the closing aligns
//!       with the call's opening line rather than the heredoc opener line.
//!     * Message: `MSG_ARG` when `argument?`, else `MSG`, embedding the stripped
//!       closing and opening lines.
//!
//!   Offense range is RuboCop's `node.loc.heredoc_end`: the terminator line's
//!   leading whitespace plus its label (the closing line with no trailing
//!   newline), built from the `HeredocEnd` token extended left to its line start.
//!   Autocorrect mirrors `indented_end`: replace the leading `closing_indent`
//!   spaces with `opening_indent` spaces (a single surgical edit on the leading
//!   whitespace range).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// RuboCop `SIMPLE_HEREDOC = '<<'`.
const SIMPLE_HEREDOC: &str = "<<";

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct ClosingHeredocIndentation;

#[cop(
    name = "Layout/ClosingHeredocIndentation",
    description = "Checks the indentation of here document closings.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ClosingHeredocIndentation {
    #[on_node(kind = "str")]
    fn check_str(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "xstr")]
    fn check_xstr(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // `node.heredoc?` — only heredoc string nodes are visited by `on_heredoc`.
    let Some(opener_tok) = heredoc_start_token(node, cx) else {
        return;
    };
    let Some(end_tok) = heredoc_end_token(node, cx) else {
        return;
    };

    let source = cx.source();

    // `return if heredoc_type(node) == SIMPLE_HEREDOC` — skip plain `<<`.
    if heredoc_type(cx.raw_source(opener_tok)) == SIMPLE_HEREDOC {
        return;
    }

    // `opening_indentation` / `closing_indentation`.
    let opening_line = source_line(source, opener_tok.start);
    let closing_line = source_line(source, end_tok.start);
    let opening_indent = indent_level(opening_line);
    let closing_indent = indent_level(closing_line);

    // `return if opening_indentation(node) == closing_indentation(node)`.
    if opening_indent == closing_indent {
        return;
    }

    // `return if argument_indentation_correct?(node)`.
    if argument_indentation_correct(node, closing_indent, cx) {
        return;
    }

    // Offense range = `node.loc.heredoc_end`: the closing line's leading
    // whitespace + label (no trailing newline). The `HeredocEnd` token spans the
    // label *and* its newline, so extend left to the line start and clamp the
    // end to the line content (drop any trailing `\n`/`\r`).
    let line_start = line_start_offset(source, end_tok.start);
    let line_end = line_start + closing_line.len() as u32;
    let offense_range = Range {
        start: line_start,
        end: line_end,
    };

    let message = message(node, closing_line, opening_line, cx);
    cx.emit_offense(offense_range, &message, None);

    // `indented_end`: replace the leading `closing_indent` spaces with
    // `opening_indent` spaces. A single surgical edit on the leading-whitespace
    // run, leaving the label untouched.
    let indent_range = Range {
        start: line_start,
        end: line_start + closing_indent as u32,
    };
    cx.emit_edit(indent_range, &" ".repeat(opening_indent));
}

/// `heredoc_type(node)` — the leading `<<` plus its optional `~`/`-` sigil char.
/// RuboCop matches `node.source[/^<<[~-]?/]`. Returns `"<<"`, `"<<~"`, or `"<<-"`.
fn heredoc_type(opener: &str) -> &str {
    let after = opener.strip_prefix("<<").unwrap_or("");
    match after.as_bytes().first() {
        Some(b'~') => "<<~",
        Some(b'-') => "<<-",
        _ => SIMPLE_HEREDOC,
    }
}

/// `argument_indentation_correct?(node)`.
///
/// ```text
/// return false unless node.argument? || node.chained?
/// opening_indentation(find_node_used_heredoc_argument(node.parent)) ==
///   closing_indentation(node)
/// ```
fn argument_indentation_correct(node: NodeId, closing_indent: usize, cx: &Cx<'_>) -> bool {
    if !(cx.is_argument(node) || cx.is_chained(node)) {
        return false;
    }
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    let used = find_node_used_heredoc_argument(parent, cx);
    let opening_line = source_line(cx.source(), cx.range(used).start);
    indent_level(opening_line) == closing_indent
}

/// `find_node_used_heredoc_argument(node)`: walk up while the parent is a
/// `send` (not `csend` — RuboCop's `send_type?`), returning the topmost such
/// node.
fn find_node_used_heredoc_argument(node: NodeId, cx: &Cx<'_>) -> NodeId {
    let mut current = node;
    while let Some(parent) = cx.parent(current).get() {
        if matches!(cx.kind(parent), NodeKind::Send { .. }) {
            current = parent;
        } else {
            break;
        }
    }
    current
}

/// `message(node)` — `MSG_ARG` when the node is an `argument?`, else `MSG`,
/// embedding the stripped closing and opening lines.
fn message(node: NodeId, closing_line: &str, opening_line: &str, cx: &Cx<'_>) -> String {
    let closing = closing_line.trim();
    let opening = opening_line.trim();
    if cx.is_argument(node) {
        format!(
            "`{closing}` is not aligned with `{opening}` or beginning of method definition."
        )
    } else {
        format!("`{closing}` is not aligned with `{opening}`.")
    }
}

/// The `HeredocStart` token contained in `node`'s source range, if any
/// (RuboCop's `node.heredoc?`).
fn heredoc_start_token(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    cx.tokens_in(cx.range(node))
        .iter()
        .find(|t| t.kind == SourceTokenKind::HeredocStart)
        .map(|t| t.range)
}

/// The `HeredocEnd` terminator token paired with `node`'s heredoc, via FIFO
/// index pairing.
///
/// A node's source range covers its opener but **not** its body/terminator
/// (parser-gem shape). Ruby reads heredoc bodies in *opener order*: the k-th
/// `HeredocStart` (source order) is closed by the k-th `HeredocEnd`. So for
/// stacked openers on one line — `foo(<<~A, <<~B)` — A's opener (index 0) pairs
/// with A's terminator (the first `HeredocEnd`), and B's opener (index 1) pairs
/// with B's terminator (the second). A "first `HeredocEnd` after this opener"
/// heuristic would mispair B with A's terminator, because both terminators lie
/// after B's opener line.
///
/// We index by the node's own `HeredocStart`. For a heredoc whose body
/// interpolates another heredoc, the node range contains multiple
/// `HeredocStart`s; the node's own opener is the one at the node range's start,
/// i.e. the first `HeredocStart` in the node range.
fn heredoc_end_token(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let opener = heredoc_start_token(node, cx)?;
    let starts = cx
        .sorted_tokens()
        .iter()
        .filter(|t| t.kind == SourceTokenKind::HeredocStart);
    // Index of this node's opener among all openers in source order.
    let index = starts.take_while(|t| t.range.start < opener.start).count();
    cx.sorted_tokens()
        .iter()
        .filter(|t| t.kind == SourceTokenKind::HeredocEnd)
        .nth(index)
        .map(|t| t.range)
}

/// RuboCop's `indent_level(source_line)`: `source_line[/\A */].length` — the
/// count of leading **space** characters only (tabs do not count).
fn indent_level(line: &str) -> usize {
    line.bytes().take_while(|&b| b == b' ').count()
}

/// The full source line (no trailing newline) containing byte `offset`.
fn source_line(source: &str, offset: u32) -> &str {
    let bytes = source.as_bytes();
    let start = line_start_offset(source, offset) as usize;
    let end = bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(source.len(), |i| start + i);
    &source[start..end]
}

/// Byte offset of the first byte on the line containing `offset`.
fn line_start_offset(source: &str, offset: u32) -> u32 {
    source.as_bytes()[..offset as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos as u32 + 1)
}

murphy_plugin_api::submit_cop!(ClosingHeredocIndentation);

#[cfg(test)]
mod tests {
    use super::ClosingHeredocIndentation;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── good cases ────────────────────────────────────────────────────────────

    #[test]
    fn accepts_correctly_indented_closing_heredoc() {
        test::<ClosingHeredocIndentation>().expect_no_offenses(indoc! {r#"
            class Test
              def foo
                <<-SQL
                  bar
                SQL
              end
            end
        "#});
    }

    #[test]
    fn accepts_correctly_indented_empty_heredoc() {
        test::<ClosingHeredocIndentation>().expect_no_offenses(indoc! {r#"
            def foo
              <<-NIL

              NIL
            end
        "#});
    }

    #[test]
    fn accepts_plain_double_angle_heredoc() {
        // `<<NIL` (SIMPLE_HEREDOC) is never checked.
        test::<ClosingHeredocIndentation>().expect_no_offenses(indoc! {r#"
            def foo
              <<NIL

            NIL
            end
        "#});
    }

    #[test]
    fn accepts_argument_position_heredoc_aligned_with_method() {
        // The closing `EOS` aligns with the beginning of the method definition
        // (`include_examples` opener column 0), not the heredoc opener column.
        test::<ClosingHeredocIndentation>().expect_no_offenses(indoc! {r#"
            include_examples :offense,
                             <<-EOS
              bar
            EOS
        "#});
    }

    #[test]
    fn accepts_chained_heredoc_aligned_with_method() {
        test::<ClosingHeredocIndentation>().expect_no_offenses(indoc! {r#"
            include_examples :offense,
                             <<-EOS.strip_indent
              bar
            EOS
        "#});
    }

    #[test]
    fn accepts_argument_position_heredoc_aligned_with_opener() {
        // The closing aligns with the heredoc opener column.
        test::<ClosingHeredocIndentation>().expect_no_offenses(indoc! {r#"
            include_examples :offense,
                             <<-EOS
                               foo
                                 bar
                             EOS
        "#});
    }

    #[test]
    fn accepts_argument_heredoc_with_blank_line_in_body() {
        // RuboCop spec: "accepts correctly indented closing heredoc when heredoc
        // contents with blank line". The closing aligns with the opener column.
        test::<ClosingHeredocIndentation>().expect_no_offenses(indoc! {r#"
            def_node_matcher :eval_without_location?, <<~PATTERN
              {
                (send $(send _ $:sort ...) ${:[] :at :slice} {(int 0) (int -1)})

                (send $(send _ $:sort_by _) ${:last :first})
              }
            PATTERN
        "#});
    }

    #[test]
    fn accepts_content_before_closing_heredoc() {
        // RuboCop spec: "accepts correctly indented closing heredoc when heredoc
        // contents is before closing heredoc". Body indentation varies but the
        // closing still aligns with the opener column.
        test::<ClosingHeredocIndentation>().expect_no_offenses(indoc! {r#"
            include_examples :offense,
                             <<-EOS
                               foo
              bar
                               baz
                             EOS
        "#});
    }

    #[test]
    fn accepts_empty_heredoc_in_block_argument() {
        // RuboCop spec: "accepts correctly indented closing heredoc when aligned
        // at the beginning of method definition and content is empty".
        test::<ClosingHeredocIndentation>().expect_no_offenses(indoc! {r#"
            let(:source) { <<~EOS }
            EOS
        "#});
    }

    #[test]
    fn accepts_stacked_heredocs_each_aligned() {
        // Two stacked heredocs on one opener line; each terminator aligns with
        // the call opener column 0. FIFO index pairing must match A↔A, B↔B.
        test::<ClosingHeredocIndentation>().expect_no_offenses(indoc! {r#"
            foo(<<~A, <<~B)
              a
            A
              b
            B
        "#});
    }

    // ── offenses ──────────────────────────────────────────────────────────────

    #[test]
    fn flags_under_indented_closing() {
        test::<ClosingHeredocIndentation>().expect_offense(indoc! {r#"
            class Test
              def foo
                <<-SQL
                  bar
              SQL
            ^^^^^ `SQL` is not aligned with `<<-SQL`.
              end
            end
        "#});
    }

    #[test]
    fn corrects_under_indented_closing() {
        test::<ClosingHeredocIndentation>().expect_correction(
            indoc! {r#"
                class Test
                  def foo
                    <<-SQL
                      bar
                  SQL
                ^^^^^ `SQL` is not aligned with `<<-SQL`.
                  end
                end
            "#},
            indoc! {r#"
                class Test
                  def foo
                    <<-SQL
                      bar
                    SQL
                  end
                end
            "#},
        );
    }

    #[test]
    fn flags_and_corrects_over_indented_empty_heredoc() {
        test::<ClosingHeredocIndentation>().expect_correction(
            indoc! {r#"
                def foo
                  <<-NIL

                    NIL
                ^^^^^^^ `NIL` is not aligned with `<<-NIL`.
                end
            "#},
            indoc! {r#"
                def foo
                  <<-NIL

                  NIL
                end
            "#},
        );
    }

    #[test]
    fn flags_squiggly_heredoc_closing() {
        test::<ClosingHeredocIndentation>().expect_correction(
            indoc! {r#"
                class Foo
                  def bar
                    <<~SQL
                      'Hi'
                  SQL
                ^^^^^ `SQL` is not aligned with `<<~SQL`.
                  end
                end
            "#},
            indoc! {r#"
                class Foo
                  def bar
                    <<~SQL
                      'Hi'
                    SQL
                  end
                end
            "#},
        );
    }

    #[test]
    fn flags_only_misaligned_terminator_of_stacked_heredocs() {
        // A's terminator (`A`, col 0) aligns with the `foo(` opener; B's
        // terminator (`  B`, col 2) does not. Only B must be flagged — this is
        // the regression guard for FIFO index pairing (a naive
        // "first HeredocEnd after this opener" pairing would mispair B with A's
        // terminator and check the wrong line).
        // The offense range spans the whole closing line (`  B`, columns 0-2),
        // so the carets begin at column 0 and cover the 2 spaces plus `B`.
        test::<ClosingHeredocIndentation>().expect_offense(indoc! {r#"
            foo(<<~A, <<~B)
              a
            A
              b
              B
            ^^^ `B` is not aligned with `foo(<<~A, <<~B)` or beginning of method definition.
        "#});
    }
}
