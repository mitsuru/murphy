//! `Layout/SpaceInsideBlockBraces` — checks that block braces `{ }` have (or
//! don't have) surrounding space, with separate handling for empty braces and
//! the space before block parameters.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceInsideBlockBraces
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Node-driven (on_block / on_numblock / on_itblock) like RuboCop. do/end
//!   blocks are skipped via a brace-opener check (RuboCop's `node.keywords?`).
//!   Mirrors `check_inside`'s three disjoint branches: adjacent `{}`,
//!   whitespace-only `{  }` / `{\n}`, and braces-with-contents. The args
//!   delimiter (`|`) is located by token scan rather than a node loc, which is
//!   robust across the `{|x|` / `{ |x|` spellings. Supports EnforcedStyle
//!   space(default)/no_space, EnforcedStyleForEmptyBraces no_space(default)/
//!   space, and SpaceBeforeBlockParameters (default true).
//! ```

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, Range, SourceToken, SourceTokenKind, cop,
};

#[derive(Default)]
pub struct SpaceInsideBlockBraces;

#[derive(CopOptions)]
pub struct SpaceInsideBlockBracesOptions {
    #[option(
        name = "EnforcedStyle",
        default = "space",
        description = "Block brace spacing style."
    )]
    pub enforced_style: BlockBraceStyle,
    #[option(
        name = "EnforcedStyleForEmptyBraces",
        default = "no_space",
        description = "Spacing style for empty block braces."
    )]
    pub empty_style: EmptyBlockBraceStyle,
    #[option(
        name = "SpaceBeforeBlockParameters",
        default = true,
        description = "Require a space between { and a block parameter pipe."
    )]
    pub space_before_block_parameters: bool,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum BlockBraceStyle {
    #[option(value = "space")]
    Space,
    #[option(value = "no_space")]
    NoSpace,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum EmptyBlockBraceStyle {
    #[option(value = "no_space")]
    NoSpace,
    #[option(value = "space")]
    Space,
}

#[cop(
    name = "Layout/SpaceInsideBlockBraces",
    description = "Check spacing inside block braces.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceInsideBlockBracesOptions,
)]
impl SpaceInsideBlockBraces {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>, options: &SpaceInsideBlockBracesOptions) {
        check(node, cx, options);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>, options: &SpaceInsideBlockBracesOptions) {
        check(node, cx, options);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>, options: &SpaceInsideBlockBracesOptions) {
        check(node, cx, options);
    }
}

fn check(node: NodeId, cx: &Cx<'_>, options: &SpaceInsideBlockBracesOptions) {
    let Some((left_brace, right_brace)) = brace_tokens(node, cx) else {
        return; // do/end block, or no braces found
    };

    check_inside(cx, options, node, left_brace, right_brace);
}

/// Locate the block's opening `{` and closing `}` tokens. Returns `None` for a
/// `do`/`end` block (the opener is the `do` keyword, not a brace).
fn brace_tokens(node: NodeId, cx: &Cx<'_>) -> Option<(SourceToken, SourceToken)> {
    let node_range = cx.range(node);

    let toks = cx.sorted_tokens();

    // Closer: the last `}` token within the node range. A do/end block has none
    // from the block itself (any `}` would be an inner hash); the backward
    // opener scan below distinguishes the two by hitting `do` first.
    let node_tokens = cx.tokens_in(node_range);
    let right_brace = *node_tokens
        .iter()
        .rev()
        .find(|t| t.kind == SourceTokenKind::RightBrace)?;

    // Scope-aware opener: the block's OWN opener (`{` or `do`) sits immediately
    // before its args/body. Scan BACKWARD from the body — or, for a bodyless
    // block, from the closing brace — skipping any balanced `{ … }` pair on the
    // way: a hash-literal argument (`bar({}) {`) or a brace inside a parameter
    // default (`foo { |opts = {}| opts }`). Only the block's own opener is then
    // matched. RuboCop reads `node.loc.begin` directly; this reconstructs the
    // same opener positionally. A `{` at brace-depth 0 is the opener; a `do` at
    // depth 0 means a do/end block and the cop does not apply.
    //
    // `body_start` is the scan boundary whenever the block has a real body or
    // args (always *before* the closer). Only a bodyless, argless block makes
    // `body_start` fall back to the node end (past the closing `}`); there we
    // scan from before the closer so an empty brace block (`{}`, `{  }`) still
    // finds its opener. Crucially we do NOT cap at `right_brace` in the
    // body/args case: for a do/end block with a brace argument
    // (`foo({a: 1}) do … end`) the "last `}`" is that hash argument, which sits
    // *before* `do` — capping there would skip the `do` and misread the hash as
    // the block's braces.
    let body = body_start(node, cx);
    let scan_from = if body >= node_range.end {
        right_brace.range.start
    } else {
        body
    };
    let idx = toks.partition_point(|t| t.range.start < scan_from);
    let mut depth: u32 = 0;
    let mut opener: Option<SourceToken> = None;
    for t in toks[..idx]
        .iter()
        .rev()
        .take_while(|t| t.range.start >= node_range.start)
    {
        match t.kind {
            SourceTokenKind::RightBrace => depth += 1,
            SourceTokenKind::LeftBrace if depth > 0 => depth -= 1,
            SourceTokenKind::LeftBrace => {
                opener = Some(*t);
                break;
            }
            SourceTokenKind::Other if depth == 0 && cx.raw_source(t.range) == "do" => {
                return None; // do/end block
            }
            _ => {}
        }
    }
    let left_brace = opener?;

    if right_brace.range.start < left_brace.range.end {
        return None;
    }
    Some((left_brace, right_brace))
}

