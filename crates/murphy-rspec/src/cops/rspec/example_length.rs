//! `RSpec/ExampleLength` — caps the line count of an example block's
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ExampleLength
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues:
//!   - murphy-fgcu
//!   - murphy-ttzm
//! notes: >
//!   Fixed: comment lines skipped by default, blank lines skipped, CountComments
//!   option, CountAsOne option (array/hash/method_call), message format matches
//!   RuboCop ("Example has too many lines. [N/M]"), runtime options wired via
//!   cx.options_or_default. Remaining gaps: heredoc folding in CountAsOne,
//!   alias coverage.
//! ```
//!
//! body. Mirrors RuboCop-RSpec's cop of the same name.
//!
//! ## Matched shapes
//!
//! Dispatched on `NodeKind::Block` and gates on
//! [`is_example_call`](crate::helpers::is_example_call) — the block's
//! call must be a bare `it` / `specify` / `example`. Other blocks
//! (`describe`, `context`, `before`, …) are skipped: this rule
//! specifically polices example bodies, not surrounding scaffolding.
//!
//! ## Line counting
//!
//! Counts logical source lines inside the body (between `do` and `end`,
//! not including them). Blank lines and (by default) comment lines are
//! excluded, matching RuboCop's `irrelevant_line` behavior.
//!
//! - `it { foo }` — body is `foo`, 1 line.
//! - `it do; a; b; c; end` — body covers `a; b; c`, 1 line (semicolons,
//!   not newlines).
//! - `it do\n  a\n  b\nend` — body covers `a\n  b`, 2 lines.
//! - Blank lines are not counted.
//! - Comment lines (first non-ws char is `#`) are not counted unless
//!   `CountComments: true`.
//!
//! An `it do ... end` with an empty body (no body node) is treated as
//! 0 lines and never emits.
//!
//! ## Options
//!
//! - `max` (default `5`, matching RuboCop) — bodies whose line count
//!   exceeds `max` are flagged.
//! - `count_comments` (default `false`) — when `true`, comment lines
//!   are included in the count, matching RuboCop's `CountComments: true`.
//! - `count_as_one` (default `[]`) — list of constructs to fold to 1
//!   line: `"array"`, `"hash"`, `"method_call"`. Matching RuboCop's
//!   `CountAsOne` option.
//!
//! Runtime option wiring goes through `cx.options_or_default`.
//!
//! ## No autocorrect
//!
//! Splitting an oversized example is a refactor that needs human
//! judgement (which assertions move, which setup belongs in
//! `before`); the cop reports and leaves the fix to the user.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

use super::helpers::is_example_call;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ExampleLength;

/// Cop options for [`ExampleLength`]. The schema is exported via
/// `#[derive(CopOptions)]` for the host's validation gate; runtime
/// option access goes through `cx.options_or_default`.
#[derive(CopOptions)]
pub struct ExampleLengthOptions {
    #[option(
        default = 5,
        description = "Maximum number of lines in an example body."
    )]
    pub max: i64,
    #[option(
        default = false,
        description = "Whether to count comment lines toward the example length."
    )]
    pub count_comments: bool,
    #[option(
        default = [],
        description = "Constructs to fold into one line: array, hash, method_call."
    )]
    pub count_as_one: Vec<String>,
}

#[cop(
    name = "RSpec/ExampleLength",
    description = "Caps the line count of an example body (it / specify / example).",
    default_severity = "warning",
    default_enabled = true,
    options = ExampleLengthOptions
)]
impl ExampleLength {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_example_call(cx, call) {
            return;
        }
        let Some(body_id) = body.get() else {
            return; // empty body — never long enough to exceed any max.
        };

        let opts = cx.options_or_default::<ExampleLengthOptions>();

        // Base line count: skip blank and (by default) comment lines.
        let body_src = cx.raw_source(cx.range(body_id));
        let mut line_count = count_lines(body_src, opts.count_comments);

        // CountAsOne: fold multi-line constructs to 1 line each.
        if !opts.count_as_one.is_empty() {
            line_count = apply_count_as_one(cx, body_id, &opts, line_count);
        }

        if line_count <= opts.max as usize {
            return;
        }

        cx.emit_offense(
            cx.range(node),
            &format!(
                "Example has too many lines. [{line_count}/{max}]",
                max = opts.max
            ),
            None,
        );
    }
}

