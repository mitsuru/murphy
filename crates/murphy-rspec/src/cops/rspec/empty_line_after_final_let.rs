//! `RSpec/EmptyLineAfterFinalLet` — require an empty line after the last `let`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/EmptyLineAfterFinalLet
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example_group_with_body?`: `(block
//!   (send #rspec? #ExampleGroups.all ...) args !nil?)`) which locates the
//!   last `let?` child (`Helpers.all`: `let` / `let!`, block or
//!   `block_pass` send form) and applies
//!   `EmptyLineSeparation#missing_separating_line_offense` to it. A
//!   trailing `let` that is not the last child flags when the line after
//!   its final `end` / `}` is not blank. The offense range is the final
//!   line's content (`offending_loc`), rebuilt here from the `let` range
//!   end line trimmed of leading whitespace. Detection is at parity for
//!   the common shapes; comment / `rubocop:enable` directive skipping,
//!   heredoc-aware `FinalEndLocation`, and `Numblock` handling are not
//!   ported in this batch (status: partial, autocorrect as gap — upstream
//!   inserts `\n`).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is an RSpec-or-bare example group with
//! a non-empty body:
//!
//! - `describe` with `let(:a) { 1 }` immediately followed by `it` —
//!   flagged (the `let`'s closing line).
//! - `describe` with `let(:a) { 1 }` + blank line + `it` — not flagged.
//! - `describe` ending with `let` (no example after) — `last_child?`, not
//!   flagged.
//! - `describe` without any `let` — not flagged.
//!
//! ## No autocorrect
//!
//! Upstream inserts a newline after the offense. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{is_example_group_name, is_rspec_or_bare_receiver};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLineAfterFinalLet;

#[cop(
    name = "RSpec/EmptyLineAfterFinalLet",
    description = "Checks if there is an empty line after the last let block.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EmptyLineAfterFinalLet {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(call)
        else {
            return;
        };
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        if !is_example_group_name(cx.symbol_str(method)) {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        let Some(final_let) = find_final_let(cx, body_id) else {
            return;
        };
        if is_last_child(cx, final_let) {
            return;
        }
        if has_blank_line_after(cx, final_let) {
            return;
        }
        let method = let_method_name(cx, final_let).unwrap_or("let".to_string());
        if let Some(range) = final_line_content_range(cx, final_let) {
            cx.emit_offense(
                range,
                &format!("Add an empty line after the last `{method}`."),
                None,
            );
        }
    }
}

/// The last `let?` child of the group body, scanning in reverse.
///
/// `let?` is `(block (send nil? #Helpers.all ...) ...)` or
/// `(send nil? #Helpers.all _ block_pass)`. The body is either a `Begin`
/// list or a single node.
fn find_final_let(cx: &Cx<'_>, body: NodeId) -> Option<NodeId> {
    match *cx.kind(body) {
        NodeKind::Begin(list) => {
            let children = cx.list(list);
            children.iter().rev().find(|id| is_let_node(cx, **id)).copied()
        }
        _ => {
            if is_let_node(cx, body) {
                Some(body)
            } else {
                None
            }
        }
    }
}

/// `true` when `id` is a `let` / `let!` block or a `let` send with a
/// `block_pass` (`&blk`) trailing arg. Receiver must be bare.
fn is_let_node(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Block { call, .. } => {
            let NodeKind::Send {
                receiver, method, ..
            } = *cx.kind(call)
            else {
                return false;
            };
            if receiver != OptNodeId::NONE {
                return false;
            }
            matches!(cx.symbol_str(method), "let" | "let!")
        }
        NodeKind::Send {
            receiver, method, ..
        } => {
            if receiver != OptNodeId::NONE {
                return false;
            }
            if !matches!(cx.symbol_str(method), "let" | "let!") {
                return false;
            }
            // `(send nil? #Helpers.all _ block_pass)`: last arg is a
            // `BlockPass`. Bare `let(:a)` without a block is not a `let?`.
            let NodeKind::Send { args, .. } = *cx.kind(id) else {
                return false;
            };
            let arg_ids = cx.list(args);
            let Some(&last) = arg_ids.last() else {
                return false;
            };
            matches!(*cx.kind(last), NodeKind::BlockPass(_))
        }
        _ => false,
    }
}

