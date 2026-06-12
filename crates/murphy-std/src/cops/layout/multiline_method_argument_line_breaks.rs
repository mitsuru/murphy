//! `Layout/MultilineMethodArgumentLineBreaks` — each argument in a multi-line
//! method call must start on its own line.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/MultilineMethodArgumentLineBreaks
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `send`/`csend` nodes whose argument list spans more than one
//!   physical line and where two or more arguments share a line. Mirrors
//!   RuboCop's `on_send` plus the `MultilineElementLineBreaks` mixin:
//!
//!   - Skips the `[]=` setter method (`node.method?(:[]=)`).
//!   - Expands a trailing brace-less hash argument (implicit kwargs) into its
//!     individual key/value pairs before the line-break check, exactly like
//!     `args = args[0...-1] + last_arg.children if last_arg.hash_type? &&
//!     !last_arg.braces?`.
//!   - `check_line_breaks`: returns early via `all_on_same_line?`; with the
//!     default `AllowMultilineFinalElement: false`, the list is single-line
//!     when `first.first_line == last.last_line`; with `true`, only the
//!     elements' *start* lines are compared (`same_line?(first, last)`), so a
//!     multi-line final element does not force the list multi-line.
//!   - Offense loop uses `last_seen_line` seeded to -1: each child whose
//!     `first_line <= last_seen_line` is flagged; otherwise `last_seen_line`
//!     advances to that child's `last_line`. The first argument is therefore
//!     never flagged (RuboCop defers it to `FirstMethodArgumentLineBreak`).
//!
//!   Autocorrect: not implemented (v1 gap). RuboCop inserts a newline before
//!   each offending argument; the detect-only port ships without it.
//! ```
//!
//! ## Matched shapes
//!
//! `send`/`csend` nodes whose argument list spans more than one line and where
//! a non-first argument begins on the same line as the previous argument.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str =
    "Each argument in a multi-line method call must start on a separate line.";

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct MultilineMethodArgumentLineBreaks;

/// Options for [`MultilineMethodArgumentLineBreaks`].
/// `AllowMultilineFinalElement` matches RuboCop verbatim; the default is
/// `false`.
#[derive(CopOptions)]
pub struct MultilineMethodArgumentLineBreaksOptions {
    #[option(
        name = "AllowMultilineFinalElement",
        default = false,
        description = "Allow the final argument to span multiple lines without flagging it."
    )]
    pub allow_multiline_final_element: bool,
}

