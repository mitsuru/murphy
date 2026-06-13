//! `Layout/EmptyLinesAroundAccessModifier` — requires blank lines around bare
//! access modifiers (`public`/`protected`/`private`/`module_function`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/EmptyLinesAroundAccessModifier
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: [murphy-xjua]
//! notes: >
//!   Ports `on_send` plus the `around`/`only_before` `EnforcedStyle`
//!   helpers. RuboCop tracks the enclosing class/module/sclass and block
//!   via per-file ivars set in `on_class`/`on_module`/`on_sclass`/`on_block`;
//!   Murphy is stateless and derives those from the modifier's nearest
//!   enclosing ancestors (`cx.parent` walk). Class/module/sclass-bodied
//!   modifiers take the simple path; in-block modifiers honour the
//!   `no_empty_lines` default of `Layout/EmptyLinesAroundBlockBody`
//!   (first-child suppresses the before-insert, last-child suppresses the
//!   after-insert). Messages match RuboCop's four context-dependent
//!   strings. Autocorrect inserts/removes blank lines per style.
//!   GAP (murphy-xjua): the cross-cop config lookup
//!   (`no_empty_lines_around_block_body?`) is NOT read dynamically — the
//!   single-surface ABI gives a cop only its own options, so this cop
//!   assumes `Layout/EmptyLinesAroundBlockBody`'s default `no_empty_lines`.
//!   Diverges only when that cop is configured to `empty_lines` AND the
//!   modifier sits inside a `do…end`/`{}` block.
//! ```
//!
//! ## Algorithm
//!
//! `on_send` fires for bare access modifiers. The cop derives the enclosing
//! class/module/sclass first/last line and the enclosing block first line,
//! then mirrors RuboCop's `expected_empty_lines?`, `message`, and the
//! before/after correction split.

use crate::cops::util::{line_is_blank, line_is_comment, line_of, nth_line_start};
use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG_AFTER: &str = "Keep a blank line after `";
const MSG_BEFORE_AND_AFTER: &str = "Keep a blank line before and after `";
const MSG_BEFORE_FOR_ONLY_BEFORE: &str = "Keep a blank line before `";
const MSG_AFTER_FOR_ONLY_BEFORE: &str = "Remove a blank line after `";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EmptyLinesAroundAccessModifier;

#[derive(CopOptions)]
pub struct EmptyLinesAroundAccessModifierOptions {
    #[option(
        name = "EnforcedStyle",
        default = "around",
        description = "Whether to require blank lines `around` the modifier or `only_before` it."
    )]
    pub enforced_style: AccessModifierBlankLineStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum AccessModifierBlankLineStyle {
    /// Blank line both before and after the modifier.
    #[option(value = "around")]
    Around,
    /// Blank line only before; no blank line after.
    #[option(value = "only_before")]
    OnlyBefore,
}

#[cop(
    name = "Layout/EmptyLinesAroundAccessModifier",
    description = "Keep blank lines around access modifiers.",
    default_severity = "warning",
    default_enabled = true,
    options = EmptyLinesAroundAccessModifierOptions
)]
impl EmptyLinesAroundAccessModifier {
    #[on_node(kind = "send", methods = ["public", "protected", "private", "module_function"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `return unless node.bare_access_modifier? && !node.block_literal?`
        if !cx.is_bare_access_modifier(node) || cx.block_node(node).get().is_some() {
            return;
        }
        let style = cx
            .options_or_default::<EmptyLinesAroundAccessModifierOptions>()
            .enforced_style;

        let ctx = Context::derive(node, cx);

        // `return if same_line?(node, node.right_sibling)`
        if same_line_as_right_sibling(node, cx) {
            return;
        }
        // `return if expected_empty_lines?(node)`
        if expected_empty_lines(node, &ctx, style, cx) {
            return;
        }

        let selector = cx.selector(node);
        let modifier = cx.raw_source(selector);
        let message = message(node, &ctx, style, cx, modifier);
        cx.emit_offense(cx.range(node), &message, None);

