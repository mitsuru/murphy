//! `Style/SingleLineMethods` — flags single-line method definitions with a body.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SingleLineMethods
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `def`/`defs` nodes that are single-line and not endless.
//!   AllowIfMethodIsEmpty: true (default) — empty method bodies (`def no_op; end`)
//!   are allowed by default.
//!   Autocorrect: rewrites the method to multi-line form. Each `;`-separated
//!   body part gets its own line with 2-space indent, and `end` is placed on a
//!   new line. The separator region (`;` + any following whitespace up to the
//!   part start) is replaced with `"\n  "` for body parts and `"\n"` for `end`.
//!   The `correct_to_endless` branch (Ruby 3.0+ interop with Style/EndlessMethod)
//!   is not implemented (conservative v1 gap; always corrects to multiline).
//! ```
//!
//! ## Matched shapes
//!
//! `def` and `defs` nodes that:
//! - Are single-line
//! - Are NOT endless (`def foo = expr` — no `end` keyword)
//! - Have a non-nil body, OR `AllowIfMethodIsEmpty: false` is configured
//!
//! ## Autocorrect
//!
//! Replaces `;` separators between the method signature and each body part
//! with newlines + 2-space indent, and places `end` on its own line.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Avoid single-line method definitions.";

/// Configuration options for `Style/SingleLineMethods`.
#[derive(CopOptions)]
pub struct SingleLineMethodsOptions {
    #[option(
        name = "AllowIfMethodIsEmpty",
        default = true,
        description = "Allow single-line method definitions when the body is empty."
    )]
    pub allow_if_method_is_empty: bool,
}

#[derive(Default)]
pub struct SingleLineMethods;

#[cop(
    name = "Style/SingleLineMethods",
    description = "Avoid single-line method definitions.",
    default_severity = "warning",
    default_enabled = true,
    options = SingleLineMethodsOptions,
)]
impl SingleLineMethods {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must be single-line.
    if !cx.is_single_line(node) {
        return;
    }

    // Skip endless methods (no `end` keyword — `def foo = expr`).
    if cx.loc(node).end_keyword() == Range::ZERO {
        return;
    }

    let opts = cx.options_or_default::<SingleLineMethodsOptions>();
    let has_body = cx.def_body(node).get().is_some();

    // Respect AllowIfMethodIsEmpty.
    if opts.allow_if_method_is_empty && !has_body {
        return;
    }

    cx.emit_offense(cx.range(node), MSG, None);

    // Autocorrect: expand to multiline.
    autocorrect(node, cx, has_body);
}

fn autocorrect(node: NodeId, cx: &Cx<'_>, has_body: bool) {
    let Some(end_tok) = find_end_token(node, cx) else {
        return;
    };

    if !has_body {
        // No body: just place `end` on its own line.
        // Replace whitespace before `end` with a newline.
        let src = cx.source();
        let before_end = scan_whitespace_before(src, end_tok.start);
        cx.emit_edit(
            Range {
                start: before_end,
                end: end_tok.start,
            },
            "\n",
        );
        return;
    }

    // With body: collect body parts.
    let body_node = cx.def_body(node).get().unwrap();
    let parts: Vec<NodeId> = if let NodeKind::Begin(list) = *cx.kind(body_node) {
        cx.list(list).to_vec()
    } else {
        vec![body_node]
    };

    // For each body part, replace the separator region before it with "\n  ".
    // The separator region is [prev_separator_start, part.start).
    // We scan backwards from part.start to find the `;` separator start.
    let src = cx.source();
    let node_start = cx.range(node).start;

    for part in &parts {
        let part_start = cx.range(*part).start;
        // Find the `;` or other separator before part_start, scanning from node_start.
        let sep_start = find_separator_before(cx, node_start, part_start);
        if sep_start < part_start {
            cx.emit_edit(
                Range {
                    start: sep_start,
                    end: part_start,
                },
                "\n  ",
            );
        }
    }

    // Replace whitespace/separator before `end` with a newline.
    let last_part_end = cx.range(*parts.last().unwrap()).end;
    let before_end = scan_whitespace_before(src, end_tok.start);
    // Use the start of trailing whitespace, but don't go before last_part.end.
    let close_start = before_end.max(last_part_end);
    cx.emit_edit(
        Range {
            start: close_start,
            end: end_tok.start,
        },
        "\n",
    );
}

