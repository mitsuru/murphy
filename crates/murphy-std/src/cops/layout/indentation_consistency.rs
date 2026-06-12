//! `Layout/IndentationConsistency` — keep indentation straight: entities on
//! the same logical depth must share the same indentation.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/IndentationConsistency
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Implements RuboCop's `normal` `EnforcedStyle` (the default). For every
//!   `begin` body (method/class/module/block bodies and `begin...end`), the
//!   children that begin their own line are required to share the column of
//!   the first such child (`check_alignment` with the first child's
//!   `display_column` as the base). A child whose column differs emits
//!   "Inconsistent indentation detected." Bare access modifiers
//!   (`public`/`private`/`protected`) are excluded from the checked set,
//!   matching `check_normal_style`. Only the first line of a multi-line child
//!   is highlighted (Murphy's one-offense-per-line model), narrowing
//!   RuboCop's whole-`source_range` highlight.
//!   Gaps (documented, not bypassed):
//!     - Autocorrect is not emitted. RuboCop re-indents the misaligned child
//!       (and all its lines) via `AlignmentCorrector`, which carries a
//!       string-literal/heredoc taboo-range subsystem; that corrector is not
//!       yet available across the single-surface ABI. The offense still
//!       fires so the misalignment is surfaced.
//!     - `EnforcedStyle: indented_internal_methods` is not implemented (the
//!       non-default style that partitions children around access modifiers).
//!     - `base_column_for_normal_style`'s access-modifier-as-first-child
//!       special case is not modelled; the base column is always the first
//!       non-access-modifier child's column.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct IndentationConsistency;

const MSG: &str = "Inconsistent indentation detected.";

#[cop(
    name = "Layout/IndentationConsistency",
    description = "Keep indentation straight.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl IndentationConsistency {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Begin(list) = *cx.kind(node) else {
            return;
        };
        check_normal_style(cx.list(list), cx);
    }

    #[on_node(kind = "kwbegin")]
    fn check_kwbegin(&self, node: NodeId, cx: &Cx<'_>) {
        // In Murphy a `begin...end` block parses as `Kwbegin(Begin(stmts))`,
        // so the inner `Begin` node carries the statements and is already
        // visited by `check_begin`. A `Kwbegin` whose sole child is that
        // inner `Begin` has nothing to align here. But a `begin...end` with a
        // single statement parses as `Kwbegin([stmt])` directly (no inner
        // `Begin`), so still run the alignment check on the kwbegin children.
        let NodeKind::Kwbegin(list) = *cx.kind(node) else {
            return;
        };
        let children = cx.list(list);
        // Skip the transparent single-`Begin` wrapper case to avoid a
        // duplicate check (the inner `Begin` is handled by `check_begin`).
        if let [only] = children
            && matches!(*cx.kind(*only), NodeKind::Begin(_))
        {
            return;
        }
        check_normal_style(children, cx);
    }
}

/// RuboCop `check_normal_style`: align the children that are not bare access
/// modifiers against the first such child's column.
fn check_normal_style(children: &[NodeId], cx: &Cx<'_>) {
    // `node.children.reject { |child| bare_access_modifier?(child) }`
    let checked: Vec<NodeId> = children
        .iter()
        .copied()
        .filter(|&c| !cx.is_bare_access_modifier(c))
        .collect();
    check_alignment(&checked, cx);
}

/// RuboCop `Alignment#check_alignment` / `each_bad_alignment` for the
/// `normal` style: the base column is the first checked child's column;
/// every later child that begins its own line (on a new source line) and
/// whose column differs from the base is flagged.
fn check_alignment(items: &[NodeId], cx: &Cx<'_>) {
    let Some(&first) = items.first() else {
        return;
    };
    let base_column = display_column(first, cx);

    let mut prev_node: Option<NodeId> = None;
    let mut first_seen = false;
    for &current in items {
        // RuboCop: `current.loc.line > prev_line`. Because items are in source
        // order, `current` is on a later line than its predecessor iff a `\n`
        // lies between their start offsets — a single O(span) scan rather than
        // counting newlines from BOF for every item (which is O(N) per item /
        // O(N²) over the block).
        let on_new_line = match prev_node {
            None => true,
            Some(prev) => {
                let prev_start = cx.range(prev).start as usize;
                let cur_start = cx.range(current).start as usize;
                cx.source()[prev_start..cur_start].contains('\n')
            }
        };
        // The very first item establishes the base and is never itself flagged
        // (column_delta would be zero anyway).
        let on_new_line = !first_seen || on_new_line;
        if on_new_line && begins_its_line(current, cx) {
            let column = display_column(current, cx);
            if column != base_column {
                cx.emit_offense(first_line_range(current, cx), MSG, None);
            }
            first_seen = true;
        }
        prev_node = Some(current);
    }
}