        // Autocorrect (before / after per style).
        if should_insert_line_before(node, &ctx, cx) {
            let line_start = nth_line_start(cx, ctx.send_first_line).unwrap_or(0);
            cx.emit_edit(
                Range {
                    start: line_start,
                    end: line_start,
                },
                "\n",
            );
        }
        correct_next_line_if_denied_style(node, &ctx, style, cx);
    }
}

/// The enclosing-context lines RuboCop holds in ivars.
struct Context {
    send_first_line: u32,
    send_last_line: u32,
    /// 0-based first line of the enclosing class/module/sclass definition,
    /// or `None` when the modifier is not inside one.
    class_first_line: Option<u32>,
    /// 0-based last line of the enclosing class/module/sclass.
    class_last_line: Option<u32>,
    /// 0-based first line of the enclosing block, or `None`.
    block_line: Option<u32>,
}

impl Context {
    fn derive(node: NodeId, cx: &Cx<'_>) -> Self {
        let send_range = cx.range(node);
        let send_first_line = line_of(send_range.start, cx);
        let send_last_line = line_of(send_range.end.saturating_sub(1), cx);

        let mut class_first_line = None;
        let mut class_last_line = None;
        let mut block_line = None;

        let mut current = cx.parent(node);
        while let Some(ancestor) = current.get() {
            match cx.kind(ancestor) {
                NodeKind::Class { superclass, .. } if class_first_line.is_none() => {
                    // `parent_class.first_line` if a superclass is present,
                    // else the class node's first line.
                    let first = superclass
                        .get()
                        .map_or(cx.range(ancestor).start, |sc| cx.range(sc).start);
                    class_first_line = Some(line_of(first, cx));
                    class_last_line =
                        Some(line_of(cx.range(ancestor).end.saturating_sub(1), cx));
                }
                NodeKind::Module { .. } if class_first_line.is_none() => {
                    class_first_line = Some(line_of(cx.range(ancestor).start, cx));
                    class_last_line =
                        Some(line_of(cx.range(ancestor).end.saturating_sub(1), cx));
                }
                NodeKind::Sclass { expr, .. } if class_first_line.is_none() => {
                    class_first_line = Some(line_of(cx.range(*expr).start, cx));
                    class_last_line =
                        Some(line_of(cx.range(ancestor).end.saturating_sub(1), cx));
                }
                _ if cx.is_any_block_type(ancestor) && block_line.is_none() => {
                    block_line = Some(line_of(cx.range(ancestor).start, cx));
                }
                _ => {}
            }
            current = cx.parent(ancestor);
        }

        Context {
            send_first_line,
            send_last_line,
            class_first_line,
            class_last_line,
            block_line,
        }
    }
}

/// `same_line?(node, node.right_sibling)` — nil sibling yields false.
fn same_line_as_right_sibling(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(sibling) = cx.right_sibling(node).get() else {
        return false;
    };
    line_of(cx.range(node).end.saturating_sub(1), cx) == line_of(cx.range(sibling).start, cx)
}

fn expected_empty_lines(
    node: NodeId,
    ctx: &Context,
    style: AccessModifierBlankLineStyle,
    cx: &Cx<'_>,
) -> bool {
    match style {
        AccessModifierBlankLineStyle::Around => empty_lines_around(ctx, cx),
        AccessModifierBlankLineStyle::OnlyBefore => allowed_only_before_style(node, ctx, cx),
    }
}

/// `allowed_only_before_style?`
fn allowed_only_before_style(node: NodeId, ctx: &Context, cx: &Cx<'_>) -> bool {
    if cx.is_special_modifier(node) {
        // `return true if processed_source[node.last_line] == 'end'`
        if line_trimmed_eq(ctx.send_last_line + 1, "end", cx) {
            return true;
        }
        // `return false if next_line_empty_and_exists?(node.last_line)`
        if next_line_empty_and_exists(ctx, cx) {
            return false;
        }
    }
    previous_line_empty(ctx, cx)
}

