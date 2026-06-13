//! `Layout/IndentationWidth` — body of a block-introducing construct must be
//! indented exactly `Width` (default 2) columns past its introducing keyword.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/IndentationWidth
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection-only port of RuboCop's core check: for each block-introducing
//!   keyword (`def`/`defs`, `class`/`module`, `if`, `while`/`until`,
//!   `case`/`when`, brace/`do` block), the body's column must be exactly
//!   `Width` (default 2) past the keyword's column — RuboCop's
//!   `column_offset_between(body.loc, base_loc)` compared to
//!   `configured_indentation_width`. The `skip_check?` guards are ported
//!   faithfully (they are the false-positive gate): same-line body, body that
//!   is not the first non-whitespace on its line (`else do_something`), and
//!   body starting with a bare access modifier. The offense covers the body's
//!   leading-indentation range, matching `offending_range`.
//!
//!   Autocorrect: NOT emitted (same decision as Murphy's companion
//!   `Layout/IndentationConsistency`). RuboCop re-indents via
//!   `AlignmentCorrector` with `other_offense_in_same_range?` interference
//!   avoidance; that corrector is not available across the single-surface ABI.
//!   The offense still fires so the misindentation is surfaced.
//!
//!   Single-surface ABI / cross-cop-config blockers (intentionally NOT
//!   bypassed — these change WHICH offenses fire and depend on sibling cop
//!   config Murphy cannot read, so the affected shapes are actively SKIPPED,
//!   not mis-handled):
//!     * `EnforcedStyleAlignWith: relative_to_receiver` (method-chain block
//!       body base = dot/selector) is not modelled; only `start_of_line` (the
//!       default) is implemented.
//!     * Assignment-RHS `if`/`while` (`x = if c ... end`) uses a base chosen
//!       by `Layout/EndAlignment` `EnforcedStyleAlignWith`. The variable-
//!       aligned style is valid but would false-fire against the keyword base,
//!       so an `if`/`while` that is the RHS of an assignment is SKIPPED.
//!     * `private def …` (`adjacent_def_modifier?`) uses a base chosen by
//!       `Layout/DefEndAlignment`; not modelled — such defs are reached via
//!       the ordinary `on_def` path and the modifier-relative base is a gap.
//!     * Tabs (`Layout/IndentationStyle: EnforcedStyle: tabs`) changes the
//!       column math (`visual_column`); only the spaces default is handled.
//!       Lines whose indentation contains a tab are SKIPPED to avoid wrong
//!       column math.
//!     * `indented_internal_methods` consistency style (access-modifier
//!       partitioning) is not modelled; only `normal` is.
//!     * `AllowedPatterns` is not modelled.
//!     * `rescue`/`resbody`/`ensure`/`kwbegin` and `case_match`/`in` handlers
//!       are not implemented (under-reporting only — safe for a partial port).
//!   `Width` defaults to the literal 2 (RuboCop's fallback chain through
//!   `Layout/IndentationWidth` itself is the identity here).
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, cop};

#[derive(Default)]
pub struct IndentationWidth;

/// Options for [`IndentationWidth`].
#[derive(CopOptions)]
pub struct IndentationWidthOptions {
    #[option(
        name = "Width",
        default = 2,
        description = "Number of columns a body must be indented past its introducing keyword."
    )]
    pub width: i64,
}

