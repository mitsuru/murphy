//! `Style/ReduceToHash` — prefer `to_h { ... }` over `each_with_object`,
//! `inject`, or `reduce` calls that build a hash from an enumerable.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ReduceToHash
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Marked unsafe in RuboCop (Safe: false) because the receiver may not be an
//!   Enumerable, and `each_with_object` returns the accumulator while `to_h`
//!   returns a fresh hash. Murphy has no Safe/SafeAutoCorrect cop-level
//!   attribute yet; the unsafe nature is documented here only.
//!
//!   Handled patterns (mirror RuboCop's node matchers exactly):
//!     each_with_object form:
//!       block:    `x.each_with_object({}) { |elem, hash| hash[k] = v }`
//!       numblock: `x.each_with_object({}) { _2[k] = v }`  (elem=_1, acc=_2)
//!     inject/reduce form:
//!       block:    `x.inject({}) { |hash, elem| hash[k] = v; hash }`
//!       numblock: `x.reduce({}) { _1[k] = v; _1 }`        (acc=_1, elem=_2)
//!
//!   Numblocks require exactly two implicit params (`max_n == 2`), mirroring
//!   RuboCop's pattern literal `2`. A numblock using only `_1`, or one using
//!   `_3`, does not match.
//!
//!   Argument guard: the single send argument must be an EMPTY hash literal
//!   (`{}`). `{a: 1}`, `Hash.new`, `[]`, `inject(0)`, and no-arg forms do not
//!   match.
//!
//!   Body shape is matched exactly:
//!     - each_with_object: a single bare `[]=` send (no trailing statements).
//!     - inject/reduce: a `Begin` of exactly two statements — the `[]=` send
//!       followed by the accumulator returned (`lvar acc`).
//!
//!   Guards (mirror RuboCop):
//!     - accumulator-referenced: key/value must NOT reference the accumulator
//!       variable (e.g. `hash[hash.size] = elem` is skipped).
//!     - nested-match: if key/value contains a nested matching builder, the
//!       OUTER call is skipped (the inner builder is flagged on its own visit).
//!
//!   Offense range: the method selector only (`loc.name`), matching RuboCop's
//!   `send_node.loc.selector`.
//!
//!   Both `send` and `csend` receivers are handled.
//!
//!   Autocorrect (whole `selector..block-end` rewrite):
//!     - brace block:  `to_h { |elem| [k, v] }`
//!     - do-end block: `to_h do |elem|\n<indent+2>[k, v]\n<indent>end`
//!     - numblock:     params dropped; body becomes `[k, v]`. For inject/reduce
//!       numblocks `_2` (element) is rewritten to `_1` to match `to_h`'s single
//!       implicit param. This uses a plain `_2`→`_1` substring replace, exactly
//!       as RuboCop's `gsub('_2', '_1')` does (parity-faithful; both would
//!       rewrite an identifier like `foo_2`, which is not a realistic concern
//!       inside an autocorrected numblock body).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! array.each_with_object({}) { |elem, hash| hash[elem.id] = elem.name }
//! array.inject({}) { |hash, elem| hash[elem.id] = elem.name; hash }
//! array.reduce({}) { |hash, elem| hash[elem.id] = elem.name; hash }
//!
//! # good
//! array.to_h { |elem| [elem.id, elem.name] }
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, Symbol, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct ReduceToHash;

#[cop(
    name = "Style/ReduceToHash",
    description = "Use `to_h { ... }` instead of `each_with_object`, `inject`, or `reduce` to build a hash.",
    default_severity = "warning",
    default_enabled = false,
    minimum_target_ruby_version = "2.6",
    options = NoOptions,
)]
impl ReduceToHash {
    #[on_node(kind = "send", methods = ["each_with_object", "inject", "reduce"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if matches!(
            cx.method_name(node),
            Some("each_with_object" | "inject" | "reduce")
        ) {
            check(node, cx);
        }
    }
}