/// `empty_lines_around?` — blank before and after.
fn empty_lines_around(ctx: &Context, cx: &Cx<'_>) -> bool {
    previous_line_empty(ctx, cx) && next_line_empty(ctx, cx)
}

/// `previous_line_empty?(send_line)` — the nearest non-comment line above the
/// modifier is blank (comments are transparent), or the modifier sits at a
/// block/class-definition start, or there is no preceding line.
fn previous_line_empty(ctx: &Context, cx: &Cx<'_>) -> bool {
    if block_start(ctx) || class_def(ctx) {
        return true;
    }
    // `previous_line_ignoring_comments` — scan upward skipping comment lines.
    let mut line = ctx.send_first_line;
    while line > 0 {
        line -= 1;
        if line_is_comment(cx, line) {
            continue;
        }
        return line_is_blank(cx, line);
    }
    // No non-comment line above → true.
    true
}

/// `next_line_empty?(last_send_line)` — the modifier is the last body line, or
/// the line after it is blank.
fn next_line_empty(ctx: &Context, cx: &Cx<'_>) -> bool {
    body_end(ctx) || line_is_blank(cx, ctx.send_last_line + 1)
}

/// `next_line_empty_and_exists?(last_send_line)`
fn next_line_empty_and_exists(ctx: &Context, cx: &Cx<'_>) -> bool {
    next_line_empty(ctx, cx) && (ctx.send_last_line + 1) as usize != total_lines(cx)
}

/// `class_def?(line)` — the modifier is the first line of the class/module body.
fn class_def(ctx: &Context) -> bool {
    ctx.class_first_line
        .is_some_and(|first| ctx.send_first_line == first + 1)
}

/// `block_start?(line)` — the modifier is the first line of the block body.
fn block_start(ctx: &Context) -> bool {
    ctx.block_line
        .is_some_and(|first| ctx.send_first_line == first + 1)
}

/// `body_end?(line)` — the modifier is the last line of the class/module body.
fn body_end(ctx: &Context) -> bool {
    ctx.class_last_line
        .is_some_and(|last| ctx.send_last_line == last.saturating_sub(1))
}

/// `message(node)` — the four context-dependent strings.
fn message(
    node: NodeId,
    ctx: &Context,
    style: AccessModifierBlankLineStyle,
    cx: &Cx<'_>,
    modifier: &str,
) -> String {
    match style {
        AccessModifierBlankLineStyle::Around => {
            if block_start(ctx) || class_def(ctx) {
                format!("{MSG_AFTER}{modifier}`.")
            } else {
                format!("{MSG_BEFORE_AND_AFTER}{modifier}`.")
            }
        }
        AccessModifierBlankLineStyle::OnlyBefore => {
            let _ = node;
            if next_line_empty(ctx, cx) {
                format!("{MSG_AFTER_FOR_ONLY_BEFORE}{modifier}`.")
            } else {
                format!("{MSG_BEFORE_FOR_ONLY_BEFORE}{modifier}`.")
            }
        }
    }
}

/// `should_insert_line_before?`
fn should_insert_line_before(node: NodeId, ctx: &Context, cx: &Cx<'_>) -> bool {
    if previous_line_empty(ctx, cx) {
        return false;
    }
    // `return true unless inside_block? && no_empty_lines_around_block_body?`
    // `no_empty_lines_around_block_body?` is assumed true (the upstream
    // default). So when the modifier is inside a block, fall through to the
    // begin/first-child guard.
    if !inside_block(node, cx) {
        return true;
    }
    // `return true unless node.parent.begin_type?`
    let Some(parent) = cx.parent(node).get() else {
        return true;
    };
    if !matches!(cx.kind(parent), NodeKind::Begin(_)) {
        return true;
    }
    // `node.parent.children.first != node`
    first_child(parent, cx) != Some(node)
}