/// First byte offset of the block body (or the closing brace if there is no
/// body), used as the scan boundary for the opener / args-delimiter search.
fn body_start(node: NodeId, cx: &Cx<'_>) -> u32 {
    if let Some(body) = cx.block_body(node).get() {
        return cx.range(body).start;
    }
    if let Some(args) = cx.block_arguments(node).get() {
        let r = cx.range(args);
        if r.end > r.start {
            return r.end;
        }
    }
    cx.range(node).end
}

fn check_inside(
    cx: &Cx<'_>,
    options: &SpaceInsideBlockBracesOptions,
    node: NodeId,
    left_brace: SourceToken,
    right_brace: SourceToken,
) {
    if left_brace.range.end == right_brace.range.start {
        // Branch 1: adjacent braces `{}`.
        adjacent_braces(cx, options, left_brace, right_brace);
        return;
    }

    let inner_range = Range {
        start: left_brace.range.end,
        end: right_brace.range.start,
    };
    let inner = cx.raw_source(inner_range);
    if inner.bytes().any(|b| !b.is_ascii_whitespace()) {
        // Branch 2: braces with contents.
        braces_with_contents_inside(cx, options, node, left_brace, right_brace, inner);
    } else if options.empty_style == EmptyBlockBraceStyle::NoSpace {
        // Branch 3: whitespace-only interior (`{  }`, `{\n}`).
        cx.emit_offense(inner_range, "Space inside empty braces detected.", None);
        cx.emit_edit(inner_range, "");
    }
}

fn adjacent_braces(
    cx: &Cx<'_>,
    options: &SpaceInsideBlockBracesOptions,
    left_brace: SourceToken,
    right_brace: SourceToken,
) {
    if options.empty_style != EmptyBlockBraceStyle::Space {
        return;
    }
    let range = Range {
        start: left_brace.range.start,
        end: right_brace.range.end,
    };
    cx.emit_offense(range, "Space missing inside empty braces.", None);
    cx.emit_edit(range, "{ }");
}

fn braces_with_contents_inside(
    cx: &Cx<'_>,
    options: &SpaceInsideBlockBracesOptions,
    node: NodeId,
    left_brace: SourceToken,
    right_brace: SourceToken,
    inner: &str,
) {
    let args_pipe = args_delimiter(cx, node, left_brace, right_brace);
    check_left_brace(cx, options, inner, left_brace, args_pipe);
    check_right_brace(cx, options, left_brace, right_brace, inner);
}

/// The opening `|` of a block parameter list, if present immediately after the
/// `{` (allowing whitespace). `None` when the block takes no `|...|` params.
fn args_delimiter(
    cx: &Cx<'_>,
    node: NodeId,
    left_brace: SourceToken,
    right_brace: SourceToken,
) -> Option<SourceToken> {
    let body = body_start(node, cx);
    let toks = cx.tokens_in(Range {
        start: left_brace.range.end,
        end: right_brace.range.start,
    });
    let limit = body.min(right_brace.range.start);
    toks.iter()
        .take_while(|t| t.range.start < limit)
        .find(|t| !matches!(t.kind, SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline))
        .filter(|t| cx.raw_source(t.range) == "|")
        .copied()
}

