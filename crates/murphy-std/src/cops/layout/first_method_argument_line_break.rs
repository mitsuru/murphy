//! `Layout/FirstMethodArgumentLineBreak` — checks for a line break before the
//! first argument in a multi-line method call.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/FirstMethodArgumentLineBreak
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-rt5p
//! notes: >
//!   Mirrors RuboCop's `on_send`/`on_csend` via the shared
//!   `FirstElementLineBreak#check_method_line_break` mixin: a parenthesized,
//!   multi-line argument list whose first argument shares the call's opening
//!   line is flagged, and an autocorrect inserts a newline before that first
//!   argument. The trailing-brace-less-hash expansion is implemented: a final
//!   `key => value` / `key: value` hash argument without braces is exploded
//!   into its pairs so each pair is treated as a positional "argument" when
//!   deciding multi-line-ness (matching RuboCop's `args.concat(args.pop.children)`).
//!   `AllowedMethods` (default `[]`) and `AllowMultilineFinalElement` (default
//!   `false`) are supported. Known gaps versus RuboCop:
//!   (1) `on_super` is not dispatched — RuboCop aliases `on_super` to the same
//!       handler, but Murphy's `cx.call_arguments` only resolves `Send`/`Csend`
//!       argument lists, so explicit-argument `super(...)` calls are not yet
//!       checked. This is an ABI-shape limitation (no `Super` argument helper),
//!       not a boundary bypass.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct (ADR 0035 const-metadata cop pattern).
#[derive(Default)]
pub struct FirstMethodArgumentLineBreak;

#[derive(CopOptions)]
pub struct FirstMethodArgumentLineBreakOptions {
    #[option(
        name = "AllowMultilineFinalElement",
        default = false,
        description = "Allow the final element of the argument list to span multiple lines without a leading line break."
    )]
    pub allow_multiline_final_element: bool,
    #[option(
        name = "AllowedMethods",
        default = [],
        description = "Method calls whose argument lists are exempt from this cop."
    )]
    pub allowed_methods: Vec<String>,
}

#[cop(
    name = "Layout/FirstMethodArgumentLineBreak",
    description = "Add a line break before the first argument of a multi-line method argument list.",
    default_severity = "warning",
    default_enabled = true,
    options = FirstMethodArgumentLineBreakOptions,
)]
impl FirstMethodArgumentLineBreak {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

const MSG: &str = "Add a line break before the first argument of a multi-line method argument list.";

fn check(node: NodeId, cx: &Cx<'_>) {
    // `return if allowed_method?(node.method_name)`
    let opts = cx.options_or_default::<FirstMethodArgumentLineBreakOptions>();
    if let Some(name) = cx.method_name(node)
        && opts.allowed_methods.iter().any(|m| m == name)
    {
        return;
    }

    let args = cx.call_arguments(node);
    let Some(&first_arg) = args.first() else {
        return;
    };

    // `method_uses_parens?(node, children.first)`: the call's source line, up to
    // the first argument's column, must end with `(` + optional whitespace.
    // Without parens, RuboCop ignores the call entirely (the `# ignored`
    // doc example).
    if !method_uses_parens(node, first_arg, cx) {
        return;
    }

    // RuboCop expands a trailing brace-less hash into its pairs so each pair
    // counts as an "argument" for the multi-line decision:
    //   `args.concat(args.pop.children) if last_arg&.hash_type? && !last_arg&.braces?`
    // We collect the effective "elements" (first_line, last_line) for each.
    let &last_arg = args.last().expect("args non-empty");
    let mut elements: Vec<NodeId> = Vec::with_capacity(args.len() + 2);
    if is_braceless_hash(last_arg, cx) {
        elements.extend_from_slice(&args[..args.len() - 1]);
        elements.extend(cx.hash_pairs(last_arg));
    } else {
        elements.extend_from_slice(args);
    }
    // After expansion, the first element by source order is still `first_arg`
    // (a leading positional or, for a lone braceless hash, its first pair).
    let Some(&first_element) = elements.first() else {
        return;
    };

    // `check_children_line_break(node, elements, ignore_last:)`
    let src = cx.source();
    let call_line = line_of(cx.range(node).start, src);
    // `min = first_by_line(children)` — element with the earliest start line.
    let first_by_line = *elements
        .iter()
        .min_by_key(|&&e| line_of(cx.range(e).start, src))
        .unwrap_or(&first_element);
    let min_line = line_of(cx.range(first_by_line).start, src);
    // `return if line != min.first_line` — the first element is not on the
    // same line as the method call's opening line.
    if call_line != min_line {
        return;
    }

    // `max_line = last_line(children, ignore_last:)` then `return if line == max_line`.
    let ignore_last = opts.allow_multiline_final_element;
    let max_line = elements
        .iter()
        .map(|&e| {
            if ignore_last {
                line_of(cx.range(e).start, src)
            } else {
                line_of(cx.range(e).end.saturating_sub(1).max(cx.range(e).start), src)
            }
        })
        .max()
        .unwrap_or(call_line);
    if call_line == max_line {
        return;
    }

    // Offense at `min` (the first element by line); autocorrect inserts a
    // newline before it (`EmptyLineCorrector.insert_before`).
    let offense = cx.range(first_by_line);
    cx.emit_offense(offense, MSG, None);
    cx.emit_edit(
        Range {
            start: offense.start,
            end: offense.start,
        },
        "\n",
    );
}

/// `method_uses_parens?(node, limit)`: the substring of the call's first source
/// line up to `limit`'s start column must match `/\s*\(\s*$/` — i.e. the text
/// immediately before the first argument is an opening paren plus optional
/// whitespace (the call really opened its argument list with `(` on this line).
fn method_uses_parens(node: NodeId, first_arg: NodeId, cx: &Cx<'_>) -> bool {
    let src = cx.source().as_bytes();
    let node_start = cx.range(node).start as usize;
    let arg_start = cx.range(first_arg).start as usize;
    // The call's first source line begins at the byte after the previous `\n`.
    let line_start = src[..node_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    if arg_start < line_start || arg_start > src.len() {
        return false;
    }
    // Bytes from the line start up to (but excluding) the first argument.
    let prefix = &src[line_start..arg_start];
    // `/\s*\(\s*$/`: strip trailing spaces/tabs, then the last non-space byte
    // must be `(`.
    let trimmed_end = prefix
        .iter()
        .rposition(|&b| b != b' ' && b != b'\t')
        .map_or(0, |p| p + 1);
    trimmed_end > 0 && prefix[trimmed_end - 1] == b'('
}

/// A trailing hash argument written without braces (`foo(1, a: 2)`), matching
/// RuboCop's `last_arg.hash_type? && !last_arg.braces?`. A braced hash literal's
/// source range starts with `{`; a brace-less keyword/trailing hash starts at
/// its first pair, so its first source byte is not `{`. (`cx.loc().begin()`
/// only recognises `(`, not `{`, so we inspect the source byte directly.)
fn is_braceless_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::Hash(_)) {
        return false;
    }
    cx.source().as_bytes().get(cx.range(node).start as usize) != Some(&b'{')
}

