//! `RSpec/EmptyLineAfterHook` — require an empty line after hooks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/EmptyLineAfterHook
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`hook?`: `(any_block (send nil?
//!   #Hooks.all ...) ...)` bare receiver only) with
//!   `EmptyLineSeparation#missing_separating_line_offense`.
//!   `AllowConsecutiveOneLiners` (default true) skips a single-line hook
//!   directly followed by another single-line hook
//!   (`chained_single_line_hooks?`). A non-last hook flags when the line
//!   after its final `end` / `}` is not blank. The offense range is the
//!   final line's content (`offending_loc`), rebuilt here from the block
//!   range end line trimmed of leading whitespace. Detection is at parity
//!   for the common shapes; comment / `rubocop:enable` directive skipping,
//!   heredoc-aware `FinalEndLocation`, and `Numblock` handling are not
//!   ported in this batch (status: partial, autocorrect as gap — upstream
//!   inserts `\n`).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Block` whose call is a bare hook (`before`, `after`,
//! `around`, `prepend_before`, `append_before`, `prepend_after`,
//! `append_after`):
//!
//! - `before { x }` immediately followed by `it` — flagged.
//! - `before { x }` + blank line + `it` — not flagged.
//! - Back-to-back single-line hooks (`around { }; after { }`) — allowed by
//!   default (`AllowConsecutiveOneLiners: true`), not flagged; flagged
//!   when the option is false.
//! - Last hook in the group — `last_child?`, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream inserts a newline after the offense. This batch reports only.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLineAfterHook;

#[derive(CopOptions)]
pub struct EmptyLineAfterHookOptions {
    #[option(
        name = "AllowConsecutiveOneLiners",
        default = true,
        description = "Whether consecutive single-line hooks may touch without a blank line."
    )]
    pub allow_consecutive_one_liners: bool,
}

#[cop(
    name = "RSpec/EmptyLineAfterHook",
    description = "Checks if there is an empty line after hook blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = EmptyLineAfterHookOptions,
)]
impl EmptyLineAfterHook {
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
        if !is_hook_name(cx.symbol_str(method)) {
            return;
        }
        let opts = cx.options_or_default::<EmptyLineAfterHookOptions>();
        if opts.allow_consecutive_one_liners
            && is_single_line(cx, node)
            && next_is_single_line_hook(cx, node)
        {
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

fn is_hook_name(name: &str) -> bool {
    matches!(
        name,
        "before"
            | "after"
            | "around"
            | "prepend_before"
            | "append_before"
            | "prepend_after"
            | "append_after"
    )
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

/// `true` when the right sibling is a single-line hook block.
fn next_is_single_line_hook(cx: &Cx<'_>, node: NodeId) -> bool {
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
    if !is_hook_name(cx.symbol_str(method)) {
        return false;
    }
    is_single_line(cx, next)
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
    use super::{EmptyLineAfterHook, EmptyLineAfterHookOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn disallow_one_liners() -> EmptyLineAfterHookOptions {
        EmptyLineAfterHookOptions {
            allow_consecutive_one_liners: false,
        }
    }

    #[test]
    fn flags_missing_blank_line() {
        test::<EmptyLineAfterHook>().expect_offense(indoc! {r#"
                describe 'x' do
                  before { do_something }
                  ^^^^^^^^^^^^^^^^^^^^^^^ Add an empty line after `before`.
                  it { does_something }
                end
            "#});
    }

    #[test]
    fn does_not_flag_with_blank_line() {
        test::<EmptyLineAfterHook>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  before { do_something }

                  it { does_something }
                end
            "#});
    }

    #[test]
    fn allows_consecutive_one_liners_by_default() {
        test::<EmptyLineAfterHook>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  around { |test| test.run }
                  after { do_something }

                  it { does_something }
                end
            "#});
    }

    #[test]
    fn flags_consecutive_one_liners_when_disallowed() {
        test::<EmptyLineAfterHook>()
            .with_options(&disallow_one_liners())
            .expect_offense(indoc! {r#"
                describe 'x' do
                  around { |test| test.run }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^ Add an empty line after `around`.
                  after { do_something }
                end
            "#});
    }

    #[test]
    fn does_not_flag_last_child() {
        test::<EmptyLineAfterHook>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  before { do_something }
                end
            "#});
    }

    #[test]
    fn does_not_flag_non_hook() {
        test::<EmptyLineAfterHook>().expect_no_offenses(indoc! {r#"
                describe 'x' do
                  let(:a) { 1 }
                  it { does_something }
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(EmptyLineAfterHook);
