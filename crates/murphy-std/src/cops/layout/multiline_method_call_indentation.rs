//! `Layout/MultilineMethodCallIndentation` — the method-name part of a method
//! call that spans more than one line must be indented consistently.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineMethodCallIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports the default `EnforcedStyle: aligned` style, semantic-alignment
//!   case only. For a leading-dot continuation (`.method` / `&.method` that
//!   begins its own line), RuboCop's `alignment_base` is
//!   `semantic_alignment_base || syntactic_alignment_base`; the semantic base
//!   wins first and is the dot of the first call in the chain that carries a
//!   dot. Murphy aligns the continuation's `dot.column` with that anchor
//!   dot's column and reports a misalignment offense on the
//!   `dot..selector` range.
//!
//!   Two RuboCop early-returns are reproduced verbatim and are what make the
//!   `Enabled: true` default safe from false positives:
//!
//!   - `semantic_alignment_node`: `return if argument_in_method_call(node,
//!     :with_parentheses)` — a chain that is an argument inside a
//!     parenthesized call (`expect(foo.bar\n.baz)`) is skipped; RuboCop
//!     aligns those via the indentation fallback, not the semantic base.
//!   - `first_call_alignment_node`: `return if node.loc.dot.line !=
//!     node.first_line` — the anchor dot must sit on the chain expression's
//!     first line. `obj\n.foo\n.bar` (receiver alone on line 1, first dot on
//!     line 2) therefore has no semantic base and is skipped.
//!   - `first_call_alignment_node`: `return if method_on_receiver_last_line?(
//!     node, base_receiver, :begin)` — a chain whose base receiver is a
//!     parenthesized / `begin...end` expression (`(a || b).foo\n.bar`) has no
//!     semantic base. Murphy over-skips any `Begin` base receiver (safe
//!     under-fire).
//!
//!   Gaps (documented, not covered):
//!   - `indented` and `indented_relative_to_receiver` styles.
//!   - `IndentationWidth` interaction (only meaningful for `indented`).
//!   - The `syntactic_alignment_base` fallbacks (assignment-RHS where the
//!     anchor is not on line 1, operator-RHS, keyword-special indentation)
//!     and the no-base `check_regular_indentation` fallback — all of which
//!     `semantic_alignment_base` declines, returning nil.
//!   - Hash-pair alignment (`hash_pair_aligned?` / `check_hash_pair_*`),
//!     multi-line block-chain anchors (`find_multiline_block_chain_node`),
//!     `get_dot_right_above`, and the receiver-last-line `begin`/`array`
//!     special cases.
//!   - Other grouped-expression contexts handled by `not_for_this_cop?`
//!     beyond the `Begin` base-receiver skip above.
//!   - Autocorrect (RuboCop realigns via `AlignmentCorrector`).
//! ```
//!
//! ## Matched shapes
//!
//! `send`/`csend` nodes whose leading-dot selector begins its own line and is
//! misaligned with the first dotted call in the chain (anchored on line 1).

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineMethodCallIndentation;

/// Options for [`MultilineMethodCallIndentation`]. `EnforcedStyle` matches
/// RuboCop verbatim; the default is `aligned`. Only `aligned` is enforced;
/// the other styles are accepted by the option parser but treated as a
/// documented no-op gap.
#[derive(CopOptions)]
pub struct MultilineMethodCallIndentationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "aligned",
        description = "How the method-name part of a multi-line method call is indented."
    )]
    pub enforced_style: IndentationStyle,
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

/// 1-based line of a byte offset.
fn line_of(offset: u32, src: &str) -> usize {
    1 + src.as_bytes()[..offset as usize]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
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
    // Only the default `aligned` style is enforced (see parity notes).
    if opts.enforced_style != IndentationStyle::Aligned {
        return;
    }

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

    // `right_hand_side` — when the dot and selector are on the same line, the
    // RHS spans `dot.start..selector.end`; the offense highlights this range.
    // `semantic_alignment_base` requires `rhs.source.start_with?('.', '&.')`,
    // i.e. the dot leads (the continuation is a leading-dot line).
    let rhs_start = dot.start;
    // The RHS begins its own line (`begins_its_line?(rhs)`), else the call is
    // single-line or trailing-dot — not a semantic-alignment case.
    if !begins_its_line(rhs_start, src) {
        return;
    }

    // `semantic_alignment_node`: skip a chain that is an argument inside a
    // parenthesized call.
    if is_arg_of_parenthesized_call(node, cx) {
        return;
    }

    // `first_call_has_a_dot` — the bottom-most call in the chain carrying a
    // dot is the alignment anchor; `base` is the chain's base receiver.
    let Some((anchor, base)) = first_dotted_call_in_chain(node, cx) else {
        return;
    };
    // `first_call_alignment_node`: the node aligns to a *different* call.
    if anchor == node {
        return;
    }
    // `first_call_alignment_node`: `return if method_on_receiver_last_line?(
    // node, base_receiver, :begin)` — when the chain's base receiver is a
    // parenthesized / `begin...end` expression, RuboCop declines the semantic
    // base. We over-skip (any `Begin` base receiver) which is a safe
    // under-fire; RuboCop is the documented exception to "don't match Begin".
    if matches!(cx.kind(base), NodeKind::Begin(_)) {
        return;
    }
    let anchor_dot = cx.loc(anchor).dot();
    if anchor_dot == Range::ZERO {
        return;
    }
    // `first_call_alignment_node`: `return if node.loc.dot.line !=
    // node.first_line` — the anchor dot must be on the chain expression's
    // first line. The chain expression starts at the anchor's full range.
    let anchor_expr_start = cx.range(anchor).start;
    if line_of(anchor_dot.start, src) != line_of(anchor_expr_start, src) {
        return;
    }

    let expected_column = column_of(anchor_dot.start, src);
    let actual_column = column_of(dot.start, src);
    if actual_column == expected_column {
        return;
    }

    let rhs_range = Range {
        start: rhs_start,
        end: selector.end,
    };
    let rhs_src = cx.raw_source(rhs_range);
    // RuboCop's `base_source`: `@base.source[/[^\n]*/]` where `@base =
    // anchor.dot.join(anchor.selector)`. Span the anchor's dot through its
    // selector, then clip to the first line.
    let anchor_selector = cx.loc(anchor).name;
    let base_range = Range {
        start: anchor_dot.start,
        end: anchor_selector.end.max(anchor_dot.end),
    };
    let base_src = cx.raw_source(base_range);
    let base_src = base_src.split('\n').next().unwrap_or(base_src);
    let base_line = line_of(anchor_dot.start, src);
    let message = format!("Align `{rhs_src}` with `{base_src}` on line {base_line}.");
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

    fn indented() -> MultilineMethodCallIndentationOptions {
        MultilineMethodCallIndentationOptions {
            enforced_style: IndentationStyle::Indented,
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

    // ----- non-aligned styles are a documented no-op gap ------------------

    #[test]
    fn indented_style_does_not_fire() {
        test::<MultilineMethodCallIndentation>()
            .with_options(&indented())
            .expect_no_offenses(indoc! {"
                Thing.a
                .c
            "});
    }
}

murphy_plugin_api::submit_cop!(MultilineMethodCallIndentation);
