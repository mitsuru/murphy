//! `Lint/SharedMutableDefault` — checks `Hash.new` with a mutable shared
//! default value.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/SharedMutableDefault
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Matches RuboCop's `Hash.new` send coverage for array/hash literals,
//!   Array.new/Hash.new defaults, capacity keyword handling, frozen defaults,
//!   block/default_proc-safe forms, and scalar defaults. No autocorrect.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

const MSG: &str = "Do not create a Hash with a mutable default value as the default value can accidentally be changed.";

#[derive(Default)]
pub struct SharedMutableDefault;

#[cop(
    name = "Lint/SharedMutableDefault",
    description = "Checks Hash creation with a mutable shared default value.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SharedMutableDefault {
    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if hash_initialized_with_mutable_shared_object(node, cx) {
            cx.emit_offense(cx.range(node), MSG, None);
        }
    }
}

fn hash_initialized_with_mutable_shared_object(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
        return false;
    };
    let Some(receiver) = receiver.get() else {
        return false;
    };
    if !cx.is_global_const(receiver, "Hash") {
        return false;
    }

    let args = cx.list(args);
    match args {
        [only] => mutable_default_arg(*only, cx),
        [first, second] => mutable_default_arg(*first, cx) && !capacity_keyword_argument(*first, cx) && capacity_keyword_argument(*second, cx),
        _ => false,
    }
}

fn mutable_default_arg(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Array(_) => true,
        NodeKind::Hash(_) => !capacity_keyword_argument(node, cx),
        NodeKind::Send { receiver, method, args } => {
            cx.symbol_str(method) == "new"
                && cx.list(args).is_empty()
                && receiver
                    .get()
                    .is_some_and(|receiver| cx.is_global_const(receiver, "Array") || cx.is_global_const(receiver, "Hash"))
        }
        _ => false,
    }
}

fn capacity_keyword_argument(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Hash(pairs) = *cx.kind(node) else {
        return false;
    };
    cx.list(pairs).iter().any(|&pair| {
        let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
            return false;
        };
        matches!(*cx.kind(key), NodeKind::Sym(sym) if cx.symbol_str(sym) == "capacity")
    })
}

#[cfg(test)]
mod tests {
    use super::SharedMutableDefault;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_hash_new_array_literal() {
        test::<SharedMutableDefault>().expect_offense(indoc! {r#"
            Hash.new([])
            ^^^^^^^^^^^^ Do not create a Hash with a mutable default value as the default value can accidentally be changed.
        "#});
    }

    #[test]
    fn flags_hash_and_constructor_defaults() {
        test::<SharedMutableDefault>().expect_offense(indoc! {r#"
            Hash.new({})
            ^^^^^^^^^^^^ Do not create a Hash with a mutable default value as the default value can accidentally be changed.
            Hash.new(Array.new)
            ^^^^^^^^^^^^^^^^^^^ Do not create a Hash with a mutable default value as the default value can accidentally be changed.
            Hash.new(Hash.new)
            ^^^^^^^^^^^^^^^^^^ Do not create a Hash with a mutable default value as the default value can accidentally be changed.
        "#});
    }

    #[test]
    fn flags_hash_default_with_capacity_keyword() {
        test::<SharedMutableDefault>().expect_offense(indoc! {r#"
            Hash.new({}, capacity: 42)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not create a Hash with a mutable default value as the default value can accidentally be changed.
        "#});
    }

    #[test]
    fn accepts_unrelated_and_safe_defaults() {
        test::<SharedMutableDefault>().expect_no_offenses(indoc! {r#"
            []
            {}
            Array.new
            Hash.new
            Hash.new { |h, k| h[k] = [] }
            Hash.new { [] }
            Hash.new(0)
            Hash.new(false)
            Hash.new(true)
            Hash.new(nil)
            Hash.new([].freeze)
            Hash.new({}.freeze)
            Hash.new([].freeze, capacity: 42)
            Hash.new({}.freeze, capacity: 42)
            Hash.new(Array.new.freeze)
            Hash.new(Hash.new.freeze)
            Hash.new(capacity: 42)
        "#});
    }
}

murphy_plugin_api::submit_cop!(SharedMutableDefault);
