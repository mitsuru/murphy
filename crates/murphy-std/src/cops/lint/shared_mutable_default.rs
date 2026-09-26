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

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, def_node_matcher};

// RuboCop parity: `Lint/SharedMutableDefault` matcher for `Hash.new` is
// `(send (const {nil? cbase} :Hash) :new ...)`. In Murphy `::Hash`
// collapses to `Const{scope:None}`, so a single `nil?` scope covers bare
// and `::`-prefixed forms — equivalent to the prior `is_global_const`
// check. The mutable-default argument inspection (`Array`/`Hash` literals,
// `Array.new`/`Hash.new` constructors, `capacity:` keywords) stays
// hand-rolled below.
def_node_matcher!(hash_new, "(send (const nil? :Hash) :new ...)");

// RuboCop parity: `Lint/SharedMutableDefault` inner mutable defaults include
// `(send (const {nil? cbase} {:Array :Hash}) :new)` (zero args, send-only).
// In Murphy `::Array` / `::Hash` collapse to `Const{scope:None}`: `nil?`
// covers bare + `::` (pinned by `boundary_flags_cbase_array_new_inner`).
// Namespaced `Foo::Array` still does not flag (pinned by
// `boundary_ignores_namespaced_array_new_inner`). `send` covers `Send` only,
// matching the `NodeKind::Send` guard (csend still does not flag, pinned by
// `boundary_ignores_csend_array_new_inner`).
def_node_matcher!(
    is_array_or_hash_new,
    "(send (const nil? {:Array :Hash}) :new)"
);

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
    // `(send (const nil? :Hash) :new ...)` — top-level `Hash.new`.
    if !hash_new(node, cx) {
        return false;
    }
    let NodeKind::Send { args, .. } = *cx.kind(node) else {
        return false;
    };

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
        NodeKind::Send { .. } => {
            // `(send (const nil? {:Array :Hash}) :new)` (`Array`/`Hash` /
            // `::Array`/`::Hash`, top-level only, zero args). `send` covers
            // `Send` only.
            is_array_or_hash_new(node, cx)
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

    // --- Boundary characterization (murphy-ft88.3): pin the exact node set
    // the hand-rolled `is_global_const(receiver, "Hash")` predicate matches,
    // so the `(send (const nil? :Hash) :new ...)` refactor can be proven
    // equivalent. `::Hash` collapses to `Const{scope:None}` in Murphy, so a
    // single `nil?` scope covers bare + `::`-prefixed forms.

    #[test]
    fn boundary_flags_cbase_hash_new_outer() {
        test::<SharedMutableDefault>().expect_offense(indoc! {r#"
            ::Hash.new([])
            ^^^^^^^^^^^^^^ Do not create a Hash with a mutable default value as the default value can accidentally be changed.
        "#});
    }

    #[test]
    fn boundary_ignores_namespaced_hash_new() {
        // `Foo::Hash` has a non-nil const scope, so it is not flagged.
        test::<SharedMutableDefault>().expect_no_offenses("Foo::Hash.new([])
");
    }

    #[test]
    fn boundary_ignores_csend_hash_new() {
        // `&.` is a `csend` node, not `send`; RuboCop's `(send ...)`
        // pattern does not match it, and `#[on_node(kind = "send")]`
        // never dispatches on it.
        test::<SharedMutableDefault>().expect_no_offenses("Hash&.new([])
");
    }

    // --- Boundary characterization (murphy-ft88.13): pin the exact node set
    // the hand-rolled `Array.new`/`Hash.new` inner (`is_global_const` +
    // `NodeKind::Send` + empty args) matches, so the verbatim
    // `(send (const nil? {:Array :Hash}) :new)` refactor can be proven
    // equivalent.

    #[test]
    fn boundary_flags_cbase_array_new_inner() {
        test::<SharedMutableDefault>().expect_offense(indoc! {r#"
            Hash.new(::Array.new)
            ^^^^^^^^^^^^^^^^^^^^^ Do not create a Hash with a mutable default value as the default value can accidentally be changed.
        "#});
    }

    #[test]
    fn boundary_ignores_namespaced_array_new_inner() {
        test::<SharedMutableDefault>().expect_no_offenses("Hash.new(Foo::Array.new)\n");
    }

    #[test]
    fn boundary_ignores_csend_array_new_inner() {
        test::<SharedMutableDefault>().expect_no_offenses("Hash.new(Array&.new)\n");
    }

}

murphy_plugin_api::submit_cop!(SharedMutableDefault);