/// Apply `CountAsOne` folding: for each outermost foldable descendant of
/// `body_id`, subtract `(subtree_lines - 1)` from `base`. Descendants of
/// an already-folded node are skipped (outermost-only semantics).
fn apply_count_as_one(
    cx: &Cx<'_>,
    body_id: NodeId,
    opts: &ExampleLengthOptions,
    base: usize,
) -> usize {
    let fold_array = opts.count_as_one.iter().any(|s| s == "array");
    let fold_hash = opts.count_as_one.iter().any(|s| s == "hash");
    let fold_method_call = opts.count_as_one.iter().any(|s| s == "method_call");

    let mut count = base as isize;
    // Track already-folded ranges so we skip nested descendants.
    let mut folded_ranges: Vec<Range> = Vec::new();

    let descendants = cx.descendants(body_id);
    for desc in descendants {
        let range = cx.range(desc);
        // Skip if this node is inside a previously folded subtree.
        if folded_ranges
            .iter()
            .any(|fr| range.start >= fr.start && range.end <= fr.end)
        {
            continue;
        }

        let should_fold = match cx.kind(desc) {
            NodeKind::Array(_) => fold_array,
            NodeKind::Hash(_) => fold_hash,
            NodeKind::Send { .. } | NodeKind::Csend { .. } => fold_method_call,
            _ => false,
        };

        if should_fold {
            let node_src = cx.raw_source(range);
            let node_lines = count_lines(node_src, opts.count_comments);
            if node_lines > 1 {
                count -= (node_lines as isize) - 1;
                folded_ranges.push(range);
            }
        }
    }

    count.max(0) as usize
}

