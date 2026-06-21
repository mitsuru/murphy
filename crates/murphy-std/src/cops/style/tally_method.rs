//! `Style/TallyMethod` — prefer `Enumerable#tally` over manual counting patterns.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TallyMethod
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full parity with all four RuboCop patterns (each as block/numblock/itblock
//!   variant where applicable):
//!     1. `each_with_object(Hash.new(0)) { |e, h| h[e] += 1 }`
//!        (also the numblock form `{ _2[_1] += 1 }`)
//!     2. `group_by(&:itself).transform_values(&:count|size|length)`
//!     3. `group_by { |x| x }.transform_values(&:count|size|length)`
//!        (also numblock `{ _1 }` and itblock `{ it }` identity forms)
//!     4. `group_by(...).transform_values { |v| v.count|size|length }`
//!        (transform-block as block/numblock/itblock; group_by as any of the
//!        identity forms from patterns 2/3)
//!
//!   Offense location matches RuboCop exactly: pattern 1 offends on the
//!   `each_with_object` selector; patterns 2-4 offend on the `group_by`
//!   selector. Autocorrect replaces `[group_by/each_with_object selector ..
//!   end of the whole chain]` with `tally` (RuboCop's `replacement_range`).
//!
//!   Counting methods are exactly `{count, size, length}`. `Hash.new(0)`
//!   accepts a bare or `::`-scoped `Hash` const and requires the literal arg
//!   `0`; the `op_asgn` must be `+= 1` with the index receiver bound to the
//!   hash param and the index subscript bound to the element param.
//!
//!   Safe-navigation (`&.`) chains are flagged too, mirroring RuboCop's
//!   `alias on_csend on_send`.
//!
//!   `safe_autocorrect = false` mirrors RuboCop's `Safe: false` — the
//!   correction only applies under `--fix-all`/`-A` (RuboCop's `-A`), not the
//!   safe `--fix`/`-a`, because static analysis cannot prove the receiver is
//!   an `Enumerable`.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! array.each_with_object(Hash.new(0)) { |item, counts| counts[item] += 1 }
//! array.group_by(&:itself).transform_values(&:count)
//! array.group_by { |item| item }.transform_values(&:size)
//! array.group_by { |item| item }.transform_values { |v| v.length }
//!
//! # good
//! array.tally
//! ```
//!
//! ## Autocorrect
//!
//! Single whole-range replacement from the `group_by` / `each_with_object`
//! selector start through the end of the whole expression, replaced with
//! `tally`. Marked `safe_autocorrect = false`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, Symbol, cop};

const MSG_EACH_WITH_OBJECT: &str = "Use `tally` instead of `each_with_object`.";
const MSG_GROUP_BY: &str = "Use `tally` instead of `group_by` and `transform_values`.";

/// Counting methods that `transform_values` may call.
const COUNTING_METHODS: &[&str] = &["count", "size", "length"];

/// Stateless unit struct.
#[derive(Default)]
pub struct TallyMethod;

#[cop(
    name = "Style/TallyMethod",
    description = "Prefer `Enumerable#tally` over manual counting patterns.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
    safe_autocorrect = false,
)]
impl TallyMethod {
    /// Pattern 1 (each_with_object) — a block over an `each_with_object` call.
    /// Pattern 4 (transform-block) — a block over a `transform_values` call.
    ///
    /// All three block kinds dispatch here because the numblock (`_2[_1] += 1`,
    /// `_1.size`) and itblock (`it.count`) spellings are also valid shapes.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_any_block(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_any_block(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_any_block(node, cx);
    }

    /// Patterns 2 & 3 — a bare `transform_values` send (no block) whose
    /// counting argument is `&:count|size|length`.
    #[on_node(kind = "send", methods = ["transform_values"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_transform_values_send(node, cx);
    }

    /// Safe-navigation form — RuboCop aliases `on_csend` to `on_send`, so
    /// `recv&.transform_values(&:count)` is also flagged.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) != Some("transform_values") {
            return;
        }
        check_transform_values_send(node, cx);
    }
}

/// Shared `transform_values` send handler for patterns 2 & 3 (no block).
fn check_transform_values_send(node: NodeId, cx: &Cx<'_>) {
    // A `transform_values` that owns a block is handled by `check_block`
    // (pattern 4); skip it here.
    if cx.block_node(node).get().is_some() {
        return;
    }
    check_transform_values_symbol(node, cx);
}