/// RuboCop `display_column(range)`: the column of the node's start measured in
/// characters from the start of its line. (ASCII-only column; Murphy does not
/// yet weight by `Unicode::DisplayWidth`, so full-width glyphs count as one
/// column — a documented simplification shared with sibling layout cops.)
fn display_column(node: NodeId, cx: &Cx<'_>) -> usize {
    let start = cx.range(node).start as usize;
    let src = cx.source();
    let line_start = src[..start].rfind('\n').map_or(0, |pos| pos + 1);
    src[line_start..start].chars().count()
}

/// RuboCop `begins_its_line?(range)`: the node is the first non-whitespace on
/// its line (only spaces/tabs precede it on that line).
fn begins_its_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let start = cx.range(node).start as usize;
    let src = cx.source().as_bytes();
    let line_start = src[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    src[line_start..start].iter().all(|&b| b == b' ' || b == b'\t')
}

/// Narrow a (possibly multi-line) child node's range to its first line,
/// excluding the trailing `\n`. Murphy emits one offense per line, so a
/// multi-line child is highlighted on its first line only.
fn first_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let range = cx.range(node);
    let src = cx.source().as_bytes();
    let end = src[range.start as usize..range.end as usize]
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| range.start + i as u32)
        .unwrap_or(range.end);
    Range {
        start: range.start,
        end,
    }
}

#[cfg(test)]
mod tests {
    use super::IndentationConsistency;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── Clean (no offense) ────────────────────────────────────────────────────

    #[test]
    fn accepts_uniform_method_body() {
        test::<IndentationConsistency>().expect_no_offenses(indoc! {r#"
            def foo
              a
              b
            end
        "#});
    }

    #[test]
    fn accepts_uniform_class_body() {
        test::<IndentationConsistency>().expect_no_offenses(indoc! {r#"
            class C
              def a; end
              def b; end
            end
        "#});
    }

    #[test]
    fn accepts_single_statement_body() {
        test::<IndentationConsistency>().expect_no_offenses(indoc! {r#"
            def foo
              a
            end
        "#});
    }

    #[test]
    fn accepts_multiline_child() {
        // A multi-line call child does not trip the cop — its continuation
        // line is not a separate Begin child.
        test::<IndentationConsistency>().expect_no_offenses(indoc! {r#"
            def foo
              a
              foo(1,
                  2)
            end
        "#});
    }

    #[test]
    fn accepts_heredoc_child() {
        test::<IndentationConsistency>().expect_no_offenses(indoc! {r#"
            def foo
              a
              x = <<~RUBY
                hello
              RUBY
            end
        "#});
    }

    #[test]
    fn accepts_uniform_begin_end_body() {
        test::<IndentationConsistency>().expect_no_offenses(indoc! {r#"
            begin
              a
              b
            end
        "#});
    }

    // ── Offenses ──────────────────────────────────────────────────────────────

    #[test]
    fn flags_misaligned_second_statement() {
        // Second statement is indented one space deeper than the first.
        test::<IndentationConsistency>().expect_offense(indoc! {r#"
            def foo
              a
               b
               ^ Inconsistent indentation detected.
            end
        "#});
    }

    #[test]
    fn flags_misaligned_method_in_class() {
        test::<IndentationConsistency>().expect_offense(indoc! {r#"
            class C
              def a; end
                def b; end
                ^^^^^^^^^^ Inconsistent indentation detected.
            end
        "#});
    }

    #[test]
    fn flags_under_indented_statement() {
        test::<IndentationConsistency>().expect_offense(indoc! {r#"
            def foo
                a
              b
              ^ Inconsistent indentation detected.
            end
        "#});
    }

    #[test]
    fn flags_misaligned_in_begin_end() {
        test::<IndentationConsistency>().expect_offense(indoc! {r#"
            begin
              a
                b
                ^ Inconsistent indentation detected.
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(IndentationConsistency);
