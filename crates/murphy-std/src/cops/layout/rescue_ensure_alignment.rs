//! `Layout/RescueEnsureAlignment` — checks that `rescue` and `ensure`
//! keywords align with the column of their enclosing block's anchor keyword.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/RescueEnsureAlignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports the common alignment path: the `rescue`/`ensure` keyword must
//!   share the column of its enclosing anchor (an explicit `begin...end`,
//!   `def`/`defs`, `class`/`module`/`sclass`, or block opener). Covers the
//!   assignment-RHS anchor (`x = begin...rescue`) and access-modifier anchor
//!   (`private def ... rescue`) cases. Modifier-form `rescue` (`foo rescue
//!   bar`) is skipped — its keyword is mid-line, never line-leading.
//!
//!   Murphy collapses parser-gem's `:kwbegin` and `:begin` into a single
//!   `NodeKind::Begin`; an explicit `begin...end` is distinguished by its
//!   source starting with the `begin` keyword (parser-gem `:kwbegin`),
//!   whereas an implicit method-body wrapper (`:begin`) does not and is
//!   skipped during the ancestor walk.
//!
//!   Gaps vs RuboCop (documented, not silently dropped):
//!     * `Layout/BeginEndAlignment: EnforcedStyleAlignWith: start_of_line`
//!       — cross-cop config that shifts the alignment column to the start of
//!       the anchor's line. Murphy always aligns to the anchor keyword.
//!     * `aligned_with_line_break_method?` — the leading-dot / selector
//!       alignment refinement for blocks chained off a multi-line method
//!       call. Murphy uses the block opener column unconditionally.
//! ```
//!
//! ## Autocorrect
//!
//! Replace the leading whitespace on the keyword's line (line start →
//! keyword start) with `new_column` spaces, but only when that span is
//! entirely whitespace (RuboCop's `whitespace.source.strip.empty?` guard).

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RescueEnsureAlignment;

#[cop(
    name = "Layout/RescueEnsureAlignment",
    description = "Align `rescue` and `ensure` with their enclosing block keyword.",
    default_severity = "warning",
    default_enabled = true
)]
impl RescueEnsureAlignment {
    #[on_node(kind = "resbody")]
    fn check_resbody(&self, node: NodeId, cx: &Cx<'_>) {
        // The `Resbody` node's range spans `rescue` through its body, so trim
        // to just the leading `rescue` keyword (prism `RescueNode` location
        // begins at `rescue`). Guard against the range not actually starting
        // with `rescue` (defensive — bounds-safe).
        let range = cx.range(node);
        let kw_end = range.start + b"rescue".len() as u32;
        if kw_end > range.end {
            return;
        }
        let kw_range = Range {
            start: range.start,
            end: kw_end,
        };
        if cx.raw_source(kw_range) != "rescue" {
            return;
        }
        check(node, kw_range, cx);
    }

    #[on_node(kind = "ensure")]
    fn check_ensure(&self, node: NodeId, cx: &Cx<'_>) {
        // The `Ensure` node carries the *whole* begin/def block range (the
        // translator pushes it with the BeginNode range), so we must token-
        // scan for the `ensure` keyword to get its position.
        let Some(kw_range) = ensure_keyword_range(node, cx) else {
            return;
        };
        check(node, kw_range, cx);
    }
}

fn check(node: NodeId, kw_range: Range, cx: &Cx<'_>) {
    let source = cx.source();

    // Modifier-form `rescue` (`foo rescue bar`) is never line-leading — its
    // keyword sits mid-line. RuboCop skips these (`modifier?`). A line-leading
    // keyword (only whitespace before it on its line) is block-form.
    if !is_line_leading(source, kw_range.start) {
        return;
    }

    let Some(anchor) = alignment_node(node, cx) else {
        return;
    };
    let alignment_start = alignment_location(anchor, cx);

    let kw_col = column_of(source, kw_range.start);
    let anchor_col = column_of(source, alignment_start);

    // Aligned already, or on the same line as the anchor → no offense.
    if kw_col == anchor_col || line_of(source, kw_range.start) == line_of(source, alignment_start) {
        return;
    }

    let kw_text = cx.raw_source(kw_range);
    let kw_line = line_of(source, kw_range.start);
    let anchor_line = line_of(source, alignment_start);
    let beginning = cx.raw_source(Range {
        start: alignment_start,
        end: anchor_end(anchor, alignment_start, cx),
    });
    let message = format!(
        "`{kw_text}` at {kw_line}, {kw_col} is not aligned with `{beginning}` at {anchor_line}, {anchor_col}."
    );
    cx.emit_offense(kw_range, &message, None);

    // Autocorrect: replace line-leading whitespace with `anchor_col` spaces,
    // only if that span is entirely whitespace.
    let line_start = line_start_of(source, kw_range.start);
    let leading = &source[line_start as usize..kw_range.start as usize];
    if leading.bytes().all(|b| b == b' ' || b == b'\t') {
        cx.emit_edit(
            Range {
                start: line_start,
                end: kw_range.start,
            },
            &" ".repeat(anchor_col),
        );
    }
}

