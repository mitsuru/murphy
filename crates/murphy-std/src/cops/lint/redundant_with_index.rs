//! `Lint/RedundantWithIndex` — checks for unused `with_index` values.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RedundantWithIndex
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial Murphy v1 port covers the common block, numblock, itblock, and
//!   safe-navigation shapes with autocorrection. It intentionally limits
//!   detection to calls whose block uses only one logical argument, matching
//!   RuboCop's redundant-index criterion.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range, def_node_matcher};

// Verbatim port of the call head (murphy-s1yc.39):
// `(call _ {:each_with_index :with_index} ...)` — `call` = `{send csend}`
// covers safe-navigation (`ary&.each_with_index { |v| v }`), mirroring the
// upstream inner `$(call _ {:each_with_index :with_index} ...)` which also
// matches csend. The `_` receiver binds an absent or present receiver per
// murphy-if9y; trailing `...` absorbs any argument list (e.g. the
// `with_index(1)` offset), so the one-logical-arg block guard and the
// chained-receiver guard below apply separately.
def_node_matcher!(
    redundant_with_index_call,
    "(call _ {:each_with_index :with_index} ...)"
);

#[derive(Default)]
pub struct RedundantWithIndex;

#[cop(
    name = "Lint/RedundantWithIndex",
    description = "Checks for redundant `with_index` calls.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RedundantWithIndex {
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

    if method == "each_with_index" {
        cx.emit_offense(range, "Use `each` instead of `each_with_index`.", None);
        cx.emit_edit(range, "each");
    } else {
        cx.emit_offense(range, "Remove redundant `with_index`.", None);
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

    // Verbatim `(call _ {:each_with_index :with_index} ...)` head: filters to
    // the redundant-index methods on either send or csend (safe-navigation),
    // with any receiver (absent or present). Without this, an unrelated call
    // with any receiver (e.g. `ary.map`) would run the block-arg checks
    // instead of being rejected by the method set up front.
    if !redundant_with_index_call(call, cx) {
        return None;
    }
    let method = cx.method_name(call)?;
    if !matches!(method, "each_with_index" | "with_index") {
        return None;
    }
    // Trailing `...` absorbs any argument list, so the head matches even
    // `each_with_index(1)` (offset form). That shape is still redundant and
    // must be flagged (the offset is dropped by the `each` correction), so
    // no zero-arg guard applies here -- unlike the sort_by/min_by batches.
    // The one-logical-arg block guard above and the chained-receiver guard
    // below apply separately.
    if method == "with_index" {
        let receiver = cx.call_receiver(call).get()?;
        cx.call_receiver(receiver).get()?;
    }
    Some(call)
}

fn plain_block_arg_count(args: NodeId, cx: &Cx<'_>) -> usize {
    let NodeKind::Args(list) = *cx.kind(args) else {
        return 0;
    };
    cx.list(list)
        .iter()
        .filter(|&&arg| !matches!(cx.kind(arg), NodeKind::Shadowarg(_) | NodeKind::Blockarg(_)))
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
            b'\'' | b'"' => i = skip_quoted_string(source, i, end),
            b'{' => return Some(i as u32),
            b'd' if source.get(i..i + 2) == Some(b"do") && word_boundary(source, i, i + 2) => {
                return Some(i as u32);
            }
            _ => i += 1,
        }
    }
    None
}

