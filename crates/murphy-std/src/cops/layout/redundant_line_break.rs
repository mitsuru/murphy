//! `Layout/RedundantLineBreak` — flags an expression broken across multiple
//! lines that would fit, unchanged, on a single line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/RedundantLineBreak
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports the core: a multiline `send`/`csend`/assignment expression that is
//!   suitable as a single line (fits in the line-length budget, contains no
//!   interior comment, and is safe to join) is flagged, and autocorrected by
//!   collapsing to one line. The "whole expression" is grown through `send`
//!   parents, convertible blocks, and binary operators, matching RuboCop's
//!   `on_send` walk. `safe_to_split?`, `index_access_call_chained?`, and the
//!   `InspectBlocks` config (default false) are ported.
//!
//!   Gaps vs RuboCop (documented, not silently dropped):
//!     * `Layout/LineLength: Max` is read from cross-cop config by RuboCop;
//!       Murphy hardcodes the RuboCop default of 120 (`MAX_LINE_LENGTH`).
//!     * The `to_single_line` quote-continuation regexes (`" \\\n '"` →
//!       `" + '`, etc.) are not ported — `safe_to_split?` already rejects
//!       multiline string literals, so the join collapses interior runs of
//!       whitespace/backslash-newline to a single space, which is correct for
//!       the shapes that reach the autocorrect.
//!     * `other_cop_takes_precedence?` defers to `Layout/SingleLineBlockChain`
//!       when that cop is enabled; Murphy does not read that cop's enabled
//!       state and always inspects (the two cops' autocorrects are
//!       compatible — SingleLineBlockChain splits, this joins, and the offense
//!       only fires on already-multiline single-line-block chains).
//!     * `require_backslash?` for `operator_keyword?` (`and`/`or`) — Murphy
//!       only flags operator-keyword expressions when a trailing backslash
//!       continues the operator line, matching RuboCop.
//! ```
//!
//! ## Autocorrect
//!
//! Replace the whole expression with its single-line form: collapse every
//! interior line break (and any leading backslash + surrounding whitespace)
//! to a single space, then strip.

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedundantLineBreak;

/// Maximum single-line length, mirroring RuboCop's `Layout/LineLength: Max`
/// default. RuboCop reads the user's configured value; Murphy hardcodes the
/// default (documented gap).
const MAX_LINE_LENGTH: usize = 120;

const MSG: &str = "Redundant line break detected.";

#[derive(murphy_plugin_api::CopOptions)]
pub struct RedundantLineBreakOptions {
    #[option(
        name = "InspectBlocks",
        default = false,
        description = "Whether to inspect blocks that could be written on a single line."
    )]
    pub inspect_blocks: bool,
}

#[cop(
    name = "Layout/RedundantLineBreak",
    description = "Do not break up an expression into multiple lines when it fits on a single line.",
    default_severity = "warning",
    default_enabled = false,
    options = RedundantLineBreakOptions,
)]
impl RedundantLineBreak {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // RuboCop refuses to run when the buffer ends with `%\n\n` (a percent
    // literal edge case that breaks single-line reconstruction).
    if cx.source().ends_with("%\n\n") {
        return;
    }

    let opts = cx.options_or_default::<RedundantLineBreakOptions>();

    // Grow to the "whole expression": walk up through send parents,
    // convertible blocks, and binary operators.
    let grown = whole_expression(node, cx);

    // Multiple hooks reach the same grown node (`foo`, `foo.bar`, `foo.bar.baz`
    // all grow to the chain; a block's call grows to the block). Emit once, on
    // the canonical entry, mirroring RuboCop's `ignore_node` suppression of
    // inner nodes. The canonical entry is the *base* of the grown expression:
    // the node whose own growth produced `grown` and that has no in-chain
    // receiver of its own (the deepest receiver / first operand).
    if !is_canonical_entry(node, grown, cx) {
        return;
    }
    let node = grown;

    if !offense(node, cx, opts.inspect_blocks) {
        return;
    }