#[cop(
    name = "Layout/MultilineMethodArgumentLineBreaks",
    description = "Each argument in a multi-line method call must start on a separate line.",
    default_severity = "warning",
    default_enabled = false,
    options = MultilineMethodArgumentLineBreaksOptions,
)]
impl MultilineMethodArgumentLineBreaks {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns the 1-based line number of a byte offset.
fn line_of(offset: u32, src: &[u8]) -> usize {
    1 + src[..offset as usize].iter().filter(|&&b| b == b'\n').count()
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // `return if node.method?(:[]=)` — the index-setter never line-breaks.
    if cx.method_name(node) == Some("[]=") {
        return;
    }

    let args = cx.call_arguments(node);
    if args.is_empty() {
        return;
    }

    // Expand a trailing brace-less hash into its pairs, matching
    // `args = args[0...-1] + last_arg.children if last_arg.hash_type? &&
    // !last_arg.braces?`. A brace-less hash is a `Hash` node whose source does
    // not start with `{`.
    let last_arg = args[args.len() - 1];
    let children: Vec<NodeId> = if let NodeKind::Hash(list) = cx.kind(last_arg) {
        if cx.raw_source(cx.range(last_arg)).starts_with('{') {
            args.to_vec()
        } else {
            let mut expanded = args[..args.len() - 1].to_vec();
            expanded.extend_from_slice(cx.list(*list));
            expanded
        }
    } else {
        args.to_vec()
    };

    check_line_breaks(&children, cx);
}

fn check_line_breaks(children: &[NodeId], cx: &Cx<'_>) {
    let opts = cx.options_or_default::<MultilineMethodArgumentLineBreaksOptions>();
    let src = cx.source().as_bytes();

    // `all_on_same_line?`: empty → true (already filtered to non-empty here).
    let first = children[0];
    let last = children[children.len() - 1];
    let first_start_line = line_of(cx.range(first).start, src);
    let last_line = if opts.allow_multiline_final_element {
        // `same_line?(first, last)` — compare start lines only.
        line_of(cx.range(last).start, src)
    } else {
        // `first.first_line == last.last_line`.
        line_of(cx.range(last).end, src)
    };
    if first_start_line == last_line {
        return;
    }

    // Offense loop: `last_seen_line = -1`; flag any child whose first line is
    // `<= last_seen_line`, else advance `last_seen_line` to its last line.
    let mut last_seen_line: i64 = -1;
    for &child in children {
        let first_line = line_of(cx.range(child).start, src) as i64;
        if last_seen_line >= first_line {
            cx.emit_offense(offending_range(child, cx), MSG, None);
        } else {
            last_seen_line = line_of(cx.range(child).end, src) as i64;
        }
    }
}

/// Highlight the offending argument, trimmed to its first line so a
/// multi-line argument does not over-highlight.
fn offending_range(arg: NodeId, cx: &Cx<'_>) -> Range {
    let r = cx.range(arg);
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
        MultilineMethodArgumentLineBreaks, MultilineMethodArgumentLineBreaksOptions,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    fn allow_final() -> MultilineMethodArgumentLineBreaksOptions {
        MultilineMethodArgumentLineBreaksOptions {
            allow_multiline_final_element: true,
        }
    }

    #[test]
    fn flags_arg_sharing_line_with_previous() {
        test::<MultilineMethodArgumentLineBreaks>().expect_offense(indoc! {"
            foo(a, b,
                   ^ Each argument in a multi-line method call must start on a separate line.
              c)
        "});
    }

    #[test]
    fn accepts_each_arg_on_own_line() {
        test::<MultilineMethodArgumentLineBreaks>().expect_no_offenses(indoc! {"
            foo(
              a,
              b,
              c
            )
        "});
    }

    #[test]
    fn accepts_single_line_call() {
        test::<MultilineMethodArgumentLineBreaks>().expect_no_offenses("foo(a, b, c)\n");
    }

    #[test]
    fn accepts_no_args() {
        test::<MultilineMethodArgumentLineBreaks>().expect_no_offenses("foo()\n");
    }

    #[test]
    fn flags_multiple_args_on_first_line() {
        test::<MultilineMethodArgumentLineBreaks>().expect_offense(indoc! {"
            foo(a,
              b, c)
                 ^ Each argument in a multi-line method call must start on a separate line.
        "});
    }

    #[test]
    fn does_not_flag_first_argument() {
        // First argument never flagged even when it shares its line.
        test::<MultilineMethodArgumentLineBreaks>().expect_offense(indoc! {"
            foo(a, b,
                   ^ Each argument in a multi-line method call must start on a separate line.
              c
            )
        "});
    }

    #[test]
    fn skips_index_setter() {
        test::<MultilineMethodArgumentLineBreaks>().expect_no_offenses(indoc! {"
            foo[a, b] = c
        "});
    }

    // Trailing brace-less hash is expanded into its pairs before the check.
    #[test]
    fn flags_braceless_trailing_hash_pairs_on_same_line() {
        test::<MultilineMethodArgumentLineBreaks>().expect_offense(indoc! {"
            foo(bar,
              a: 1, b: 2)
                    ^^^^ Each argument in a multi-line method call must start on a separate line.
        "});
    }

    #[test]
    fn accepts_braceless_trailing_hash_pairs_on_own_lines() {
        test::<MultilineMethodArgumentLineBreaks>().expect_no_offenses(indoc! {"
            foo(bar,
              a: 1,
              b: 2)
        "});
    }

    // AllowMultilineFinalElement: false (default) flags a multi-line final
    // argument that shares the opening line with earlier arguments. Both `b`
    // and the hash share line 1, so both are flagged (RuboCop's
    // `last_seen_line` does not advance past a flagged child).
    #[test]
    fn default_flags_multiline_final_element() {
        test::<MultilineMethodArgumentLineBreaks>().expect_offense(indoc! {"
            foo(a, b, {
                   ^ Each argument in a multi-line method call must start on a separate line.
                      ^ Each argument in a multi-line method call must start on a separate line.
              foo: 'bar'
            })
        "});
    }

    // AllowMultilineFinalElement: true accepts the same shape — only the
    // arguments' start lines are compared, and all start on line 1.
    #[test]
    fn allow_final_accepts_multiline_final_element() {
        test::<MultilineMethodArgumentLineBreaks>()
            .with_options(&allow_final())
            .expect_no_offenses(indoc! {"
                foo(a, b, {
                  foo: 'bar'
                })
            "});
    }

    #[test]
    fn flags_safe_navigation_call() {
        test::<MultilineMethodArgumentLineBreaks>().expect_offense(indoc! {"
            obj&.foo(a, b,
                        ^ Each argument in a multi-line method call must start on a separate line.
              c)
        "});
    }
}

murphy_plugin_api::submit_cop!(MultilineMethodArgumentLineBreaks);