/// Count logical source lines spanned by `text`:
/// - Skip lines that are empty or whitespace-only (blank lines).
/// - Skip lines whose first non-whitespace character is `#` (comment
///   lines), unless `count_comments` is `true`.
///
/// This matches RuboCop's `irrelevant_line` behavior.
fn count_lines(text: &str, count_comments: bool) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.split('\n')
        .filter(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return false; // blank line
            }
            if !count_comments && trimmed.starts_with('#') {
                return false; // comment line, skipped by default
            }
            true
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::{ExampleLength, ExampleLengthOptions, count_lines};
    use murphy_plugin_api::test_support::{indoc, run_cop, run_cop_with_options, test};

    /// `run_cop` only dispatches the one cop type so every emission is
    /// already a `RSpec/ExampleLength` offense.
    fn hits(source: &str) -> usize {
        run_cop::<ExampleLength>(source).len()
    }

    #[test]
    fn count_lines_handles_basic_shapes() {
        assert_eq!(count_lines("", false), 0);
        assert_eq!(count_lines("foo", false), 1);
        assert_eq!(count_lines("a; b", false), 1);
        assert_eq!(count_lines("a\nb", false), 2);
        assert_eq!(count_lines("a\nb\nc", false), 3);
        // Trailing newline: "a\n" splits to ["a",""] -- "" is blank, so 1.
        assert_eq!(count_lines("a\n", false), 1);
        // Blank lines are skipped.
        assert_eq!(count_lines("a\n\nb", false), 2);
        // Comment lines are skipped by default.
        assert_eq!(count_lines("a\n# comment\nb", false), 2);
        // Comment lines included when count_comments=true.
        assert_eq!(count_lines("a\n# comment\nb", true), 3);
    }

    #[test]
    fn flags_body_exceeding_default_max() {
        // 6-line body, default Max = 5 -- must emit exactly once.
        let src = indoc! {r#"
            it "works" do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
        "#};
        assert_eq!(hits(src), 1);
    }

    #[test]
    fn does_not_flag_body_at_default_max() {
        test::<ExampleLength>().expect_no_offenses(indoc! {r#"
                it "works" do
                  a = 1
                  b = 2
                  c = 3
                  d = 4
                  e = 5
                end
            "#});
    }

    #[test]
    fn handles_specify_and_example_aliases() {
        let src = indoc! {r#"
            specify "x" do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
            example "y" do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
        "#};
        assert_eq!(hits(src), 2);
    }

    #[test]
    fn ignores_non_example_blocks() {
        // `describe` is grouping scaffolding, not an example.
        let src = indoc! {r#"
            describe Widget do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn ignores_explicit_receiver_it_form() {
        // `Other.it "x" do ... end` -- non-bare receiver belongs to
        // some other DSL.
        let src = indoc! {r#"
            Other.it "x" do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn ignores_empty_body() {
        // `it "x" do end` -- body is None, never long enough to flag.
        let src = indoc! {r#"
            it "x" do
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn flags_brace_form_block() {
        // RSpec accepts `it { ... }` as well as `it do ... end`; both
        // parse to `NodeKind::Block`. Newlines inside the braces feed
        // the line count the same way.
        let src = indoc! {r#"
            it "works" {
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            }
        "#};
        assert_eq!(hits(src), 1);
    }

    // ------------------------------------------------------------------
    // Gap-fix tests: comment/blank line filtering + CountComments + CountAsOne
    // ------------------------------------------------------------------

    #[test]
    fn ignores_comment_lines_by_default() {
        // 5 code lines + 1 comment = 6 physical lines but only 5 logical
        // lines because comment lines are skipped by default.
        // With default Max=5 this must emit no offense.
        let src = indoc! {r#"
            it "works" do
              a = 1
              b = 2
              c = 3
              # this is a comment
              d = 4
              e = 5
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn ignores_blank_lines_by_default() {
        // 5 code lines + 1 blank = 6 physical lines but only 5 logical
        // lines because blank lines are skipped by default.
        // With default Max=5 this must emit no offense.
        let src = indoc! {r#"
            it "works" do
              a = 1
              b = 2
              c = 3

              d = 4
              e = 5
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn count_comments_true_includes_comment_lines() {
        // 3 code lines + 1 comment = 4 logical lines when CountComments: true.
        // Max=3 -> offense.
        let src = indoc! {r#"
            it "works" do
              a = 1
              b = 2
              # comment
              c = 3
            end
        "#};
        let opts = ExampleLengthOptions {
            max: 3,
            count_comments: true,
            count_as_one: vec![],
        };
        let offenses = run_cop_with_options::<ExampleLength>(src, &opts);
        assert_eq!(
            offenses.len(),
            1,
            "CountComments:true should count comment lines"
        );
        // Message format must be "Example has too many lines. [4/3]"
        assert_eq!(offenses[0].message, "Example has too many lines. [4/3]");
    }

    #[test]
    fn count_comments_false_excludes_comment_lines() {
        // Same snippet as above but CountComments: false -- 3 code lines
        // <= Max=3 -> no offense.
        let src = indoc! {r#"
            it "works" do
              a = 1
              b = 2
              # comment
              c = 3
            end
        "#};
        let opts = ExampleLengthOptions {
            max: 3,
            count_comments: false,
            count_as_one: vec![],
        };
        let offenses = run_cop_with_options::<ExampleLength>(src, &opts);
        assert_eq!(offenses.len(), 0);
    }

    #[test]
    fn message_format_uses_period_and_brackets() {
        // RuboCop emits "Example has too many lines. [N/M]" -- verify the
        // exact format when an offense fires.
        let src = indoc! {r#"
            it "works" do
              a = 1
              b = 2
              c = 3
              d = 4
              e = 5
              f = 6
            end
        "#};
        let offenses = run_cop::<ExampleLength>(src);
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Example has too many lines. [6/5]");
    }

    #[test]
    fn count_as_one_folds_array_into_one_line() {
        // Multi-line array folded to 1 line: a=1 (1) + array (folded->1) = 2 lines.
        // Max=3 -> no offense.
        let src = indoc! {r#"
            it "works" do
              a = 1
              a = [
                2,
                3,
                4,
                5,
                6,
                7,
                8,
                9
              ]
            end
        "#};
        let opts = ExampleLengthOptions {
            max: 3,
            count_comments: false,
            count_as_one: vec!["array".to_string()],
        };
        let offenses = run_cop_with_options::<ExampleLength>(src, &opts);
        assert_eq!(
            offenses.len(),
            0,
            "CountAsOne:array should fold the multi-line array to 1 line"
        );
    }

    #[test]
    fn count_as_one_folds_hash_into_one_line() {
        // Multi-line hash folded to 1 line: a=1 (1) + hash (folded->1) = 2 lines.
        // Max=3 -> no offense.
        let src = indoc! {r#"
            it "works" do
              a = 1
              b = {
                c: 2,
                d: 3,
                e: 4,
                f: 5
              }
            end
        "#};
        let opts = ExampleLengthOptions {
            max: 3,
            count_comments: false,
            count_as_one: vec!["hash".to_string()],
        };
        let offenses = run_cop_with_options::<ExampleLength>(src, &opts);
        assert_eq!(
            offenses.len(),
            0,
            "CountAsOne:hash should fold the multi-line hash to 1 line"
        );
    }
}
