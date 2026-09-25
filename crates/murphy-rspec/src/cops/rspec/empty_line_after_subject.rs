//! `RSpec/EmptyLineAfterSubject` — require an empty line after `subject`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/EmptyLineAfterSubject
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`subject?`: `(block (send nil?
//!   #Subjects.all ...) ...)` bare receiver only) gated by
//!   `InsideExampleGroup#inside_example_group?`, with
//!   `EmptyLineSeparation#missing_separating_line_offense`. A non-last
//!   `subject` inside an example group flags when the line after its final
//!   `end` / `}` is not blank. The offense range is the final line's
//!   content (`offending_loc`), rebuilt here from the block range end line
//!   trimmed of leading whitespace. `inside_example_group?` is simplified
//!   to "any ancestor block is an RSpec-or-bare example group" (upstream
//!   walks to the group root); comment / `rubocop:enable` directive
//!   skipping, heredoc-aware `FinalEndLocation`, and `Numblock` handling
//!   are not ported in this batch (status: partial, autocorrect as gap —
//!   upstream inserts `\n`).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is a bare `subject` / `subject!`
//! inside an example group:
//!
//! - `subject(:obj) { x }` immediately followed by `let(:a) { 1 }` —
//!   flagged.
//! - `subject(:obj) { x }` + blank line + `let` — not flagged.
//! - Top-level `subject { x }` with no group ancestor — not flagged.
//! - Last `subject` in the group — `last_child?`, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream inserts a newline after the offense. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

use crate::cops::rspec_helpers::{is_example_group_name, is_rspec_or_bare_receiver};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLineAfterSubject;

#[cop(
    name = "RSpec/EmptyLineAfterSubject",
    description = "Checks if there is an empty line after subject block.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EmptyLineAfterSubject {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(call)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if !is_subject_name(cx.symbol_str(method)) {
            return;
        }
        if !is_inside_example_group(cx, node) {
            return;
        }
        if is_last_child(cx, node) {
            return;
        }
        if has_blank_line_after(cx, node) {
            return;
        }
        let method = cx.symbol_str(method).to_owned();
        if let Some(range) = final_line_content_range(cx, node) {
            cx.emit_offense(range, &format!("Add an empty line after `{method}`."), None);
        }
    }
}

fn is_subject_name(name: &str) -> bool {
    matches!(name, "subject" | "subject!")
}

/// Simplified `inside_example_group?`: `true` when any ancestor `Block`
/// is an RSpec-or-bare example group. Upstream resolves the group root
/// (`example_group_root?`); the ancestor scan agrees on the common shapes
/// (group-nested subject) and only diverges for exotic roots (e.g.
/// subject inside a bare `if` at the top level with no group).
fn is_inside_example_group(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        if let NodeKind::Block { call, .. } = *cx.kind(anc) {
            let NodeKind::Send {
                receiver, method, ..
            } = *cx.kind(call)
            else {
                continue;
            };
            if !is_rspec_or_bare_receiver(cx, receiver) {
                continue;
            }
            if is_example_group_name(cx.symbol_str(method)) {
                return true;
            }
        }
    }
    false
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

/// `true` when the source line immediately after the block's end line is
/// blank (empty or whitespace-only) or the block ends at EOF.
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

/// Range of the block's final line content, trimmed of leading
/// whitespace — mirrors `EmptyLineSeparation#offending_loc`.
fn final_line_content_range(cx: &Cx<'_>, node: NodeId) -> Option<Range> {
    let range = cx.range(node);
    let src = cx.source();
    let bytes = src.as_bytes();
    let Ok(end) = usize::try_from(range.end) else {
        return None;
    };
    let line_idx = line_of_offset(bytes, end.saturating_sub(1));
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
            return Some(Range {
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
    use super::EmptyLineAfterSubject;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_missing_blank_line() {
        test::<EmptyLineAfterSubject>().expect_offense(indoc! {r#"
                describe 'x' do
                  subject(:obj) { described_class }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Add an empty line after `subject`.
                  let(:foo) { bar }
                end
            "#});
    }

    #[test]
    fn does_not_flag_with_blank_line() {
        test::<EmptyLineAfterSubject>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  subject(:obj) { described_class }

                  let(:foo) { bar }
                end
            "#});
    }

    #[test]
    fn does_not_flag_outside_group() {
        test::<EmptyLineAfterSubject>().expect_no_offenses(indoc! {r#"
                subject { described_class }
                let(:foo) { bar }
            "#});
    }

    #[test]
    fn does_not_flag_last_child() {
        test::<EmptyLineAfterSubject>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  subject { described_class }
                end
            "#});
    }

    #[test]
    fn does_not_flag_non_subject() {
        test::<EmptyLineAfterSubject>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  let(:foo) { bar }
                  it { does_something }
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(EmptyLineAfterSubject);