/// The matched accumulation shape, sufficient to register the offense and
/// build the `to_h` replacement.
struct Match {
    /// Key expression node (first `[]=` argument).
    key: NodeId,
    /// Value expression node (second `[]=` argument).
    value: NodeId,
    /// Accumulator variable symbol (must not be referenced in key/value).
    accumulator: Symbol,
    /// `true` if the block is a numblock (implicit `_1`/`_2` params).
    numblock: bool,
    /// For inject/reduce numblocks, the element is `_2` and must be rewritten
    /// to `_1` in the replacement body. For each_with_object numblocks the
    /// element is already `_1`, so no rewrite is needed.
    rewrite_elem_two_to_one: bool,
}

fn check(send: NodeId, cx: &Cx<'_>) {
    let Some(method) = cx.method_name(send) else {
        return;
    };
    let is_ewo = method == "each_with_object";

    // The send must be the `call`/`send` of an attached block (brace, do-end,
    // or numblock). RuboCop enters via `node.block_node`.
    let Some(parent) = cx.parent(send).get() else {
        return;
    };
    let m = match *cx.kind(parent) {
        NodeKind::Block { call, args, body } if call == send => {
            let body = body.get();
            match_block(is_ewo, args, body, cx)
        }
        NodeKind::Numblock {
            send: inner,
            max_n,
            body,
        } if inner == send && max_n == 2 => match_numblock(is_ewo, body.get(), cx),
        _ => return,
    };
    let Some(m) = m else {
        return;
    };

    // The single send argument must be an empty hash literal `{}`.
    let args = cx.call_arguments(send);
    let [arg] = args else {
        return;
    };
    if !is_empty_hash(*arg, cx) {
        return;
    }

    // Guard: key/value must not reference the accumulator.
    if references_variable(m.key, m.accumulator, cx)
        || references_variable(m.value, m.accumulator, cx)
    {
        return;
    }

    // Guard: skip the outer call when key/value contains a nested matching
    // builder (the nested call is flagged on its own visit).
    if nested_match(m.key, cx) || nested_match(m.value, cx) {
        return;
    }

    let message = format!("Use `to_h {{ ... }}` instead of `{method}`.");
    cx.emit_offense(cx.node(send).loc.name, &message, None);

    autocorrect(send, parent, &m, cx);
}

/// Match a named-arg block body for the given method form.
///
/// Returns the matched shape when the body has the exact `[]=` (+ trailing
/// accumulator for inject/reduce) structure with the expected param order.
fn match_block(is_ewo: bool, args: NodeId, body: Option<NodeId>, cx: &Cx<'_>) -> Option<Match> {
    let NodeKind::Args(arg_list) = *cx.kind(args) else {
        return None;
    };
    let block_args = cx.list(arg_list);
    let [first, second] = block_args else {
        return None;
    };
    let NodeKind::Arg(first_sym) = *cx.kind(*first) else {
        return None;
    };
    let NodeKind::Arg(second_sym) = *cx.kind(*second) else {
        return None;
    };

    // each_with_object: |elem, hash| → accumulator is the 2nd param.
    // inject/reduce:    |hash, elem| → accumulator is the 1st param.
    let accumulator = if is_ewo { second_sym } else { first_sym };

    let body = body?;
    let (key, value) = match_body(is_ewo, body, accumulator, cx)?;
    Some(Match {
        key,
        value,
        accumulator,
        numblock: false,
        rewrite_elem_two_to_one: false,
    })
}

/// Match a numblock body for the given method form.
fn match_numblock(is_ewo: bool, body: Option<NodeId>, cx: &Cx<'_>) -> Option<Match> {
    // each_with_object numblock: element=_1, accumulator=_2.
    // inject/reduce numblock:    accumulator=_1, element=_2.
    let accumulator = lvar_symbol(if is_ewo { "_2" } else { "_1" }, cx)?;

    let body = body?;
    let (key, value) = match_body(is_ewo, body, accumulator, cx)?;
    Some(Match {
        key,
        value,
        accumulator,
        numblock: true,
        rewrite_elem_two_to_one: !is_ewo,
    })
}

