//! `Style/MapIntoArray` — use `map` instead of `each` with `<<`, `push`, or
//! `append` to collect elements into an array.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MapIntoArray
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects two patterns where `each` pushes elements into a destination array:
//!
//!   Pattern 1: local variable initialized as `[]`:
//!     `dest = []; src.each { |e| dest << expr }; dest`
//!     → `dest = src.map { |e| expr }`
//!
//!   Pattern 2: `[].tap` block:
//!     `[].tap { |dest| src.each { |e| dest << expr } }`
//!     → `src.map { |e| expr }`
//!
//!   Array initializers covered for Pattern 1:
//!     - `dest = []`
//!     - `dest = Array[]`
//!     - `dest = Array.new` / `dest = Array.new([])`
//!     - `dest = Array([])`
//!
//!   Push methods covered: `<<`, `push`, `append`.
//!
//!   Implementation constraints for Pattern 1 (Murphy structural approach):
//!     - Requires `dest = []` as the immediately preceding sibling of the `each`
//!       block; no intervening statements are allowed.
//!     - Requires `dest` NOT to appear inside the push argument
//!       (e.g. `dest << transform(e, dest)` is rejected — would change semantics).
//!     - Trailing `dest` lvar return is removed when it immediately follows the block.
//!
//!   Gaps vs. RuboCop:
//!     - RuboCop detects `dest = []; other_code; src.each { ... }` using
//!       VariableForce reference counting. Murphy only handles the
//!       immediately-preceding-sibling case.
//!     - Return-value-used detection is not implemented: RuboCop skips autocorrect
//!       when the each block's return value is used; Murphy always corrects.
//!     - `PreferredMethods` configuration (from Style/CollectionMethods) is
//!       not supported; always uses `map`.
//!
//!   Unsafe: not all objects with `each` have `map` (e.g. `ENV`).
//! ```
//!
//! ## Examples
//!
//! ```ruby
//! # bad
//! dest = []
//! src.each { |e| dest << e * 2 }
//! dest
//!
//! # good
//! dest = src.map { |e| e * 2 }
//!
//! # bad
//! [].tap do |dest|
//!   src.each { |e| dest << e * 2 }
//! end
//!
//! # good
//! src.map { |e| e * 2 }
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct MapIntoArray;

const MSG: &str = "Use `map` instead of `each` to map elements into an array.";

/// Methods that push to an array.
const PUSH_METHODS: &[&str] = &["<<", "push", "append"];