/// Shared entry for all three block kinds (block / numblock / itblock).
fn check_any_block(node: NodeId, cx: &Cx<'_>) {
    let Some(call) = cx.block_call(node).get() else {
        return;
    };
    match cx.method_name(call) {
        Some("each_with_object") => check_each_with_object_block(node, call, cx),
        Some("transform_values") => check_transform_block(node, call, cx),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Pattern 1: each_with_object(Hash.new(0)) { |e, h| h[e] += 1 }
// ---------------------------------------------------------------------------

fn check_each_with_object_block(block: NodeId, call: NodeId, cx: &Cx<'_>) {
    // The single `each_with_object` argument must be `Hash.new(0)`.
    let send_args = cx.call_arguments(call);
    let [hash_arg] = send_args else {
        return;
    };
    if !is_hash_new_zero(*hash_arg, cx) {
        return;
    }

    // Resolve the two implicit/explicit block params (elem, hash) and the body.
    let Some((elem_sym, hash_sym, body)) = block_two_params(block, cx) else {
        return;
    };

    // Body must be `hash[elem] += 1`.
    if !is_count_op_asgn(body, elem_sym, hash_sym, cx) {
        return;
    }

    cx.emit_offense(cx.node(call).loc.name, MSG_EACH_WITH_OBJECT, None);
    emit_tally_correction(call, block, cx);
}

/// Returns `(elem_sym, hash_sym, body)` for a 2-param block — either an
/// explicit `|elem, hash|` block or a `_1`/`_2` numblock.
fn block_two_params(block: NodeId, cx: &Cx<'_>) -> Option<(ParamRef, ParamRef, NodeId)> {
    let body = cx.block_body(block).get()?;
    match *cx.kind(block) {
        NodeKind::Block { args, .. } => {
            let NodeKind::Args(args_list) = *cx.kind(args) else {
                return None;
            };
            let [elem, hash] = cx.list(args_list) else {
                return None;
            };
            let (NodeKind::Arg(elem_sym), NodeKind::Arg(hash_sym)) =
                (*cx.kind(*elem), *cx.kind(*hash))
            else {
                return None;
            };
            Some((ParamRef::Named(elem_sym), ParamRef::Named(hash_sym), body))
        }
        NodeKind::Numblock { max_n: 2, .. } => {
            Some((ParamRef::Numbered(1), ParamRef::Numbered(2), body))
        }
        _ => None,
    }
}

/// A reference to a block parameter — either by interned symbol (explicit
/// block param) or by numbered position (`_1`, `_2`).
#[derive(Clone, Copy)]
enum ParamRef {
    Named(Symbol),
    Numbered(u8),
}

impl ParamRef {
    /// Does `node` resolve to an `Lvar` that refers to this parameter?
    fn matches_lvar(self, node: NodeId, cx: &Cx<'_>) -> bool {
        let NodeKind::Lvar(sym) = *cx.kind(node) else {
            return false;
        };
        match self {
            ParamRef::Named(s) => sym == s,
            ParamRef::Numbered(n) => cx.symbol_str(sym) == format!("_{n}"),
        }
    }
}

/// `hash[elem] += 1` where `hash`/`elem` bind to the given params.
fn is_count_op_asgn(node: NodeId, elem: ParamRef, hash: ParamRef, cx: &Cx<'_>) -> bool {
    let NodeKind::OpAsgn { target, op, value } = *cx.kind(node) else {
        return false;
    };
    if cx.symbol_str(op) != "+" {
        return false;
    }
    if !matches!(*cx.kind(value), NodeKind::Int(1)) {
        return false;
    }
    // Target must be `hash[elem]` — an Index whose receiver is the hash param
    // and whose single subscript is the element param.
    let NodeKind::Index { receiver, args } = *cx.kind(target) else {
        return false;
    };
    if !hash.matches_lvar(receiver, cx) {
        return false;
    }
    let [subscript] = cx.list(args) else {
        return false;
    };
    elem.matches_lvar(*subscript, cx)
}

/// `Hash.new(0)` — bare or `::`-scoped `Hash`, method `new`, single arg `0`.
fn is_hash_new_zero(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.method_name(node) != Some("new") {
        return false;
    }
    let Some(recv) = cx.call_receiver(node).get() else {
        return false;
    };
    if !cx.is_global_const(recv, "Hash") {
        return false;
    }
    let [arg] = cx.call_arguments(node) else {
        return false;
    };
    matches!(*cx.kind(*arg), NodeKind::Int(0))
}

// ---------------------------------------------------------------------------
// Patterns 2 & 3: group_by(<identity>).transform_values(&:count|size|length)
// ---------------------------------------------------------------------------

fn check_transform_values_symbol(transform_send: NodeId, cx: &Cx<'_>) {
    // The single argument must be `&:count|size|length`.
    let [arg] = cx.call_arguments(transform_send) else {
        return;
    };
    if !is_counting_block_pass(*arg, cx) {
        return;
    }
    // The receiver must be an identity `group_by` (pattern 2 symbol form or
    // pattern 3 identity-block form).
    let Some(recv) = cx.call_receiver(transform_send).get() else {
        return;
    };
    let Some(group_by) = identity_group_by(recv, cx) else {
        return;
    };
    register_group_by_offense(group_by, transform_send, cx);
}

/// `&:count|size|length` — a block-pass of a counting-method symbol.
fn is_counting_block_pass(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::BlockPass(inner) = *cx.kind(node) else {
        return false;
    };
    let Some(sym_node) = inner.get() else {
        return false;
    };
    matches!(*cx.kind(sym_node), NodeKind::Sym(s) if COUNTING_METHODS.contains(&cx.symbol_str(s)))
}

/// If `node` is an identity `group_by` (any of the symbol / block / numblock /
/// itblock identity forms), return the underlying `group_by` send node.
fn identity_group_by(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(node) {
        // Pattern 2: `group_by(&:itself)` — a bare send.
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {
            if cx.method_name(node) != Some("group_by") {
                return None;
            }
            let [arg] = cx.call_arguments(node) else {
                return None;
            };
            is_itself_block_pass(*arg, cx).then_some(node)
        }
        // Pattern 3: `group_by { |x| x }` / `{ _1 }` / `{ it }` — identity block.
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => {
            let call = cx.block_call(node).get()?;
            if cx.method_name(call) != Some("group_by") {
                return None;
            }
            if !cx.call_arguments(call).is_empty() {
                return None;
            }
            is_identity_block(node, cx).then_some(call)
        }
        _ => None,
    }
}

/// `&:itself` — a block-pass of the `:itself` symbol.
fn is_itself_block_pass(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::BlockPass(inner) = *cx.kind(node) else {
        return false;
    };
    let Some(sym_node) = inner.get() else {
        return false;
    };
    matches!(*cx.kind(sym_node), NodeKind::Sym(s) if cx.symbol_str(s) == "itself")
}

/// A block whose body returns its single parameter unchanged:
/// `{ |x| x }`, `{ _1 }`, or `{ it }`.
fn is_identity_block(block: NodeId, cx: &Cx<'_>) -> bool {
    let Some(body) = cx.block_body(block).get() else {
        return false;
    };
    match *cx.kind(block) {
        NodeKind::Block { args, .. } => {
            let NodeKind::Args(args_list) = *cx.kind(args) else {
                return false;
            };
            let [param] = cx.list(args_list) else {
                return false;
            };
            let NodeKind::Arg(param_sym) = *cx.kind(*param) else {
                return false;
            };
            ParamRef::Named(param_sym).matches_lvar(body, cx)
        }
        NodeKind::Numblock { max_n: 1, .. } => ParamRef::Numbered(1).matches_lvar(body, cx),
        NodeKind::Itblock { .. } => {
            matches!(*cx.kind(body), NodeKind::Lvar(s) if cx.symbol_str(s) == "it")
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Pattern 4: group_by(<identity>).transform_values { |v| v.count|size|length }
// ---------------------------------------------------------------------------

fn check_transform_block(block: NodeId, transform_send: NodeId, cx: &Cx<'_>) {
    // The `transform_values` call must take no positional arguments.
    if !cx.call_arguments(transform_send).is_empty() {
        return;
    }
    // The block body must be `<param>.count|size|length`.
    if !is_counting_transform_block(block, cx) {
        return;
    }
    // The receiver of `transform_values` must be an identity `group_by`.
    let Some(recv) = cx.call_receiver(transform_send).get() else {
        return;
    };
    let Some(group_by) = identity_group_by(recv, cx) else {
        return;
    };
    register_group_by_offense(group_by, block, cx);
}

/// `{ |v| v.count }`, `{ _1.size }`, or `{ it.length }` — a single-param block
/// whose body is a counting-method call on that param.
fn is_counting_transform_block(block: NodeId, cx: &Cx<'_>) -> bool {
    let Some(body) = cx.block_body(block).get() else {
        return false;
    };
    // Body must be a counting-method call on its receiver.
    if !matches!(cx.method_name(body), Some(m) if COUNTING_METHODS.contains(&m)) {
        return false;
    }
    if !cx.call_arguments(body).is_empty() {
        return false;
    }
    let Some(recv) = cx.call_receiver(body).get() else {
        return false;
    };
    // The call receiver must be the block's single parameter.
    match *cx.kind(block) {
        NodeKind::Block { args, .. } => {
            let NodeKind::Args(args_list) = *cx.kind(args) else {
                return false;
            };
            let [param] = cx.list(args_list) else {
                return false;
            };
            let NodeKind::Arg(param_sym) = *cx.kind(*param) else {
                return false;
            };
            ParamRef::Named(param_sym).matches_lvar(recv, cx)
        }
        NodeKind::Numblock { max_n: 1, .. } => ParamRef::Numbered(1).matches_lvar(recv, cx),
        NodeKind::Itblock { .. } => {
            matches!(*cx.kind(recv), NodeKind::Lvar(s) if cx.symbol_str(s) == "it")
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Shared offense / autocorrect for the group_by patterns.
// ---------------------------------------------------------------------------

/// Emit the `group_by`/`transform_values` offense on the `group_by` selector
/// and replace the whole chain with `tally`.
///
/// `end_node` is the node whose end bounds the replacement: the bare
/// `transform_values` send for patterns 2/3, or the transform block for
/// pattern 4.
fn register_group_by_offense(group_by: NodeId, end_node: NodeId, cx: &Cx<'_>) {
    cx.emit_offense(cx.node(group_by).loc.name, MSG_GROUP_BY, None);
    emit_tally_correction(group_by, end_node, cx);
}

/// Replace `[selector start of `start_node` .. end of `end_node`]` with
/// `tally` (RuboCop's `replacement_range`).
fn emit_tally_correction(start_node: NodeId, end_node: NodeId, cx: &Cx<'_>) {
    let range = Range {
        start: cx.node(start_node).loc.name.start,
        end: cx.range(end_node).end,
    };
    cx.emit_edit(range, "tally");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::TallyMethod;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Pattern 1: each_with_object -------------------------------------

    #[test]
    fn flags_each_with_object_block() {
        test::<TallyMethod>().expect_offense(indoc! {"
            array.each_with_object(Hash.new(0)) { |item, counts| counts[item] += 1 }
                  ^^^^^^^^^^^^^^^^ Use `tally` instead of `each_with_object`.
        "});
    }

    #[test]
    fn flags_each_with_object_numblock() {
        test::<TallyMethod>().expect_offense(indoc! {"
            array.each_with_object(Hash.new(0)) { _2[_1] += 1 }
                  ^^^^^^^^^^^^^^^^ Use `tally` instead of `each_with_object`.
        "});
    }

    #[test]
    fn flags_each_with_object_cbase_hash() {
        test::<TallyMethod>().expect_offense(indoc! {"
            array.each_with_object(::Hash.new(0)) { |item, counts| counts[item] += 1 }
                  ^^^^^^^^^^^^^^^^ Use `tally` instead of `each_with_object`.
        "});
    }

    #[test]
    fn corrects_each_with_object() {
        test::<TallyMethod>().expect_correction(
            indoc! {"
                array.each_with_object(Hash.new(0)) { |item, counts| counts[item] += 1 }
                      ^^^^^^^^^^^^^^^^ Use `tally` instead of `each_with_object`.
            "},
            "array.tally\n",
        );
    }

    // ----- Pattern 2: group_by(&:itself).transform_values(&:count) ---------

    #[test]
    fn flags_group_by_symbol_count() {
        test::<TallyMethod>().expect_offense(indoc! {"
            array.group_by(&:itself).transform_values(&:count)
                  ^^^^^^^^ Use `tally` instead of `group_by` and `transform_values`.
        "});
    }

    #[test]
    fn corrects_group_by_symbol() {
        test::<TallyMethod>().expect_correction(
            indoc! {"
                array.group_by(&:itself).transform_values(&:count)
                      ^^^^^^^^ Use `tally` instead of `group_by` and `transform_values`.
            "},
            "array.tally\n",
        );
    }

    // ----- Pattern 3: group_by { identity }.transform_values(&:count) ------

    #[test]
    fn flags_group_by_identity_block_size() {
        test::<TallyMethod>().expect_offense(indoc! {"
            array.group_by { |item| item }.transform_values(&:size)
                  ^^^^^^^^ Use `tally` instead of `group_by` and `transform_values`.
        "});
    }

    #[test]
    fn flags_group_by_numblock_length() {
        test::<TallyMethod>().expect_offense(indoc! {"
            array.group_by { _1 }.transform_values(&:length)
                  ^^^^^^^^ Use `tally` instead of `group_by` and `transform_values`.
        "});
    }

    #[test]
    fn flags_group_by_itblock_count() {
        test::<TallyMethod>().expect_offense(indoc! {"
            array.group_by { it }.transform_values(&:count)
                  ^^^^^^^^ Use `tally` instead of `group_by` and `transform_values`.
        "});
    }

    // ----- Pattern 4: group_by(...).transform_values { |v| v.count } -------

    #[test]
    fn flags_transform_block() {
        test::<TallyMethod>().expect_offense(indoc! {"
            array.group_by { |item| item }.transform_values { |v| v.length }
                  ^^^^^^^^ Use `tally` instead of `group_by` and `transform_values`.
        "});
    }

    #[test]
    fn flags_transform_block_symbol_group_by() {
        test::<TallyMethod>().expect_offense(indoc! {"
            array.group_by(&:itself).transform_values { |v| v.count }
                  ^^^^^^^^ Use `tally` instead of `group_by` and `transform_values`.
        "});
    }

    #[test]
    fn flags_transform_numblock() {
        test::<TallyMethod>().expect_offense(indoc! {"
            array.group_by { _1 }.transform_values { _1.size }
                  ^^^^^^^^ Use `tally` instead of `group_by` and `transform_values`.
        "});
    }

    #[test]
    fn flags_transform_itblock() {
        test::<TallyMethod>().expect_offense(indoc! {"
            array.group_by { it }.transform_values { it.count }
                  ^^^^^^^^ Use `tally` instead of `group_by` and `transform_values`.
        "});
    }

    #[test]
    fn corrects_transform_block() {
        test::<TallyMethod>().expect_correction(
            indoc! {"
                array.group_by { |item| item }.transform_values { |v| v.length }
                      ^^^^^^^^ Use `tally` instead of `group_by` and `transform_values`.
            "},
            "array.tally\n",
        );
    }

    // ----- Safe navigation (csend) — RuboCop aliases on_csend to on_send ---

    #[test]
    fn flags_csend_group_by_symbol() {
        test::<TallyMethod>().expect_offense(indoc! {"
            arr&.group_by(&:itself)&.transform_values(&:count)
                 ^^^^^^^^ Use `tally` instead of `group_by` and `transform_values`.
        "});
    }

    #[test]
    fn flags_csend_each_with_object() {
        test::<TallyMethod>().expect_offense(indoc! {"
            arr&.each_with_object(Hash.new(0)) { |item, counts| counts[item] += 1 }
                 ^^^^^^^^^^^^^^^^ Use `tally` instead of `each_with_object`.
        "});
    }

    // ----- Negatives -------------------------------------------------------

    #[test]
    fn accepts_hash_new_without_arg() {
        test::<TallyMethod>()
            .expect_no_offenses("array.each_with_object(Hash.new) { |item, counts| counts[item] += 1 }\n");
    }

    #[test]
    fn accepts_op_asgn_not_one() {
        test::<TallyMethod>().expect_no_offenses(
            "array.each_with_object(Hash.new(0)) { |item, counts| counts[item] += 2 }\n",
        );
    }

    #[test]
    fn accepts_empty_hash_accumulator() {
        test::<TallyMethod>()
            .expect_no_offenses("array.each_with_object({}) { |item, counts| counts[item] += 1 }\n");
    }

    #[test]
    fn accepts_non_identity_group_by_symbol() {
        test::<TallyMethod>().expect_no_offenses("array.group_by(&:foo).transform_values(&:count)\n");
    }

    #[test]
    fn accepts_non_identity_group_by_block() {
        test::<TallyMethod>()
            .expect_no_offenses("array.group_by { |item| item.bar }.transform_values(&:size)\n");
    }

    #[test]
    fn accepts_non_counting_transform_symbol() {
        test::<TallyMethod>()
            .expect_no_offenses("array.group_by(&:itself).transform_values(&:sum)\n");
    }

    #[test]
    fn accepts_multi_arg_group_by_block() {
        test::<TallyMethod>()
            .expect_no_offenses("array.group_by { |a, b| a }.transform_values(&:count)\n");
    }

    #[test]
    fn accepts_non_counting_transform_block() {
        test::<TallyMethod>()
            .expect_no_offenses("array.group_by(&:itself).transform_values { |v| v.sum }\n");
    }

    #[test]
    fn accepts_plain_transform_values() {
        test::<TallyMethod>().expect_no_offenses("hash.transform_values(&:count)\n");
    }
}

murphy_plugin_api::submit_cop!(TallyMethod);
