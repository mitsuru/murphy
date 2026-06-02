//! `Style/EachWithObject` — prefer `each_with_object` over `inject`/`reduce`
//! when the accumulator is returned unchanged at the end.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EachWithObject
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Block form (`inject`/`reduce` with explicit 2-arg block) is fully
//!   covered with autocorrect. Numblock form (`inject(x) { _1[_2] = _2; _1 }`)
//!   is detected but autocorrect is not implemented due to complexity of
//!   renaming `_1`/`_2` references throughout the body while avoiding
//!   clobbering. Itblock form is excluded (Ruby 3.4+ only, no inject idiom).
//!   Basic-literal argument guard matches RuboCop: `inject(0)`, `inject("")`,
//!   `inject(:sym)`, `inject(true)`, `inject(false)`, `inject(nil)` are NOT
//!   flagged. `inject({})`, `inject([])` and no-argument forms ARE flagged.
//!   Accumulator-reassignment guard: if the first block param is re-assigned
//!   (Lvasgn targeting that symbol) anywhere in the body, the offense is skipped.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! [1, 2].inject({}) { |a, e| a[e] = e; a }
//! [1, 2].reduce([]) { |a, e| a << e; a }
//!
//! # good
//! [1, 2].each_with_object({}) { |e, a| a[e] = e }
//!
//! # no offense — basic literal argument (int/str/float/sym/bool/nil)
//! [1, 2].inject(0) { |a, e| a + e }
//!
//! # no offense — accumulator reassigned
//! [1, 2].inject({}) { |a, e| a = {}; a }
//!
//! # no offense — last expr is not the accumulator
//! [1, 2].inject({}) { |a, e| a[e] = e; e }
//! ```
//!
//! ## Autocorrect
//!
//! 1. Rename `inject`/`reduce` → `each_with_object` (selector rename)
//! 2. Swap block params: `|a, e|` → `|e, a|`
//! 3. Remove trailing return of accumulator (last expression)
//!
//! For a multi-statement body ending in `; acc` or newline + `acc`, the
//! trailing accumulator expression is removed. If the body IS just `acc`,
//! the body becomes empty (nil).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Use `each_with_object` instead of `%method%`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct EachWithObject;

#[cop(
    name = "Style/EachWithObject",
    description = "Prefer `each_with_object` over `inject`/`reduce` when accumulator is returned.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EachWithObject {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_inject_block(node, cx);
    }
}

fn check_inject_block(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call, args, body } = *cx.kind(node) else {
        return;
    };

    // Must be inject/reduce.
    let NodeKind::Send { method, .. } = *cx.kind(call) else {
        return;
    };
    let method_name = cx.symbol_str(method);
    if method_name != "inject" && method_name != "reduce" {
        return;
    }

    // Must have receiver.
    if cx.call_receiver(call).get().is_none() {
        return;
    }

    // Must have exactly one send argument (the initial value).
    let send_args = cx.call_arguments(call);
    if send_args.len() > 1 {
        return;
    }

    // If there IS a send arg, it must NOT be a basic literal.
    if send_args.first().copied().is_some_and(|arg| is_basic_literal(arg, cx)) {
        return;
    }

    // Block must have exactly 2 named args.
    let NodeKind::Args(args_list) = *cx.kind(args) else {
        return;
    };
    let block_args = cx.list(args_list);
    if block_args.len() != 2 {
        return;
    }
    let acc_arg = block_args[0];
    let NodeKind::Arg(acc_sym) = *cx.kind(acc_arg) else {
        return;
    };

    // Body must be present.
    let Some(body_id) = body.get() else {
        return;
    };

    // Last expression of body must be an lvar referencing the accumulator.
    let Some(return_val) = last_expr(body_id, cx) else {
        return;
    };
    let NodeKind::Lvar(ret_sym) = *cx.kind(return_val) else {
        return;
    };
    if ret_sym != acc_sym {
        return;
    }

    // Accumulator must NOT be reassigned in the body.
    if accumulator_reassigned(body_id, acc_sym, cx) {
        return;
    }

    let msg = MSG.replace("%method%", method_name);
    cx.emit_offense(cx.node(call).loc.name, &msg, None);

    // Autocorrect.
    autocorrect_block(call, args, body_id, return_val, cx);
}