#[cop(
    name = "Style/MapIntoArray",
    description = "Use `map` instead of `each` with `<<`, `push`, or `append` to collect into an array.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
    safe_autocorrect = false,
)]
impl MapIntoArray {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_each_block(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_each_block(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_each_block(node, cx);
    }
}

/// Main check: returns whether `block_node` is an `each` block with a single
/// push statement, and if so, dispatches to pattern analysis.
fn check_each_block(block_node: NodeId, cx: &Cx<'_>) {
    // The block must call `each` on a non-nil, non-self receiver.
    let each_call = block_call(block_node, cx);
    if cx.method_name(each_call) != Some("each") {
        return;
    }
    let Some(each_receiver) = cx.call_receiver(each_call).get() else {
        return; // no receiver (self.each or bare each — skip)
    };
    // Skip `self` receiver.
    if matches!(cx.kind(each_receiver), NodeKind::SelfExpr) {
        return;
    }

    // The block body must be exactly one push statement: `dest.<<(expr)` etc.
    let Some(body) = block_body(block_node, cx).get() else {
        return;
    };
    let Some((dest_name, push_arg)) = extract_push_call(body, cx) else {
        return;
    };

    let Some(parent) = cx.parent(block_node).get() else {
        return;
    };

    // Check if this is the [].tap pattern: parent is a Block (the tap block).
    if matches!(cx.kind(parent), NodeKind::Block { .. }) {
        check_tap_pattern(block_node, parent, dest_name, push_arg, each_call, each_receiver, cx);
        return;
    }

    // For the local-variable pattern: parent must be a `begin` node.
    if !matches!(cx.kind(parent), NodeKind::Begin(_)) {
        return;
    }

    check_local_var_pattern(block_node, parent, dest_name, push_arg, each_call, each_receiver, cx);
}

/// Handles Pattern 2: `[].tap { |dest| <each_block> }`.
/// `parent` is the tap block (already confirmed to be a Block node by `check_each_block`).
fn check_tap_pattern(
    block_node: NodeId,
    parent: NodeId,   // The tap block
    dest_name: &str,
    push_arg: NodeId,
    _each_call: NodeId,
    each_receiver: NodeId,
    cx: &Cx<'_>,
) {
    // parent is the tap Block: Block(tap_call, tap_args, body=each_block)
    let NodeKind::Block { call: tap_call, args: tap_args, body: tap_body } = *cx.kind(parent) else {
        return;
    };
    // The tap block body must be exactly our `each` block.
    let Some(tap_body_node) = tap_body.get() else {
        return;
    };
    if tap_body_node != block_node {
        // Tap body is more than just the each block → skip.
        return;
    }
    // The tap call must be `.tap` on an empty array literal.
    if cx.method_name(tap_call) != Some("tap") {
        return;
    }
    let Some(tap_recv) = cx.call_receiver(tap_call).get() else {
        return;
    };
    if !is_empty_array_literal(tap_recv, cx) {
        return;
    }
    // The tap block must have exactly one arg matching dest_name.
    let tap_args_list = block_args_list(tap_args, cx);
    if tap_args_list.len() != 1 {
        return;
    }
    let NodeKind::Arg(tap_arg_sym) = *cx.kind(tap_args_list[0]) else {
        return;
    };
    if cx.symbol_str(tap_arg_sym) != dest_name {
        return;
    }

    // Emit offense on the each block.
    cx.emit_offense(cx.range(block_node), MSG, None);

    // Autocorrect: replace the entire tap block with `src.map { |e| expr }`.
    autocorrect_tap(parent, push_arg, each_receiver, cx);
}

/// Handles Pattern 1: `dest = []; src.each { |e| dest << expr }; dest`.
fn check_local_var_pattern(
    block_node: NodeId,
    parent: NodeId,
    dest_name: &str,
    push_arg: NodeId,
    each_call: NodeId,
    each_receiver: NodeId,
    cx: &Cx<'_>,
) {
    let NodeKind::Begin(list) = *cx.kind(parent) else {
        return;
    };
    let stmts = cx.list(list);

    // Find the index of block_node in stmts.
    let Some(block_idx) = stmts.iter().position(|&s| s == block_node) else {
        return;
    };
    if block_idx == 0 {
        return; // No preceding statement for assignment.
    }

    // The statement immediately before must be `dest = <empty_array>`.
    let prev = stmts[block_idx - 1];
    if !is_empty_array_assignment_to(prev, dest_name, cx) {
        return;
    }

    if !dest_used_only_for_mapping(parent, block_node, prev, dest_name, cx) {
        return;
    }

    // Guard: reject if `dest` appears inside the push argument (would change semantics).
    // e.g., `dest << transform(e, dest)` — dest referenced in the argument.
    if lvar_appears_in(push_arg, dest_name, cx) {
        return;
    }

    // Emit offense on the whole block node (consistent with RuboCop's add_offense(block)).
    cx.emit_offense(cx.range(block_node), MSG, None);

    // Autocorrect: replace prev + block + optional trailing lvar with `dest = src.map { ... }`.
    // Check for trailing lvar reference.
    let next_is_dest_lvar = if block_idx + 1 < stmts.len() {
        let next = stmts[block_idx + 1];
        is_lvar_of_name(next, dest_name, cx)
    } else {
        false
    };

    autocorrect_local_var(
        prev,
        block_node,
        if next_is_dest_lvar { Some(stmts[block_idx + 1]) } else { None },
        dest_name,
        push_arg,
        each_call,
        each_receiver,
        cx,
    );
}

// ---------------------------------------------------------------------------
// Autocorrect helpers
// ---------------------------------------------------------------------------

/// Autocorrect the local-variable pattern.
/// Transforms:
///   `dest = []; src.each { |e| dest << expr }; dest`
///   → `dest = src.map { |e| expr }`
#[allow(clippy::too_many_arguments)]
fn autocorrect_local_var(
    asgn_node: NodeId,
    block_node: NodeId,
    trailing_lvar: Option<NodeId>,
    dest_name: &str,
    push_arg: NodeId,
    _each_call: NodeId,
    each_receiver: NodeId,
    cx: &Cx<'_>,
) {
    // Build the replacement: `dest = src.map { |e| expr }`
    // = dest_name + " = " + receiver_source + ".map" + block_source_minus_each
    let receiver_src = cx.raw_source(cx.range(each_receiver));
    let arg_src = cx.raw_source(cx.range(push_arg));

    // Reconstruct block args and the block delimiters from the each block.
    // We want to preserve the block form (do..end vs { }) as closely as possible.
    // Strategy: replace just the method call part (each→map) and the push body (dest<<expr→expr).

    // For simplicity, build whole replacement:
    // `{dest_name} = {receiver}.map {block_args} {expr}`
    // We need the block args source and delimiters.
    let block_args_str = block_args_source(block_node, cx);
    let block_delims = block_delimiters(block_node, cx);

    let replacement = format!(
        "{} = {}.map{}{} {}{}",
        dest_name,
        receiver_src,
        block_delims.0,
        block_args_str,
        arg_src,
        block_delims.1,
    );

    // Range to replace: from asgn_node start to block_node end.
    let replace_range = Range {
        start: cx.range(asgn_node).start,
        end: cx.range(block_node).end,
    };
    cx.emit_edit(replace_range, &replacement);

    // Remove trailing `dest` lvar if present (including preceding whitespace/newline).
    if let Some(trailing) = trailing_lvar {
        // Delete from end of block_node to end of trailing lvar.
        // This covers the newline+spaces+dest.
        let del_range = Range {
            start: cx.range(block_node).end,
            end: cx.range(trailing).end,
        };
        cx.emit_edit(del_range, "");
    }
}

/// Autocorrect the tap pattern.
/// Transforms:
///   `[].tap { |dest| src.each { |e| dest << expr } }`
///   → `src.map { |e| expr }`
fn autocorrect_tap(
    tap_block: NodeId,
    push_arg: NodeId,
    each_receiver: NodeId,
    cx: &Cx<'_>,
) {
    // Get the each block from the tap block body.
    let NodeKind::Block { body, .. } = *cx.kind(tap_block) else { return; };
    let Some(each_block) = body.get() else { return; };

    let receiver_src = cx.raw_source(cx.range(each_receiver));
    let arg_src = cx.raw_source(cx.range(push_arg));
    let block_args_str = block_args_source(each_block, cx);
    let block_delims = block_delimiters(each_block, cx);

    let replacement = format!(
        "{}.map{}{} {}{}",
        receiver_src,
        block_delims.0,
        block_args_str,
        arg_src,
        block_delims.1,
    );

    cx.emit_edit(cx.range(tap_block), &replacement);
}

/// Return the block args string including pipes: ` |e|` or ` |k, v|` (with leading space).
/// Constructs the pipe-wrapped argument list from the Arg symbol names.
fn block_args_source(block_node: NodeId, cx: &Cx<'_>) -> String {
    let NodeKind::Block { args, .. } = *cx.kind(block_node) else {
        return String::new();
    };
    let NodeKind::Args(args_list) = *cx.kind(args) else {
        return String::new();
    };
    let children = cx.list(args_list);
    if children.is_empty() {
        return String::new();
    }

    // Build "|arg1, arg2|" from the Arg symbol names.
    let mut names = Vec::new();
    for &child in children {
        let name_str = match *cx.kind(child) {
            NodeKind::Arg(sym) => cx.symbol_str(sym).to_string(),
            _ => return String::new(), // complex arg pattern, skip
        };
        names.push(name_str);
    }
    format!(" |{}|", names.join(", "))
}

/// Return `("{", "}")` or `("do", "end")` block delimiters.
fn block_delimiters(block_node: NodeId, cx: &Cx<'_>) -> (&'static str, &'static str) {
    // Check source for `do` keyword after the call.
    let src = cx.source();
    let call = block_call(block_node, cx);
    let call_end = cx.range(call).end as usize;
    if call_end < src.len() {
        let after_call = &src[call_end..];
        let trimmed = after_call.trim_start();
        if trimmed.starts_with("do") {
            return (" do", "
end");
        }
    }
    (" {", " }")
}

// ---------------------------------------------------------------------------
// Shape helpers
// ---------------------------------------------------------------------------

/// Return the call node from a block (Block/Numblock/Itblock).
fn block_call(block_node: NodeId, cx: &Cx<'_>) -> NodeId {
    match *cx.kind(block_node) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => send,
        _ => block_node,
    }
}

/// Return the body of a block.
fn block_body(block_node: NodeId, cx: &Cx<'_>) -> OptNodeId {
    match *cx.kind(block_node) {
        NodeKind::Block { body, .. } => body,
        NodeKind::Numblock { body, .. } | NodeKind::Itblock { body, .. } => body,
        _ => OptNodeId::NONE,
    }
}

/// Return the args node id from a Block (not Numblock/Itblock).
fn block_args_list<'a>(args: NodeId, cx: &Cx<'a>) -> &'a [NodeId] {
    match *cx.kind(args) {
        NodeKind::Args(list) => cx.list(list),
        _ => &[],
    }
}