#[cop(
    name = "Layout/IndentationWidth",
    description = "Body of a block-introducing construct must be indented Width columns past its keyword.",
    default_severity = "warning",
    default_enabled = true,
    options = IndentationWidthOptions,
)]
impl IndentationWidth {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
        let base = cx.loc(node).keyword();
        check_indentation(base, cx.def_body(node), cx, options);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
        let base = cx.loc(node).keyword();
        check_indentation(base, cx.def_body(node), cx, options);
    }

    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
        let base = cx.loc(node).keyword();
        check_members(base, class_or_module_body(node, cx), cx, options);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
        let base = cx.loc(node).keyword();
        check_members(base, class_or_module_body(node, cx), cx, options);
    }

    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
        // Ternaries and modifier-form `x if c` have no keyword base to check.
        let base = cx.loc(node).keyword();
        if base == Range::ZERO {
            return;
        }
        // Assignment-RHS `if` (`x = if c ... end`) is governed by
        // Layout/EndAlignment config (a base Murphy cannot resolve) and the
        // variable-aligned style is valid — skip to avoid a false positive.
        if is_assignment_rhs(node, cx) {
            return;
        }
        check_indentation(base, cx.if_then_branch(node), cx, options);
        // The `else` branch indentation (relative to the `else` keyword) is a
        // documented under-reporting gap: Murphy's Cx exposes no `else`-keyword
        // location, and locating it via token scanning risks false positives.
        // The `then` branch (the common case) is covered; `elsif` chains get
        // their own `on_if` visit. See the parity block.
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
        check_loop(node, cx, options);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
        check_loop(node, cx, options);
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
        for &when_node in cx.case_when_branches(node) {
            let base = cx.loc(when_node).keyword();
            check_indentation(base, cx.when_body(when_node), cx, options);
        }
        if let Some(&last_when) = cx.case_when_branches(node).last() {
            let base = cx.loc(last_when).keyword();
            check_indentation(base, cx.case_else_branch(node), cx, options);
        }
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
        check_block_body(node, cx, options);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
        check_block_body(node, cx, options);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
        check_block_body(node, cx, options);
    }
}

/// `on_while`/`on_until`: only single-line-condition loops are checked
/// (matching RuboCop's `single_line_condition?` guard), the body is indented
/// past the keyword. Post-form (`begin..end while c`) and assignment-RHS loops
/// are skipped.
fn check_loop(node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
    let base = cx.loc(node).keyword();
    if base == Range::ZERO {
        return;
    }
    if is_assignment_rhs(node, cx) {
        return;
    }
    if !single_line_condition(node, cx) {
        return;
    }
    check_indentation(base, loop_body(node, cx), cx, options);
}

/// `on_block`: the body is indented past the `end` keyword (the block opener's
/// indentation base under the default `start_of_line` style).
fn check_block_body(node: NodeId, cx: &Cx<'_>, options: &IndentationWidthOptions) {
    let end_kw = cx.loc(node).end_keyword();
    if end_kw == Range::ZERO {
        return;
    }
    // `return unless begins_its_line?(end_loc)`.
    if !begins_its_line(cx, end_kw.start) {
        return;
    }
    check_indentation(end_kw, cx.block_body(node), cx, options);
}

/// `check_members`: the first member of a class/module body is indented past
/// the keyword. (Members after the first are handled by
/// `Layout/IndentationConsistency`; RuboCop's normal style checks the first
/// against `base`, and consistency owns the rest.)
fn check_members(
    base: Range,
    body: OptNodeId,
    cx: &Cx<'_>,
    options: &IndentationWidthOptions,
) {
    let Some(body) = body.get() else {
        return;
    };
    // RuboCop checks the first member of a `begin` body; a single-statement
    // body is itself the member.
    let member = match *cx.kind(body) {
        NodeKind::Begin(list) => cx.list(list).first().copied(),
        _ => Some(body),
    };
    let Some(member) = member else {
        return;
    };
    // `select_check_member`: a leading bare access modifier is not checked
    // here under the default (indent) AccessModifierIndentation style.
    if cx.is_bare_access_modifier(member) {
        return;
    }
    check_indentation(base, OptNodeId::some(member), cx, options);
}

/// RuboCop's `check_indentation(base_loc, body_node)` core. `base` is the
/// keyword range; `body` is the construct's body.
fn check_indentation(
    base: Range,
    body: OptNodeId,
    cx: &Cx<'_>,
    options: &IndentationWidthOptions,
) {
    if base == Range::ZERO {
        return;
    }
    let Some(body) = body.get() else {
        return;
    };
    let body_start = cx.range(body).start;

    // ── skip_check? ─────────────────────────────────────────────────────
    // `same_line?(body_node, base_loc)`: body shares the keyword's line.
    if line_of(cx, body_start) == line_of(cx, base.start) {
        return;
    }
    // `starts_with_access_modifier?`.
    if starts_with_access_modifier(body, cx) {
        return;
    }
    // `body_node.loc.column != first_char_pos_on_line`: the body is not the
    // first non-whitespace char on its line (e.g. `else do_something`).
    if !begins_its_line(cx, body_start) {
        return;
    }
    // Tabs in the body's leading indentation → column math differs
    // (visual_column). Skip to avoid wrong-width offenses on tab files.
    if leading_indent_has_tab(cx, body_start) || leading_indent_has_tab(cx, base.start) {
        return;
    }

    // ── column_offset_between (non-tabs) = body.column - base.column ────
    let indentation = column_of(cx, body_start) as i64 - column_of(cx, base.start) as i64;
    let configured = options.width.max(0);
    if indentation == configured {
        return;
    }

    let msg = message(configured, indentation);
    let range = offending_range(body_start, indentation);
    cx.emit_offense(range, &msg, None);
}

