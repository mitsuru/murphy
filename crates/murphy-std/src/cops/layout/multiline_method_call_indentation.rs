//! `Layout/MultilineMethodCallIndentation` — the method-name part of a method
//! call that spans more than one line must be indented consistently.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineMethodCallIndentation
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports the default `EnforcedStyle: aligned` semantic-alignment case and the
//!   common leading-dot call-chain cases for `indented` and
//!   `indented_relative_to_receiver`. Aligned continuations match the first
//!   dotted call's column. `indented` continuations use the chain's line
//!   indentation plus `IndentationWidth`; `indented_relative_to_receiver`
//!   continuations use the first receiver's column plus that width. A cop-level
//!   `IndentationWidth` overrides the resolved `Layout/IndentationWidth.Width`;
//!   a null/unset value falls back to the resolved shared width. Relative style
//!   accounts for `*` and `**` receiver wrappers.
//!
//!   The parenthesized-call argument guard (`expect(foo.bar\n.baz)`) is
//!   conservatively applied to all styles. RuboCop may use a regular-indentation
//!   fallback for this shape; Murphy skips it. The aligned semantic path also
//!   preserves these RuboCop guards:
//!
//!   - `first_call_alignment_node`: the anchor dot must sit on the chain
//!     expression's first line. `obj\n.foo\n.bar` therefore has no aligned
//!     semantic base and is skipped by the aligned style.
//!   - A chain whose base receiver is a parenthesized / `begin...end`
//!     expression (`(a || b).foo\n.bar`) has no aligned semantic base. Murphy
//!     over-skips any `Begin` base receiver for the aligned style.
//!
//!   Gaps (documented, not covered):
//!   - The styles are currently enforced for leading-dot call chains with a
//!     first dotted receiver. RuboCop's trailing-dot, selector-only, syntactic
//!     alignment, and general no-base indentation paths are not ported.
//!   - Hash-pair alignment, multiline block-chain anchors, `get_dot_right_above`,
//!     and the receiver-last-line `begin`/`array` cases.
//!   - Other grouped-expression contexts handled by `not_for_this_cop?`.
//!   - Autocorrect (RuboCop realigns via `AlignmentCorrector`).
//! ```
//!
//! ## Matched shapes
//!
//! `send`/`csend` nodes whose leading-dot selector begins its own line and is
//! misindented relative to the selected `EnforcedStyle`.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineMethodCallIndentation;

/// Options for [`MultilineMethodCallIndentation`]. `EnforcedStyle` matches
/// RuboCop verbatim; the default is `aligned`. `IndentationWidth` overrides the
/// shared `Layout/IndentationWidth.Width` for the indented styles.
#[derive(CopOptions)]
pub struct MultilineMethodCallIndentationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "aligned",
        description = "How the method-name part of a multi-line method call is indented."
    )]
    pub enforced_style: IndentationStyle,
    #[option(
        name = "IndentationWidth",
        description = "Indentation width in spaces (null/unset uses Layout/IndentationWidth.Width)."
    )]
    pub indentation_width: Option<i64>,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum IndentationStyle {
    /// Selectors align with the dot of the first call in the chain.
    #[option(value = "aligned")]
    Aligned,
    /// Selectors use standard indentation relative to the receiver line.
    #[option(value = "indented")]
    Indented,
    /// Selectors indent `IndentationWidth` spaces beyond the receiver.
    #[option(value = "indented_relative_to_receiver")]
    IndentedRelativeToReceiver,
}

