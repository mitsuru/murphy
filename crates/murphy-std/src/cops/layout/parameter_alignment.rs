//! `Layout/ParameterAlignment` — parameters of a multi-line method definition
//! must be aligned.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/ParameterAlignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `def`/`defs` nodes with two or more parameters where a parameter
//!   that begins its own line is not aligned with the configured base column.
//!   Mirrors RuboCop's `Alignment#check_alignment` / `each_bad_alignment`:
//!   only parameters that *begin their own line* are checked, and only those
//!   whose column differs from the base column are flagged.
//!
//!   - with_first_parameter (default): base column = the first parameter's
//!     display column.
//!   - with_fixed_indentation: base column = the indentation of the line
//!     containing the `def` keyword plus the configured indentation width
//!     (default 2).
//!
//!   Columns are computed with `.chars().count()` from the line start so
//!   multi-byte source aligns by visible column, matching RuboCop's
//!   `display_column` (modulo full Unicode east-asian-width handling, which is
//!   a known minor gap).
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop shifts each misaligned
//!   parameter to the base column via `AlignmentCorrector`; the detect-only
//!   port ships without it.
//! ```
//!
//! ## Matched shapes
//!
//! `def`/`defs` nodes with `args.size >= 2` where a later parameter begins its
//! own line at a column other than the base column.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const ALIGN_PARAMS_MSG: &str =
    "Align the parameters of a method definition if they span more than one line.";
const FIXED_INDENT_MSG: &str = "Use one level of indentation for parameters \
    following the first line of a multi-line method definition.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct ParameterAlignment;

/// Options for [`ParameterAlignment`]. `EnforcedStyle` matches RuboCop
/// verbatim; the default is `with_first_parameter`. `IndentationWidth`
/// overrides the indentation width used by `with_fixed_indentation` (default 2,
/// mirroring `Layout/IndentationWidth`).
#[derive(CopOptions)]
pub struct ParameterAlignmentOptions {
    #[option(
        name = "EnforcedStyle",
        default = "with_first_parameter",
        description = "How to align parameters following the first line of a method definition."
    )]
    pub enforced_style: ParameterAlignmentStyle,
    #[option(
        name = "IndentationWidth",
        default = 0,
        description = "Indentation width for `with_fixed_indentation` (0 = use the default of 2)."
    )]
    pub indentation_width: i64,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ParameterAlignmentStyle {
    /// Align with the first parameter's column.
    #[option(value = "with_first_parameter")]
    WithFirstParameter,
    /// Indent one level past the `def` keyword's line.
    #[option(value = "with_fixed_indentation")]
    WithFixedIndentation,
}

