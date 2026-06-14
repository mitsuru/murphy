//! `Layout/LineEndStringConcatenationIndentation` — checks the indentation of
//! the next line after a line that ends with a string literal and a backslash.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/LineEndStringConcatenationIndentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports `on_dstr` + `strings_concatenated_with_backslash?` +
//!   `always_indented?` + `check_aligned` + `check_indented` + `base_column`.
//!   Fires on a `dstr` that is a backslash string concatenation: multiline,
//!   every child a `str`/`dstr`, and no child itself multiline.
//!
//!   - aligned (default), and the parent is NOT one of
//!     `[nil, block, begin, def, defs, if]`: every part from the second on must
//!     share the column of the part before it (`check_aligned(children, 1)`).
//!   - indented, or the parent IS one of those "always indented" types: the
//!     second part must be indented one `IndentationWidth` (default 2) past the
//!     base column (`check_indented`), and parts from the third on must align
//!     with the part before them (`check_aligned(children, 2)`).
//!
//!   `base_column` of the first part is the enclosing hash `pair`'s column when
//!   the concatenation is a hash value, otherwise the column of the first
//!   non-whitespace character on the first part's line. Columns are display
//!   columns (`.chars().count()` from the line start) so multi-byte source
//!   aligns by visible column.
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop reindents the offending
//!   line via `AlignmentCorrector`; the detect-only port ships without it,
//!   matching the precedent set by `Layout/ParameterAlignment`.
//!
//!   `Enabled: pending` upstream → `default_enabled = false`.
//! ```
//!
//! ## Matched shapes
//!
//! `dstr` nodes formed by `'a' \`-style backslash string concatenation whose
//! continuation lines are mis-indented for the configured `EnforcedStyle`.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG_ALIGN: &str = "Align parts of a string concatenated with backslash.";
const MSG_INDENT: &str = "Indent the first part of a string concatenated with backslash.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct LineEndStringConcatenationIndentation;

/// Options for [`LineEndStringConcatenationIndentation`]. `EnforcedStyle`
/// matches RuboCop; the default is `aligned`. `IndentationWidth` overrides the
/// width used by the indented style (default 2, mirroring
/// `Layout/IndentationWidth`).
#[derive(CopOptions)]
pub struct LineEndStringConcatenationIndentationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "aligned",
        description = "Whether concatenation parts are aligned with the first part or indented one level."
    )]
    pub enforced_style: ConcatenationStyle,
    // `Option<i64>` so the bundled default `IndentationWidth: ~` (JSON null)
    // decodes to `None` instead of erroring the option struct and discarding the
    // user's other keys; `None` falls back to width 2.
    #[option(
        name = "IndentationWidth",
        description = "Indentation width for the indented style (null/unset falls back to RuboCop's default of 2)."
    )]
    pub indentation_width: Option<i64>,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ConcatenationStyle {
    /// Concatenated parts align with the first part (default).
    #[option(value = "aligned")]
    Aligned,
    /// The second line is indented one step past the first.
    #[option(value = "indented")]
    Indented,
}

#[cop(
    name = "Layout/LineEndStringConcatenationIndentation",
    description = "Checks the indentation of lines after a string literal ending with a backslash.",
    default_severity = "warning",
    default_enabled = false,
    options = LineEndStringConcatenationIndentationOptions,
)]
impl LineEndStringConcatenationIndentation {
    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        if !strings_concatenated_with_backslash(node, cx) {
            return;
        }
        let children = cx.list(dstr_list(node, cx));
        if children.is_empty() {
            return;
        }

        let opts = cx.options_or_default::<LineEndStringConcatenationIndentationOptions>();
        let src = cx.source();

        if opts.enforced_style == ConcatenationStyle::Aligned && !always_indented(node, cx) {
            check_aligned(cx, children, 1, src);
        } else {
            check_indented(cx, children, &opts, src);
            check_aligned(cx, children, 2, src);
        }
    }
}

/// The child list of a `dstr` node.
fn dstr_list(node: NodeId, cx: &Cx<'_>) -> murphy_plugin_api::NodeList {
    match *cx.kind(node) {
        NodeKind::Dstr(list) => list,
        _ => murphy_plugin_api::NodeList::EMPTY,
    }
}

