//! `RSpec/EmptyLineAfterExampleGroup` — require an empty line after example groups.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/EmptyLineAfterExampleGroup
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`spec_group?`: `(any_block (send #rspec?
//!   {#SharedGroups.all #ExampleGroups.all} ...) ...)` RSpec-or-bare
//!   receiver) with `EmptyLineSeparation#missing_separating_line_offense`.
//!   A non-last group (its parent is a `Begin` and it is not the last
//!   child) flags when the line after its final `end` / `}` is not blank.
//!   The offense range is the final line's content (`offending_loc`),
//!   rebuilt here from the block range end line trimmed of leading
//!   whitespace. Detection is at parity for the common shapes; comment /
//!   `rubocop:enable` directive skipping, heredoc-aware `FinalEndLocation`,
//!   and `Numblock` handling are not ported in this batch (status:
//!   partial, autocorrect as gap — upstream inserts `\n`).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is an RSpec-or-bare example group
//! (`describe`, `context`, `feature`, `example_group`, skipped / focused
//! variants, plus `shared_examples` / `shared_context` family):
//!
//! - `describe '#a' do; end` immediately followed by `describe '#b'` —
//!   flagged (the first `end`).
//! - `describe '#a' do; end` + blank line + `describe '#b'` — not flagged.
//! - Last group in the file — `last_child?`, not flagged.
//! - `it 'x' do; end` — not a group, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream inserts a newline after the offense. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

use crate::cops::rspec_helpers::{is_example_group_name, is_rspec_or_bare_receiver};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLineAfterExampleGroup;

#[cop(
    name = "RSpec/EmptyLineAfterExampleGroup",
    description = "Checks if there is an empty line after example group blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EmptyLineAfterExampleGroup {
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
        if !is_rspec_or_bare_receiver(cx, receiver) {
            return;
        }
        if !is_group_name(cx.symbol_str(method)) {
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
            cx.emit_offense(
                range,
                &format!("Add an empty line after `{method}`."),
                None,
            );
        }
    }
}

/// `true` when `name` is an example group or shared group selector
/// (`spec_group?`: `{#SharedGroups.all #ExampleGroups.all}`).
fn is_group_name(name: &str) -> bool {
    is_example_group_name(name) || is_shared_group_name(name)
}

fn is_shared_group_name(name: &str) -> bool {
    matches!(
        name,
        "shared_examples" | "shared_examples_for" | "shared_context"
    )
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
    use super::EmptyLineAfterExampleGroup;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_missing_blank_line() {
        test::<EmptyLineAfterExampleGroup>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  describe '#bar' do
                  end
                  ^^^ Add an empty line after `describe`.
                  describe '#baz' do
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_with_blank_line() {
        test::<EmptyLineAfterExampleGroup>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  describe '#bar' do
                  end

                  describe '#baz' do
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_last_child() {
        test::<EmptyLineAfterExampleGroup>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  describe '#bar' do
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_non_group() {
        test::<EmptyLineAfterExampleGroup>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  it 'x' do
                  end
                  it 'y' do
                  end
                end
            "#});
    }

    #[test]
    fn flags_shared_group_without_blank_line() {
        test::<EmptyLineAfterExampleGroup>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  shared_examples 'a' do
                  end
                  ^^^ Add an empty line after `shared_examples`.
                  describe '#b' do
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_non_rspec_receiver() {
        test::<EmptyLineAfterExampleGroup>().expect_no_offenses(indoc! {r#"
                Other.describe 'a' do
                end
                Other.describe 'b' do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(EmptyLineAfterExampleGroup);