#[cop(
    name = "Layout/ParameterAlignment",
    description = "Align the parameters of a multi-line method definition.",
    default_severity = "warning",
    default_enabled = true,
    options = ParameterAlignmentOptions,
)]
impl ParameterAlignment {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Visible column (0-based, char count) of a byte offset within its line.
fn display_column(offset: u32, src: &str) -> usize {
    let line_start = src[..offset as usize].rfind('\n').map_or(0, |p| p + 1);
    src[line_start..offset as usize].chars().count()
}

/// Returns true when `offset` is the first non-whitespace byte on its line,
/// i.e. the parameter begins its own line.
fn begins_its_line(offset: u32, src: &str) -> bool {
    let bytes = src.as_bytes();
    let line_start = src[..offset as usize].rfind('\n').map_or(0, |p| p + 1);
    bytes[line_start..offset as usize]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<ParameterAlignmentOptions>();

    let Some(args_node) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(list) = cx.kind(args_node) else {
        return;
    };
    let params = cx.list(*list);
    // RuboCop: `return if node.arguments.size < 2`.
    if params.len() < 2 {
        return;
    }

    let src = cx.source();
    let fixed = opts.enforced_style == ParameterAlignmentStyle::WithFixedIndentation;

    // Base column: first parameter's display column (with_first_parameter), or
    // the def-keyword line's indentation + indentation width (with_fixed).
    let base_column = if fixed {
        let kw_start = keyword_start(node, cx);
        let line_start = src[..kw_start as usize].rfind('\n').map_or(0, |p| p + 1);
        let indent = src[line_start..kw_start as usize].chars().count();
        indent + indentation_width(&opts)
    } else {
        display_column(cx.range(params[0]).start, src)
    };

    let msg = if fixed {
        FIXED_INDENT_MSG
    } else {
        ALIGN_PARAMS_MSG
    };

    // Each parameter that begins its own line must sit at `base_column`.
    for &param in params {
        let start = cx.range(param).start;
        if !begins_its_line(start, src) {
            continue;
        }
        if display_column(start, src) != base_column {
            cx.emit_offense(offending_range(param, cx), msg, None);
        }
    }
}

/// Configured indentation width for `with_fixed_indentation` (0 → default 2).
fn indentation_width(opts: &ParameterAlignmentOptions) -> usize {
    if opts.indentation_width > 0 {
        opts.indentation_width as usize
    } else {
        2
    }
}

/// The `def` keyword's start offset (used as the indentation anchor).
fn keyword_start(node: NodeId, cx: &Cx<'_>) -> u32 {
    let kw = cx.loc(node).keyword();
    if kw != Range::ZERO {
        kw.start
    } else {
        cx.range(node).start
    }
}

/// Highlight the offending parameter, trimmed to its first line.
fn offending_range(param: NodeId, cx: &Cx<'_>) -> Range {
    let r = cx.range(param);
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
    use super::{ParameterAlignment, ParameterAlignmentOptions, ParameterAlignmentStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn fixed() -> ParameterAlignmentOptions {
        ParameterAlignmentOptions {
            enforced_style: ParameterAlignmentStyle::WithFixedIndentation,
            indentation_width: 0,
        }
    }

    // with_first_parameter (default) --------------------------------------

    #[test]
    fn accepts_aligned_with_first_parameter() {
        test::<ParameterAlignment>().expect_no_offenses(indoc! {"
            def foo(bar,
                    baz)
              123
            end
        "});
    }

    #[test]
    fn accepts_each_on_own_line_indented() {
        test::<ParameterAlignment>().expect_no_offenses(indoc! {"
            def foo(
              bar,
              baz
            )
              123
            end
        "});
    }

    #[test]
    fn flags_misaligned_second_parameter() {
        test::<ParameterAlignment>().expect_offense(indoc! {"
            def foo(bar,
                 baz)
                 ^^^ Align the parameters of a method definition if they span more than one line.
              123
            end
        "});
    }

    #[test]
    fn flags_misaligned_when_open_on_own_line() {
        test::<ParameterAlignment>().expect_offense(indoc! {"
            def foo(
              bar,
                 baz)
                 ^^^ Align the parameters of a method definition if they span more than one line.
              123
            end
        "});
    }

    #[test]
    fn accepts_single_line_signature() {
        test::<ParameterAlignment>().expect_no_offenses(indoc! {"
            def foo(bar, baz)
              123
            end
        "});
    }

    #[test]
    fn accepts_single_parameter() {
        test::<ParameterAlignment>().expect_no_offenses(indoc! {"
            def foo(
              bar
            )
              123
            end
        "});
    }

    // with_fixed_indentation ----------------------------------------------

    #[test]
    fn fixed_accepts_one_level_indentation() {
        test::<ParameterAlignment>()
            .with_options(&fixed())
            .expect_no_offenses(indoc! {"
                def foo(bar,
                  baz)
                  123
                end
            "});
    }

    #[test]
    fn fixed_flags_aligned_with_first_parameter() {
        test::<ParameterAlignment>()
            .with_options(&fixed())
            .expect_offense(indoc! {"
                def foo(bar,
                        baz)
                        ^^^ Use one level of indentation for parameters following the first line of a multi-line method definition.
                  123
                end
            "});
    }

    #[test]
    fn fixed_accepts_each_on_own_line() {
        test::<ParameterAlignment>()
            .with_options(&fixed())
            .expect_no_offenses(indoc! {"
                def foo(
                  bar,
                  baz
                )
                  123
                end
            "});
    }

    #[test]
    fn flags_singleton_method() {
        test::<ParameterAlignment>().expect_offense(indoc! {"
            def self.foo(bar,
                 baz)
                 ^^^ Align the parameters of a method definition if they span more than one line.
              123
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(ParameterAlignment);