fn let_method_name(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Block { call, .. } => {
            let NodeKind::Send { method, .. } = *cx.kind(call) else {
                return None;
            };
            Some(cx.symbol_str(method).to_owned())
        }
        NodeKind::Send { method, .. } => Some(cx.symbol_str(method).to_owned()),
        _ => None,
    }
}

/// `true` when `node` is the last child: no `Begin` parent, or last in
/// its `Begin` list. Mirrors `EmptyLineSeparation#last_child?`.
fn is_last_child(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return true;
    };
    let NodeKind::Begin(list) = *cx.kind(parent) else {
        return true;
    };
    let children = cx.list(list);
    children.last() == Some(&node)
}

/// `true` when the source line immediately after the node's end line is
/// blank (empty or whitespace-only) or the node ends at EOF.
fn has_blank_line_after(cx: &Cx<'_>, node: NodeId) -> bool {
    let range = cx.range(node);
    let src = cx.source();
    let bytes = src.as_bytes();
    let Ok(end) = usize::try_from(range.end) else {
        return false;
    };
    if end >= bytes.len() {
        return true;
    }
    let raw: Vec<&str> = src.split('\n').collect();
    let containing = line_of_offset(bytes, end);
    let next_idx = containing + 1;
    if next_idx >= raw.len() {
        return true;
    }
    raw[next_idx].trim().is_empty()
}

/// Byte offset → 0-based line index.
fn line_of_offset(bytes: &[u8], offset: usize) -> usize {
    bytes[..offset.min(bytes.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

/// Range of the node's final line content, trimmed of leading whitespace
/// — mirrors `EmptyLineSeparation#offending_loc`.
fn final_line_content_range(
    cx: &Cx<'_>,
    node: NodeId,
) -> Option<murphy_plugin_api::Range> {
    let range = cx.range(node);
    let src = cx.source();
    let bytes = src.as_bytes();
    let Ok(end) = usize::try_from(range.end) else {
        return None;
    };
    let line_idx = line_of_offset(bytes, end.saturating_sub(1).max(0));
    let mut offset = 0usize;
    let raw: Vec<&str> = src.split('\n').collect();
    if line_idx >= raw.len() {
        return None;
    }
    for (i, line) in raw.iter().enumerate() {
        let line_start = offset;
        let line_end = offset + line.len();
        if i == line_idx {
            let trimmed = line.len() - line.trim_start().len();
            return Some(murphy_plugin_api::Range {
                start: (line_start + trimmed) as u32,
                end: line_end as u32,
            });
        }
        offset = line_end + 1; // + '\n'
    }
    None
}

#[cfg(test)]
mod tests {
    use super::EmptyLineAfterFinalLet;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_missing_blank_line_after_let() {
        test::<EmptyLineAfterFinalLet>().expect_offense(indoc! {r#"
                describe 'x' do
                  let(:foo) { bar }
                  ^^^^^^^^^^^^^^^^^ Add an empty line after the last `let`.
                  it { does_something }
                end
            "#});
    }

    #[test]
    fn does_not_flag_with_blank_line() {
        test::<EmptyLineAfterFinalLet>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  let(:foo) { bar }

                  it { does_something }
                end
            "#});
    }

    #[test]
    fn does_not_flag_let_as_last_child() {
        test::<EmptyLineAfterFinalLet>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  let(:foo) { bar }
                end
            "#});
    }

    #[test]
    fn does_not_flag_without_let() {
        test::<EmptyLineAfterFinalLet>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  it { does_something }
                end
            "#});
    }

    #[test]
    fn flags_only_after_last_let() {
        test::<EmptyLineAfterFinalLet>().expect_offense(indoc! {r#"
                describe 'x' do
                  let(:foo) { bar }
                  let(:something) { other }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^ Add an empty line after the last `let`.
                  it { does_something }
                end
            "#});
    }

    #[test]
    fn does_not_flag_non_group() {
        test::<EmptyLineAfterFinalLet>().expect_no_offenses(indoc! {r#"
                it 'x' do
                  let(:foo) { bar }
                  foo
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(EmptyLineAfterFinalLet);