/// RuboCop's `offending_range`: the indentation whitespace between the start
/// of the body's column-0 indent and the body's start (when over-indented),
/// or a zero-width range at the body start (when under-indented).
fn offending_range(body_start: u32, indentation: i64) -> Range {
    // `ind = begin_pos - indentation` (spaces case).
    let ind = (body_start as i64 - indentation).max(0) as u32;
    if indentation >= 0 {
        Range {
            start: ind,
            end: body_start,
        }
    } else {
        Range {
            start: body_start,
            end: ind,
        }
    }
}

fn message(configured: i64, indentation: i64) -> String {
    format!("Use {configured} (not {indentation}) spaces for indentation.")
}

// ── node-shape helpers ───────────────────────────────────────────────────

fn class_or_module_body(node: NodeId, cx: &Cx<'_>) -> OptNodeId {
    match *cx.kind(node) {
        NodeKind::Class { body, .. } | NodeKind::Module { body, .. } => body,
        _ => OptNodeId::NONE,
    }
}

fn loop_body(node: NodeId, cx: &Cx<'_>) -> OptNodeId {
    match *cx.kind(node) {
        NodeKind::While { body, .. } | NodeKind::Until { body, .. } => body,
        _ => OptNodeId::NONE,
    }
}

/// A single-line condition loop (`while cond` on one line) — RuboCop's
/// `single_line_condition?`. Post-form loops (`begin..end while c`) are
/// excluded.
fn single_line_condition(node: NodeId, cx: &Cx<'_>) -> bool {
    let (cond, post) = match *cx.kind(node) {
        NodeKind::While { cond, post, .. } | NodeKind::Until { cond, post, .. } => (cond, post),
        _ => return false,
    };
    if post {
        return false;
    }
    let cond_range = cx.range(cond);
    line_of(cx, cond_range.start) == line_of(cx, cond_range.end.saturating_sub(1))
}

/// True when `node` is the value (RHS) of an assignment — RuboCop's
/// `check_assignment` redirect path that `ignore_node`s the RHS.
fn is_assignment_rhs(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if !cx.is_assignment(parent) {
        return false;
    }
    // The node must be the value position, not the target. For simple
    // assignments the value is the last child.
    cx.children(parent).last() == Some(&node)
}

/// RuboCop's `starts_with_access_modifier?`: the body is a `begin` whose first
/// child is a bare access modifier.
fn starts_with_access_modifier(body: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Begin(list) = *cx.kind(body) else {
        return false;
    };
    cx.list(list)
        .first()
        .is_some_and(|&first| cx.is_bare_access_modifier(first))
}

// ── column / line helpers (shared shape with the indentation cops) ───────

/// Byte column (chars before `offset` on its line) of `offset`.
fn column_of(cx: &Cx<'_>, offset: u32) -> usize {
    let src = cx.source();
    let off = offset as usize;
    let line_start = src[..off].rfind('\n').map_or(0, |p| p + 1);
    src[line_start..off].chars().count()
}

/// 1-based line number of `offset`.
fn line_of(cx: &Cx<'_>, offset: u32) -> usize {
    let src = cx.source();
    src[..offset as usize].bytes().filter(|&b| b == b'\n').count() + 1
}

/// True when only whitespace precedes `offset` on its line.
fn begins_its_line(cx: &Cx<'_>, offset: u32) -> bool {
    let src = cx.source().as_bytes();
    let mut i = offset as usize;
    while i > 0 {
        match src[i - 1] {
            b' ' | b'\t' => i -= 1,
            b'\n' => return true,
            _ => return false,
        }
    }
    true
}

/// True when the leading indentation of `offset`'s line contains a tab.
fn leading_indent_has_tab(cx: &Cx<'_>, offset: u32) -> bool {
    let src = cx.source().as_bytes();
    let off = offset as usize;
    let line_start = src[..off].iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
    src[line_start..off].contains(&b'\t')
}

#[cfg(test)]
mod tests;

murphy_plugin_api::submit_cop!(IndentationWidth);
