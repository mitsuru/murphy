//! `Lint/RedundantWithObject` — checks for unused `with_object` values.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantWithObject
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial Murphy v1 port covers the common block, numblock, itblock, and
//!   safe-navigation shapes with autocorrection. It intentionally limits
//!   detection to calls whose block uses only one logical argument, matching
//!   RuboCop's redundant-object criterion.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

#[derive(Default)]
pub struct RedundantWithObject;

#[cop(
    name = "Lint/RedundantWithObject",
    description = "Checks for redundant `with_object` calls.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RedundantWithObject {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(block: NodeId, cx: &Cx<'_>) {
    let Some(call) = redundant_call(block, cx) else {
        return;
    };
    let Some(method) = cx.method_name(call) else {
        return;
    };
    let Some(range) = call_tail_range(call, block, cx) else {
        return;
    };

    if method == "each_with_object" {
        cx.emit_offense(range, "Use `each` instead of `each_with_object`.", None);
        cx.emit_edit(range, "each");
    } else {
        cx.emit_offense(range, "Remove redundant `with_object`.", None);
        if let Some(dot) = cx.call_operator_loc(call) {
            cx.emit_edit(
                Range {
                    start: dot.start,
                    end: range.end,
                },
                "",
            );
        }
    }
}

fn redundant_call(block: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let call = match *cx.kind(block) {
        NodeKind::Block { call, args, .. } => {
            if plain_block_arg_count(args, cx) != 1 {
                return None;
            }
            call
        }
        NodeKind::Numblock { send, max_n, .. } => {
            if max_n != 1 {
                return None;
            }
            send
        }
        NodeKind::Itblock { send, .. } => send,
        _ => return None,
    };

    let method = cx.method_name(call)?;
    if !matches!(method, "each_with_object" | "with_object") {
        return None;
    }
    if cx.call_arguments(call).is_empty() {
        return None;
    }
    Some(call)
}

fn plain_block_arg_count(args: NodeId, cx: &Cx<'_>) -> usize {
    let NodeKind::Args(list) = *cx.kind(args) else {
        return 0;
    };
    cx.list(list)
        .iter()
        .filter(|&&arg| matches!(cx.kind(arg), NodeKind::Arg(_)))
        .count()
}

fn call_tail_range(call: NodeId, block: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let start = cx.selector(call).start;
    let mut end = find_block_opener(cx.selector(call).end, cx.range(block).end, cx)?;
    let bytes = cx.source().as_bytes();
    while end > start && bytes[end as usize - 1].is_ascii_whitespace() {
        end -= 1;
    }
    Some(Range { start, end })
}

fn find_block_opener(from: u32, to: u32, cx: &Cx<'_>) -> Option<u32> {
    let source = cx.source().as_bytes();
    let mut i = from as usize;
    let end = to as usize;
    while i < end {
        match source[i] {
            b'{' => return Some(i as u32),
            b'd' if source.get(i..i + 2) == Some(b"do") && word_boundary(source, i, i + 2) => {
                return Some(i as u32);
            }
            _ => i += 1,
        }
    }
    None
}

fn word_boundary(source: &[u8], start: usize, end: usize) -> bool {
    let before = start == 0 || !is_ident(source[start - 1]);
    let after = end >= source.len() || !is_ident(source[end]);
    before && after
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use murphy_plugin_api::test_support::{indoc, test};

    use super::RedundantWithObject;

    #[test]
    fn corrects_each_with_object_block() {
        test::<RedundantWithObject>().expect_correction(
            indoc! {r#"
                ary.each_with_object([]) { |v| v }
                    ^^^^^^^^^^^^^^^^^^^^ Use `each` instead of `each_with_object`.
            "#},
            "ary.each { |v| v }\n",
        );
    }

    #[test]
    fn corrects_safe_navigation_each_with_object() {
        test::<RedundantWithObject>().expect_correction(
            indoc! {r#"
                ary&.each_with_object([]) { |v| v }
                     ^^^^^^^^^^^^^^^^^^^^ Use `each` instead of `each_with_object`.
            "#},
            "ary&.each { |v| v }\n",
        );
    }

    #[test]
    fn corrects_chained_with_object() {
        test::<RedundantWithObject>().expect_correction(
            indoc! {r#"
                ary.each.with_object([]) { |v| v }
                         ^^^^^^^^^^^^^^^ Remove redundant `with_object`.
            "#},
            "ary.each { |v| v }\n",
        );
    }

    #[test]
    fn corrects_numblock_and_itblock() {
        test::<RedundantWithObject>()
            .expect_correction(
                indoc! {r#"
                    ary.each_with_object([]) { _1 }
                        ^^^^^^^^^^^^^^^^^^^^ Use `each` instead of `each_with_object`.
                "#},
                "ary.each { _1 }\n",
            )
            .expect_correction(
                indoc! {r#"
                    ary.each.with_object([]) { it }
                             ^^^^^^^^^^^^^^^ Remove redundant `with_object`.
                "#},
                "ary.each { it }\n",
            );
    }

    #[test]
    fn accepts_used_object_argument() {
        test::<RedundantWithObject>()
            .expect_no_offenses("ary.each_with_object([]) { |v, o| v; o }\n")
            .expect_no_offenses("ary.each_with_object { |v| v }\n");
    }
}

murphy_plugin_api::submit_cop!(RedundantWithObject);
