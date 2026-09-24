//! `Layout/SpaceBeforeFirstArg` — enforces exactly one space between a
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceBeforeFirstArg
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `on_send`/`on_csend`: flags a parenthesis-less method
//!   call whose first argument is separated from the method name by zero or
//!   more-than-one space, and rewrites the gap to a single space. Operator
//!   methods, setter methods, and parenthesized calls are skipped. The
//!   zero-space case (`something'hello'`) emits a zero-length insert-point
//!   offense. `AllowForAlignment` (default `true`) ports RuboCop's
//!   `PrecedingFollowingAlignment#aligned_with_something?` (`aligned_token?`):
//!   nearest content lines (blank and full-line comments skipped) are tested
//!   for `aligned_words?` (`/\s\S/` at the argument column or token-text
//!   equality) and token-aware `aligned_equals_operator?` (trailing-`=`/ `<<`
//!   end-column equality via the token stream), with a second-pass
//!   same-indentation search and character-based columns. Cross-line first
//!   arguments are exempt, matching RuboCop's `same_line?` guard inside
//!   `expect_params_after_method_name?`.
//! ```
//!
//! method name and its first argument for calls without parentheses.
//! Mirrors RuboCop's same-named cop.

use murphy_plugin_api::{CopOptions, Cx, NodeId, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceBeforeFirstArg;

#[derive(CopOptions)]
pub struct SpaceBeforeFirstArgOptions {
    #[option(
        name = "AllowForAlignment",
        default = true,
        description = "Allow extra spaces that vertically align the first argument."
    )]
    pub allow_for_alignment: bool,
}