/// Walk ancestors to find the alignment anchor, applying RuboCop's
/// refinements for assignment RHS and access modifiers.
fn alignment_node(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let ancestor = ancestor_anchor(node, cx)?;

    // An explicit `begin...end` (parser-gem `:kwbegin`) is its own anchor; no
    // refinement applies.
    if is_kwbegin(ancestor, cx) {
        return Some(ancestor);
    }

    // Assignment RHS: `x = begin ... rescue`. If the anchor's parent is an
    // assignment and they share a line, align to the assignment target.
    if let Some(assignment) = assignment_parent(ancestor, cx)
        && line_of(cx.source(), cx.range(ancestor).start)
            == line_of(cx.source(), cx.range(assignment).start)
    {
        return Some(assignment);
    }

    // Access modifier: `private def ... rescue`. If the anchor is a def whose
    // parent is an access modifier (`private`/`protected`/`public` or
    // `private_class_method`/`public_class_method`), align to the modifier.
    if let Some(modifier) = access_modifier_parent(ancestor, cx) {
        return Some(modifier);
    }

    Some(ancestor)
}

/// First ancestor that is a valid anchor type. A bare `Begin` (parser-gem
/// `:begin`, the implicit method-body wrapper) is NOT an anchor — skip it and
/// keep walking to the enclosing def/class/etc.
fn ancestor_anchor(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    for ancestor in cx.ancestors(node) {
        match *cx.kind(ancestor) {
            NodeKind::Kwbegin(_) => return Some(ancestor),
            // An explicit `begin...end` (`begin` prefix) is an anchor; a bare
            // `:begin` (implicit method-body wrapper) is not — keep walking.
            NodeKind::Begin(_) if is_kwbegin(ancestor, cx) => return Some(ancestor),
            NodeKind::Begin(_) => {}
            NodeKind::Def { .. }
            | NodeKind::Defs { .. }
            | NodeKind::Class { .. }
            | NodeKind::Sclass { .. }
            | NodeKind::Module { .. }
            | NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Itblock { .. } => return Some(ancestor),
            _ => {}
        }
    }
    None
}

/// `Begin` node whose source starts with the `begin` keyword (word boundary) —
/// parser-gem's `:kwbegin`.
fn is_kwbegin(node: NodeId, cx: &Cx<'_>) -> bool {
    if matches!(*cx.kind(node), NodeKind::Kwbegin(_)) {
        return true;
    }
    if !matches!(*cx.kind(node), NodeKind::Begin(_)) {
        return false;
    }
    let src = cx.raw_source(cx.range(node));
    src.strip_prefix("begin")
        .is_some_and(|rest| rest.is_empty() || !is_ident_byte(rest.as_bytes()[0]))
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// If `node`'s parent is an assignment, return it.
fn assignment_parent(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let parent = cx.parent(node).get()?;
    if cx.is_assignment(parent) {
        Some(parent)
    } else {
        None
    }
}

/// If `node` is a def whose parent is an access modifier call, return the
/// modifier node.
fn access_modifier_parent(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    if !cx.is_any_def_type(node) {
        return None;
    }
    let parent = cx.parent(node).get()?;
    if cx.is_access_modifier(parent) {
        return Some(parent);
    }
    if cx.call_receiver(parent).get().is_none()
        && cx
            .method_name(parent)
            .is_some_and(|m| matches!(m, "private_class_method" | "public_class_method"))
    {
        return Some(parent);
    }
    None
}

/// The byte offset to align to — the anchor keyword's start.
fn alignment_location(anchor: NodeId, cx: &Cx<'_>) -> u32 {
    cx.range(anchor).start
}

/// End offset for the `beginning` snippet shown in the message, mirroring
/// RuboCop's `alignment_source`:
///   * `begin` / block / kwbegin → the opener keyword/brace.
///   * `def`/`defs`/`class`/`module`/`sclass` → through the construct name.
///   * assignment / access modifier → through the RHS construct opener
///     (`begin` / `do` / `{`).
///
/// Murphy does not populate `loc.name` for these constructs, so the end is
/// computed from the anchor's first source line via tokens.
fn anchor_end(anchor: NodeId, start: u32, cx: &Cx<'_>) -> u32 {
    let anchor_range = cx.range(anchor);
    match *cx.kind(anchor) {
        // `begin` keyword opener.
        NodeKind::Begin(_) | NodeKind::Kwbegin(_) => start + b"begin".len() as u32,
        // `def NAME` / `def self.NAME` — end at the method name.
        NodeKind::Def { .. } | NodeKind::Defs { .. } => def_header_end(anchor_range, cx),
        // `class NAME` / `module NAME` / `class << NAME` — end at the name.
        NodeKind::Class { .. } | NodeKind::Module { .. } | NodeKind::Sclass { .. } => {
            class_header_end(anchor_range, cx)
        }
        // Assignment / access modifier — end at the RHS construct opener
        // (`begin` / `do` / `{`) on the anchor's first line.
        _ => construct_opener_end(anchor_range, cx).unwrap_or_else(|| {
            // Fall back to the first line's end.
            first_line_end(cx.source(), anchor_range)
        }),
    }
}

/// End offset of `def NAME` (or `def self.NAME`) — through the method name.
fn def_header_end(range: Range, cx: &Cx<'_>) -> u32 {
    method_name_end(range, cx).unwrap_or_else(|| first_line_end(cx.source(), range))
}

/// Find the end of the method name in a `def`/`defs` header.
fn method_name_end(range: Range, cx: &Cx<'_>) -> Option<u32> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < range.start);
    let line_toks: Vec<_> = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < range.end && t.kind != SourceTokenKind::Newline)
        .collect();
    // First token is `def`. The name is everything up to the first `(` or the
    // 2nd whitespace-separated identifier group.
    // Walk: skip `def`; the name spans `self` `.` `NAME` or just `NAME`.
    let mut i = 0;
    // skip `def`
    if i < line_toks.len()
        && &source[line_toks[i].range.start as usize..line_toks[i].range.end as usize] == b"def"
    {
        i += 1;
    }
    let mut end = None;
    while i < line_toks.len() {
        let tok = line_toks[i];
        if tok.kind == SourceTokenKind::LeftParen {
            break;
        }
        let text = &source[tok.range.start as usize..tok.range.end as usize];
        // `self` and `.` are part of a singleton name; keep going.
        if text == b"self" || text == b"." {
            end = Some(tok.range.end);
            i += 1;
            continue;
        }
        // The first identifier after `def`/`self.` is the method name.
        end = Some(tok.range.end);
        break;
    }
    end
}