/// Find the start of the separator (`;` plus any surrounding whitespace)
/// immediately before `to`, scanning forward from `from`.
/// Returns the position of the `;` token if found, or `to` if not found.
fn find_separator_before(cx: &Cx<'_>, from: u32, to: u32) -> u32 {
    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();
    let idx = toks.partition_point(|t| t.range.start < from);
    // Walk forward to find the last `;` token before `to`.
    let mut last_semi: Option<u32> = None;
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        if tok.kind == SourceTokenKind::Other
            && &src[tok.range.start as usize..tok.range.end as usize] == b";"
        {
            last_semi = Some(tok.range.start);
        }
    }
    last_semi.unwrap_or(to)
}

/// Find the `end` keyword token that terminates the method definition.
fn find_end_token(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let node_end = cx.range(node).end;
    let toks = cx.sorted_tokens();
    let src = cx.source().as_bytes();
    let idx = toks.partition_point(|t| t.range.end < node_end);
    if let Some(tok) = toks.get(idx) {
        if tok.range.end == node_end
            && tok.kind == SourceTokenKind::Other
            && &src[tok.range.start as usize..tok.range.end as usize] == b"end"
        {
            return Some(tok.range);
        }
    }
    None
}

/// Scan backwards from `pos` to find the start of a whitespace run.
fn scan_whitespace_before(src: &str, pos: u32) -> u32 {
    let bytes = src.as_bytes();
    let mut i = pos as usize;
    while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
        i -= 1;
    }
    i as u32
}

#[cfg(test)]
mod tests {
    use super::SingleLineMethods;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_single_line_method_with_body() {
        test::<SingleLineMethods>().expect_offense(indoc! {"
            def some_method; body end
            ^^^^^^^^^^^^^^^^^^^^^^^^^  Avoid single-line method definitions.
        "});
    }

    #[test]
    fn flags_single_line_singleton_method() {
        test::<SingleLineMethods>().expect_offense(indoc! {"
            def self.foo; bar end
            ^^^^^^^^^^^^^^^^^^^^^  Avoid single-line method definitions.
        "});
    }

    #[test]
    fn flags_single_line_with_hash_body() {
        test::<SingleLineMethods>().expect_offense(indoc! {"
            def link_to(url); {:name => url}; end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^  Avoid single-line method definitions.
        "});
    }

    #[test]
    fn accepts_multiline_method() {
        test::<SingleLineMethods>().expect_no_offenses(indoc! {"
            def some_method
              do_stuff
            end
        "});
    }

    #[test]
    fn accepts_empty_method_by_default() {
        test::<SingleLineMethods>().expect_no_offenses("def no_op; end\n");
    }

    #[test]
    fn accepts_endless_method() {
        test::<SingleLineMethods>().expect_no_offenses("def foo = bar\n");
    }

    #[test]
    fn corrects_single_line_method() {
        test::<SingleLineMethods>().expect_correction(
            indoc! {"
                def some_method; body end
                ^^^^^^^^^^^^^^^^^^^^^^^^^  Avoid single-line method definitions.
            "},
            "def some_method\n  body\nend\n",
        );
    }

    #[test]
    fn corrects_single_line_method_with_args() {
        test::<SingleLineMethods>().expect_correction(
            indoc! {"
                def f(x); b = foo end
                ^^^^^^^^^^^^^^^^^^^^^  Avoid single-line method definitions.
            "},
            "def f(x)\n  b = foo\nend\n",
        );
    }

    #[test]
    fn corrects_singleton_method() {
        test::<SingleLineMethods>().expect_correction(
            indoc! {"
                def self.foo; bar end
                ^^^^^^^^^^^^^^^^^^^^^  Avoid single-line method definitions.
            "},
            "def self.foo\n  bar\nend\n",
        );
    }
}

murphy_plugin_api::submit_cop!(SingleLineMethods);
