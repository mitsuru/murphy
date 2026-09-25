//! `RSpec/EmptyLineAfterExample` — require an empty line after examples.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/EmptyLineAfterExample
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`example?`: `(block (send nil?
//!   #Examples.all ...) ...)` bare receiver only) with
//!   `EmptyLineSeparation#missing_separating_line_offense`. A non-last
//!   example (its parent is a `Begin` and it is not the last child) flags
//!   when the line after its final `end` / `}` is not blank.
//!   `AllowConsecutiveOneLiners` (default true) skips back-to-back
//!   single-line examples (`node.single_line? &&
//!   next_sibling.single_line?`). The offense range is the final line's
//!   content (`offending_loc`: the `end` line for multiline blocks, the
//!   whole one-liner for brace blocks), rebuilt here from the block range
//!   end line trimmed of leading whitespace. Detection is at parity for
//!   the common shapes; comment / `rubocop:enable` directive skipping and
//!   heredoc-aware `FinalEndLocation` are not ported in this batch
//!   (status: partial, autocorrect as gap — upstream inserts `\n`).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is a bare example (`it`, `specify`,
//! `example`, `scenario`, `its`, focused / skipped / pending variants):
//!
//! - `it 'a' do; end` immediately followed by `it 'b'` — flagged (the
//!   first `end`).
//! - `it 'a' do; end` + blank line + `it 'b'` — not flagged.
//! - Last example in the group — `last_child?`, not flagged.
//! - `it { one }` + `it { two }` single-line pair — allowed by default
//!   (`AllowConsecutiveOneLiners: true`), not flagged; flagged when the
//!   option is false.
//!
//! ## No autocorrect
//!
//! Upstream inserts a newline after the offense. This batch reports only.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLineAfterExample;

#[derive(CopOptions)]
pub struct EmptyLineAfterExampleOptions {
    #[option(
        name = "AllowConsecutiveOneLiners",
        default = true,
        description = "Whether consecutive single-line examples may touch without a blank line."
    )]
    pub allow_consecutive_one_liners: bool,
}

#[cop(
    name = "RSpec/EmptyLineAfterExample",
    description = "Checks if there is an empty line after example blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = EmptyLineAfterExampleOptions,
)]
impl EmptyLineAfterExample {
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
        if !is_example_name(cx.symbol_str(method)) {
            return;
        }
        if is_last_child(cx, node) {
            return;
        }
        let opts = cx.options_or_default::<EmptyLineAfterExampleOptions>();
        if opts.allow_consecutive_one_liners
            && is_single_line(cx, node)
            && next_is_single_line_example(cx, node)
        {
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

fn is_example_name(name: &str) -> bool {
    matches!(
        name,
        "it" | "specify"
            | "example"
            | "scenario"
            | "its"
            | "fit"
            | "fspecify"
            | "fexample"
            | "fscenario"
            | "focus"
            | "xit"
            | "xspecify"
            | "xexample"
            | "xscenario"
            | "skip"
            | "pending"
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

/// `true` when the block range contains no newline (single-line).
fn is_single_line(cx: &Cx<'_>, node: NodeId) -> bool {
    let range = cx.range(node);
    let src = cx.source().as_bytes();
    let (Ok(lo), Ok(hi)) = (usize::try_from(range.start), usize::try_from(range.end)) else {
        return false;
    };
    if hi > src.len() || lo > hi {
        return false;
    }
    !src[lo..hi].contains(&b'\n')
}

/// `true` when the right sibling is a single-line example block.
fn next_is_single_line_example(cx: &Cx<'_>, node: NodeId) -> bool {
    let Some(next) = cx.right_sibling(node).get() else {
        return false;
    };
    let NodeKind::Block { call, .. } = *cx.kind(next) else {
        return false;
    };
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(call)
    else {
        return false;
    };
    if receiver != OptNodeId::NONE {
        return false;
    }
    if !is_example_name(cx.symbol_str(method)) {
        return false;
    }
    is_single_line(cx, next)
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
    // End line index (0-based): newlines before `end`.
    let end_line = bytes[..end.min(bytes.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count();
    let lines: Vec<&str> = src.lines().collect();
    // `lines()` drops the trailing empty; reconstruct blank check on the
    // raw split to handle EOF correctly.
    let raw: Vec<&str> = src.split('\n').collect();
    // Block end sits on `end_line` (0-based) when `end` is at EOL; when
    // `end` is mid-line (brace `}` + more text?) use the containing line.
    // The next line is `end_line + 1` in raw split terms when the block
    // ends exactly at a newline boundary, else the line after containing.
    let containing = line_of_offset(bytes, end);
    let next_idx = containing + 1;
    if next_idx >= raw.len() {
        return true;
    }
    // If block ends mid-line (e.g. `it { one }; foo`), the remainder of
    // the containing line is code, not a blank separator.
    let _ = (lines, end_line);
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
    let line_idx = line_of_offset(bytes, end.saturating_sub(1).max(0));
    // Rebuild line start/end offsets from raw splits.
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
    use super::{EmptyLineAfterExample, EmptyLineAfterExampleOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn disallow_one_liners() -> EmptyLineAfterExampleOptions {
        EmptyLineAfterExampleOptions {
            allow_consecutive_one_liners: false,
        }
    }

    #[test]
    fn flags_missing_blank_line() {
        test::<EmptyLineAfterExample>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  it 'does this' do
                  end
                  ^^^ Add an empty line after `it`.
                  it 'does that' do
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_with_blank_line() {
        test::<EmptyLineAfterExample>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  it 'does this' do
                  end

                  it 'does that' do
                  end
                end
            "#});
    }

    #[test]
    fn does_not_flag_last_child() {
        test::<EmptyLineAfterExample>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  it 'does this' do
                  end
                end
            "#});
    }

    #[test]
    fn allows_consecutive_one_liners_by_default() {
        test::<EmptyLineAfterExample>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  it { one }
                  it { two }
                end
            "#});
    }

    #[test]
    fn flags_consecutive_one_liners_when_disallowed() {
        test::<EmptyLineAfterExample>()
            .with_options(&disallow_one_liners())
            .expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  it { one }
                  ^^^^^^^^^^ Add an empty line after `it`.
                  it { two }
                end
            "#});
    }

    #[test]
    fn does_not_flag_non_example() {
        test::<EmptyLineAfterExample>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  before do
                  end
                  it 'x' do
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(EmptyLineAfterExample);