    let node_range = cx.range(node);
    let src = cx.raw_source(node_range);
    let single = to_single_line(src);
    let single = single.trim();
    // RuboCop highlights the whole (multiline) node; Murphy highlights the
    // node's first physical line so the offense range is single-line and the
    // reported location (line:col of the node start) is identical. The
    // autocorrect still rewrites the entire node range.
    cx.emit_offense(first_line_range(cx.source(), node_range), MSG, None);
    cx.emit_edit(node_range, single);
}

/// The first physical line of `range` (start → first `\n`, clamped to
/// `range.end`).
fn first_line_range(source: &str, range: Range) -> Range {
    let bytes = source.as_bytes();
    let end = bytes[range.start as usize..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(range.end, |i| range.start + i as u32)
        .min(range.end);
    Range {
        start: range.start,
        end,
    }
}

/// True if `node` is the canonical (base) entry of the grown expression — the
/// deepest node from which growth begins. A node is canonical iff none of its
/// growth-children (chain receiver, binary-op left operand, or block call)
/// itself grows to the same `grown` node. This ensures the offense fires once.
fn is_canonical_entry(node: NodeId, grown: NodeId, cx: &Cx<'_>) -> bool {
    // The chain receiver.
    if let Some(recv) = cx.call_receiver(node).get()
        && whole_expression(recv, cx) == grown
    {
        return false;
    }
    // Binary-op left operand.
    if let NodeKind::And { lhs, .. } | NodeKind::Or { lhs, .. } = *cx.kind(node)
        && whole_expression(lhs, cx) == grown
    {
        return false;
    }
    // A block's call grows to the block; the call is the base, not the block.
    if cx.is_any_block_type(node) {
        return false;
    }
    true
}

/// Grow `node` to the "whole expression": through `send`/`csend` parents,
/// convertible blocks, and binary operators (`and`/`or`).
fn whole_expression(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    loop {
        let Some(parent) = cx.parent(node).get() else {
            return node;
        };
        let grow = matches!(*cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. })
            || convertible_block(node, parent, cx)
            || matches!(*cx.kind(parent), NodeKind::And { .. } | NodeKind::Or { .. });
        if grow {
            node = parent;
        } else {
            return node;
        }
    }
}

/// RuboCop's `convertible_block?`: `node` is the send of a block parent, and
/// the send is parenthesized or takes no arguments.
fn convertible_block(node: NodeId, parent: NodeId, cx: &Cx<'_>) -> bool {
    if !cx.is_any_block_type(parent) {
        return false;
    }
    if cx.block_call(parent).get() != Some(node) {
        return false;
    }
    // Parenthesized or argument-free send.
    cx.loc(node).begin() != Range::ZERO || cx.call_arguments(node).is_empty()
}

fn offense(node: NodeId, cx: &Cx<'_>, inspect_blocks: bool) -> bool {
    if !cx.is_multiline(node) {
        return false;
    }
    if !suitable_as_single_line(node, cx) {
        return false;
    }
    // Operator keyword (`and`/`or`) requires a trailing backslash on the
    // operator's line to be a redundant break.
    if cx.is_operator_keyword(node) {
        return require_backslash(node, cx);
    }
    !index_access_call_chained(node, cx) && !configured_to_not_be_inspected(node, cx, inspect_blocks)
}

/// `suitable_as_single_line?`: fits the line budget, no interior comment, safe
/// to join.
fn suitable_as_single_line(node: NodeId, cx: &Cx<'_>) -> bool {
    !too_long(node, cx) && !comment_within(node, cx) && safe_to_split(node, cx)
}

fn too_long(node: NodeId, cx: &Cx<'_>) -> bool {
    let src = cx.raw_source(cx.range(node));
    let single = to_single_line(src);
    // Add the node's starting column — the single line begins at the node's
    // indentation, so total length is indent + collapsed length.
    let indent = column_of(cx.source(), cx.range(node).start);
    indent + single.trim().chars().count() > MAX_LINE_LENGTH
}

/// True if a comment falls within the node's source range.
fn comment_within(node: NodeId, cx: &Cx<'_>) -> bool {
    !cx.comments_in_range(cx.range(node)).is_empty()
}