/// `should_insert_line_after?`
fn should_insert_line_after(node: NodeId, cx: &Cx<'_>) -> bool {
    if !inside_block(node, cx) {
        return true;
    }
    let Some(parent) = cx.parent(node).get() else {
        return true;
    };
    if !matches!(cx.kind(parent), NodeKind::Begin(_)) {
        // Single-statement body: node is its own first-and-last child.
        return false;
    }
    last_child(parent, cx) != Some(node)
}

/// `correct_next_line_if_denied_style`
fn correct_next_line_if_denied_style(
    node: NodeId,
    ctx: &Context,
    style: AccessModifierBlankLineStyle,
    cx: &Cx<'_>,
) {
    if !should_insert_line_after(node, cx) {
        return;
    }
    match style {
        AccessModifierBlankLineStyle::Around => {
            if !next_line_empty(ctx, cx) {
                // Insert `\n` after the modifier's whole line.
                let line_start = nth_line_start(cx, ctx.send_last_line).unwrap_or(0);
                let line_end = whole_line_end(line_start, cx);
                cx.emit_edit(
                    Range {
                        start: line_end,
                        end: line_end,
                    },
                    "\n",
                );
            }
        }
        AccessModifierBlankLineStyle::OnlyBefore => {
            if next_line_empty_and_exists(ctx, cx) {
                // Remove the blank line directly after the modifier.
                if let Some(start) = nth_line_start(cx, ctx.send_last_line + 1) {
                    let end = whole_line_end(start, cx);
                    cx.emit_edit(Range { start, end }, "");
                }
            }
        }
    }
}

/// `inside_block?` — `node.parent.block_type? || (node.parent.begin_type? &&
/// node.parent.parent&.block_type?)`.
fn inside_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if cx.is_any_block_type(parent) {
        return true;
    }
    if matches!(cx.kind(parent), NodeKind::Begin(_))
        && let Some(grandparent) = cx.parent(parent).get()
    {
        return cx.is_any_block_type(grandparent);
    }
    false
}

/// First child node of a `Begin` (the parent body wrapper).
fn first_child(parent: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match cx.kind(parent) {
        NodeKind::Begin(list) => cx.list(*list).first().copied(),
        _ => None,
    }
}

fn last_child(parent: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match cx.kind(parent) {
        NodeKind::Begin(list) => cx.list(*list).last().copied(),
        _ => None,
    }
}

/// Byte offset just past the `\n` terminating the line that begins at
/// `line_start` (or EOF for the final line).
fn whole_line_end(line_start: u32, cx: &Cx<'_>) -> u32 {
    let bytes = cx.source().as_bytes();
    let start = (line_start as usize).min(bytes.len());
    bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |pos| start + pos + 1) as u32
}

/// `processed_source[line] == text` after trimming, for 0-based `line`.
fn line_trimmed_eq(line: u32, text: &str, cx: &Cx<'_>) -> bool {
    let Some(start) = nth_line_start(cx, line) else {
        return false;
    };
    let bytes = cx.source().as_bytes();
    let start = start as usize;
    if start > bytes.len() {
        return false;
    }
    let end = bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |pos| start + pos);
    cx.raw_source(Range {
        start: start as u32,
        end: end as u32,
    })
    .trim()
        == text
}

/// `processed_source.lines.size` — number of physical lines (a trailing
/// newline does not add an empty final line).
fn total_lines(cx: &Cx<'_>) -> usize {
    let src = cx.source();
    if src.is_empty() {
        return 0;
    }
    let nl = src.as_bytes().iter().filter(|&&b| b == b'\n').count();
    if src.ends_with('\n') { nl } else { nl + 1 }
}

#[cfg(test)]
mod tests {
    use super::EmptyLinesAroundAccessModifier;
    use murphy_plugin_api::test_support::{run_cop, run_cop_with_edits, test};

    #[test]
    fn flags_missing_blanks_around_private() {
        let src = "class Foo\n  def bar; end\n  private\n  def baz; end\nend\n";
        let offenses = run_cop::<EmptyLinesAroundAccessModifier>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(offenses[0].message, "Keep a blank line before and after `private`.");
    }