/// Match the body structure and extract `(key, value)` from the `[]=` send.
///
/// - each_with_object: body is exactly `acc[]= key value`.
/// - inject/reduce: body is exactly `Begin(acc[]= key value, acc)`.
fn match_body(
    is_ewo: bool,
    body: NodeId,
    accumulator: Symbol,
    cx: &Cx<'_>,
) -> Option<(NodeId, NodeId)> {
    if is_ewo {
        match_index_set(body, accumulator, cx)
    } else {
        let NodeKind::Begin(list) = *cx.kind(body) else {
            return None;
        };
        let [set, ret] = cx.list(list) else {
            return None;
        };
        // Trailing statement must return the accumulator unchanged.
        if !is_lvar(*ret, accumulator, cx) {
            return None;
        }
        match_index_set(*set, accumulator, cx)
    }
}

/// Match `accumulator[key] = value` (a `[]=` send on the accumulator lvar).
fn match_index_set(node: NodeId, accumulator: Symbol, cx: &Cx<'_>) -> Option<(NodeId, NodeId)> {
    if cx.method_name(node) != Some("[]=") {
        return None;
    }
    let receiver = cx.call_receiver(node).get()?;
    if !is_lvar(receiver, accumulator, cx) {
        return None;
    }
    // `[]=` carries exactly two arguments: the key and the value.
    let [key, value] = cx.call_arguments(node) else {
        return None;
    };
    Some((*key, *value))
}

/// Emit the `selector..block-end` → `to_h { ... }` rewrite.
fn autocorrect(send: NodeId, block: NodeId, m: &Match, cx: &Cx<'_>) {
    let selector = cx.node(send).loc.name;
    let block_end = cx.range(block).end;

    let key_src = adjusted_source(m.key, m, cx);
    let value_src = adjusted_source(m.value, m, cx);
    let body = format!("[{key_src}, {value_src}]");

    let braces = uses_braces(block, cx);
    let replacement = if m.numblock {
        if braces {
            format!("to_h {{ {body} }}")
        } else {
            do_end_replacement(block, &body, None, cx)
        }
    } else {
        let arg = element_arg_source(send, block, cx);
        if braces {
            format!("to_h {{ |{arg}| {body} }}")
        } else {
            do_end_replacement(block, &body, Some(&arg), cx)
        }
    };

    cx.emit_edit(
        Range {
            start: selector.start,
            end: block_end,
        },
        &replacement,
    );
}

/// Build the `do ... end` replacement body with RuboCop's indentation rules:
/// the body line is indented to `column + 2` and the `end` to `column`.
fn do_end_replacement(block: NodeId, body: &str, arg: Option<&str>, cx: &Cx<'_>) -> String {
    let column = block_column(block, cx);
    let indent = " ".repeat(column);
    let args = arg.map_or(String::new(), |a| format!(" |{a}|"));
    format!("to_h do{args}\n{indent}  {body}\n{indent}end")
}

/// Source text of the element parameter, used to name the `to_h` block param.
///
/// each_with_object named block: element is the FIRST block param.
/// inject/reduce named block:    element is the SECOND block param.
fn element_arg_source(send: NodeId, block: NodeId, cx: &Cx<'_>) -> String {
    let NodeKind::Block { args, .. } = *cx.kind(block) else {
        return "elem".to_owned();
    };
    let NodeKind::Args(arg_list) = *cx.kind(args) else {
        return "elem".to_owned();
    };
    let block_args = cx.list(arg_list);
    let is_ewo = cx.method_name(send) == Some("each_with_object");
    let idx = usize::from(!is_ewo);
    block_args
        .get(idx)
        .map(|&a| cx.raw_source(cx.range(a)).to_owned())
        .unwrap_or_else(|| "elem".to_owned())
}

/// Source for a key/value expression, rewriting `_2` → `_1` for inject/reduce
/// numblocks so the implicit `to_h` param lines up.
fn adjusted_source(expr: NodeId, m: &Match, cx: &Cx<'_>) -> String {
    let src = cx.raw_source(cx.range(expr));
    if m.rewrite_elem_two_to_one {
        src.replace("_2", "_1")
    } else {
        src.to_owned()
    }
}

