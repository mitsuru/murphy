//! `Layout/MultilineMethodParameterLineBreaks` — each parameter in a
//! multi-line method definition must start on its own line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineMethodParameterLineBreaks
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `def`/`defs` nodes whose parameter list spans more than one
//!   physical line and where two or more parameters share a line. Mirrors
//!   RuboCop's `MultilineElementLineBreaks#check_line_breaks`: an offense is
//!   reported (one per offending parameter) at each parameter that does not
//!   begin its own line, except the first parameter (RuboCop's NOTE defers
//!   moving the first argument to `Layout/FirstMethodParameterLineBreak`).
//!
//!   The `AllowMultilineFinalElement` config key is honored. It changes
//!   RuboCop's `all_on_same_line?` guard: with the default `false`, the list
//!   is single-line when the first parameter's first line equals the last
//!   parameter's *last* line; with `true`, only the parameters' *start* lines
//!   are compared, so a final parameter that merely spans lines does not force
//!   the list multi-line and is not flagged.
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop's corrector inserts a
//!   newline before each offending parameter; the detect-only port ships
//!   without it.
//! ```
//!
//! ## Matched shapes
//!
//! `def`/`defs` nodes whose argument list spans more than one line and where
//! a non-first parameter begins on the same line as the previous parameter.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str =
    "Each parameter in a multi-line method definition must start on a separate line.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineMethodParameterLineBreaks;

/// Options for [`MultilineMethodParameterLineBreaks`]. `AllowMultilineFinalElement`
/// matches RuboCop verbatim; the default is `false`.
#[derive(CopOptions)]
pub struct MultilineMethodParameterLineBreaksOptions {
    #[option(
        name = "AllowMultilineFinalElement",
        default = false,
        description = "Allow the final parameter to span multiple lines without flagging it."
    )]
    pub allow_multiline_final_element: bool,
}

#[cop(
    name = "Layout/MultilineMethodParameterLineBreaks",
    description = "Each parameter in a multi-line method definition must start on a separate line.",
    default_severity = "warning",
    default_enabled = false,
    options = MultilineMethodParameterLineBreaksOptions,
)]
impl MultilineMethodParameterLineBreaks {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns the 1-based line number of a byte offset.
fn line_of(offset: u32, src: &[u8]) -> usize {
    1 + src[..offset as usize].iter().filter(|&&b| b == b'\n').count()
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<MultilineMethodParameterLineBreaksOptions>();

    let Some(args_node) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(list) = cx.kind(args_node) else {
        return;
    };
    let params = cx.list(*list);
    if params.is_empty() {
        return;
    }

    let src = cx.source().as_bytes();

    // RuboCop's `all_on_same_line?` guard. With the default
    // `AllowMultilineFinalElement: false`, the list is "single line" only if
    // the first parameter's first line equals the last parameter's *last*
    // line. With `AllowMultilineFinalElement: true`, only the *start* lines
    // are compared (`same_line?(first, last)`), so a final parameter that
    // merely spans lines does not force the whole list multi-line.
    let first_start_line = line_of(cx.range(params[0]).start, src);
    let last = params[params.len() - 1];
    let last_line = if opts.allow_multiline_final_element {
        line_of(cx.range(last).start, src)
    } else {
        line_of(cx.range(last).end, src)
    };
    if first_start_line == last_line {
        return;
    }

    // Each parameter (other than the first) must begin on a line strictly
    // after the previous parameter's start line. RuboCop reports an offense
    // on every parameter that does not start its own line.
    let mut prev_start_line = first_start_line;
    for &param in &params[1..] {
        let start = cx.range(param).start;
        let this_start_line = line_of(start, src);

        if this_start_line == prev_start_line {
            cx.emit_offense(offending_range(param, cx), MSG, None);
        }
        prev_start_line = this_start_line;
    }
}

/// Highlight the offending parameter (its source range, trimmed to its first
/// line so multi-line defaults do not over-highlight).
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
    use super::{
        MultilineMethodParameterLineBreaks, MultilineMethodParameterLineBreaksOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    fn allow_final() -> MultilineMethodParameterLineBreaksOptions {
        MultilineMethodParameterLineBreaksOptions {
            allow_multiline_final_element: true,
        }
    }

    #[test]
    fn flags_param_sharing_line_with_previous() {
        test::<MultilineMethodParameterLineBreaks>().expect_offense(indoc! {"
            def foo(a, b,
                       ^ Each parameter in a multi-line method definition must start on a separate line.
              c
            )
            end
        "});
    }

    #[test]
    fn accepts_each_param_on_own_line() {
        test::<MultilineMethodParameterLineBreaks>().expect_no_offenses(indoc! {"
            def foo(
              a,
              b,
              c
            )
            end
        "});
    }

    #[test]
    fn accepts_single_line_signature() {
        test::<MultilineMethodParameterLineBreaks>().expect_no_offenses(indoc! {"
            def foo(a, b, c)
            end
        "});
    }

    #[test]
    fn accepts_no_args() {
        test::<MultilineMethodParameterLineBreaks>().expect_no_offenses(indoc! {"
            def foo
            end
        "});
    }

    #[test]
    fn flags_multiple_params_on_first_line() {
        test::<MultilineMethodParameterLineBreaks>().expect_offense(indoc! {"
            def foo(a,
              b, c
                 ^ Each parameter in a multi-line method definition must start on a separate line.
            )
            end
        "});
    }

    #[test]
    fn accepts_multiline_default_value_each_on_own_line() {
        test::<MultilineMethodParameterLineBreaks>().expect_no_offenses(indoc! {"
            def foo(
              a,
              b = {
                foo: 'bar'
              }
            )
            end
        "});
    }

    #[test]
    fn flags_singleton_method() {
        test::<MultilineMethodParameterLineBreaks>().expect_offense(indoc! {"
            def self.foo(a, b,
                            ^ Each parameter in a multi-line method definition must start on a separate line.
              c
            )
            end
        "});
    }

    // AllowMultilineFinalElement: false (default) flags a multi-line final
    // parameter that shares the opening line with earlier parameters.
    #[test]
    fn default_flags_multiline_final_element() {
        test::<MultilineMethodParameterLineBreaks>().expect_offense(indoc! {"
            def foo(a, b = {
                       ^^^^^ Each parameter in a multi-line method definition must start on a separate line.
              foo: 'bar'
            })
            end
        "});
    }

    // AllowMultilineFinalElement: true accepts the same shape — only the
    // parameters' start lines are compared, and both start on line 1.
    #[test]
    fn allow_final_accepts_multiline_final_element() {
        test::<MultilineMethodParameterLineBreaks>()
            .with_options(&allow_final())
            .expect_no_offenses(indoc! {"
                def foo(a, b = {
                  foo: 'bar'
                })
                end
            "});
    }
}

murphy_plugin_api::submit_cop!(MultilineMethodParameterLineBreaks);