/// RuboCop `strings_concatenated_with_backslash?`: the dstr is multiline, every
/// child is a `str`/`dstr`, and no child is itself multiline.
fn strings_concatenated_with_backslash(node: NodeId, cx: &Cx<'_>) -> bool {
    if !cx.is_multiline(node) {
        return false;
    }
    let children = cx.list(dstr_list(node, cx));
    if children.is_empty() {
        return false;
    }
    children.iter().all(|&c| {
        matches!(cx.kind(c), NodeKind::Str(_) | NodeKind::Dstr(_)) && !cx.is_multiline(c)
    })
}

/// RuboCop `always_indented?`: parent type is one of `[nil, block, begin, def,
/// defs, if]` (nil = no parent / top level). `Numblock`/`Itblock` are NOT in
/// the upstream list (their type is `:numblock`/`:itblock`, not `:block`), so
/// they are deliberately excluded to match RuboCop exactly.
fn always_indented(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.parent(node).get() {
        None => true, // nil parent
        Some(parent) => matches!(
            cx.kind(parent),
            NodeKind::Block { .. }
                | NodeKind::Begin(_)
                | NodeKind::Def { .. }
                | NodeKind::Defs { .. }
                | NodeKind::If { .. }
        ),
    }
}

/// RuboCop `check_aligned(children, start_index)`: each part from
/// `start_index` must share the column of the part before it.
fn check_aligned(cx: &Cx<'_>, children: &[NodeId], start_index: usize, src: &str) {
    if children.len() <= start_index {
        return;
    }
    let mut base_column = display_column(cx.range(children[start_index - 1]).start, src);
    for &child in &children[start_index..] {
        let column = display_column(cx.range(child).start, src);
        if column != base_column {
            cx.emit_offense(offending_range(child, cx), MSG_ALIGN, None);
        }
        base_column = column;
    }
}

/// RuboCop `check_indented(children)`: the second part must be indented one
/// `IndentationWidth` past the base column of the first part.
fn check_indented(
    cx: &Cx<'_>,
    children: &[NodeId],
    opts: &LineEndStringConcatenationIndentationOptions,
    src: &str,
) {
    if children.len() < 2 {
        return;
    }
    let expected = base_column(children[0], cx, src) + indentation_width(opts);
    let actual = display_column(cx.range(children[1]).start, src);
    if expected != actual {
        cx.emit_offense(offending_range(children[1], cx), MSG_INDENT, None);
    }
}

/// RuboCop `base_column(child)`: the grandparent's column when it is a hash
/// `pair`, otherwise the first non-whitespace column of the child's source line.
fn base_column(child: NodeId, cx: &Cx<'_>, src: &str) -> usize {
    // `child.parent.parent` — the child str's parent is the dstr; its parent is
    // the grandparent.
    if let Some(grandparent) = cx
        .parent(child)
        .get()
        .and_then(|parent| cx.parent(parent).get())
        .filter(|&gp| matches!(cx.kind(gp), NodeKind::Pair { .. }))
    {
        return display_column(cx.range(grandparent).start, src);
    }
    // `child.source_range.source_line =~ /\S/` — column of the first
    // non-whitespace character on the child's line.
    let start = cx.range(child).start as usize;
    let line_start = src[..start].rfind('\n').map_or(0, |p| p + 1);
    let bytes = src.as_bytes();
    let mut i = line_start;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    src[line_start..i].chars().count()
}

/// Configured indentation width. Only an unset (`None`) override falls back to
/// 2; an explicit `0` is honoured (Ruby treats `0` as truthy in
/// `cop_config['IndentationWidth'] || …`). Negatives clamp to 0.
fn indentation_width(opts: &LineEndStringConcatenationIndentationOptions) -> usize {
    opts.indentation_width.map_or(2, |w| w.max(0) as usize)
}

/// Visible column (0-based, char count) of a byte offset within its line.
fn display_column(offset: u32, src: &str) -> usize {
    let line_start = src[..offset as usize].rfind('\n').map_or(0, |p| p + 1);
    src[line_start..offset as usize].chars().count()
}