#[cop(
    name = "Layout/SpaceBeforeFirstArg",
    description = "Put one space between the method name and the first argument.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceBeforeFirstArgOptions,
)]
impl SpaceBeforeFirstArg {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

const MSG: &str = "Put one space between the method name and the first argument.";

fn check(node: NodeId, cx: &Cx<'_>) {
    // `regular_method_call_with_arguments?`: has arguments, not an operator
    // method (`a + b`), not a setter method (`obj.foo = x`).
    let args = cx.call_arguments(node);
    let Some(&first_arg) = args.first() else {
        return;
    };
    if cx.is_operator_method(node) || cx.is_setter_method(node) {
        return;
    }
    // `return if node.parenthesized?`
    if cx.is_parenthesized(node) {
        return;
    }

    // The method-name selector range; without it we cannot locate the gap.
    let selector = cx.selector(node);
    if selector == Range::ZERO {
        return;
    }
    let name_end = selector.end as usize;
    let arg_start = cx.range(first_arg).start as usize;
    // Defensive: a malformed range where the argument precedes the selector.
    if arg_start < name_end {
        return;
    }

    let src = cx.source().as_bytes();
    // `space` = the whitespace immediately preceding the first argument, i.e.
    // `range_with_surrounding_space(first_arg, side: :left)` clipped to the
    // run of spaces/tabs directly before the arg. We start at `arg_start` and
    // walk left over spaces/tabs, but never past the method-name end.
    let mut space_start = arg_start;
    while space_start > name_end && matches!(src[space_start - 1], b' ' | b'\t') {
        space_start -= 1;
    }
    let space_len = arg_start - space_start;

    // `return if space.length == 1` — already exactly one space, clean.
    if space_len == 1 {
        return;
    }

    if !expect_params_after_method_name(cx, node, first_arg, space_start, arg_start) {
        return;
    }

    let space = Range {
        start: space_start as u32,
        end: arg_start as u32,
    };
    cx.emit_offense(space, MSG, None);
    cx.emit_edit(space, " ");
}

/// RuboCop's `expect_params_after_method_name?`:
/// - always expect when there is zero space between the method name and the
///   first argument (`something'hello'`);
/// - otherwise, only when the first argument is on the same line as the call
///   AND it is not exempted by `AllowForAlignment`.
fn expect_params_after_method_name(
    cx: &Cx<'_>,
    node: NodeId,
    first_arg: NodeId,
    space_start: usize,
    arg_start: usize,
) -> bool {
    // `no_space_between_method_name_and_first_argument?`
    if space_start == arg_start {
        return true;
    }

    // `same_line?(first_arg, node)` — both ends on the same source line. The
    // gap is whitespace-only (spaces/tabs) by construction, so the call name
    // and arg share a line iff there is no newline between the call start and
    // the arg. We approximate with the gap: a whitespace-only gap on one line.
    let src = cx.source().as_bytes();
    let call_start = cx.range(node).start as usize;
    let same_line = !src[call_start..arg_start].contains(&b'\n');
    if !same_line {
        return false;
    }

    // `!(allow_for_alignment? && aligned_with_something?(first_arg))`
    let opts = cx.options_or_default::<SpaceBeforeFirstArgOptions>();
    if opts.allow_for_alignment && aligned_with_something(cx, first_arg) {
        return false;
    }
    true
}

/// RuboCop `PrecedingFollowingAlignment#aligned_with_something?` for a
/// first-argument range: `aligned_with_adjacent_line?(range, aligned_token?)`.
///
/// Two phases, matching `aligned_with_any_line_range?`:
/// 1. nearest preceding/following content line (blank lines and full-line
///    comments skipped), tested with `aligned_token?`;
/// 2. if neither matches, the nearest preceding/following line with the same
///    indentation as the current line (different-indent lines skipped).
///
/// `aligned_token?` is `aligned_words? || aligned_equals_operator?`:
/// - `aligned_words?`: `/\s\S/` at `[column-1, 2]` (preceding char is
///   whitespace and the column holds non-whitespace) or the argument text
///   itself appears at the same column on the adjacent line;
/// - `aligned_equals_operator?`: the argument ends with `=` (or is `<<`) and
///   its end column equals the end column of the first assignment/comparison
///   operator on the adjacent line (found via the token stream so `=` inside
///   strings is ignored).
///
/// Columns are character counts (RuboCop `range.column` / `String#[]`), not
/// byte offsets, so CJK text aligns correctly.
fn aligned_with_something(cx: &Cx<'_>, first_arg: NodeId) -> bool {
    let range = cx.range(first_arg);
    let src = cx.source();
    let src_len = src.len();
    let arg_start = range.start as usize;
    let arg_end = range.end as usize;
    if arg_start > src_len || arg_end > src_len || arg_start > arg_end {
        return false;
    }

    let mut line_starts: Vec<usize> = vec![0];
    for (i, b) in src.as_bytes().iter().enumerate() {
        if *b == b'\n' {
            let next = i + 1;
            if next <= src_len {
                line_starts.push(next);
            }
        }
    }
    let line_count = line_starts.len();
    if line_count == 0 {
        return false;
    }
    let cur_idx = line_starts
        .partition_point(|&s| s <= arg_start)
        .saturating_sub(1)
        .min(line_count.saturating_sub(1));
    let cur_line_start = line_starts[cur_idx];
    let start_col = src
        .get(cur_line_start..arg_start)
        .map(|s| s.chars().count())
        .unwrap_or(0);
    let end_idx = line_starts
        .partition_point(|&s| s <= arg_end.min(src_len))
        .saturating_sub(1)
        .min(line_count.saturating_sub(1));
    let end_line_start = line_starts[end_idx];
    let cur_end_col = src
        .get(end_line_start..arg_end.min(src_len))
        .map(|s| s.chars().count())
        .unwrap_or(0);

    let token_text = cx.raw_source(range);
    let token_chars: Vec<char> = token_text.chars().collect();
    let token_len = token_chars.len();
    if token_len == 0 {
        return false;
    }
    let ends_with_eq = token_chars.last().is_some_and(|&c| c == '=');
    let is_lshft = token_text == "<<";

    let line_byte_end = |idx: usize| -> usize {
        let start = line_starts[idx];
        let bytes = src.as_bytes();
        if start >= bytes.len() {
            return bytes.len();
        }
        bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |p| start + p)
    };
    let line_content = |idx: usize| -> &str {
        let s = line_starts[idx];
        let e = line_byte_end(idx);
        src.get(s..e).unwrap_or("")
    };

    let mut comment_lines = std::collections::HashSet::new();
    for c in cx.comments() {
        let cs = c.range.start as usize;
        if cs > src_len {
            continue;
        }
        let c_idx = line_starts
            .partition_point(|&s| s <= cs)
            .saturating_sub(1)
            .min(line_count.saturating_sub(1));
        let ls = line_starts[c_idx];
        if let Some(prefix) = src.get(ls..cs.min(src_len))
            && prefix.chars().all(is_ruby_whitespace_char)
        {
            comment_lines.insert(c_idx);
        }
    }
    let is_blank = |idx: usize| -> bool { line_content(idx).chars().all(is_ruby_whitespace_char) };
    let indent_of = |idx: usize| -> Option<usize> {
        let mut count = 0;
        for ch in line_content(idx).chars() {
            if is_ruby_whitespace_char(ch) {
                count += 1;
            } else {
                return Some(count);
            }
        }
        None
    };

