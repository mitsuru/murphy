//! `Style/PartitionInsteadOfDoubleSelect` — suggest `partition` over a
//! consecutive `select`/`reject` (or negated double-`select`) pair on the
//! same receiver with the same block body.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/PartitionInsteadOfDoubleSelect
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-e7bz.59.1]
//! notes: >
//!   Flags two consecutive statements that call a select-family method
//!   (`select`/`filter`/`find_all`) and `reject` (complementary pair, identical
//!   predicate) — or the same select-family/`reject` method twice with one
//!   predicate the negation of the other — on the same receiver. Covers brace
//!   blocks, `do…end` blocks, numbered-parameter blocks, `it`-blocks,
//!   safe-navigation receivers, and symbol-proc / block-pass forms
//!   (`&:positive?`), including the cross case of a block paired with a
//!   matching symbol-proc. The two statements may each be bare or a
//!   single-variable assignment, but must be immediate siblings inside an
//!   *implicit* statement sequence (top-level program, method/class/module
//!   body) — mirroring RuboCop's `begin_type?`, which excludes an explicit
//!   `begin…end` block (parser-gem `kwbegin`). Murphy models both as
//!   [`NodeKind::Begin`], so `is_begin` discriminates by the preceding
//!   `begin` keyword token.
//!
//!   Offense range matches RuboCop's `add_offense(container)` — the whole
//!   container statement, including the multiline span when the offending
//!   statement is a `do…end` block (verified against rubocop 1.87.0).
//!
//!   Autocorrect (the `select, reject = recv.partition { … }` rewrite, which
//!   only fires in RuboCop when both statements are local-variable
//!   assignments) is a v1 gap — tracked by murphy-e7bz.59.1. The cop is
//!   `Safe: false` upstream, so offense-only is the conservative shipping
//!   state. `Enabled: pending` upstream → `default_enabled = false`.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, SourceTokenKind, cop};

#[derive(Default)]
pub struct PartitionInsteadOfDoubleSelect;

const SELECT_METHODS: [&str; 3] = ["select", "filter", "find_all"];

fn is_select_method(name: &str) -> bool {
    SELECT_METHODS.contains(&name)
}

fn is_candidate_method(name: &str) -> bool {
    is_select_method(name) || name == "reject"
}

#[cop(
    name = "Style/PartitionInsteadOfDoubleSelect",
    description = "Suggest `partition` over consecutive `select`/`reject` calls on the same receiver.",
    default_severity = "warning",
    default_enabled = false,
    options = murphy_plugin_api::NoOptions
)]
impl PartitionInsteadOfDoubleSelect {
    /// Brace / `do…end` block: `arr.select { |x| … }`.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_candidate(node, cx);
    }

    /// Numbered-parameter block: `arr.select { _1 > 0 }`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_candidate(node, cx);
    }

    /// `it`-parameter block: `arr.select { it > 0 }`.
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_candidate(node, cx);
    }

    /// Symbol-proc / block-pass send: `arr.select(&:positive?)`.
    #[on_node(kind = "send", methods = ["select", "filter", "find_all", "reject"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if has_block_pass_last_arg(node, cx) {
            find_and_register_offense(node, cx);
        }
    }

    /// Safe-navigation block-pass send: `arr&.select(&:positive?)`.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        // `methods = [...]` is not supported on csend — filter manually.
        let Some(name) = cx.method_name(node) else {
            return;
        };
        if is_candidate_method(name) && has_block_pass_last_arg(node, cx) {
            find_and_register_offense(node, cx);
        }
    }
}

/// Block-handler entry: bail unless the block wraps a candidate
/// select-family/`reject` call, then funnel into the shared logic with the
/// *block* node as the candidate.
fn check_block_candidate(block: NodeId, cx: &Cx<'_>) {
    if cx.method_name(block).is_some_and(is_candidate_method) {
        find_and_register_offense(block, cx);
    }
}

/// True when the node is a call whose final argument is a block-pass
/// (`&:sym` / `&method`).
fn has_block_pass_last_arg(call: NodeId, cx: &Cx<'_>) -> bool {
    cx.call_arguments(call)
        .last()
        .is_some_and(|&arg| matches!(cx.kind(arg), NodeKind::BlockPass(_)))
}