fn check_left_brace(
    cx: &Cx<'_>,
    options: &SpaceInsideBlockBracesOptions,
    inner: &str,
    left_brace: SourceToken,
    args_pipe: Option<SourceToken>,
) {
    if inner.starts_with(|c: char| !c.is_whitespace()) {
        no_space_inside_left_brace(cx, options, left_brace, args_pipe);
    } else {
        space_inside_left_brace(cx, options, left_brace, args_pipe);
    }
}

fn no_space_inside_left_brace(
    cx: &Cx<'_>,
    options: &SpaceInsideBlockBracesOptions,
    left_brace: SourceToken,
    args_pipe: Option<SourceToken>,
) {
    if let Some(pipe) = args_pipe {
        // `{|x|` — pipe directly follows `{`.
        if left_brace.range.end == pipe.range.start && options.space_before_block_parameters {
            let range = Range {
                start: left_brace.range.start,
                end: pipe.range.end,
            };
            cx.emit_offense(range, "Space between { and | missing.", None);
            cx.emit_edit(
                Range {
                    start: left_brace.range.end,
                    end: left_brace.range.end,
                },
                " ",
            );
        }
        // else: correct.
    } else {
        // `{x` — content directly follows `{`.
        no_space(
            cx,
            options,
            Range {
                start: left_brace.range.end,
                end: left_brace.range.end,
            },
            "Space missing inside {.",
            " ",
            false,
        );
    }
}

fn space_inside_left_brace(
    cx: &Cx<'_>,
    options: &SpaceInsideBlockBracesOptions,
    left_brace: SourceToken,
    args_pipe: Option<SourceToken>,
) {
    if let Some(pipe) = args_pipe {
        // `{ |x|` — space between `{` and `|`.
        if !options.space_before_block_parameters {
            let range = Range {
                start: left_brace.range.end,
                end: pipe.range.start,
            };
            // Only correct a horizontal gap: if the `{` and `|` are on
            // different lines (multiline block), deleting the range would
            // collapse the newline and join the lines, which is destructive.
            if !cx.raw_source(range).bytes().any(|b| b == b'\n') {
                cx.emit_offense(range, "Space between { and | detected.", None);
                cx.emit_edit(range, "");
            }
        }
        // else: correct.
    } else {
        // `{ x` — space between `{` and content. Offense only under no_space.
        let range = space_after_left_brace(cx, left_brace);
        space(cx, options, range, "Space inside { detected.");
    }
}

fn check_right_brace(
    cx: &Cx<'_>,
    options: &SpaceInsideBlockBracesOptions,
    left_brace: SourceToken,
    right_brace: SourceToken,
    inner: &str,
) {
    let single_line = is_single_line(cx, left_brace, right_brace);
    if single_line && inner.ends_with(|c: char| !c.is_whitespace()) {
        // `x}` — content directly before `}`.
        no_space(
            cx,
            options,
            Range {
                start: right_brace.range.start,
                end: right_brace.range.start,
            },
            "Space missing inside }.",
            " ",
            true,
        );
    } else if single_line {
        // `x }` — space before `}` on a single line. Offense only under no_space.
        let range = space_before_right_brace(cx, right_brace);
        space(cx, options, range, "Space inside } detected.");
    }
    // Multiline blocks: the right-brace indentation is owned by other cops.
}

/// `no_space`: an offense only when the configured style is `space` (a space
/// is required and missing).
fn no_space(
    cx: &Cx<'_>,
    options: &SpaceInsideBlockBracesOptions,
    range: Range,
    msg: &str,
    insert: &str,
    insert_before: bool,
) {
    if options.enforced_style == BlockBraceStyle::Space {
        cx.emit_offense(range, msg, None);
        let edit_range = if insert_before {
            Range {
                start: range.start,
                end: range.start,
            }
        } else {
            Range {
                start: range.end,
                end: range.end,
            }
        };
        cx.emit_edit(edit_range, insert);
    }
}