    let aligned_words = |adj_idx: usize| -> bool {
        let adj_chars: Vec<char> = line_content(adj_idx).chars().collect();
        if start_col >= 1 && start_col < adj_chars.len() {
            let prev = adj_chars[start_col - 1];
            let cur = adj_chars[start_col];
            if is_ruby_whitespace_char(prev) && !is_ruby_whitespace_char(cur) {
                return true;
            }
        }
        if start_col + token_len <= adj_chars.len()
            && adj_chars[start_col..start_col + token_len] == token_chars[..]
        {
            return true;
        }
        false
    };

    let aligned_equals = |adj_idx: usize| -> bool {
        let ls = line_starts[adj_idx];
        let le = line_byte_end(adj_idx);
        if ls >= le {
            return false;
        }
        let lr = Range {
            start: ls as u32,
            end: le as u32,
        };
        for tok in cx.tokens_in(lr) {
            if tok.kind != SourceTokenKind::Other {
                continue;
            }
            let text = cx.raw_source(tok.range);
            if !is_assignment_or_comparison_operator(text) {
                continue;
            }
            let tok_end = tok.range.end as usize;
            if tok_end < ls || tok_end > src_len {
                return false;
            }
            let adj_end_col = src
                .get(ls..tok_end.min(src_len))
                .map(|s| s.chars().count())
                .unwrap_or(0);
            if cur_end_col != adj_end_col {
                return false;
            }
            if ends_with_eq {
                return true;
            }
            if is_lshft && text == "=" {
                return true;
            }
            return false;
        }
        false
    };

    let aligned_token =
        |adj_idx: usize| -> bool { aligned_words(adj_idx) || aligned_equals(adj_idx) };

    let test_direction = |is_pre: bool, indent_filter: Option<usize>| -> bool {
        if is_pre {
            let mut idx = cur_idx;
            loop {
                if idx == 0 {
                    return false;
                }
                idx -= 1;
                if comment_lines.contains(&idx) {
                    continue;
                }
                if is_blank(idx) {
                    continue;
                }
                if let Some(f) = indent_filter
                    && indent_of(idx) != Some(f)
                {
                    continue;
                }
                return aligned_token(idx);
            }
        } else {
            let mut idx = cur_idx;
            loop {
                idx += 1;
                if idx >= line_count {
                    return false;
                }
                if comment_lines.contains(&idx) {
                    continue;
                }
                if is_blank(idx) {
                    continue;
                }
                if let Some(f) = indent_filter
                    && indent_of(idx) != Some(f)
                {
                    continue;
                }
                return aligned_token(idx);
            }
        }
    };

    if test_direction(true, None) || test_direction(false, None) {
        return true;
    }
    if let Some(base) = indent_of(cur_idx)
        && (test_direction(true, Some(base)) || test_direction(false, Some(base)))
    {
        return true;
    }
    false
}

#[inline]
fn is_ruby_whitespace_char(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0B' | '\x0C')
}

/// RuboCop `ASSIGNMENT_OR_COMPARISON_TOKENS` spellings: operators ending with
/// `=` (assignment, op-assignment, comparison) plus `<<`. Mirrors
/// `cops::util` (private there) so the `aligned_equals_operator?` token search
/// stays token-aware and ignores `=` inside strings.
fn is_assignment_or_comparison_operator(text: &str) -> bool {
    if text == "<<" {
        return true;
    }
    text.ends_with('=') && text.bytes().all(|b| b.is_ascii_punctuation())
}

#[cfg(test)]
mod tests {
    use super::{SpaceBeforeFirstArg, SpaceBeforeFirstArgOptions};
    use murphy_plugin_api::test_support::{indoc, run_cop_with_options_and_edits, test};

    #[test]
    fn flags_multiple_spaces_before_first_arg() {
        test::<SpaceBeforeFirstArg>().expect_correction(
            indoc! {r#"
                something  x
                         ^^ Put one space between the method name and the first argument.
            "#},
            "something x\n",
        );
    }

    #[test]
    fn flags_multiple_spaces_before_first_arg_with_multiple_args() {
        test::<SpaceBeforeFirstArg>().expect_correction(
            indoc! {r#"
                something   y, z
                         ^^^ Put one space between the method name and the first argument.
            "#},
            "something y, z\n",
        );
    }

    #[test]
    fn accepts_single_space_before_first_arg() {
        test::<SpaceBeforeFirstArg>()
            .expect_no_offenses("something x\nsomething y, z\nsomething 'hello'\n");
    }

    #[test]
    fn accepts_parenthesized_call() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses("something(  x)\n");
    }

    #[test]
    fn accepts_operator_method() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses("a  +  b\n");
    }