/// Mirror of RuboCop's `find_and_register_offense`: resolve the statement
/// container, find its matching left sibling, and emit if the pair matches.
fn find_and_register_offense(node: NodeId, cx: &Cx<'_>) {
    let Some(container) = node_container(node, cx) else {
        return;
    };
    let Some(sibling_container) = left_sibling(container, cx) else {
        return;
    };
    let Some(sibling) = find_matching_candidate(node, sibling_container, cx) else {
        return;
    };

    let first = cx
        .method_name(sibling)
        .expect("matching candidate has a method name");
    let second = cx
        .method_name(node)
        .expect("candidate has a method name");
    let message = format!(
        "Use `partition` instead of consecutive `{first}` and `{second}` calls."
    );
    cx.emit_offense(cx.range(container), &message, None);
}

/// Mirror of RuboCop's `node_container`:
/// - parent is a `begin` → the node itself is the statement container;
/// - parent is an assignment whose own parent is a `begin` → the assignment
///   is the container.
///
/// Anything else → not a top-level statement, no container.
fn node_container(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let parent = cx.parent(node).get()?;
    if is_begin(parent, cx) {
        return Some(node);
    }
    if cx.is_assignment(parent) {
        let grandparent = cx.parent(parent).get()?;
        if is_begin(grandparent, cx) {
            return Some(parent);
        }
    }
    None
}

/// The immediate left sibling of `container` within its `begin` parent.
fn left_sibling(container: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let parent = cx.parent(container).get()?;
    let siblings = cx.children(parent);
    let pos = siblings.iter().position(|&s| s == container)?;
    if pos == 0 {
        return None;
    }
    Some(siblings[pos - 1])
}

/// Mirror of RuboCop's `find_matching_candidate` + `extract_candidate`:
/// pull a candidate call out of the sibling container, require the same
/// receiver, and require a matching predicate pair.
fn find_matching_candidate(node: NodeId, sibling_container: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let sibling = extract_candidate(sibling_container, cx)?;
    if !same_receiver(node, sibling, cx) {
        return None;
    }
    if matching_pair(node, sibling, cx) {
        Some(sibling)
    } else {
        None
    }
}

/// Mirror of `extract_candidate`: unwrap an assignment to its RHS, then
/// accept either a candidate block or a candidate block-pass send.
fn extract_candidate(container: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let inner = if cx.is_assignment(container) {
        *cx.children(container).last()?
    } else {
        container
    };
    if is_any_block(inner, cx) {
        if cx.method_name(inner).is_some_and(is_candidate_method) {
            return Some(inner);
        }
        return None;
    }
    if is_call(inner, cx)
        && cx.method_name(inner).is_some_and(is_candidate_method)
        && has_block_pass_last_arg(inner, cx)
    {
        return Some(inner);
    }
    None
}

/// Both candidates target the same receiver expression (compared by source
/// text, since equal expressions are distinct AST node instances).
fn same_receiver(node: NodeId, sibling: NodeId, cx: &Cx<'_>) -> bool {
    let node_recv = cx.call_receiver(call_of(node, cx)).get();
    let sibling_recv = cx.call_receiver(call_of(sibling, cx)).get();
    match (node_recv, sibling_recv) {
        (Some(a), Some(b)) => cx.raw_source(cx.range(a)) == cx.raw_source(cx.range(b)),
        (None, None) => true,
        _ => false,
    }
}

/// Mirror of `matching_pair?`:
/// - complementary methods (select-family ↔ reject) with an equivalent
///   predicate, OR
/// - the same method with one predicate the negation of the other.
fn matching_pair(node: NodeId, sibling: NodeId, cx: &Cx<'_>) -> bool {
    (complementary_pair(node, sibling, cx) && equivalent_predicate(node, sibling, cx))
        || (same_method(node, sibling, cx) && negated_predicate(node, sibling, cx))
}

fn complementary_pair(node: NodeId, sibling: NodeId, cx: &Cx<'_>) -> bool {
    let Some(m1) = cx.method_name(node) else {
        return false;
    };
    let Some(m2) = cx.method_name(sibling) else {
        return false;
    };
    (is_select_method(m1) && m2 == "reject") || (m1 == "reject" && is_select_method(m2))
}