    #[test]
    fn accepts_blanks_around_private() {
        test::<EmptyLinesAroundAccessModifier>().expect_no_offenses(
            "class Foo\n  def bar; end\n\n  private\n\n  def baz; end\nend\n",
        );
    }

    #[test]
    fn accepts_private_at_class_start() {
        // No blank required *before* a modifier on the first body line, but a
        // blank IS required after. This has the after-blank.
        test::<EmptyLinesAroundAccessModifier>()
            .expect_no_offenses("class Foo\n  private\n\n  def baz; end\nend\n");
    }

    #[test]
    fn flags_missing_after_at_class_start() {
        let src = "class Foo\n  private\n  def baz; end\nend\n";
        let offenses = run_cop::<EmptyLinesAroundAccessModifier>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(offenses[0].message, "Keep a blank line after `private`.");
    }

    #[test]
    fn accepts_private_at_body_end() {
        // A modifier on the last body line needs no after-blank, but needs a
        // before-blank.
        test::<EmptyLinesAroundAccessModifier>()
            .expect_no_offenses("class Foo\n  def bar; end\n\n  private\nend\n");
    }

    #[test]
    fn comments_are_transparent_for_before_check() {
        // Blank line, then a comment, then the modifier → before-blank is
        // satisfied (comment is transparent).
        test::<EmptyLinesAroundAccessModifier>().expect_no_offenses(
            "class Foo\n  def bar; end\n\n  # a comment\n  private\n\n  def baz; end\nend\n",
        );
    }

    #[test]
    fn corrects_missing_blanks() {
        let src = "class Foo\n  def bar; end\n  private\n  def baz; end\nend\n";
        let result = run_cop_with_edits::<EmptyLinesAroundAccessModifier>(src);
        assert_eq!(result.offenses.len(), 1);
        // Two edits: insert a blank before and after the modifier line.
        assert_eq!(result.edits.len(), 2);
        assert!(result.edits.iter().all(|e| e.replacement == "\n"));
    }

    #[test]
    fn accepts_modifier_inside_module() {
        test::<EmptyLinesAroundAccessModifier>().expect_no_offenses(
            "module M\n  def a; end\n\n  private\n\n  def b; end\nend\n",
        );
    }

    #[test]
    fn flags_inside_block_class_new() {
        // `Class.new do … private … end` — the in-block branch.
        let src = "Class.new do\n  def bar; end\n  private\n  def baz; end\nend\n";
        let offenses = run_cop::<EmptyLinesAroundAccessModifier>(src);
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
    }

    // only_before style.

    fn only_before_opts() -> super::EmptyLinesAroundAccessModifierOptions {
        super::EmptyLinesAroundAccessModifierOptions {
            enforced_style: super::AccessModifierBlankLineStyle::OnlyBefore,
        }
    }

    #[test]
    fn only_before_accepts_no_blank_after() {
        let offenses = murphy_plugin_api::test_support::run_cop_with_options::<
            EmptyLinesAroundAccessModifier,
        >(
            "class Foo\n  def bar; end\n\n  private\n  def baz; end\nend\n",
            &only_before_opts(),
        );
        assert!(offenses.is_empty(), "expected no offenses, got {offenses:?}");
    }

    #[test]
    fn only_before_flags_blank_after() {
        let offenses = murphy_plugin_api::test_support::run_cop_with_options::<
            EmptyLinesAroundAccessModifier,
        >(
            "class Foo\n  def bar; end\n\n  private\n\n  def baz; end\nend\n",
            &only_before_opts(),
        );
        assert_eq!(offenses.len(), 1, "expected 1 offense, got {offenses:?}");
        assert_eq!(offenses[0].message, "Remove a blank line after `private`.");
    }
}

murphy_plugin_api::submit_cop!(EmptyLinesAroundAccessModifier);