/// 1-based source line number containing byte `offset`.
fn line_of(offset: u32, src: &str) -> usize {
    src.as_bytes()[..offset as usize]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::{FirstMethodArgumentLineBreak, FirstMethodArgumentLineBreakOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_first_arg_on_same_line_as_call() {
        test::<FirstMethodArgumentLineBreak>().expect_correction(
            indoc! {r#"
                method(foo, bar,
                       ^^^ Add a line break before the first argument of a multi-line method argument list.
                  baz)
            "#},
            "method(\nfoo, bar,\n  baz)\n",
        );
    }

    #[test]
    fn accepts_line_break_before_first_arg() {
        test::<FirstMethodArgumentLineBreak>().expect_no_offenses(indoc! {r#"
            method(
              foo, bar,
              baz)
        "#});
    }

    #[test]
    fn accepts_method_without_parens() {
        // The `# ignored` doc example: no parens → not checked.
        test::<FirstMethodArgumentLineBreak>().expect_no_offenses(indoc! {r#"
            method foo, bar,
              baz
        "#});
    }

    #[test]
    fn accepts_single_line_call() {
        test::<FirstMethodArgumentLineBreak>().expect_no_offenses("method(foo, bar, baz)\n");
    }

    #[test]
    fn accepts_no_arguments() {
        test::<FirstMethodArgumentLineBreak>().expect_no_offenses("method()\nmethod\n");
    }

    #[test]
    fn flags_braceless_trailing_hash_pairs() {
        // The trailing braceless hash is exploded into its pairs, so the call
        // is multi-line and the first arg shares the opening line → flag.
        test::<FirstMethodArgumentLineBreak>().expect_correction(
            indoc! {r#"
                method(foo, bar,
                       ^^^ Add a line break before the first argument of a multi-line method argument list.
                  baz: "a",
                  qux: "b")
            "#},
            "method(\nfoo, bar,\n  baz: \"a\",\n  qux: \"b\")\n",
        );
    }

    #[test]
    fn flags_with_explicit_brace_hash_default() {
        // AllowMultilineFinalElement: false (default) — a multi-line final
        // braced hash still triggers since the first arg shares the open line.
        test::<FirstMethodArgumentLineBreak>().expect_correction(
            indoc! {r#"
                method(foo, bar, {
                       ^^^ Add a line break before the first argument of a multi-line method argument list.
                  baz: "a",
                })
            "#},
            "method(\nfoo, bar, {\n  baz: \"a\",\n})\n",
        );
    }

    #[test]
    fn accepts_multiline_final_element_when_allowed() {
        let opts = FirstMethodArgumentLineBreakOptions {
            allow_multiline_final_element: true,
            allowed_methods: vec![],
        };
        test::<FirstMethodArgumentLineBreak>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                method(foo, bar, {
                  baz: "a",
                  qux: "b",
                })
            "#});
    }

    #[test]
    fn accepts_allowed_method() {
        let opts = FirstMethodArgumentLineBreakOptions {
            allow_multiline_final_element: false,
            allowed_methods: vec!["method".to_string()],
        };
        test::<FirstMethodArgumentLineBreak>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                method(foo, bar,
                  baz)
            "#});
    }

    #[test]
    fn flags_on_csend() {
        test::<FirstMethodArgumentLineBreak>().expect_correction(
            indoc! {r#"
                obj&.method(foo, bar,
                            ^^^ Add a line break before the first argument of a multi-line method argument list.
                  baz)
            "#},
            "obj&.method(\nfoo, bar,\n  baz)\n",
        );
    }
}

murphy_plugin_api::submit_cop!(FirstMethodArgumentLineBreak);