/// `space`: an offense only when the configured style is `no_space` (a space is
/// present and unwanted).
fn space(cx: &Cx<'_>, options: &SpaceInsideBlockBracesOptions, range: Range, msg: &str) {
    if options.enforced_style == BlockBraceStyle::NoSpace {
        if range.start >= range.end {
            return;
        }
        cx.emit_offense(range, msg, None);
        cx.emit_edit(range, "");
    }
}

fn space_after_left_brace(cx: &Cx<'_>, left_brace: SourceToken) -> Range {
    let src = cx.source().as_bytes();
    let mut end = left_brace.range.end as usize;
    while src.get(end).is_some_and(|&b| b == b' ' || b == b'\t') {
        end += 1;
    }
    Range {
        start: left_brace.range.end,
        end: end as u32,
    }
}

fn space_before_right_brace(cx: &Cx<'_>, right_brace: SourceToken) -> Range {
    let src = cx.source().as_bytes();
    let mut start = right_brace.range.start as usize;
    while start > 0 && src.get(start - 1).is_some_and(|&b| b == b' ' || b == b'\t') {
        start -= 1;
    }
    Range {
        start: start as u32,
        end: right_brace.range.start,
    }
}

fn is_single_line(cx: &Cx<'_>, left_brace: SourceToken, right_brace: SourceToken) -> bool {
    let between = cx.raw_source(Range {
        start: left_brace.range.start,
        end: right_brace.range.end,
    });
    !between.bytes().any(|b| b == b'\n')
}

#[cfg(test)]
mod tests {
    use super::{
        BlockBraceStyle, EmptyBlockBraceStyle, SpaceInsideBlockBraces, SpaceInsideBlockBracesOptions,
    };
    use murphy_plugin_api::test_support::{indoc, run_cop_with_edits, run_cop_with_options_and_edits, test};

    // ── default (space) style ───────────────────────────────────────────────

    #[test]
    fn space_style_accepts_spaced_block() {
        test::<SpaceInsideBlockBraces>().expect_no_offenses("foo { bar }\n");
    }

    #[test]
    fn space_style_flags_missing_left_space() {
        let result = run_cop_with_edits::<SpaceInsideBlockBraces>("foo {bar }\n");
        assert_eq!(result.offenses.len(), 1, "offenses: {:?}", result.offenses);
        assert_eq!(result.offenses[0].message, "Space missing inside {.");
        assert_eq!(result.edits.len(), 1);
        assert_eq!(result.edits[0].replacement, " ");
    }

    #[test]
    fn space_style_flags_missing_right_space() {
        // `no_space` path emits a zero-length insert range; verify via edits.
        let result = run_cop_with_edits::<SpaceInsideBlockBraces>("foo { bar}\n");
        assert_eq!(result.offenses.len(), 1, "offenses: {:?}", result.offenses);
        assert_eq!(result.offenses[0].message, "Space missing inside }.");
        assert_eq!(result.edits.len(), 1);
        assert_eq!(result.edits[0].replacement, " ");
    }

    #[test]
    fn space_style_flags_missing_both() {
        // Both inserts are zero-length ranges; verify the fixpoint correction.
        let result = run_cop_with_edits::<SpaceInsideBlockBraces>("foo {bar}\n");
        assert_eq!(result.offenses.len(), 2, "offenses: {:?}", result.offenses);
        assert!(
            result
                .offenses
                .iter()
                .any(|o| o.message == "Space missing inside {."),
            "offenses: {:?}",
            result.offenses
        );
        assert!(
            result
                .offenses
                .iter()
                .any(|o| o.message == "Space missing inside }."),
            "offenses: {:?}",
            result.offenses
        );
    }

    // ── empty braces ────────────────────────────────────────────────────────

    #[test]
    fn default_accepts_tight_empty_braces() {
        test::<SpaceInsideBlockBraces>().expect_no_offenses("foo {}\n");
    }