/// End offset of `class NAME` / `module NAME` / `class << NAME` — through the
/// constant/identifier name.
fn class_header_end(range: Range, cx: &Cx<'_>) -> u32 {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < range.start);
    let line_toks: Vec<_> = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < range.end && t.kind != SourceTokenKind::Newline)
        .collect();
    // Skip the `class`/`module` keyword, then take the name token(s)
    // (`<<` + identifier for sclass, or the constant path).
    let mut end = range.start;
    let mut i = 0;
    if i < line_toks.len() {
        i += 1; // skip class/module keyword
    }
    while i < line_toks.len() {
        let tok = line_toks[i];
        let text = &source[tok.range.start as usize..tok.range.end as usize];
        // Stop at superclass `<` (but not `<<` for sclass).
        if text == b"<" {
            break;
        }
        end = tok.range.end;
        i += 1;
    }
    if end == range.start {
        first_line_end(cx.source(), range)
    } else {
        end
    }
}

/// End offset of the first `begin` / `do` / `{` construct opener on the
/// anchor's first line.
fn construct_opener_end(range: Range, cx: &Cx<'_>) -> Option<u32> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < range.start);
    let line_end = first_line_end(cx.source(), range);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < line_end)
        .find(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other
                    && matches!(
                        &source[t.range.start as usize..t.range.end as usize],
                        b"begin" | b"do"
                    ))
        })
        .map(|t| t.range.end)
}

/// End offset of the first line of `range`.
fn first_line_end(source: &str, range: Range) -> u32 {
    let bytes = source.as_bytes();
    bytes[range.start as usize..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(range.end, |i| range.start + i as u32)
        .min(range.end)
}

/// Scan for the `ensure` keyword token within the node's range.
fn ensure_keyword_range(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < node_range.start);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.end <= node_range.end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"ensure"
        })
        .map(|t| t.range)
}

/// True if only whitespace precedes `offset` on its line.
fn is_line_leading(source: &str, offset: u32) -> bool {
    let line_start = line_start_of(source, offset);
    source[line_start as usize..offset as usize]
        .bytes()
        .all(|b| b == b' ' || b == b'\t')
}

fn line_start_of(source: &str, offset: u32) -> u32 {
    source.as_bytes()[..offset as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1) as u32
}

