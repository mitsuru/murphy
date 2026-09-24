//! `Security/CompoundHash` — prefer `Array#hash` to custom hash combinators.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Security/CompoundHash
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors the three independent RuboCop checks: outermost `^`, `+`, `*`,
//!   or `|` combinators (and their op-assign forms) inside a static `hash`
//!   definition or block-form `define_method(:hash)`/`define_singleton_method`;
//!   a one-element Array's direct, argument-free `.hash`; and direct element
//!   `.hash`/`&.hash` calls inside an Array's normal, argument-free `.hash`.
//!   Singleton `def self.hash` is included. The redundant-element rule requires
//!   direct parent/outer-send shapes; nested expressions, safe-nav on the
//!   outer array call, and indirect `public_send(:hash)` are not matched.
//!   `Enabled: pending` and `Safe: false` are represented in metadata. This
//!   cop has no autocorrect, matching RuboCop 1.87.0.
//! ```
//!
//! The send/assignment hooks are method-filtered before ancestor walks. The
//! redundant-hash check uses two parent links, so it does not walk or rescan
//! array descendants.

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, cop};

const HASH_COMBINATORS: &[&str] = &["^", "+", "*", "|"];
const RESTRICT_ON_SEND: &[&str] = &["hash", "^", "+", "*", "|"];

const COMBINATOR_MESSAGE: &str = "Use `[...].hash` instead of combining hash values manually.";
const MONUPLE_MESSAGE: &str = "Delegate hash directly without wrapping in an array when only using a single value.";
const REDUNDANT_MESSAGE: &str = "Calling .hash on elements of a hashed array is redundant.";

#[derive(Default)]
pub struct CompoundHash;

#[cop(
    name = "Security/CompoundHash",
    description = "When overwriting Object#hash to combine values, prefer delegating to Array#hash over writing a custom implementation.",
    default_severity = "warning",
    default_enabled = false,
    safe = false,
    safe_autocorrect = false,
    options = NoOptions,
)]
impl CompoundHash {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(method) = cx.method_name(node) else {
            return;
        };
        if !RESTRICT_ON_SEND.contains(&method) {
            return;
        }

        if is_bad_hash_combinator(node, cx)
            && !has_bad_hash_combinator_ancestor(node, cx)
            && has_hash_method_ancestor(node, cx)
        {
            cx.emit_offense(cx.range(node), COMBINATOR_MESSAGE, None);
        }

        if method == "hash" {
            if is_monuple_hash(node, cx) {
                cx.emit_offense(cx.range(node), MONUPLE_MESSAGE, None);
            }
            if is_redundant_element_hash(node, cx) {
                cx.emit_offense(cx.range(node), REDUNDANT_MESSAGE, None);
            }
        }
    }

    // RuboCop aliases on_csend to on_send. Its `send` combinator and
    // monuple patterns do not match Csend, but the redundant-element pattern
    // accepts a safe-navigation `.hash` as the array element.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) == Some("hash") && is_redundant_element_hash(node, cx) {
            cx.emit_offense(cx.range(node), REDUNDANT_MESSAGE, None);
        }
    }

    #[on_node(kind = "op_asgn")]
    fn check_op_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_bad_hash_combinator(node, cx)
            || has_bad_hash_combinator_ancestor(node, cx)
            || !has_hash_method_ancestor(node, cx)
        {
            return;
        }
        cx.emit_offense(cx.range(node), COMBINATOR_MESSAGE, None);
    }
}

/// Match RuboCop's `(send | op-asgn) _ {:^ | :+ | :* | :|} _` pattern.
/// A send must have exactly one argument; op-assignments always have one RHS.
fn is_bad_hash_combinator(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Send { method, args, .. } => {
            HASH_COMBINATORS.contains(&cx.symbol_str(method)) && cx.list(args).len() == 1
        }
        NodeKind::OpAsgn { op, .. } => HASH_COMBINATORS.contains(&cx.symbol_str(op)),
        _ => false,
    }
}

fn has_bad_hash_combinator_ancestor(node: NodeId, cx: &Cx<'_>) -> bool {
    let mut ancestor = cx.parent(node).get();
    while let Some(current) = ancestor {
        if is_bad_hash_combinator(current, cx) {
            return true;
        }
        ancestor = cx.parent(current).get();
    }
    false
}

fn has_hash_method_ancestor(node: NodeId, cx: &Cx<'_>) -> bool {
    let mut ancestor = cx.parent(node).get();
    while let Some(current) = ancestor {
        if is_hash_method_definition(current, cx) {
            return true;
        }
        ancestor = cx.parent(current).get();
    }
    false
}

fn is_hash_method_definition(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => {
            cx.symbol_str(name) == "hash"
        }
        NodeKind::Block { call, .. } => is_dynamic_hash_method_block(call, cx),
        _ => false,
    }
}

/// Match a block-form `define_method(:hash)` or
/// `define_singleton_method(:hash)`. The source pattern requires one Symbol
/// argument and an actual ordinary block node.
fn is_dynamic_hash_method_block(call: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(*cx.kind(call), NodeKind::Send { .. }) {
        return false;
    }
    if !matches!(cx.method_name(call), Some("define_method" | "define_singleton_method")) {
        return false;
    }

    let arguments = cx.call_arguments(call);
    if arguments.len() != 1 {
        return false;
    }
    matches!(
        *cx.kind(arguments[0]),
        NodeKind::Sym(name) if cx.symbol_str(name) == "hash"
    )
}