fn skip_quoted_string(source: &[u8], start: usize, end: usize) -> usize {
    let quote = source[start];
    let mut i = start + 1;
    while i < end {
        if source[i] == b'\\' {
            i += 2;
        } else if source[i] == quote {
            return i + 1;
        } else {
            i += 1;
        }
    }
    i
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

    use super::RedundantWithIndex;

    #[test]
    fn corrects_each_with_index_block() {
        test::<RedundantWithIndex>().expect_correction(
            indoc! {r#"
                ary.each_with_index { |v| v }
                    ^^^^^^^^^^^^^^^ Use `each` instead of `each_with_index`.
            "#},
            "ary.each { |v| v }\n",
        );
    }

    #[test]
    fn skips_block_opener_inside_string_literal() {
        test::<RedundantWithIndex>().expect_correction(
            indoc! {r#"
                ary.each_with_index("{") { |v| v }
                    ^^^^^^^^^^^^^^^^^^^^ Use `each` instead of `each_with_index`.
            "#},
            "ary.each { |v| v }\n",
        );
    }

    #[test]
    fn corrects_safe_navigation_each_with_index() {
        test::<RedundantWithIndex>().expect_correction(
            indoc! {r#"
                ary&.each_with_index { |v| v }
                     ^^^^^^^^^^^^^^^ Use `each` instead of `each_with_index`.
            "#},
            "ary&.each { |v| v }\n",
        );
    }

    #[test]
    fn removes_each_with_index_offset_argument() {
        test::<RedundantWithIndex>().expect_correction(
            indoc! {r#"
                ary.each_with_index(1) { |v| v }
                    ^^^^^^^^^^^^^^^^^^ Use `each` instead of `each_with_index`.
            "#},
            "ary.each { |v| v }\n",
        );
    }

    #[test]
    fn corrects_chained_with_index() {
        test::<RedundantWithIndex>().expect_correction(
            indoc! {r#"
                ary.each.with_index(1) { |v| v }
                         ^^^^^^^^^^^^^ Remove redundant `with_index`.
            "#},
            "ary.each { |v| v }\n",
        );
    }

    #[test]
    fn corrects_numblock_and_itblock() {
        test::<RedundantWithIndex>()
            .expect_correction(
                indoc! {r#"
                    ary.each_with_index { _1 }
                        ^^^^^^^^^^^^^^^ Use `each` instead of `each_with_index`.
                "#},
                "ary.each { _1 }\n",
            )
            .expect_correction(
                indoc! {r#"
                    ary.each.with_index { it }
                             ^^^^^^^^^^ Remove redundant `with_index`.
                "#},
                "ary.each { it }\n",
            );
    }

    #[test]
    fn accepts_used_index_argument() {
        test::<RedundantWithIndex>()
            .expect_no_offenses("ary.each_with_index { |v, i| v; i }\n")
            .expect_no_offenses("ary.with_index { |v| v }\n")
            .expect_no_offenses("with_index { _1 }\n");
    }

    #[test]
    fn accepts_destructured_value_with_index_argument() {
        test::<RedundantWithIndex>().expect_no_offenses("ary.each_with_index { |(a, b), i| a }\n");
    }

    // --- Characterization (murphy-s1yc.39): pin the exact node set the
    // block/numblock/itblock dispatch with hand-rolled method_name
    // each_with_index/with_index matches, so the verbatim
    // `(call _ {:each_with_index :with_index} ...)` port can be proven
    // byte-identical. `call` covers safe-navigation; the `_` receiver binds
    // an absent or present receiver per murphy-if9y; trailing `...` absorbs
    // any argument list (e.g. the `with_index(1)` offset), so the
    // one-logical-arg block guard and the chained-receiver guard below apply
    // separately.

    #[test]
    fn s1yc39_flags_csend_corrects() {
        // Safe-navigation: `call` covers `csend`, mirroring upstream inner
        // `$(call _ {:each_with_index :with_index} ...)` which also matches
        // csend. Pre-port block dispatch already flags via method_name; the
        // verbatim head collapses the workaround and keeps it byte-identical.
        // Verified vs standalone NodePattern: `(call _ {:each_with_index
        // :with_index} ...)` matches csend.
        test::<RedundantWithIndex>().expect_correction(
            indoc! {r#"
                ary&.each_with_index { |v| v }
                     ^^^^^^^^^^^^^^^ Use `each` instead of `each_with_index`.
            "#},
            "ary&.each { |v| v }\n",
        );
    }

    #[test]
    fn s1yc39_flags_bare() {
        // Bare `each_with_index { |v| v }` has no receiver; the `_` wildcard
        // binds the absent receiver per murphy-if9y, so the verbatim head
        // matches and the one-arg block guard below flags -- pinned here.
        // (Upstream has an extra `return unless node.receiver` guard outside
        // the matcher, so upstream would not flag bare; murphy preserves its
        // flag behavior byte-identical with the head matching.)
        // Verified vs standalone NodePattern: `(call _ {:each_with_index
        // :with_index} ...)` matches bare send.
        test::<RedundantWithIndex>().expect_offense(indoc! {r#"
            each_with_index { |v| v }
            ^^^^^^^^^^^^^^^ Use `each` instead of `each_with_index`.
        "#});
    }

    #[test]
    fn s1yc39_flags_with_args() {
        // Offset arg: trailing `...` absorbs any argument list, so the head
        // matches `each_with_index(1)` and the block guard below still flags
        // (the offset is dropped by the `each` correction) -- pinned here.
        // Verified vs standalone NodePattern: `(call _ {:each_with_index
        // :with_index} ...)` matches `ary.each_with_index(1)`.
        test::<RedundantWithIndex>().expect_correction(
            indoc! {r#"
                ary.each_with_index(1) { |v| v }
                    ^^^^^^^^^^^^^^^^^^ Use `each` instead of `each_with_index`.
            "#},
            "ary.each { |v| v }\n",
        );
    }

    #[test]
    fn s1yc39_accepts_unrelated() {
        // Unrelated `map` is not in the head method set, so the verbatim head
        // rejects it up front -- pinned here.
        // Verified vs standalone NodePattern: `(call _ {:each_with_index
        // :with_index} ...)` does not match `ary.map`.
        test::<RedundantWithIndex>().expect_no_offenses("ary.map { |v| v }\n");
    }
}

murphy_plugin_api::submit_cop!(RedundantWithIndex);