#[cop(
    name = "Layout/MultilineMethodCallIndentation",
    description = "Enforce consistent indentation of multi-line method-call selectors.",
    default_severity = "warning",
    default_enabled = true,
    options = MultilineMethodCallIndentationOptions,
)]
impl MultilineMethodCallIndentation {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// 0-based column of a byte offset, counting characters (not bytes) from the
/// start of its physical line.
fn column_of(offset: u32, src: &str) -> usize {
    let bytes = src.as_bytes();
    let line_start = bytes[..offset as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    src[line_start..offset as usize].chars().count()
}

/// Number of leading indentation columns on the line containing `offset`.
fn indentation_of_line(offset: u32, src: &str) -> usize {
    let bytes = src.as_bytes();
    let line_start = bytes[..offset as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    src[line_start..offset as usize]
        .chars()
        .take_while(|&ch| ch == ' ' || ch == '\t')
        .count()
}

/// 1-based line of a byte offset. Used only for the offense message (cold
/// path); same-line tests use [`spans_newline`] to stay O(span).
fn line_of(offset: u32, src: &str) -> usize {
    1 + src.as_bytes()[..offset as usize]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

/// Whether the source between two byte offsets contains a newline — an
/// O(span) same-line check that avoids scanning from the file start.
fn spans_newline(src: &str, start: u32, end: u32) -> bool {
    start < end && src.as_bytes()[start as usize..end as usize].contains(&b'\n')
}

/// Whether the byte offset is the first non-whitespace byte of its physical
/// line — RuboCop's `begins_its_line?`.
fn begins_its_line(offset: u32, src: &str) -> bool {
    let bytes = src.as_bytes();
    let line_start = bytes[..offset as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    bytes[line_start..offset as usize]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<MultilineMethodCallIndentationOptions>();
    let style = opts.enforced_style;
    let indentation_width = opts
        .indentation_width
        .unwrap_or(cx.indentation_width())
        .max(0) as usize;

    // `relevant_node?` — only method calls with an explicit dot operator.
    let dot = cx.loc(node).dot();
    if dot == Range::ZERO {
        return;
    }
    // The selector (method name) range.
    let selector = cx.loc(node).name;
    if selector == Range::ZERO {
        return;
    }

    let src = cx.source();

    // `right_hand_side` — for the leading-dot shapes supported here, the RHS
    // spans the dot through the selector and must begin its own line.
    let rhs_start = dot.start;
    if !begins_its_line(rhs_start, src) {
        return;
    }

    // `semantic_alignment_node`: skip a chain that is an argument inside a
    // parenthesized call.
    if is_arg_of_parenthesized_call(node, cx) {
        return;
    }

    // Locate the first dotted call in the receiver chain. `base` is the
    // receiver before that first call and anchors the indented styles.
    let Some((anchor, base)) = first_dotted_call_in_chain(node, cx) else {
        return;
    };
    let base_range = cx.range(base);
    let actual_column = column_of(dot.start, src);
    let rhs_range = Range {
        start: rhs_start,
        end: selector.end,
    };
    let rhs_src = cx.raw_source(rhs_range);

    let (expected_column, message) = match style {
        IndentationStyle::Aligned => {
            // The aligned style requires a distinct, first-line dotted anchor.
            if anchor == node || matches!(cx.kind(base), NodeKind::Begin(_)) {
                return;
            }
            let anchor_dot = cx.loc(anchor).dot();
            if anchor_dot == Range::ZERO {
                return;
            }
            let anchor_expr_start = cx.range(anchor).start;
            if spans_newline(src, anchor_expr_start, anchor_dot.start) {
                return;
            }

            let anchor_selector = cx.loc(anchor).name;
            let anchor_base_range = Range {
                start: anchor_dot.start,
                end: anchor_selector.end.max(anchor_dot.end),
            };
            let anchor_base_src = cx.raw_source(anchor_base_range);
            let anchor_base_src = anchor_base_src.split('\n').next().unwrap_or(anchor_base_src);
            let base_line = line_of(anchor_dot.start, src);
            (
                column_of(anchor_dot.start, src),
                format!("Align `{rhs_src}` with `{anchor_base_src}` on line {base_line}."),
            )
        }
        IndentationStyle::Indented => {
            let line_indent = indentation_of_line(base_range.start, src);
            let used_indentation = actual_column as isize - line_indent as isize;
            (
                line_indent + indentation_width,
                format!(
                    "Use {indentation_width} (not {used_indentation}) spaces for indenting an expression spanning multiple lines."
                ),
            )
        }
        IndentationStyle::IndentedRelativeToReceiver => {
            let splat_operator_width = cx.parent(node).get().map_or(0, |parent| {
                match cx.kind(parent) {
                    NodeKind::Splat(_) => 1,
                    NodeKind::Kwsplat(_) => 2,
                    _ => 0,
                }
            });
            let extra_width = indentation_width.saturating_sub(splat_operator_width);
            let base_src = cx.raw_source(base_range);
            let base_src = base_src.split('\n').next().unwrap_or(base_src);
            let base_line = line_of(base_range.start, src);
            (
                column_of(base_range.start, src) + extra_width,
                format!(
                    "Indent `{rhs_src}` {indentation_width} spaces more than `{base_src}` on line {base_line}."
                ),
            )
        }
    };

    if actual_column == expected_column {
        return;
    }
    cx.emit_offense(rhs_range, &message, None);
}

/// Walk down the receiver chain to the base receiver, then up to the first
/// call node that carries a dot operator — RuboCop's `first_call_has_a_dot`.
/// Returns `(anchor, base_receiver)`.
fn first_dotted_call_in_chain(node: NodeId, cx: &Cx<'_>) -> Option<(NodeId, NodeId)> {
    // `find_base_receiver`: descend receivers to the bottom.
    let mut base = node;
    while let Some(recv) = cx.call_receiver(base).get() {
        base = recv;
    }
    // `node = base.parent; node = node.parent until node.loc?(:dot)`.
    let mut current = cx.parent(base).get()?;
    loop {
        if cx.loc(current).dot() != Range::ZERO {
            return Some((current, base));
        }
        current = cx.parent(current).get()?;
    }
}

/// Whether `node`'s chain top is an argument of a parenthesized call —
/// RuboCop's `argument_in_method_call(node, :with_parentheses)`.
fn is_arg_of_parenthesized_call(node: NodeId, cx: &Cx<'_>) -> bool {
    // Climb to the top of the call chain (a node that is some other call's
    // receiver should follow that chain up first; but the chain top is the
    // node whose parent is not a call where it is the receiver).
    let mut top = node;
    while let Some(parent) = cx.parent(top).get() {
        // If `top` is the receiver of `parent` (a call), keep climbing.
        if cx.call_receiver(parent).get() == Some(top) {
            top = parent;
            continue;
        }
        // Otherwise `top` is a leaf of the chain; check whether `parent` is a
        // call with parentheses and `top` is one of its arguments.
        if cx.call_arguments(parent).contains(&top) && cx.loc(parent).begin() != Range::ZERO {
            return true;
        }
        break;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{
        IndentationStyle, MultilineMethodCallIndentation,
        MultilineMethodCallIndentationOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};
    use murphy_plugin_api::CopOptions;

    fn indented() -> MultilineMethodCallIndentationOptions {
        MultilineMethodCallIndentationOptions {
            enforced_style: IndentationStyle::Indented,
            indentation_width: None,
        }
    }

    fn indented_with_width(width: i64) -> MultilineMethodCallIndentationOptions {
        MultilineMethodCallIndentationOptions {
            enforced_style: IndentationStyle::Indented,
            indentation_width: Some(width),
        }
    }

    fn indented_relative_to_receiver() -> MultilineMethodCallIndentationOptions {
        MultilineMethodCallIndentationOptions {
            enforced_style: IndentationStyle::IndentedRelativeToReceiver,
            indentation_width: None,
        }
    }

    // ----- skip guards (must not fire) ------------------------------------

    #[test]
    fn skips_chain_argument_in_parenthesized_call() {
        // `argument_in_method_call(node, :with_parentheses)` — the chain is an
        // argument of `expect(...)`, so the semantic base does not apply.
        test::<MultilineMethodCallIndentation>().expect_no_offenses(indoc! {"
            expect(foo.bar
              .baz)
        "});
    }

    #[test]
    fn skips_receiver_alone_on_first_line() {
        // `obj\n.foo\n.bar` — the first dotted call `.foo` has its dot on
        // line 2, not the chain's first line, so there is no semantic base.
        test::<MultilineMethodCallIndentation>().expect_no_offenses(indoc! {"
            obj
              .foo
              .bar
        "});
    }

    #[test]
    fn skips_begin_base_receiver() {
        // `method_on_receiver_last_line?(node, base_receiver, :begin)` — the
        // chain's base receiver is a parenthesized expression, so there is no
        // semantic base and the misaligned `.bar` must NOT be flagged.
        test::<MultilineMethodCallIndentation>().expect_no_offenses(indoc! {"
            (a || b).foo
              .bar
        "});
    }

    #[test]
    fn accepts_single_line_call() {
        test::<MultilineMethodCallIndentation>().expect_no_offenses("foo.bar.baz\n");
    }

    #[test]
    fn accepts_method_with_no_dots() {
        test::<MultilineMethodCallIndentation>().expect_no_offenses("puts something\n");
    }

    // ----- aligned style: positive cases ----------------------------------

    #[test]
    fn flags_misaligned_continuation() {
        // `.c` should align with `.b` (column 5); it sits at column 0.
        test::<MultilineMethodCallIndentation>().expect_offense(indoc! {"
            Thing.a
            .c
            ^^ Align `.c` with `.a` on line 1.
        "});
    }

    #[test]
    fn accepts_aligned_chain() {
        test::<MultilineMethodCallIndentation>().expect_no_offenses(indoc! {"
            Thing.a
                 .b
                 .c
        "});
    }

    #[test]
    fn flags_misaligned_third_link() {
        test::<MultilineMethodCallIndentation>().expect_offense(indoc! {"
            Thing.a
                 .b
              .c
              ^^ Align `.c` with `.a` on line 1.
        "});
    }

    #[test]
    fn flags_misaligned_chain_in_assignment() {
        // Assignment-RHS chain: the anchor `.foo` is on line 1, so the
        // semantic base fires before the syntactic assignment-RHS fallback.
        test::<MultilineMethodCallIndentation>().expect_offense(indoc! {"
            x = obj.foo
              .bar
              ^^^^ Align `.bar` with `.foo` on line 1.
        "});
    }

    #[test]
    fn accepts_aligned_chain_in_assignment() {
        test::<MultilineMethodCallIndentation>().expect_no_offenses(indoc! {"
            x = obj.foo
                   .bar
                   .baz
        "});
    }

    #[test]
    fn flags_safe_navigation_continuation() {
        test::<MultilineMethodCallIndentation>().expect_offense(indoc! {"
            Thing.a
            &.c
            ^^^ Align `&.c` with `.a` on line 1.
        "});
    }

    // ----- indented styles -------------------------------------------------

    #[test]
    fn indented_style_flags_misaligned_continuation() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented())
            .expect_offense(indoc! {"
                Thing.a
                .c
                ^^ Use 2 (not 0) spaces for indenting an expression spanning multiple lines.
            "});
    }

    #[test]
    fn indented_style_checks_the_first_leading_dot_call() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented())
            .expect_offense(indoc! {"
                Thing
                .c
                ^^ Use 2 (not 0) spaces for indenting an expression spanning multiple lines.
            "});
    }

    #[test]
    fn indented_style_uses_cop_indentation_width() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented_with_width(4))
            .expect_offense(indoc! {"
                Thing.a
                  .c
                  ^^ Use 4 (not 2) spaces for indenting an expression spanning multiple lines.
            "});
    }

    #[test]
    fn indented_style_falls_back_to_resolved_indentation_width() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented())
            .with_indentation_width(4)
            .expect_no_offenses(indoc! {"
                Thing.a
                    .c
            "});
    }

    #[test]
    fn relative_style_indents_from_the_receiver_column() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented_relative_to_receiver())
            .expect_offense(indoc! {"
                x = Thing.a
                 .c
                 ^^ Indent `.c` 2 spaces more than `Thing` on line 1.
            "});
    }

    #[test]
    fn relative_style_accepts_a_continuation_at_receiver_plus_width() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented_relative_to_receiver())
            .expect_no_offenses(indoc! {"
                x = Thing.a
                      .c
            "});
    }

    #[test]
    fn indented_style_accepts_later_links_at_the_same_indentation() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented())
            .expect_no_offenses(indoc! {"
                Thing.a
                  .b
                  .c
            "});
    }

    #[test]
    fn relative_style_accepts_later_links_at_receiver_plus_width() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented_relative_to_receiver())
            .expect_no_offenses(indoc! {"
                x = Thing.a
                      .b
                      .c
            "});
    }

    #[test]
    fn relative_style_accounts_for_splat_operator_width() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented_relative_to_receiver())
            .expect_no_offenses(indoc! {"
                [
                  *foo
                    .bar
                ]
            "});
    }

    #[test]
    fn relative_style_falls_back_to_resolved_indentation_width() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented_relative_to_receiver())
            .with_indentation_width(4)
            .expect_no_offenses(indoc! {"
                x = Thing.a
                        .c
            "});
    }

    #[test]
    fn relative_style_accounts_for_kwsplat_operator_width() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented_relative_to_receiver())
            .expect_no_offenses(indoc! {"
                [
                  **foo
                    .bar
                ]
            "});
    }

    #[test]
    fn relative_style_uses_cop_indentation_width() {
        let options = MultilineMethodCallIndentationOptions {
            enforced_style: IndentationStyle::IndentedRelativeToReceiver,
            indentation_width: Some(4),
        };
        test::<MultilineMethodCallIndentation>()
            .with_options(&options)
            .expect_offense(indoc! {"
                x = Thing.a
                      .c
                      ^^ Indent `.c` 4 spaces more than `Thing` on line 1.
            "});
    }

    #[test]
    fn null_indentation_width_preserves_enforced_style() {
        let options = <MultilineMethodCallIndentationOptions as CopOptions>::from_config_json(
            br#"{"EnforcedStyle":"indented","IndentationWidth":null}"#,
        )
        .expect("null IndentationWidth must decode the options struct");

        assert!(options.enforced_style == IndentationStyle::Indented);
        assert!(options.indentation_width.is_none());
    }
}

murphy_plugin_api::submit_cop!(MultilineMethodCallIndentation);