/// Returns `Some((dest_name, push_arg_node))` if `node` is a push call:
/// `dest.<<(expr)`, `dest.push(expr)`, or `dest.append(expr)`.
/// The receiver must be an lvar (dest is a local variable).
fn extract_push_call<'a>(node: NodeId, cx: &Cx<'a>) -> Option<(&'a str, NodeId)> {
    let (receiver_opt, method, args) = match *cx.kind(node) {
        NodeKind::Send { receiver, method, args } => (receiver, method, args),
        _ => return None,
    };
    let method_str = cx.symbol_str(method);
    if !PUSH_METHODS.contains(&method_str) {
        return None;
    }
    let receiver = receiver_opt.get()?;
    // Receiver must be an lvar.
    let NodeKind::Lvar(dest_sym) = *cx.kind(receiver) else {
        return None;
    };
    let dest_name = cx.symbol_str(dest_sym);

    // Must have exactly one argument, and that argument must be "suitable"
    // (not a splat, forwarded-restarg, etc.).
    let args_list = cx.list(args);
    if args_list.len() != 1 {
        return None;
    }
    let arg = args_list[0];
    if !is_suitable_argument(arg, cx) {
        return None;
    }

    Some((dest_name, arg))
}

/// Returns `true` if the argument is "suitable" (not a bare splat, forwarded
/// restarg, etc.). This mirrors RuboCop's `suitable_argument_node?`.
fn is_suitable_argument(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Splat(inner) => inner.get().is_none(), // bare `*` is not suitable; `*x` is
        _ => true,
    }
}