fn same_method(node: NodeId, sibling: NodeId, cx: &Cx<'_>) -> bool {
    match (cx.method_name(node), cx.method_name(sibling)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Mirror of `equivalent_predicate?`: identical block (args + body) for two
/// blocks, matching symbol-proc for a block + block-pass cross pair, or
/// identical block-pass argument for two sends. Compared by source text.
fn equivalent_predicate(node: NodeId, sibling: NodeId, cx: &Cx<'_>) -> bool {
    let node_is_block = is_any_block(node, cx);
    let sibling_is_block = is_any_block(sibling, cx);
    match (node_is_block, sibling_is_block) {
        (true, true) => same_block_contents(node, sibling, cx),
        (true, false) => block_matches_block_pass(node, sibling, cx),
        (false, true) => block_matches_block_pass(sibling, node, cx),
        (false, false) => {
            block_pass_arg_src(node, cx) == block_pass_arg_src(sibling, cx)
                && block_pass_arg_src(node, cx).is_some()
        }
    }
}

/// Two blocks are equivalent when their kinds match and their args + body
/// source text match (RuboCop compares `arguments` and `body` ASTs).
fn same_block_contents(block1: NodeId, block2: NodeId, cx: &Cx<'_>) -> bool {
    if block_discriminant(block1, cx) != block_discriminant(block2, cx) {
        return false;
    }
    if !same_block_args(block1, block2, cx) {
        return false;
    }
    body_src(block1, cx) == body_src(block2, cx)
}

/// Cross case: a block whose body is `lvar.method` (a symbol-proc shape)
/// against a block-pass send whose argument is `&:method` with the same name.
fn block_matches_block_pass(block: NodeId, send: NodeId, cx: &Cx<'_>) -> bool {
    let Some(method_name) = symbol_proc_method(block, cx) else {
        return false;
    };
    // The block-pass argument must be `&:method_name`.
    let Some(&arg) = cx.call_arguments(send).last() else {
        return false;
    };
    let NodeKind::BlockPass(inner) = cx.kind(arg) else {
        return false;
    };
    let Some(sym) = inner.get() else {
        return false;
    };
    matches!(cx.kind(sym), NodeKind::Sym(_)) && sym_name(sym, cx) == Some(method_name)
}

/// `symbol_proc_method?`: a block `{ |name| name.method }` (single arg, body
/// is a receiverless-on-that-arg send) → `Some("method")`.
fn symbol_proc_method<'a>(block: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    // Only plain `Block` form has explicit args of the `{ |name| name.m }`
    // shape RuboCop's pattern requires.
    let NodeKind::Block { args, .. } = *cx.kind(block) else {
        return None;
    };
    let arg_ids = cx.children(args);
    let [arg] = arg_ids.as_slice() else {
        return None;
    };
    if !matches!(cx.kind(*arg), NodeKind::Arg(_)) {
        return None;
    }
    let arg_name = arg_name(*arg, cx)?;
    let body = cx.block_body(block).get()?;
    // body must be `lvar(arg_name).method` with no arguments.
    let method = cx.method_name(body)?;
    let recv = cx.call_receiver(body).get()?;
    if !matches!(cx.kind(recv), NodeKind::Lvar(_)) {
        return None;
    }
    if lvar_name(recv, cx) != Some(arg_name) {
        return None;
    }
    if !cx.call_arguments(body).is_empty() {
        return None;
    }
    Some(method)
}

/// Mirror of `negated_predicate?`: same block kind + args, and one body is
/// the boolean negation (`!`) of the other.
fn negated_predicate(node: NodeId, sibling: NodeId, cx: &Cx<'_>) -> bool {
    if !is_any_block(node, cx) || !is_any_block(sibling, cx) {
        return false;
    }
    if block_discriminant(node, cx) != block_discriminant(sibling, cx) {
        return false;
    }
    if !same_block_args(node, sibling, cx) {
        return false;
    }
    negated_body(node, sibling, cx) || negated_body(sibling, node, cx)
}

/// True when `block1`'s body is `!(block2's body)`.
fn negated_body(block1: NodeId, block2: NodeId, cx: &Cx<'_>) -> bool {
    let Some(body1) = cx.block_body(block1).get() else {
        return false;
    };
    let Some(body2) = cx.block_body(block2).get() else {
        return false;
    };
    if cx.method_name(body1) != Some("!") {
        return false;
    }
    let Some(recv1) = cx.call_receiver(body1).get() else {
        return false;
    };
    cx.raw_source(cx.range(recv1)) == cx.raw_source(cx.range(body2))
}

// --- small structural helpers ---------------------------------------------

/// RuboCop's `begin_type?` — true only for an *implicit* statement sequence
/// (top-level program body, method/class/module body), not for an explicit
/// `begin…end` block (parser-gem `kwbegin`) nor a parenthesized expression.
///
/// Murphy models all three as [`NodeKind::Begin`], so we discriminate by the
/// node's first token: a parenthesized expr starts with `(`, an explicit
/// `begin…end` block starts with the `begin` keyword, and an implicit
/// statement sequence starts with its first statement's own token.
fn is_begin(id: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(id), NodeKind::Begin(_)) {
        return false;
    }
    let start = cx.range(id).start;
    // An explicit `begin…end` block (parser-gem `kwbegin`) has the `begin`
    // keyword as the last significant token before its statement range —
    // murphy's node range covers only the statements, not the keyword.
    // Newlines between `begin` and the first statement are skipped.
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.end <= start);
    let prev_significant = toks[..idx].iter().rev().find(|t| {
        !matches!(
            t.kind,
            SourceTokenKind::Newline | SourceTokenKind::IgnoredNewline | SourceTokenKind::Comment
        )
    });
    !matches!(
        prev_significant,
        Some(tok)
            if tok.kind == SourceTokenKind::Other && cx.raw_source(tok.range) == "begin"
    )
}