/// Highlight the offending part, trimmed to its first line.
fn offending_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let r = cx.range(node);
    let src = cx.source().as_bytes();
    let line_end = src[r.start as usize..r.end as usize]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(r.end, |pos| r.start + pos as u32);
    Range {
        start: r.start,
        end: line_end,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConcatenationStyle, LineEndStringConcatenationIndentation,
        LineEndStringConcatenationIndentationOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};
    use murphy_plugin_api::CopOptions;

    /// Regression (sweep #384 follow-up): the bundled default
    /// `IndentationWidth: ~` merges to JSON `null`. With an `Option<i64>` field
    /// it must decode rather than error the whole struct and silently discard the
    /// user's `EnforcedStyle`.
    #[test]
    fn null_indentation_width_preserves_other_keys() {
        let opts = <LineEndStringConcatenationIndentationOptions as CopOptions>::from_config_json(
            br#"{"EnforcedStyle":"indented","IndentationWidth":null}"#,
        )
        .expect("null IndentationWidth must decode, not discard the struct");
        let reference = <LineEndStringConcatenationIndentationOptions as CopOptions>::from_config_json(
            br#"{"EnforcedStyle":"indented","IndentationWidth":4}"#,
        )
        .unwrap();
        assert!(opts.enforced_style == reference.enforced_style);
    }

    fn indented() -> LineEndStringConcatenationIndentationOptions {
        LineEndStringConcatenationIndentationOptions {
            enforced_style: ConcatenationStyle::Indented,
            indentation_width: None,
        }
    }

    // aligned (default) ---------------------------------------------------

    #[test]
    fn aligned_accepts_aligned_parts() {
        test::<LineEndStringConcatenationIndentation>().expect_no_offenses(indoc! {"
            puts 'x' \\
                 'y'
        "});
    }

    #[test]
    fn aligned_flags_misaligned_part() {
        test::<LineEndStringConcatenationIndentation>().expect_offense(indoc! {"
            puts 'x' \\
              'y'
              ^^^ Align parts of a string concatenated with backslash.
        "});
    }

    #[test]
    fn aligned_in_hash_value_accepts_aligned() {
        test::<LineEndStringConcatenationIndentation>().expect_no_offenses(indoc! {"
            my_hash = {
              first: 'a message' \\
                     'in two parts'
            }
        "});
    }

    // always-indented contexts (def body) --------------------------------

    #[test]
    fn def_body_requires_indentation() {
        // Parent is `def` → always indented regardless of aligned default.
        test::<LineEndStringConcatenationIndentation>().expect_offense(indoc! {"
            def some_method
              'x' \\
              'y'
              ^^^ Indent the first part of a string concatenated with backslash.
            end
        "});
    }

    #[test]
    fn def_body_accepts_indented() {
        test::<LineEndStringConcatenationIndentation>().expect_no_offenses(indoc! {"
            def some_method
              'x' \\
                'y'
            end
        "});
    }

    // indented style ------------------------------------------------------

    #[test]
    fn indented_flags_aligned_parts() {
        test::<LineEndStringConcatenationIndentation>()
            .with_options(&indented())
            .expect_offense(indoc! {"
                result = 'x' \\
                         'y'
                         ^^^ Indent the first part of a string concatenated with backslash.
            "});
    }

    #[test]
    fn indented_accepts_one_level() {
        test::<LineEndStringConcatenationIndentation>()
            .with_options(&indented())
            .expect_no_offenses(indoc! {"
                result = 'x' \\
                  'y'
            "});
    }

    // non-matching shapes -------------------------------------------------

    #[test]
    fn accepts_single_line_string() {
        test::<LineEndStringConcatenationIndentation>().expect_no_offenses("x = 'foo'\n");
    }

    #[test]
    fn ignores_interpolated_string() {
        // `"a#{b}c"` is a dstr with a non-str child → not a backslash concat.
        test::<LineEndStringConcatenationIndentation>().expect_no_offenses("x = \"a#{b}c\"\n");
    }

    /// Parity pin (Codex #387/#384): an explicit `IndentationWidth: 0` is
    /// honoured (Ruby treats `0` as truthy), so the `indented` style expects the
    /// continuation at `base_column + 0`. `'y'` at column 0 is accepted; `0` must
    /// not fall back to width 2.
    #[test]
    fn indented_honors_zero_indentation_width() {
        let opts = LineEndStringConcatenationIndentationOptions {
            enforced_style: ConcatenationStyle::Indented,
            indentation_width: Some(0),
        };
        test::<LineEndStringConcatenationIndentation>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {"
                result = 'x' \\
                'y'
            "});
    }
}

murphy_plugin_api::submit_cop!(LineEndStringConcatenationIndentation);