fn is_monuple_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
        return false;
    };
    if cx.method_name(node) != Some("hash") || !cx.call_arguments(node).is_empty() {
        return false;
    }
    let Some(receiver) = receiver.get() else {
        return false;
    };
    matches!(*cx.kind(receiver), NodeKind::Array(elements) if cx.list(elements).len() == 1)
}

/// Match a normal `array.hash` node with no explicit arguments.
fn is_array_hash_send(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
        return false;
    };
    if cx.method_name(node) != Some("hash") || !cx.call_arguments(node).is_empty() {
        return false;
    }
    let Some(receiver) = receiver.get() else {
        return false;
    };
    matches!(*cx.kind(receiver), NodeKind::Array(_))
}

/// Match the `^^(send array ... :hash)` ancestor plus a direct element call
/// named `hash`. The outer call must be a normal send; the element may be a
/// safe-navigation send, as in RuboCop's `on_csend` alias.
fn is_redundant_element_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. })
        || cx.method_name(node) != Some("hash")
        || !cx.call_arguments(node).is_empty()
    {
        return false;
    }

    let Some(array) = cx.parent(node).get() else {
        return false;
    };
    if !matches!(*cx.kind(array), NodeKind::Array(_)) {
        return false;
    }
    let Some(hashed_array) = cx.parent(array).get() else {
        return false;
    };
    is_array_hash_send(hashed_array, cx)
}

murphy_plugin_api::submit_cop!(CompoundHash);

#[cfg(test)]
mod tests {
    use super::CompoundHash;
    use murphy_plugin_api::test_support::{indoc, test};
    use murphy_plugin_api::Cop;

    #[test]
    fn mirrors_pending_unsafe_metadata() {
        assert_eq!(<CompoundHash as Cop>::DEFAULT_ENABLED, Some(false));
        assert_eq!(<CompoundHash as Cop>::SAFE, Some(false));
        assert_eq!(<CompoundHash as Cop>::SAFE_AUTOCORRECT, Some(false));
    }

    #[test]
    fn flags_only_outermost_combinators_inside_hash_definitions() {
        test::<CompoundHash>().expect_offense(indoc! {r#"
            a.hash ^ b.hash
            class HashImpl
              def hash
                @a.hash ^ @b.hash ^ @c.hash
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `[...].hash` instead of combining hash values manually.
                @a.hash + @b.hash * @c.hash
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `[...].hash` instead of combining hash values manually.
                @a.hash | @b.hash
                ^^^^^^^^^^^^^^^^^ Use `[...].hash` instead of combining hash values manually.
                @a.hash << @b.hash
              end
              def another
                @a.hash ^ @b.hash
              end
              def self.hash
                1 ^ 2
                ^^^^^ Use `[...].hash` instead of combining hash values manually.
              end
            end
        "#});
    }

    #[test]
    fn flags_op_assignments_and_block_form_hash_definitions() {
        test::<CompoundHash>().expect_offense(indoc! {r#"
            class HashImpl
              def hash
                x ^= y
                ^^^^^^ Use `[...].hash` instead of combining hash values manually.
                x += y
                ^^^^^^ Use `[...].hash` instead of combining hash values manually.
                x *= y
                ^^^^^^ Use `[...].hash` instead of combining hash values manually.
                x |= y
                ^^^^^^ Use `[...].hash` instead of combining hash values manually.
                x <<= y
                x ^= y ^ z
                ^^^^^^^^^^ Use `[...].hash` instead of combining hash values manually.
              end
              define_method(:hash) { 1 ^ 2 }
                                     ^^^^^ Use `[...].hash` instead of combining hash values manually.
              define_singleton_method(:hash) do
                3 | 4
                ^^^^^ Use `[...].hash` instead of combining hash values manually.
              end
            end
        "#});
    }

    #[test]
    fn flags_monuple_arrays_with_exact_send_ranges() {
        test::<CompoundHash>().expect_offense(indoc! {r#"
            [x].hash
            ^^^^^^^^ Delegate hash directly without wrapping in an array when only using a single value.
            [x,].hash
            ^^^^^^^^^ Delegate hash directly without wrapping in an array when only using a single value.
            [*xs].hash
            ^^^^^^^^^^ Delegate hash directly without wrapping in an array when only using a single value.
            [x.hash.to_s].hash
            ^^^^^^^^^^^^^^^^^^ Delegate hash directly without wrapping in an array when only using a single value.
            [[y.hash]].hash
            ^^^^^^^^^^^^^^^ Delegate hash directly without wrapping in an array when only using a single value.
        "#});
    }

    #[test]
    fn flags_direct_hash_calls_on_elements_of_hashed_arrays() {
        test::<CompoundHash>().expect_offense(indoc! {r#"
            [x.hash, y.hash].hash
             ^^^^^^ Calling .hash on elements of a hashed array is redundant.
                     ^^^^^^ Calling .hash on elements of a hashed array is redundant.
            [x.hash].hash
            ^^^^^^^^^^^^^ Delegate hash directly without wrapping in an array when only using a single value.
             ^^^^^^ Calling .hash on elements of a hashed array is redundant.
            [x&.hash, y&.hash].hash
             ^^^^^^^ Calling .hash on elements of a hashed array is redundant.
                      ^^^^^^^ Calling .hash on elements of a hashed array is redundant.
        "#});
    }

    #[test]
    fn only_direct_elements_of_normal_array_hashes_are_redundant() {
        test::<CompoundHash>().expect_no_offenses(indoc! {r#"
            [x, y].hash
            Array[x].hash
            [x].hash(foo)
            [x.hash].hash(foo)
            [x.hash]&.hash
            [x.hash].public_send(:hash)
            x.hash.hash
        "#});
    }

}