/// Autocorrect for block form:
/// 1. Rename selector: inject/reduce → each_with_object
/// 2. Swap the two block args in source
/// 3. Remove the trailing return value from the body
fn autocorrect_block(
    call: NodeId,
    args: NodeId,
    body_id: NodeId,
    return_val: NodeId,
    cx: &Cx<'_>,
) {
    // 1. Rename selector.
    cx.emit_edit(cx.node(call).loc.name, "each_with_object");

    // 2. Swap block params.
    let NodeKind::Args(args_list) = *cx.kind(args) else {
        return;
    };
    let block_args = cx.list(args_list);
    if block_args.len() != 2 {
        return;
    }
    let acc_range = cx.range(block_args[0]);
    let elem_range = cx.range(block_args[1]);
    let acc_src = cx.raw_source(acc_range).to_owned();
    let elem_src = cx.raw_source(elem_range).to_owned();
    cx.emit_edit(acc_range, &elem_src);
    cx.emit_edit(elem_range, &acc_src);

    // 3. Remove trailing return value from body.
    remove_trailing_return(body_id, return_val, cx);
}

/// Remove the trailing `return_val` expression from the body.
///
/// Removes the trailing return value from the body.
///
/// For a `Begin` body, removes either the whole line (if it occupies its own line)
/// or from the preceding `;` separator (inline form).
/// For a single-expression body, replaces the body with empty.
fn remove_trailing_return(
    body_id: NodeId,
    return_val: NodeId,
    cx: &Cx<'_>,
) {
    let source = cx.source().as_bytes();

    // Case 1: body IS the return value (single-expr body).
    if body_id == return_val {
        cx.emit_edit(cx.range(body_id), "");
        return;
    }

    // Case 2: body is a Begin node ending with return_val.
    let NodeKind::Begin(list) = *cx.kind(body_id) else {
        return;
    };
    let children = cx.list(list);
    if children.last() != Some(&return_val) {
        return;
    }

    let ret_range = cx.range(return_val);

    // Check if the return value occupies a whole line (whitespace before it on the line).
    let line_start = source[..ret_range.start as usize]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);

    let before_ret = &source[line_start..ret_range.start as usize];
    let whole_line = before_ret.iter().all(|&b| b == b' ' || b == b'\t');

    if whole_line {
        // Remove from line start to end of return_val (include trailing newline if present).
        let end_offset = {
            let ret_end = ret_range.end as usize;
            if source.get(ret_end) == Some(&b'\n') {
                (ret_end + 1) as u32
            } else {
                ret_range.end
            }
        };
        cx.emit_edit(
            Range {
                start: line_start as u32,
                end: end_offset,
            },
            "",
        );
    } else {
        // Single-line body like `a[e] = e; a` — remove from the semicolon/space before `a`.
        // Find the preceding separator (`;` or whitespace after previous statement).
        let sep_start = find_before_separator(return_val, cx);
        cx.emit_edit(
            Range {
                start: sep_start,
                end: ret_range.end,
            },
            "",
        );
    }
}

/// Find the start of the separator (`;` or space) before the given node.
fn find_before_separator(node: NodeId, cx: &Cx<'_>) -> u32 {
    let node_start = cx.range(node).start;
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    // Find a semicolon or newline token just before this node.
    let lo = toks.partition_point(|t| t.range.start < node_start);
    if lo == 0 {
        return node_start;
    }

    // Check the last token before this node for `;` or newline.
    if let Some(tok) = toks[..lo].last() {
        if tok.kind == SourceTokenKind::Other
            && &source[tok.range.start as usize..tok.range.end as usize] == b";"
        {
            return tok.range.start;
        }
        if tok.kind == SourceTokenKind::Newline || tok.kind == SourceTokenKind::IgnoredNewline {
            return tok.range.start;
        }
    }

    // Fall back: remove from one space before the node.
    if node_start > 0 && source[node_start as usize - 1] == b' ' {
        node_start - 1
    } else {
        node_start
    }
}

/// Get the last expression of a block body.
///
/// - If body is a `Begin` node, returns the last child.
/// - Otherwise returns the body node itself.
fn last_expr(body: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(body) {
        NodeKind::Begin(list) => cx.list(list).last().copied(),
        _ => Some(body),
    }
}

/// Returns true if the node is a basic literal (int, float, str, sym, true, false, nil).
fn is_basic_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Int(_)
            | NodeKind::Float(_)
            | NodeKind::Str(_)
            | NodeKind::Sym(_)
            | NodeKind::True_
            | NodeKind::False_
            | NodeKind::Nil
    )
}

