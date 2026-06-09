//! `Lint/Loop` — prefer `Kernel#loop` over post-condition begin/end loops.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/Loop
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's post-condition while/until detection and unsafe
//!   autocorrection shape using the folded `post: true` loop nodes.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Use `Kernel#loop` with `break` rather than `begin/end/until`(or `while`).";

#[derive(Default)]
pub struct Loop;

#[cop(
    name = "Lint/Loop",
    description = "Prefer Kernel#loop over begin/end while/until post loops.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl Loop {
    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !cx.is_post_condition_loop(node) {
        return;
    }
    let (cond, body, break_keyword) = match *cx.kind(node) {
        NodeKind::While { cond, body, .. } => (cond, body, "unless"),
        NodeKind::Until { cond, body, .. } => (cond, body, "if"),
        _ => return,
    };
    let Some(body) = body.get() else { return; };
    let body_begin = cx.loc(body).keyword();
    let body_end = cx.loc(body).end_keyword();
    if body_begin == Range::ZERO || body_end == Range::ZERO {
        return;
    }
    let Some(keyword) = post_keyword_range(node, body_end.end, cx) else {
        return;
    };

    cx.emit_offense(keyword, MSG, None);
    cx.emit_edit(body_begin, "loop do");
    cx.emit_edit(
        Range {
            start: body_end.end,
            end: cx.range(node).end,
        },
        "",
    );
    let indent = line_indent(body_end.start, cx.source());
    let condition = cx.raw_source(cx.range(cond));
    cx.emit_edit(
        Range {
            start: body_end.start,
            end: body_end.start,
        },
        &format!("break {break_keyword} {condition}\n{indent}"),
    );
}

fn post_keyword_range(node: NodeId, search_start: u32, cx: &Cx<'_>) -> Option<Range> {
    let keyword = match cx.kind(node) {
        NodeKind::While { .. } => "while",
        NodeKind::Until { .. } => "until",
        _ => return None,
    };
    let source = cx.source();
    let start = search_start as usize;
    let end = cx.range(node).end as usize;
    let rel = source.get(start..end)?.find(keyword)?;
    Some(Range {
        start: (start + rel) as u32,
        end: (start + rel + keyword.len()) as u32,
    })
}

fn line_indent(offset: u32, source: &str) -> &str {
    let offset = offset as usize;
    let line_start = source
        .as_bytes()
        .get(..offset)
        .and_then(|prefix| prefix.iter().rposition(|&b| b == b'\n'))
        .map_or(0, |idx| idx + 1);
    source.get(line_start..offset).unwrap_or("")
}

murphy_plugin_api::submit_cop!(Loop);

#[cfg(test)]
mod tests {
    use super::Loop;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_begin_end_while() {
        test::<Loop>().expect_correction(
            indoc! {r#"
                begin
                  something
                end while test
                    ^^^^^ Use `Kernel#loop` with `break` rather than `begin/end/until`(or `while`).
            "#},
            "loop do\n  something\nbreak unless test\nend\n",
        );
    }

    #[test]
    fn accepts_existing_loop_break() {
        test::<Loop>().expect_no_offenses("loop do; one; two; break unless test; end\n");
    }

    #[test]
    fn flags_and_corrects_begin_end_until() {
        test::<Loop>().expect_correction(
            indoc! {r#"
                begin
                  something
                end until test
                    ^^^^^ Use `Kernel#loop` with `break` rather than `begin/end/until`(or `while`).
            "#},
            "loop do\n  something\nbreak if test\nend\n",
        );
    }

    #[test]
    fn line_indent_accepts_non_char_boundary_offsets() {
        assert_eq!(super::line_indent(1, "é\n  end"), "");
    }
}
