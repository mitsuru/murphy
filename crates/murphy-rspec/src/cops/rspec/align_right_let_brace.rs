//! `RSpec/AlignRightLetBrace` — align `}` of adjacent single-line lets.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/AlignRightLetBrace
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_new_investigation` over `AlignLetBrace`
//!   (`single_line_lets`: bare `let` / `let!` blocks spanning one line,
//!   chunked by consecutive first-lines; `offending_tokens`: members
//!   whose `:end`-token column differs from the chunk max) with
//!   `add_offense(let.loc.end)` and `Align right let brace`. Each
//!   `Block` dispatch recomputes the file-wide grouping (shared
//!   helpers in `rspec_helpers`), so every member of a chunk reaches
//!   the same verdict as upstream's single pass. Columns are
//!   character-based like upstream `loc.column`. Detection is at
//!   parity; autocorrect (pad before `}`) is not ported in this batch
//!   — same convention as `RSpec/AlignLeftLetBrace` (status: partial,
//!   autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block`; only single-line bare `let` / `let!` blocks
//! participate:
//!
//! - `let(:foobar) { blahblah }` / `let(:baz) { bar }` on consecutive
//!   lines — the second `}` is left of the max, flagged (the `}`
//!   token).
//! - Aligned `}` columns — clean.
//! - Single `let`, or lets separated by a blank line — each its own
//!   chunk, clean.
//! - Multiline `let` — not single-line, clean.
//!
//! ## No autocorrect
//!
//! Upstream inserts padding before `}`. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{
    adjacent_let_chunks, block_close_token, char_column_of_offset, is_bare_let_call,
    single_line_let_blocks,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct AlignRightLetBrace;

#[cop(
    name = "RSpec/AlignRightLetBrace",
    description = "Checks that right braces for adjacent single line lets are aligned.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions
)]
impl AlignRightLetBrace {
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
        let Some(close) = block_close_token(cx, node) else {
            return;
        };
        let src = cx.source();
        let own_column = char_column_of_offset(src, close.start);
        let lets = single_line_let_blocks(cx);
        let chunks = adjacent_let_chunks(cx, &lets);
        let Some(chunk) = chunks.iter().find(|c| c.contains(&node)) else {
            return;
        };
        let target = chunk
            .iter()
            .filter_map(|&member| block_close_token(cx, member))
            .map(|range| char_column_of_offset(src, range.start))
            .max();
        if target == Some(own_column) {
            return;
        }
        cx.emit_offense(close, "Align right let brace", None);
    }
}

#[cfg(test)]
mod tests {
    use super::AlignRightLetBrace;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_misaligned_right_braces() {
        test::<AlignRightLetBrace>().expect_offense(indoc! {r#"
                let(:foobar) { blahblah }
                let(:baz)    { bar }
                                   ^ Align right let brace
                let(:a)      { b }
                                 ^ Align right let brace
            "#});
    }
    #[test]
    fn does_not_flag_aligned_right_braces() {
        test::<AlignRightLetBrace>().expect_no_offenses(indoc! {r#"
                let(:foobar) { blahblah }
                let(:baz)    { bar      }
                let(:a)      { b        }
            "#});
    }
    #[test]
    fn does_not_flag_single_let() {
        // A one-member chunk's max is its own column (verified vs 3.7.0).
        test::<AlignRightLetBrace>().expect_no_offenses(indoc! {r#"
                let(:a) { b }
            "#});
    }
    #[test]
    fn does_not_flag_chunks_split_by_blank_line() {
        // Non-consecutive first lines form separate chunks (verified vs 3.7.0).
        test::<AlignRightLetBrace>().expect_no_offenses(indoc! {r#"
                let(:foobar) { blahblah }
                
                let(:a) { b }
            "#});
    }
    #[test]
    fn does_not_flag_multiline_let() {
        // `single_line_lets` only (verified vs 3.7.0).
        test::<AlignRightLetBrace>().expect_no_offenses(indoc! {r#"
                let(:foobar) do
                  blahblah
                end
                let(:a) { b }
            "#});
    }
    #[test]
    fn does_not_flag_explicit_receiver() {
        // `let?` requires a bare receiver (verified vs 3.7.0).
        test::<AlignRightLetBrace>().expect_no_offenses(indoc! {r#"
                let(:foobar) { blahblah }
                foo.let(:a) { b }
            "#});
    }
}

murphy_plugin_api::submit_cop!(AlignRightLetBrace);