    #[test]
    fn accepts_setter_method() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses("obj.foo =  x\n");
    }

    #[test]
    fn accepts_call_without_arguments() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses("something\n");
    }

    /// A multiline receiver chain (`foo` on line 1, `.bar  baz` on line 2) is
    /// intentionally skipped: RuboCop's `expect_params_after_method_name?`
    /// calls `same_line?(first_arg, node)` where `node` is the full send and
    /// `node.loc.line` is the expression-start line (the receiver's line). With
    /// the receiver and the argument on different lines, `same_line?` is false,
    /// so RuboCop does not flag the extra space. Murphy mirrors this by keying
    /// the same-line check on the node's expression start, not the selector.
    #[test]
    fn accepts_multiline_receiver_chain() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses("foo\n  .bar  baz\n");
    }

    /// `on_csend` dispatch: a safe-navigation call `foo&.bar  baz` is treated
    /// the same as `on_send`.
    #[test]
    fn flags_multiple_spaces_before_first_arg_on_csend() {
        test::<SpaceBeforeFirstArg>().expect_correction(
            indoc! {r#"
                foo&.bar  baz
                        ^^ Put one space between the method name and the first argument.
            "#},
            "foo&.bar baz\n",
        );
    }

    /// Zero space between method name and first argument (`something'hello'`).
    /// The offense range is a zero-length insert point, which the caret
    /// annotation format cannot represent, so we verify via run_cop + edits.
    #[test]
    fn flags_zero_space_before_string_argument() {
        let opts = SpaceBeforeFirstArgOptions {
            allow_for_alignment: true,
        };
        let result =
            run_cop_with_options_and_edits::<SpaceBeforeFirstArg>("something'hello'\n", &opts);
        assert_eq!(
            result.offenses.len(),
            1,
            "expected 1 offense, got {:?}",
            result.offenses
        );
        assert_eq!(
            result.offenses[0].message,
            "Put one space between the method name and the first argument."
        );
        assert_eq!(result.edits.len(), 1, "expected 1 edit");
        assert_eq!(result.edits[0].replacement, " ");
    }

    /// `AllowForAlignment: true` (default) exempts an argument aligned with a
    /// non-whitespace character directly above it.
    #[test]
    fn accepts_aligned_argument_when_allow_for_alignment() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses(indoc! {r#"
            foo    1
            foobar 2
        "#});
    }

    /// RuboCop `aligned_words?` requires the preceding character to be
    /// whitespace (`/\s\S/` at `[left_edge-1, 2]`), not merely a non-whitespace
    /// character at the same column. `foobar22` has `2` at the `1` column but
    /// the preceding char is also `2`, so RuboCop flags `foo    1`.
    #[test]
    fn flags_extra_space_when_adjacent_char_lacks_preceding_space() {
        test::<SpaceBeforeFirstArg>().expect_offense(indoc! {r#"
            foobar22
            foo    1
               ^^^^ Put one space between the method name and the first argument.
        "#});
    }

    /// RuboCop's second-pass same-indentation search: the nearest following
    /// line (`  x = 1`, indent 2) does not align, but the farther same-indent
    /// line (`foo    2`, indent 0) does, so both `foo` lines are accepted.
    #[test]
    fn accepts_alignment_via_same_indent_fallback() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses(indoc! {r#"
            foo    1
              x = 1
            foo    2
        "#});
    }

    /// Columns are character-based, not byte-based. `foo    1` (arg at char
    /// column 7) aligns with `あa     2` (arg at char column 7 despite the
    /// 3-byte `あ`), so RuboCop accepts both lines.
    #[test]
    fn accepts_char_column_alignment_with_multibyte() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses(
            "foo    1
あa     2
",
        );
    }

    /// With `AllowForAlignment: false`, alignment spacing is flagged.
    #[test]
    fn flags_aligned_argument_when_disallow_for_alignment() {
        let opts = SpaceBeforeFirstArgOptions {
            allow_for_alignment: false,
        };
        test::<SpaceBeforeFirstArg>()
            .with_options(&opts)
            .expect_correction(
                indoc! {r#"
                    foo    1
                       ^^^^ Put one space between the method name and the first argument.
                    foobar 2
                "#},
                "foo 1\nfoobar 2\n",
            );
    }
}

murphy_plugin_api::submit_cop!(SpaceBeforeFirstArg);
