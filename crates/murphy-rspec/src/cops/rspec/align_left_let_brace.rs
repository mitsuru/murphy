//! `RSpec/AlignLeftLetBrace` — align `{` of adjacent single-line lets.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/AlignLeftLetBrace
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_new_investigation` over `AlignLetBrace`
//!   (`single_line_lets`: bare `let` / `let!` blocks spanning one line,
//!   chunked by consecutive first-lines; `offending_tokens`: members
//!   whose `:begin`-token column differs from the chunk max) with
//!   `add_offense(let.loc.begin)` and `Align left let brace`. Each
//!   `Block` dispatch recomputes the file-wide grouping (shared
//!   helpers in `rspec_helpers`), so every member of a chunk reaches
//!   the same verdict as upstream's single pass. Columns are
//!   character-based like upstream `loc.column`. Detection is at
//!   parity; autocorrect (pad before `{`) is not ported in this batch
//!   — same convention as `RSpec/EmptyLineAfterFinalLet` (status:
//!   partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block`; only single-line bare `let` / `let!` blocks
//! participate:
//!
//! - `let(:foobar) { x }` / `let(:a) { y }` on consecutive lines —
//!   the second `{` is left of the max, flagged (the `{` token).
//! - Aligned `{` columns — clean.
//! - Single `let`, or lets separated by a blank line / another node
//!   line — each its own chunk, clean.
//! - Multiline `let` (`do...end` over lines) — not single-line, clean.
//!
//! ## No autocorrect
//!
//! Upstream inserts padding before `{`. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{
    adjacent_let_chunks, block_open_token, char_column_of_offset, is_bare_let_call,
    single_line_let_blocks,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct AlignLeftLetBrace;

#[cop(
    name = "RSpec/AlignLeftLetBrace",
    description = "Checks that left braces for adjacent single line lets are aligned.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions
)]
impl AlignLeftLetBrace {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        if !is_bare_let_call(cx, call) {
            return;
        }
        if !cx.is_single_line(node) {
            return;
        }
        let Some(open) = block_open_token(cx, node) else {
            return;
        };
        let src = cx.source();
        let own_column = char_column_of_offset(src, open.start);
        let lets = single_line_let_blocks(cx);
        let chunks = adjacent_let_chunks(cx, &lets);
        let Some(chunk) = chunks.iter().find(|c| c.contains(&node)) else {
            return;
        };
        let target = chunk
            .iter()
            .filter_map(|&member| block_open_token(cx, member))
            .map(|range| char_column_of_offset(src, range.start))
            .max();
        if target == Some(own_column) {
            return;
        }
        cx.emit_offense(open, "Align left let brace", None);
    }
}

#[cfg(test)]
mod tests {
    use super::AlignLeftLetBrace;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_misaligned_left_braces() {
        test::<AlignLeftLetBrace>().expect_offense(indoc! {r#"
                let(:foobar) { blahblah }
                let(:baz) { bar }
                          ^ Align left let brace
                let(:a) { b }
                        ^ Align left let brace
            "#});
    }
    #[test]
    fn does_not_flag_aligned_left_braces() {
        test::<AlignLeftLetBrace>().expect_no_offenses(indoc! {r#"
                let(:foobar) { blahblah }
                let(:baz)    { bar }
                let(:a)      { b }
            "#});
    }
    #[test]
    fn does_not_flag_single_let() {
        // A one-member chunk's max is its own column (verified vs 3.7.0: `target_column_for` over the singleton).
        test::<AlignLeftLetBrace>().expect_no_offenses(indoc! {r#"
                let(:a) { b }
            "#});
    }
    #[test]
    fn does_not_flag_chunks_split_by_blank_line() {
        // Non-consecutive first lines form separate chunks (verified vs 3.7.0: `adjacent_let_chunks`).
        test::<AlignLeftLetBrace>().expect_no_offenses(indoc! {r#"
                let(:foobar) { blahblah }
                
                let(:a) { b }
            "#});
    }
    #[test]
    fn does_not_flag_multiline_let() {
        // `single_line_lets` only (verified vs 3.7.0).
        test::<AlignLeftLetBrace>().expect_no_offenses(indoc! {r#"
                let(:foobar) do
                  blahblah
                end
                let(:a) { b }
            "#});
    }
    #[test]
    fn does_not_flag_let_bang_mixed_when_aligned() {
        // `Helpers.all` is `let` / `let!` (verified vs 3.7.0).
        test::<AlignLeftLetBrace>().expect_no_offenses(indoc! {r#"
                let(:foobar) { blahblah }
                let!(:baz)   { bar }
            "#});
    }
    #[test]
    fn does_not_flag_explicit_receiver() {
        // `let?` requires a bare receiver (verified vs 3.7.0).
        test::<AlignLeftLetBrace>().expect_no_offenses(indoc! {r#"
                let(:foobar) { blahblah }
                foo.let(:a) { b }
            "#});
    }
}

murphy_plugin_api::submit_cop!(AlignLeftLetBrace);