    #[test]
    fn default_flags_whitespace_only_empty_braces() {
        test::<SpaceInsideBlockBraces>().expect_correction(
            indoc! {r#"
                foo {  }
                     ^^ Space inside empty braces detected.
            "#},
            "foo {}\n",
        );
    }

    #[test]
    fn default_flags_multiline_whitespace_only_empty_braces() {
        // Branch 3 must catch `{\n}` too — newlines are tokens in Murphy.
        let result = run_cop_with_edits::<SpaceInsideBlockBraces>("foo {\n}\n");
        assert_eq!(result.offenses.len(), 1, "offenses: {:?}", result.offenses);
        assert_eq!(result.offenses[0].message, "Space inside empty braces detected.");
    }

    #[test]
    fn empty_space_style_flags_tight_empty_braces() {
        let opts = SpaceInsideBlockBracesOptions {
            enforced_style: BlockBraceStyle::Space,
            empty_style: EmptyBlockBraceStyle::Space,
            space_before_block_parameters: true,
        };
        test::<SpaceInsideBlockBraces>()
            .with_options(&opts)
            .expect_correction(
                indoc! {r#"
                    foo {}
                        ^^ Space missing inside empty braces.
                "#},
                "foo { }\n",
            );
    }

    // ── block parameters: SpaceBeforeBlockParameters = true (default) ────────

    #[test]
    fn default_accepts_space_before_pipe() {
        test::<SpaceInsideBlockBraces>().expect_no_offenses("foo { |x| bar(x) }\n");
    }

    #[test]
    fn default_flags_missing_space_before_pipe() {
        let result = run_cop_with_edits::<SpaceInsideBlockBraces>("foo {|x| bar(x) }\n");
        assert_eq!(result.offenses.len(), 1, "offenses: {:?}", result.offenses);
        assert_eq!(result.offenses[0].message, "Space between { and | missing.");
        assert_eq!(result.edits.len(), 1);
        assert_eq!(result.edits[0].replacement, " ");
    }

    // ── block parameters: SpaceBeforeBlockParameters = false ────────────────

    #[test]
    fn no_space_before_params_flags_space_before_pipe() {
        let opts = SpaceInsideBlockBracesOptions {
            enforced_style: BlockBraceStyle::Space,
            empty_style: EmptyBlockBraceStyle::NoSpace,
            space_before_block_parameters: false,
        };
        test::<SpaceInsideBlockBraces>()
            .with_options(&opts)
            .expect_correction(
                indoc! {r#"
                    foo { |x| bar(x) }
                         ^ Space between { and | detected.
                "#},
                "foo {|x| bar(x) }\n",
            );
    }

    #[test]
    fn no_space_before_params_does_not_collapse_multiline_pipe() {
        // The `|` is on a new line — deleting the gap would join the lines.
        // Must not emit a destructive correction.
        let opts = SpaceInsideBlockBracesOptions {
            enforced_style: BlockBraceStyle::Space,
            empty_style: EmptyBlockBraceStyle::NoSpace,
            space_before_block_parameters: false,
        };
        test::<SpaceInsideBlockBraces>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                foo {
                  |x| bar(x)
                }
            "#});
    }

    #[test]
    fn no_space_before_params_accepts_tight_pipe() {
        let opts = SpaceInsideBlockBracesOptions {
            enforced_style: BlockBraceStyle::Space,
            empty_style: EmptyBlockBraceStyle::NoSpace,
            space_before_block_parameters: false,
        };
        test::<SpaceInsideBlockBraces>()
            .with_options(&opts)
            .expect_no_offenses("foo {|x| bar(x) }\n");
    }

    // ── no_space style ──────────────────────────────────────────────────────

    #[test]
    fn no_space_style_flags_inner_spaces() {
        let opts = SpaceInsideBlockBracesOptions {
            enforced_style: BlockBraceStyle::NoSpace,
            empty_style: EmptyBlockBraceStyle::NoSpace,
            space_before_block_parameters: true,
        };
        let result = run_cop_with_options_and_edits::<SpaceInsideBlockBraces>("foo { bar }\n", &opts);
        assert_eq!(result.offenses.len(), 2, "offenses: {:?}", result.offenses);
    }

    #[test]
    fn no_space_style_accepts_tight_block() {
        let opts = SpaceInsideBlockBracesOptions {
            enforced_style: BlockBraceStyle::NoSpace,
            empty_style: EmptyBlockBraceStyle::NoSpace,
            space_before_block_parameters: true,
        };
        test::<SpaceInsideBlockBraces>()
            .with_options(&opts)
            .expect_no_offenses("foo {bar}\n");
    }

    // ── cross-cop: must NOT fire on do/end blocks or hashes ─────────────────

    #[test]
    fn does_not_flag_do_end_block() {
        test::<SpaceInsideBlockBraces>().expect_no_offenses(indoc! {r#"
            foo do |x|
              bar(x)
            end
        "#});
    }

    #[test]
    fn does_not_flag_hash_literal() {
        // A hash literal `{ a: 1 }` is not a block; this cop must ignore it.
        test::<SpaceInsideBlockBraces>().expect_no_offenses("h = { a: 1 }\n");
    }

    #[test]
    fn accepts_multiline_block() {
        test::<SpaceInsideBlockBraces>().expect_no_offenses(indoc! {r#"
            foo {
              bar
            }
        "#});
    }

    /// Mastodon FP: a `do/end` block whose receiver chain contains a nested
    /// brace block and a hash-literal argument. The outer block opener is `do`,
    /// so the cop must return None. The old opener scan started at the node
    /// range and picked the nested `flat_map` block's `{`; the closer scan
    /// picked the `each_with_object({})` hash arg's `}`, producing a bogus
    /// "Space missing inside }". Scanning the opener from the *call node's end*
    /// finds the block's own `do` first and bails out.
    #[test]
    fn does_not_flag_do_end_block_with_nested_brace_and_hash_arg() {
        test::<SpaceInsideBlockBraces>().expect_no_offenses(indoc! {r#"
            SOURCES.flat_map { |k| k }.each_with_object({}) do |a, h|
              h
            end
        "#});
    }

    #[test]
    fn does_not_flag_numblock_with_inner_hash_braces() {
        // A numbered-parameter block whose body is a hash literal
        // (`items.map { {value: _1} }`). The block's own braces are spaced
        // correctly; the inner hash `{` must NOT be mistaken for the block opener
        // (it would otherwise be flagged "Space missing inside {").
        test::<SpaceInsideBlockBraces>().expect_no_offenses("items.map { {value: _1} }\n");
    }

    #[test]
    fn does_not_flag_block_param_default_with_braces() {
        // A brace block with a parameter whose default is a hash literal
        // (`foo { |opts = {}| opts }`). The block's own braces are spaced
        // correctly; the inner default-hash `{` must NOT be mistaken for the
        // block opener (it would otherwise be flagged "Space missing inside {").
        test::<SpaceInsideBlockBraces>().expect_no_offenses("foo { |opts = {}| opts }\n");
    }

    #[test]
    fn flags_own_brace_spacing_with_param_default_braces() {
        // The block's own braces ARE still checked even when a parameter default
        // contains braces — confirms the opener is located before the params.
        let opts = SpaceInsideBlockBracesOptions {
            enforced_style: BlockBraceStyle::Space,
            empty_style: EmptyBlockBraceStyle::NoSpace,
            space_before_block_parameters: true,
        };
        let result =
            run_cop_with_options_and_edits::<SpaceInsideBlockBraces>("foo {|opts = {}| opts }\n", &opts);
        assert_eq!(result.offenses.len(), 1, "offenses: {:?}", result.offenses);
    }

    #[test]
    fn does_not_flag_do_end_block_with_tight_hash_argument() {
        // A do/end block whose receiver call has a (tight) hash-literal argument
        // before `do` (`foo({a: 1}) do |x| x end`). The hash `{a: 1}` is a call
        // argument, NOT the block's braces — the block opens with `do`, so the
        // cop must not apply. Regression: the "last `}`" closer is the hash's,
        // and the opener scan must still reach `do` rather than pairing the hash
        // braces.
        test::<SpaceInsideBlockBraces>().expect_no_offenses("foo({a: 1}) do |x|\n  x\nend\n");
    }

    #[test]
    fn flags_numblock_own_brace_spacing() {
        // The block's own braces ARE still checked on a numblock (`{_1}` is
        // missing the inner spaces), confirming the opener is located correctly.
        let opts = SpaceInsideBlockBracesOptions {
            enforced_style: BlockBraceStyle::Space,
            empty_style: EmptyBlockBraceStyle::NoSpace,
            space_before_block_parameters: true,
        };
        let result = run_cop_with_options_and_edits::<SpaceInsideBlockBraces>("items.map {_1}\n", &opts);
        assert_eq!(result.offenses.len(), 2, "offenses: {:?}", result.offenses);
    }
}
murphy_plugin_api::submit_cop!(SpaceInsideBlockBraces);