fn is_any_block(id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(id),
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. }
    )
}

fn is_call(id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(id), NodeKind::Send { .. } | NodeKind::Csend { .. })
}

/// The underlying call of a candidate: the block's call, or the send itself.
fn call_of(id: NodeId, cx: &Cx<'_>) -> NodeId {
    cx.block_call(id).get().unwrap_or(id)
}

/// A discriminant distinguishing block kinds (block / numblock / itblock).
fn block_discriminant(id: NodeId, cx: &Cx<'_>) -> u8 {
    match cx.kind(id) {
        NodeKind::Block { .. } => 0,
        NodeKind::Numblock { .. } => 1,
        NodeKind::Itblock { .. } => 2,
        _ => 255,
    }
}

/// Compare two blocks' argument lists by the source text of each arg node.
/// numblock/itblock carry no explicit args node, so two same-kind blocks
/// trivially share an (empty) arg list — kind equality is enforced by the
/// caller via [`block_discriminant`].
fn same_block_args(block1: NodeId, block2: NodeId, cx: &Cx<'_>) -> bool {
    let args1 = block_arg_nodes(block1, cx);
    let args2 = block_arg_nodes(block2, cx);
    if args1.len() != args2.len() {
        return false;
    }
    args1
        .iter()
        .zip(args2.iter())
        .all(|(&a, &b)| cx.raw_source(cx.range(a)) == cx.raw_source(cx.range(b)))
}

/// The individual arg nodes of a `Block`'s `args` list (empty for
/// numblock/itblock, which have no explicit args node).
fn block_arg_nodes(block: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    match *cx.kind(block) {
        NodeKind::Block { args, .. } => cx.children(args),
        _ => Vec::new(),
    }
}

/// Source text of a block's body (empty string for an empty body).
fn body_src<'a>(block: NodeId, cx: &Cx<'a>) -> &'a str {
    cx.block_body(block)
        .get()
        .map_or("", |b| cx.raw_source(cx.range(b)))
}

/// Source text of a block-pass send's final block-pass argument (`&:sym`).
fn block_pass_arg_src<'a>(send: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    let &arg = cx.call_arguments(send).last()?;
    if !matches!(cx.kind(arg), NodeKind::BlockPass(_)) {
        return None;
    }
    Some(cx.raw_source(cx.range(arg)))
}

fn arg_name<'a>(arg: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match cx.kind(arg) {
        NodeKind::Arg(sym) => Some(cx.symbol_str(*sym)),
        _ => None,
    }
}

fn lvar_name<'a>(lvar: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match cx.kind(lvar) {
        NodeKind::Lvar(sym) => Some(cx.symbol_str(*sym)),
        _ => None,
    }
}

fn sym_name<'a>(sym: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match cx.kind(sym) {
        NodeKind::Sym(s) => Some(cx.symbol_str(*s)),
        _ => None,
    }
}

murphy_plugin_api::submit_cop!(PartitionInsteadOfDoubleSelect);