/// 1-based line number of `offset`.
fn line_of(source: &str, offset: u32) -> usize {
    source.as_bytes()[..offset as usize]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// 0-based column (character count) of `offset`.
fn column_of(source: &str, offset: u32) -> usize {
    let line_start = line_start_of(source, offset) as usize;
    source[line_start..offset as usize].chars().count()
}

#[cfg(test)]
mod tests {
    use super::RescueEnsureAlignment;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_misaligned_rescue_in_begin() {
        test::<RescueEnsureAlignment>().expect_offense(indoc! {"
            begin
              something
              rescue
              ^^^^^^ `rescue` at 3, 2 is not aligned with `begin` at 1, 0.
              handle
            end
        "});
    }

    #[test]
    fn corrects_misaligned_rescue_in_begin() {
        test::<RescueEnsureAlignment>().expect_correction(
            indoc! {"
                begin
                  something
                  rescue
                  ^^^^^^ `rescue` at 3, 2 is not aligned with `begin` at 1, 0.
                  handle
                end
            "},
            indoc! {"
                begin
                  something
                rescue
                  handle
                end
            "},
        );
    }

    #[test]
    fn accepts_aligned_rescue_in_begin() {
        test::<RescueEnsureAlignment>().expect_no_offenses(indoc! {"
            begin
              something
            rescue
              handle
            end
        "});
    }

    #[test]
    fn flags_misaligned_ensure_in_begin() {
        test::<RescueEnsureAlignment>().expect_offense(indoc! {"
            begin
              something
              ensure
              ^^^^^^ `ensure` at 3, 2 is not aligned with `begin` at 1, 0.
              cleanup
            end
        "});
    }

    #[test]
    fn accepts_aligned_ensure_in_begin() {
        test::<RescueEnsureAlignment>().expect_no_offenses(indoc! {"
            begin
              something
            ensure
              cleanup
            end
        "});
    }

    #[test]
    fn flags_misaligned_rescue_in_def() {
        test::<RescueEnsureAlignment>().expect_offense(indoc! {"
            def foo
              bar
              rescue
              ^^^^^^ `rescue` at 3, 2 is not aligned with `def foo` at 1, 0.
              baz
            end
        "});
    }

    #[test]
    fn accepts_aligned_rescue_in_def() {
        test::<RescueEnsureAlignment>().expect_no_offenses(indoc! {"
            def foo
              bar
            rescue
              baz
            end
        "});
    }

    #[test]
    fn accepts_modifier_rescue() {
        // Modifier-form rescue is never flagged.
        test::<RescueEnsureAlignment>().expect_no_offenses("x = foo rescue bar\n");
    }

    #[test]
    fn accepts_aligned_rescue_in_class() {
        test::<RescueEnsureAlignment>().expect_no_offenses(indoc! {"
            class Foo
              bar
            rescue
              baz
            end
        "});
    }

    #[test]
    fn flags_both_rescue_and_ensure() {
        test::<RescueEnsureAlignment>().expect_offense(indoc! {"
            begin
              something
              rescue
              ^^^^^^ `rescue` at 3, 2 is not aligned with `begin` at 1, 0.
              handle
              ensure
              ^^^^^^ `ensure` at 5, 2 is not aligned with `begin` at 1, 0.
              cleanup
            end
        "});
    }

    #[test]
    fn accepts_assignment_begin_rescue_aligned_to_begin() {
        // `x = begin ... rescue` aligns `rescue` to the `begin` keyword column
        // (col 4 here), matching RuboCop's kwbegin anchor.
        test::<RescueEnsureAlignment>().expect_no_offenses(indoc! {"
            x = begin
                  foo
                rescue
                  bar
                end
        "});
    }

    #[test]
    fn flags_assignment_begin_rescue_misaligned() {
        // `rescue` at col 0 is not aligned with the `begin` keyword (col 4).
        test::<RescueEnsureAlignment>().expect_offense(indoc! {"
            x = begin
              foo
            rescue
            ^^^^^^ `rescue` at 3, 0 is not aligned with `begin` at 1, 4.
              bar
            end
        "});
    }

    #[test]
    fn flags_assignment_block_rescue_misaligned() {
        // `result = [...].map do ... rescue` aligns `rescue` to the assignment
        // target column (the `result` column), since the block opener shares a
        // line with the assignment.
        test::<RescueEnsureAlignment>().expect_offense(indoc! {"
            result = [1, 2, 3].map do |el|
              rescue StandardError
              ^^^^^^ `rescue` at 2, 2 is not aligned with `result = [1, 2, 3].map do` at 1, 0.
            end
        "});
    }

    #[test]
    fn accepts_aligned_rescue_in_access_modified_def() {
        // `private def ... rescue` aligns `rescue` to the `private` column.
        test::<RescueEnsureAlignment>().expect_no_offenses(indoc! {"
            private def foo
              bar
            rescue
              baz
            end
        "});
    }

    #[test]
    fn flags_access_modified_def_rescue_misaligned() {
        test::<RescueEnsureAlignment>().expect_offense(indoc! {"
            private def test
              'foo'
              rescue
              ^^^^^^ `rescue` at 3, 2 is not aligned with `private def test` at 1, 0.
              'baz'
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(RescueEnsureAlignment);