/// RuboCop's `safe_to_split?`: no descendant control-flow / def / rescue /
/// ensure, no heredoc or `\n`-bearing string, no multiline begin / symbol.
fn safe_to_split(node: NodeId, cx: &Cx<'_>) -> bool {
    for d in cx.descendants(node) {
        match *cx.kind(d) {
            NodeKind::If { .. }
            | NodeKind::Case { .. }
            | NodeKind::Kwbegin(_)
            | NodeKind::Def { .. }
            | NodeKind::Defs { .. }
            | NodeKind::Rescue { .. }
            | NodeKind::Ensure { .. } => return false,
            // Heredoc or embedded `\n` in a string makes joining unsafe.
            NodeKind::Str(_) | NodeKind::Dstr(_)
                if cx.raw_source(cx.range(d)).contains('\n') =>
            {
                return false;
            }
            // Multiline begin / symbol is unsafe to join.
            NodeKind::Begin(_) | NodeKind::Sym(_) | NodeKind::Dsym(_)
                if cx.is_multiline(d) =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}

/// `require_backslash?`: the operator line ends with a backslash.
fn require_backslash(node: NodeId, cx: &Cx<'_>) -> bool {
    // The operator is between lhs and rhs. Find the lhs end, then the line it
    // sits on; that line must end with `\`.
    let (NodeKind::And { lhs, .. } | NodeKind::Or { lhs, .. }) = *cx.kind(node) else {
        return false;
    };
    let lhs_end = cx.range(lhs).end;
    let source = cx.source();
    let bytes = source.as_bytes();
    // Find the end of the lhs's physical line.
    let line_end = bytes[lhs_end as usize..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |i| lhs_end as usize + i);
    let line = &source[..line_end];
    line.trim_end_matches([' ', '\t']).ends_with('\\')
        || source[lhs_end as usize..line_end]
            .trim_end_matches([' ', '\t'])
            .ends_with('\\')
}

/// `index_access_call_chained?`: `foo[...][...]` — a `[]` send whose receiver
/// is itself a `[]` send.
fn index_access_call_chained(node: NodeId, cx: &Cx<'_>) -> bool {
    if !is_index_send(node, cx) {
        return false;
    }
    cx.call_receiver(node)
        .get()
        .is_some_and(|recv| is_index_send(recv, cx))
}

fn is_index_send(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Index { .. })
        || (matches!(*cx.kind(node), NodeKind::Send { .. })
            && cx.method_name(node) == Some("[]"))
}

/// `configured_to_not_be_inspected?`: a block (or a multiline-block-bearing
/// descendant) is skipped unless `InspectBlocks` is set.
fn configured_to_not_be_inspected(node: NodeId, cx: &Cx<'_>, inspect_blocks: bool) -> bool {
    if inspect_blocks {
        return false;
    }
    if cx.is_any_block_type(node) {
        return true;
    }
    cx.descendants(node)
        .into_iter()
        .any(|d| cx.is_any_block_type(d) && cx.is_multiline(d))
}

/// Collapse a multiline source snippet to a single line: join interior line
/// breaks (with or without a trailing backslash) and surrounding whitespace,
/// including the space within method chaining (`\n  .foo` / `\n  &.foo`).
fn to_single_line(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' || (b == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'\n') {
            // Skip the backslash if present.
            if b == b'\\' {
                i += 1;
            }
            // Skip the newline.
            i += 1;
            // Skip following whitespace.
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            // Method chaining: if the next chars are `.` or `&.`, do not
            // insert a space (`foo\n  .bar` → `foo.bar`).
            let is_chain = bytes.get(i) == Some(&b'.')
                || (bytes.get(i) == Some(&b'&') && bytes.get(i + 1) == Some(&b'.'));
            if !is_chain {
                out.push(' ');
            }
        } else {
            out.push(b as char);
            i += 1;
        }
    }
    out
}