#[cfg(test)]
mod tests {
    use super::PartitionInsteadOfDoubleSelect;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_select_then_reject() {
        test::<PartitionInsteadOfDoubleSelect>().expect_offense(indoc! {"
            positives = arr.select { |x| x > 0 }
            negatives = arr.reject { |x| x > 0 }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `partition` instead of consecutive `select` and `reject` calls.
        "});
    }

    #[test]
    fn flags_reject_then_select() {
        test::<PartitionInsteadOfDoubleSelect>().expect_offense(indoc! {"
            negatives = arr.reject { |x| x > 0 }
            positives = arr.select { |x| x > 0 }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `partition` instead of consecutive `reject` and `select` calls.
        "});
    }

    #[test]
    fn flags_do_end_block_paired_with_brace() {
        // `do…end` first, single-line brace second. Exercises do/end
        // participation in matching (multiline body extraction, do/end↔brace
        // block-kind equality) while keeping the offense range — which lands
        // on the *second* statement — expressible via single-line carets.
        //
        // The mirror case (do/end as the offending second statement) produces
        // a multiline offense range verified against rubocop 1.87.0
        // (start_line 4 .. last_line 6, length 41), but a multiline range is
        // not expressible through `expect_offense` carets, so it is not
        // asserted here.
        test::<PartitionInsteadOfDoubleSelect>().expect_offense(indoc! {"
            positives = arr.select do |x|
              x > 0
            end
            negatives = arr.reject { |x| x > 0 }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `partition` instead of consecutive `select` and `reject` calls.
        "});
    }

    #[test]
    fn flags_symbol_proc() {
        test::<PartitionInsteadOfDoubleSelect>().expect_offense(indoc! {"
            positives = arr.select(&:positive?)
            negatives = arr.reject(&:positive?)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `partition` instead of consecutive `select` and `reject` calls.
        "});
    }

    #[test]
    fn flags_cross_block_and_symbol_proc() {
        test::<PartitionInsteadOfDoubleSelect>().expect_offense(indoc! {"
            positives = arr.select { |x| x.positive? }
            negatives = arr.reject(&:positive?)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `partition` instead of consecutive `select` and `reject` calls.
        "});
    }

    #[test]
    fn flags_negated_same_method() {
        test::<PartitionInsteadOfDoubleSelect>().expect_offense(indoc! {"
            a = arr.select { |x| x.positive? }
            b = arr.select { |x| !x.positive? }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `partition` instead of consecutive `select` and `select` calls.
        "});
    }

    #[test]
    fn flags_bare_statements() {
        test::<PartitionInsteadOfDoubleSelect>().expect_offense(indoc! {"
            arr.select { |x| x > 0 }
            arr.reject { |x| x > 0 }
            ^^^^^^^^^^^^^^^^^^^^^^^^ Use `partition` instead of consecutive `select` and `reject` calls.
        "});
    }

    #[test]
    fn flags_safe_navigation_receiver() {
        test::<PartitionInsteadOfDoubleSelect>().expect_offense(indoc! {"
            positives = arr&.select { |x| x > 0 }
            negatives = arr&.reject { |x| x > 0 }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `partition` instead of consecutive `select` and `reject` calls.
        "});
    }

    #[test]
    fn flags_inside_method_body() {
        // RuboCop's `node_container` matches an implicit statement sequence —
        // a method body is one — verified against rubocop 1.87.0.
        test::<PartitionInsteadOfDoubleSelect>().expect_offense(indoc! {"
            def foo
              a = arr.select { |x| x > 0 }
              b = arr.reject { |x| x > 0 }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `partition` instead of consecutive `select` and `reject` calls.
            end
        "});
    }

    #[test]
    fn accepts_explicit_begin_end_block() {
        // An explicit `begin…end` block is parser-gem `kwbegin`, which
        // RuboCop's `begin_type?` excludes — verified silent on rubocop
        // 1.87.0. Murphy models it as a `Begin` node too, so this guards
        // against the kwbegin false positive.
        test::<PartitionInsteadOfDoubleSelect>().expect_no_offenses(indoc! {"
            begin
              a = arr.select { |x| x > 0 }
              b = arr.reject { |x| x > 0 }
            end
        "});
    }

    #[test]
    fn accepts_different_receivers() {
        test::<PartitionInsteadOfDoubleSelect>().expect_no_offenses(indoc! {"
            positives = arr1.select { |x| x > 0 }
            negatives = arr2.reject { |x| x > 0 }
        "});
    }

    #[test]
    fn accepts_different_block_bodies() {
        test::<PartitionInsteadOfDoubleSelect>().expect_no_offenses(indoc! {"
            positives = arr.select { |x| x > 0 }
            negatives = arr.reject { |x| x < 0 }
        "});
    }

    #[test]
    fn accepts_non_consecutive_calls() {
        test::<PartitionInsteadOfDoubleSelect>().expect_no_offenses(indoc! {"
            positives = arr.select { |x| x > 0 }
            do_something
            negatives = arr.reject { |x| x > 0 }
        "});
    }

    #[test]
    fn accepts_single_select() {
        test::<PartitionInsteadOfDoubleSelect>()
            .expect_no_offenses("arr.select { |x| x > 0 }\n");
    }

    #[test]
    fn accepts_same_method_non_negated() {
        // Two plain `select`s with the same predicate are not a partition.
        test::<PartitionInsteadOfDoubleSelect>().expect_no_offenses(indoc! {"
            a = arr.select { |x| x > 0 }
            b = arr.select { |x| x > 0 }
        "});
    }
}