/// `true` if the block delimiter is `{ }` (vs `do ... end`).
///
/// A brace block's source ends in `}`; a do-end block ends in `end`.
fn uses_braces(block: NodeId, cx: &Cx<'_>) -> bool {
    let end = cx.range(block).end as usize;
    cx.source().as_bytes()[..end].last() == Some(&b'}')
}

/// Column (0-based) of the block's start line, for do-end indentation.
fn block_column(block: NodeId, cx: &Cx<'_>) -> usize {
    let start = cx.range(block).start as usize;
    let src = cx.source();
    let line_start = src[..start].rfind('\n').map_or(0, |p| p + 1);
    src[line_start..start].chars().count()
}

/// Returns `true` if `node` is exactly the empty hash literal `{}`.
fn is_empty_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Hash(list) if cx.list(list).is_empty())
}

/// Returns `true` if `node` is an `Lvar` referencing `sym`.
fn is_lvar(node: NodeId, sym: Symbol, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Lvar(s) if s == sym)
}

/// Resolve a numblock implicit-param name (e.g. `_1`) to its `Symbol` by
/// searching the AST. Returns `None` if the name never appears.
fn lvar_symbol(name: &str, cx: &Cx<'_>) -> Option<Symbol> {
    // Numblock implicit params are guaranteed to be referenced in the body
    // (the `[]=` receiver), so the symbol must be interned somewhere.
    find_lvar_symbol(cx.root(), name, cx)
}

fn find_lvar_symbol(node: NodeId, name: &str, cx: &Cx<'_>) -> Option<Symbol> {
    if let NodeKind::Lvar(s) = *cx.kind(node)
        && cx.symbol_str(s) == name
    {
        return Some(s);
    }
    for child in cx.children(node) {
        if let Some(s) = find_lvar_symbol(child, name, cx) {
            return Some(s);
        }
    }
    None
}

/// Returns `true` if `sym` is referenced (as an `Lvar`) anywhere in the
/// subtree rooted at `node`.
fn references_variable(node: NodeId, sym: Symbol, cx: &Cx<'_>) -> bool {
    if is_lvar(node, sym, cx) {
        return true;
    }
    cx.children(node)
        .iter()
        .any(|&child| references_variable(child, sym, cx))
}

/// Returns `true` if the subtree rooted at `node` contains a nested matching
/// `each_with_object`/`inject`/`reduce` hash-builder call.
fn nested_match(node: NodeId, cx: &Cx<'_>) -> bool {
    if is_builder_call(node, cx) {
        return true;
    }
    cx.children(node)
        .iter()
        .any(|&child| nested_match(child, cx))
}