/// 0-based column (character count) of `offset` on its line.
fn column_of(source: &str, offset: u32) -> usize {
    let start = offset as usize;
    let line_start = source.as_bytes()[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    source[line_start..start].chars().count()
}

#[cfg(test)]
mod tests {
    use super::{RedundantLineBreak, RedundantLineBreakOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn inspect_blocks() -> RedundantLineBreakOptions {
        RedundantLineBreakOptions {
            inspect_blocks: true,
        }
    }

    // NOTE: the offense range is the node's first physical line (Murphy
    // highlights one line; RuboCop highlights the whole multiline node). The
    // autocorrect rewrites the entire expression.

    #[test]
    fn flags_method_chain_split_across_lines() {
        // Leading-dot chain: the break before each `.` is removed without
        // inserting a space (`foo\n  .bar` → `foo.bar`).
        test::<RedundantLineBreak>().expect_offense(indoc! {"
            foo
            ^^^ Redundant line break detected.
              .bar
              .baz
        "});
    }

    #[test]
    fn corrects_method_chain_to_single_line() {
        test::<RedundantLineBreak>().expect_correction(
            indoc! {"
                foo
                ^^^ Redundant line break detected.
                  .bar
                  .baz
            "},
            "foo.bar.baz\n",
        );
    }

    #[test]
    fn accepts_single_line_call() {
        test::<RedundantLineBreak>().expect_no_offenses("foo.bar.baz\n");
    }

    #[test]
    fn flags_method_args_split_across_lines() {
        // RuboCop's canonical shape: the first arg shares the `(` line.
        test::<RedundantLineBreak>().expect_correction(
            indoc! {"
                my_method(1,
                ^^^^^^^^^^^^ Redundant line break detected.
                  2,
                  3)
            "},
            "my_method(1, 2, 3)\n",
        );
    }

    #[test]
    fn accepts_too_long_for_single_line() {
        // The single-line form would exceed 120 chars.
        let long_arg = "x".repeat(130);
        let src = format!("foo(\n  {long_arg}\n)\n");
        test::<RedundantLineBreak>().expect_no_offenses(&src);
    }

    #[test]
    fn accepts_when_comment_within() {
        test::<RedundantLineBreak>().expect_no_offenses(indoc! {"
            foo(
              a, # keep this
              b
            )
        "});
    }

    #[test]
    fn accepts_heredoc_argument() {
        test::<RedundantLineBreak>().expect_no_offenses(indoc! {"
            foo(<<~TEXT)
              hello
            TEXT
        "});
    }

    #[test]
    fn accepts_block_by_default() {
        // Blocks are skipped unless InspectBlocks is set.
        test::<RedundantLineBreak>().expect_no_offenses(indoc! {"
            foo do
              bar
            end
        "});
    }

    #[test]
    fn flags_block_when_inspect_blocks_enabled() {
        test::<RedundantLineBreak>()
            .with_options(&inspect_blocks())
            .expect_correction(
                indoc! {"
                    foo { |x|
                    ^^^^^^^^^ Redundant line break detected.
                      bar
                    }
                "},
                "foo { |x| bar }\n",
            );
    }

    #[test]
    fn corrects_call_with_args_then_chain() {
        // RuboCop case: `foo(x, y, z)\n  .bar\n  .baz` → `foo(x, y, z).bar.baz`.
        test::<RedundantLineBreak>().expect_correction(
            indoc! {"
                foo(1,
                ^^^^^^ Redundant line break detected.
                  2)
                  .bar
                  .baz
            "},
            "foo(1, 2).bar.baz\n",
        );
    }

    #[test]
    fn accepts_index_access_chained() {
        // `foo[...][...]` is excluded.
        test::<RedundantLineBreak>().expect_no_offenses(indoc! {"
            foo[
              a
            ][b]
        "});
    }

    #[test]
    fn accepts_expression_with_interior_if() {
        // `safe_to_split?` rejects control-flow descendants.
        test::<RedundantLineBreak>().expect_no_offenses(indoc! {"
            foo(if x
              y
            end)
        "});
    }
}

murphy_plugin_api::submit_cop!(RedundantLineBreak);