/// Returns true if the accumulator variable (by symbol) is re-assigned anywhere
/// in the subtree rooted at `body`.
fn accumulator_reassigned(body: NodeId, acc_sym: murphy_plugin_api::Symbol, cx: &Cx<'_>) -> bool {
    accumulator_reassigned_in(body, acc_sym, cx)
}

fn accumulator_reassigned_in(
    node: NodeId,
    acc_sym: murphy_plugin_api::Symbol,
    cx: &Cx<'_>,
) -> bool {
    // Check if this node is an Lvasgn targeting acc_sym.
    if matches!(*cx.kind(node), NodeKind::Lvasgn { name, .. } if name == acc_sym) {
        return true;
    }

    // Recurse into children.
    for child in cx.children(node) {
        if accumulator_reassigned_in(child, acc_sym, cx) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::EachWithObject;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Positive cases (offense) ------------------------------------

    #[test]
    fn flags_inject_with_hash_accumulator() {
        test::<EachWithObject>().expect_offense(indoc! {"
            [1, 2].inject({}) { |a, e| a[e] = e; a }
                   ^^^^^^ Use `each_with_object` instead of `inject`.
        "});
    }

    #[test]
    fn flags_reduce_with_array_accumulator() {
        test::<EachWithObject>().expect_offense(indoc! {"
            [1, 2].reduce([]) { |a, e| a << e; a }
                   ^^^^^^ Use `each_with_object` instead of `reduce`.
        "});
    }

    #[test]
    fn flags_inject_without_initial_value() {
        test::<EachWithObject>().expect_offense(indoc! {"
            [1, 2].inject { |a, e| a + e; a }
                   ^^^^^^ Use `each_with_object` instead of `inject`.
        "});
    }

    // ----- Negative cases (no offense) --------------------------------

    #[test]
    fn accepts_inject_with_integer_initial_value() {
        test::<EachWithObject>().expect_no_offenses("[1, 2].inject(0) { |a, e| a + e }\n");
    }

    #[test]
    fn accepts_inject_with_string_initial_value() {
        test::<EachWithObject>().expect_no_offenses(
            "[1, 2].inject(\"\") { |a, e| a + e.to_s; a }\n",
        );
    }

    #[test]
    fn accepts_inject_with_symbol_initial_value() {
        test::<EachWithObject>().expect_no_offenses("[1, 2].inject(:+)\n");
    }

    #[test]
    fn accepts_inject_where_acc_is_reassigned() {
        test::<EachWithObject>().expect_no_offenses(
            "[1, 2].inject({}) { |a, e| a = {}; a }\n",
        );
    }

    #[test]
    fn accepts_inject_where_last_expr_is_not_acc() {
        test::<EachWithObject>().expect_no_offenses(
            "[1, 2].inject({}) { |a, e| a[e] = e; e }\n",
        );
    }

    #[test]
    fn accepts_inject_without_receiver() {
        test::<EachWithObject>().expect_no_offenses("inject({}) { |a, e| a[e] = e; a }\n");
    }

    // ----- Autocorrect -----------------------------------------------

    #[test]
    fn corrects_inject_hash() {
        test::<EachWithObject>().expect_correction(
            indoc! {"
                [1, 2].inject({}) { |a, e| a[e] = e; a }
                       ^^^^^^ Use `each_with_object` instead of `inject`.
            "},
            "[1, 2].each_with_object({}) { |e, a| a[e] = e }\n",
        );
    }

    #[test]
    fn corrects_reduce_array() {
        test::<EachWithObject>().expect_correction(
            indoc! {"
                [1, 2].reduce([]) { |a, e| a << e; a }
                       ^^^^^^ Use `each_with_object` instead of `reduce`.
            "},
            "[1, 2].each_with_object([]) { |e, a| a << e }\n",
        );
    }

    #[test]
    fn corrects_multiline_inject() {
        test::<EachWithObject>().expect_correction(
            indoc! {"
                [1, 2].inject({}) do |a, e|
                       ^^^^^^ Use `each_with_object` instead of `inject`.
                  a[e] = e
                  a
                end
            "},
            "[1, 2].each_with_object({}) do |e, a|\n  a[e] = e\nend\n",
        );
    }
}
murphy_plugin_api::submit_cop!(EachWithObject);