/// Returns `true` if `node` is itself a matching hash-builder call (the same
/// shape this cop flags), used by the nested-match guard.
fn is_builder_call(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(method) = cx.method_name(node) else {
        return false;
    };
    if !matches!(method, "each_with_object" | "inject" | "reduce") {
        return false;
    }
    let is_ewo = method == "each_with_object";
    // Require the empty-hash argument as well, mirroring the full matcher.
    let args = cx.call_arguments(node);
    let [arg] = args else {
        return false;
    };
    if !is_empty_hash(*arg, cx) {
        return false;
    }
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    match *cx.kind(parent) {
        NodeKind::Block { call, args, body } if call == node => {
            match_block(is_ewo, args, body.get(), cx).is_some()
        }
        NodeKind::Numblock {
            send: inner,
            max_n,
            body,
        } if inner == node && max_n == 2 => match_numblock(is_ewo, body.get(), cx).is_some(),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::ReduceToHash;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Positive cases (offense) ------------------------------------

    #[test]
    fn flags_each_with_object() {
        test::<ReduceToHash>().expect_offense(indoc! {"
            array.each_with_object({}) { |elem, hash| hash[elem.id] = elem.name }
                  ^^^^^^^^^^^^^^^^ Use `to_h { ... }` instead of `each_with_object`.
        "});
    }

    #[test]
    fn flags_inject() {
        test::<ReduceToHash>().expect_offense(indoc! {"
            array.inject({}) { |hash, elem| hash[elem.id] = elem.name; hash }
                  ^^^^^^ Use `to_h { ... }` instead of `inject`.
        "});
    }

    #[test]
    fn flags_reduce() {
        test::<ReduceToHash>().expect_offense(indoc! {"
            array.reduce({}) { |hash, elem| hash[elem.id] = elem.name; hash }
                  ^^^^^^ Use `to_h { ... }` instead of `reduce`.
        "});
    }

    #[test]
    fn flags_each_with_object_simple_value() {
        test::<ReduceToHash>().expect_offense(indoc! {"
            array.each_with_object({}) { |elem, hash| hash[elem] = elem.to_s }
                  ^^^^^^^^^^^^^^^^ Use `to_h { ... }` instead of `each_with_object`.
        "});
    }

    #[test]
    fn flags_each_with_object_numblock() {
        test::<ReduceToHash>().expect_offense(indoc! {"
            array.each_with_object({}) { _2[_1.id] = _1.name }
                  ^^^^^^^^^^^^^^^^ Use `to_h { ... }` instead of `each_with_object`.
        "});
    }

    #[test]
    fn flags_inject_numblock() {
        test::<ReduceToHash>().expect_offense(indoc! {"
            array.inject({}) { _1[_2.id] = _2.name; _1 }
                  ^^^^^^ Use `to_h { ... }` instead of `inject`.
        "});
    }

    // ----- Negative cases (no offense) --------------------------------

    #[test]
    fn accepts_non_empty_hash_arg() {
        test::<ReduceToHash>()
            .expect_no_offenses("array.each_with_object({a: 1}) { |elem, hash| hash[elem] = elem }\n");
    }

    #[test]
    fn accepts_hash_new_arg() {
        test::<ReduceToHash>()
            .expect_no_offenses("array.each_with_object(Hash.new) { |elem, hash| hash[elem] = elem }\n");
    }

    #[test]
    fn accepts_array_arg() {
        test::<ReduceToHash>()
            .expect_no_offenses("array.each_with_object([]) { |elem, acc| acc << elem }\n");
    }

    #[test]
    fn accepts_inject_without_arg() {
        test::<ReduceToHash>().expect_no_offenses("array.inject { |hash, elem| hash[elem] = elem; hash }\n");
    }

    #[test]
    fn accepts_inject_without_trailing_accumulator() {
        // inject must return the accumulator; without it the shape differs.
        test::<ReduceToHash>()
            .expect_no_offenses("array.inject({}) { |hash, elem| hash[elem] = elem }\n");
    }

    #[test]
    fn accepts_each_with_object_extra_statement() {
        // each_with_object body must be exactly the `[]=` send.
        test::<ReduceToHash>()
            .expect_no_offenses("array.each_with_object({}) { |elem, hash| hash[elem] = elem; puts elem }\n");
    }

    #[test]
    fn accepts_accumulator_referenced_in_key() {
        test::<ReduceToHash>()
            .expect_no_offenses("array.each_with_object({}) { |elem, hash| hash[hash.size] = elem }\n");
    }

    #[test]
    fn accepts_accumulator_referenced_in_value() {
        test::<ReduceToHash>()
            .expect_no_offenses("array.each_with_object({}) { |elem, hash| hash[elem] = hash.size }\n");
    }

    #[test]
    fn accepts_opassign_index() {
        // `hash[elem] += 1` is not a plain `[]=` send.
        test::<ReduceToHash>()
            .expect_no_offenses("array.each_with_object({}) { |elem, hash| hash[elem] += 1 }\n");
    }

    #[test]
    fn accepts_each_with_object_numblock_wrong_acc() {
        // In an each_with_object numblock the accumulator is `_2`; using `_1`
        // as the index receiver is the element, not the accumulator.
        test::<ReduceToHash>()
            .expect_no_offenses("array.each_with_object({}) { _1[_2] = _2.to_s }\n");
    }

    #[test]
    fn accepts_reduce_numblock_single_param() {
        // RuboCop's pattern requires exactly two implicit params (`max_n == 2`).
        // This numblock uses only `_1`, so it must not match.
        test::<ReduceToHash>().expect_no_offenses("array.reduce({}) { _1[5] = 6; _1 }\n");
    }

    #[test]
    fn accepts_each_with_object_numblock_three_params() {
        // `_3` pushes max_n to 3, which RuboCop's literal `2` rejects.
        test::<ReduceToHash>()
            .expect_no_offenses("array.each_with_object({}) { _2[_1] = _3 }\n");
    }

    #[test]
    fn flags_inner_not_outer_when_nested() {
        // The outer builder is skipped (nested_match); the inner builder is
        // flagged on its own visit.
        test::<ReduceToHash>().expect_offense(indoc! {"
            array.each_with_object({}) { |elem, hash| hash[elem] = sub.each_with_object({}) { |e, h| h[e] = e } }
                                                                       ^^^^^^^^^^^^^^^^ Use `to_h { ... }` instead of `each_with_object`.
        "});
    }

    // ----- Autocorrect -------------------------------------------------

    #[test]
    fn corrects_each_with_object_brace() {
        test::<ReduceToHash>().expect_correction(
            indoc! {"
                array.each_with_object({}) { |elem, hash| hash[elem.id] = elem.name }
                      ^^^^^^^^^^^^^^^^ Use `to_h { ... }` instead of `each_with_object`.
            "},
            "array.to_h { |elem| [elem.id, elem.name] }\n",
        );
    }

    #[test]
    fn corrects_inject_brace() {
        test::<ReduceToHash>().expect_correction(
            indoc! {"
                array.inject({}) { |hash, elem| hash[elem.id] = elem.name; hash }
                      ^^^^^^ Use `to_h { ... }` instead of `inject`.
            "},
            "array.to_h { |elem| [elem.id, elem.name] }\n",
        );
    }

    #[test]
    fn corrects_reduce_brace_simple_value() {
        test::<ReduceToHash>().expect_correction(
            indoc! {"
                array.reduce({}) { |hash, elem| hash[elem] = elem.to_s; hash }
                      ^^^^^^ Use `to_h { ... }` instead of `reduce`.
            "},
            "array.to_h { |elem| [elem, elem.to_s] }\n",
        );
    }

    #[test]
    fn corrects_each_with_object_do_end() {
        test::<ReduceToHash>().expect_correction(
            indoc! {"
                array.each_with_object({}) do |elem, hash|
                      ^^^^^^^^^^^^^^^^ Use `to_h { ... }` instead of `each_with_object`.
                  hash[elem.id] = elem.name
                end
            "},
            "array.to_h do |elem|\n  [elem.id, elem.name]\nend\n",
        );
    }

    #[test]
    fn corrects_inject_do_end() {
        test::<ReduceToHash>().expect_correction(
            indoc! {"
                array.inject({}) do |hash, elem|
                      ^^^^^^ Use `to_h { ... }` instead of `inject`.
                  hash[elem.id] = elem.name
                  hash
                end
            "},
            "array.to_h do |elem|\n  [elem.id, elem.name]\nend\n",
        );
    }

    #[test]
    fn corrects_each_with_object_numblock() {
        test::<ReduceToHash>().expect_correction(
            indoc! {"
                array.each_with_object({}) { _2[_1.id] = _1.name }
                      ^^^^^^^^^^^^^^^^ Use `to_h { ... }` instead of `each_with_object`.
            "},
            "array.to_h { [_1.id, _1.name] }\n",
        );
    }

    #[test]
    fn corrects_inject_numblock_rewrites_elem() {
        test::<ReduceToHash>().expect_correction(
            indoc! {"
                array.inject({}) { _1[_2.id] = _2.name; _1 }
                      ^^^^^^ Use `to_h { ... }` instead of `inject`.
            "},
            "array.to_h { [_1.id, _1.name] }\n",
        );
    }

    #[test]
    fn minimum_target_ruby_version_is_set() {
        use murphy_plugin_api::{Cop, RubyVersion};
        assert_eq!(
            <ReduceToHash as Cop>::MINIMUM_TARGET_RUBY_VERSION,
            Some(RubyVersion::new(2, 6)),
        );
    }
}
murphy_plugin_api::submit_cop!(ReduceToHash);