/// Returns `true` if `node` is an empty array literal.
fn is_empty_array_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Array(list) => cx.list(list).is_empty(),
        _ => false,
    }
}

/// Returns `true` if `node` is an assignment to `dest_name` of an empty array.
/// Handles: `dest = []`, `dest = Array[]`, `dest = Array.new`, `dest = Array.new([])`,
/// `dest = Array([])`.
fn is_empty_array_assignment_to(node: NodeId, dest_name: &str, cx: &Cx<'_>) -> bool {
    let NodeKind::Lvasgn { name, value } = *cx.kind(node) else {
        return false;
    };
    if cx.symbol_str(name) != dest_name {
        return false;
    }
    let Some(val) = value.get() else {
        return false;
    };
    is_empty_array_value(val, cx)
}

/// Returns `true` if `node` represents an empty array value.
fn is_empty_array_value(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        // `[]`
        NodeKind::Array(list) => cx.list(list).is_empty(),
        // `Array[]` or `Array.new` or `Array.new([])` or `Array([])`
        NodeKind::Send { receiver, method, args } => {
            let method_str = cx.symbol_str(method);
            let args_list = cx.list(args);
            match method_str {
                "[]" => {
                    // `Array[]` — receiver must be const Array, no args
                    args_list.is_empty() && is_array_const(receiver.get(), cx)
                }
                "new" => {
                    // `Array.new` or `Array.new([])`
                    is_array_const(receiver.get(), cx)
                        && (args_list.is_empty()
                            || (args_list.len() == 1
                                && is_empty_array_literal(args_list[0], cx)))
                }
                "Array" => {
                    // `Array([])` — bare method call with single empty-array arg
                    receiver.get().is_none()
                        && args_list.len() == 1
                        && is_empty_array_literal(args_list[0], cx)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// Returns `true` if `node` is the constant `Array`.
fn is_array_const(node: Option<NodeId>, cx: &Cx<'_>) -> bool {
    let Some(n) = node else { return false; };
    match *cx.kind(n) {
        NodeKind::Const { name, scope } => {
            cx.symbol_str(name) == "Array"
                && matches!(
                    scope.get().map(|s| cx.kind(s)),
                    None | Some(NodeKind::Cbase)
                )
        }
        _ => false,
    }
}

/// Returns `true` if `node` is `lvar(dest_name)`.
fn is_lvar_of_name(node: NodeId, dest_name: &str, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Lvar(sym) => cx.symbol_str(sym) == dest_name,
        _ => false,
    }
}

/// Check that `dest` is used only for mapping in the sibling statements.
///
/// Structural check: verifies that no sibling statement BETWEEN `asgn` and
/// `block` (exclusive) contains `dest` as an lvar. This avoids the
/// cross-scope complexity of tracking `dest` references inside the block.
///
/// The block body is assumed to be exactly `dest.push(expr)` (already
/// validated by `extract_push_call`). We just need to ensure that no other
/// sibling stmt between asgn and block uses dest.
fn dest_used_only_for_mapping(
    begin_node: NodeId,
    block_node: NodeId,
    asgn_node: NodeId,
    dest_name: &str,
    cx: &Cx<'_>,
) -> bool {
    let NodeKind::Begin(list) = *cx.kind(begin_node) else {
        return false;
    };
    let stmts = cx.list(list);

    // Find indices of asgn and block.
    let Some(asgn_idx) = stmts.iter().position(|&s| s == asgn_node) else {
        return false;
    };
    let Some(block_idx) = stmts.iter().position(|&s| s == block_node) else {
        return false;
    };

    // Check that no statement BETWEEN asgn and block (exclusive) uses dest.
    for &stmt in &stmts[asgn_idx + 1..block_idx] {
        if lvar_appears_in(stmt, dest_name, cx) {
            return false;
        }
    }
    true
}

/// Returns `true` if `dest_name` appears as an `lvar` anywhere within `node`
/// (recursively, excluding scope boundaries like nested blocks).
fn lvar_appears_in(node: NodeId, dest_name: &str, cx: &Cx<'_>) -> bool {
    // Simple DFS, stopping at scope boundaries.
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        match *cx.kind(n) {
            NodeKind::Lvar(sym) => {
                if cx.symbol_str(sym) == dest_name {
                    return true;
                }
            }
            // Don't recurse into nested blocks/defs (scope boundaries).
            NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Itblock { .. }
            | NodeKind::Def { .. }
            | NodeKind::Defs { .. }
            | NodeKind::Class { .. }
            | NodeKind::Module { .. } => {}
            _ => {
                for child in cx.children(n) {
                    stack.push(child);
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::MapIntoArray;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- basic << form ---

    #[test]
    fn flags_each_with_shovel() {
        test::<MapIntoArray>().expect_offense(indoc! {"
            dest = []
            src.each { |e| dest << e * 2 }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `map` instead of `each` to map elements into an array.
            dest
        "});
    }

    #[test]
    fn corrects_each_with_shovel() {
        test::<MapIntoArray>().expect_correction(
            indoc! {"
                dest = []
                src.each { |e| dest << e * 2 }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `map` instead of `each` to map elements into an array.
                dest
            "},
            "dest = src.map { |e| e * 2 }\n",
        );
    }

    // --- push form ---

    #[test]
    fn flags_each_with_push() {
        test::<MapIntoArray>().expect_offense(indoc! {"
            dest = []
            src.each { |e| dest.push(e * 2) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `map` instead of `each` to map elements into an array.
            dest
        "});
    }

    #[test]
    fn corrects_each_with_push() {
        test::<MapIntoArray>().expect_correction(
            indoc! {"
                dest = []
                src.each { |e| dest.push(e * 2) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `map` instead of `each` to map elements into an array.
                dest
            "},
            "dest = src.map { |e| e * 2 }\n",
        );
    }

    // --- tap form ---

    #[test]
    fn flags_tap_each_shovel() {
        test::<MapIntoArray>().expect_offense(indoc! {"
            [].tap { |dest| src.each { |e| dest << e * 2 } }
                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `map` instead of `each` to map elements into an array.
        "});
    }

    #[test]
    fn corrects_tap_each_shovel() {
        test::<MapIntoArray>().expect_correction(
            indoc! {"
                [].tap { |dest| src.each { |e| dest << e * 2 } }
                                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `map` instead of `each` to map elements into an array.
            "},
            "src.map { |e| e * 2 }\n",
        );
    }

    // --- no-offense cases ---

    #[test]
    fn accepts_each_without_push() {
        test::<MapIntoArray>().expect_no_offenses("src.each { |e| puts e }\n");
    }

    #[test]
    fn accepts_dest_used_elsewhere() {
        // dest is used more than once in range → not only for mapping
        test::<MapIntoArray>().expect_no_offenses(indoc! {"
            dest = []
            src.each { |e| dest << e * 2; puts dest }
            dest
        "});
    }

    #[test]
    fn accepts_push_to_ivar() {
        // @dest is not a local variable
        test::<MapIntoArray>().expect_no_offenses(indoc! {"
            @dest = []
            src.each { |e| @dest << e * 2 }
            @dest
        "});
    }

    #[test]
    fn accepts_each_on_self() {
        test::<MapIntoArray>().expect_no_offenses("each { |e| dest << e }\n");
    }

    #[test]
    fn accepts_dest_referenced_in_push_arg() {
        // dest appears inside the push argument → semantics would change.
        test::<MapIntoArray>().expect_no_offenses(indoc! {"
            dest = []
            src.each { |e| dest << transform(e, dest) }
            dest
        "});
    }
}

murphy_plugin_api::submit_cop!(MapIntoArray);
