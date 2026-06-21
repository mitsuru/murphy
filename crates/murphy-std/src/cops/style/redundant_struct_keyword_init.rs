//! `Style/RedundantStructKeywordInit` — flags redundant `keyword_init` option
//! for `Struct.new`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantStructKeywordInit
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: [murphy-i3nb]
//! notes: >
//!   Since Ruby 3.2, `keyword_init` in `Struct.new` defaults to `nil`
//!   behaviour, so `keyword_init: nil` and `keyword_init: true` are
//!   redundant. Detection-only port:
//!     - Matches `Struct.new(..., keyword_init: true|nil)` and the
//!       safe-navigation form `Struct&.new(...)` (RuboCop aliases
//!       `on_csend on_send`).
//!     - The last argument must be a hash literal; only `keyword_init`
//!       pairs whose key is the symbol `:keyword_init` are considered
//!       (string keys `"keyword_init" => true` do NOT match, mirroring
//!       upstream's `(sym :keyword_init)` matcher).
//!     - If ANY `keyword_init: false` pair is present, the cop emits
//!       nothing — even for sibling `true`/`nil` pairs — matching
//!       RuboCop's early `return if ... keyword_init_false?`.
//!     - Receiver must be `Struct` or `::Struct` (nil/cbase scope);
//!       `Foo::Struct` is rejected (`cx.is_global_const`).
//!     - Gated at `minimum_target_ruby_version = "3.2"`; the host registry
//!       only dispatches the cop when the resolved target is >= 3.2, so
//!       this cop never fires under the default 3.1 floor.
//!     - The offense range is the whole `keyword_init: <value>` pair,
//!       and the message embeds the value's source (`true` or `nil`),
//!       verified caret-exact against standalone RuboCop 1.87.0.
//!   Autocorrect is deliberately NOT ported: upstream marks it
//!   `SafeAutoCorrect: false` (removing `keyword_init: true` changes the
//!   return value of `Struct#keyword_init?` and the initialization
//!   contract), and the bd issue specifies "no-autocorrect". Tracked as a
//!   deferred gap in murphy-i3nb.
//! ```
//!
//! ## Matched shapes
//!
//! `Send`/`Csend` nodes with method `new`, receiver `Struct`/`::Struct`, and a
//! trailing `Hash` argument containing a `keyword_init: true|nil` pair (and no
//! `keyword_init: false` pair).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantStructKeywordInit;

#[cop(
    name = "Style/RedundantStructKeywordInit",
    description = "Checks for redundant `keyword_init` option for `Struct.new`.",
    default_severity = "warning",
    default_enabled = false,
    minimum_target_ruby_version = "3.2",
    options = NoOptions,
)]
impl RedundantStructKeywordInit {
    #[on_node(kind = "send", methods = ["new"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) == Some("new") {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Receiver must be exactly `Struct` or `::Struct` (nil / cbase scope).
    let Some(recv_id) = cx.call_receiver(node).get() else {
        return;
    };
    if !cx.is_global_const(recv_id, "Struct") {
        return;
    }

    // The last argument must be a hash literal; only its pairs are inspected.
    let args = cx.call_arguments(node);
    let Some(&last_arg) = args.last() else {
        return;
    };
    let NodeKind::Hash(pairs) = *cx.kind(last_arg) else {
        return;
    };

    let pair_list = cx.list(pairs);

    // RuboCop returns early if ANY `keyword_init: false` pair is present —
    // `false` is not redundant, so the whole call is left alone (even sibling
    // `true`/`nil` pairs). Check this before emitting any offense.
    if pair_list
        .iter()
        .any(|&pair| keyword_init_value(pair, cx).is_some_and(|v| matches!(cx.kind(v), NodeKind::False_)))
    {
        return;
    }

    // Flag each `keyword_init: true|nil` pair.
    for &pair in pair_list {
        let Some(value) = keyword_init_value(pair, cx) else {
            continue;
        };
        if matches!(cx.kind(value), NodeKind::True_ | NodeKind::Nil) {
            register_offense(pair, value, cx);
        }
    }
}

/// If `pair` is a `keyword_init:` pair (symbol key), return its value node.
fn keyword_init_value(pair: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let NodeKind::Pair { key, value } = *cx.kind(pair) else {
        return None;
    };
    matches!(cx.kind(key), NodeKind::Sym(sym) if cx.symbol_str(*sym) == "keyword_init")
        .then_some(value)
}

/// Emit an offense covering the whole `keyword_init: <value>` pair, with the
/// value's source embedded in the message (matching RuboCop verbatim).
fn register_offense(pair: NodeId, value: NodeId, cx: &Cx<'_>) {
    let value_src = cx.raw_source(cx.range(value));
    let msg = format!("Remove the redundant `keyword_init: {value_src}`.");
    cx.emit_offense(cx.range(pair), &msg, None);
}

#[cfg(test)]
mod tests {
    use super::RedundantStructKeywordInit;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_keyword_init_true() {
        test::<RedundantStructKeywordInit>().expect_offense(indoc! {r#"
            Struct.new(:foo, keyword_init: true)
                             ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
        "#});
    }

    #[test]
    fn flags_keyword_init_nil() {
        test::<RedundantStructKeywordInit>().expect_offense(indoc! {r#"
            Struct.new(:foo, keyword_init: nil)
                             ^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: nil`.
        "#});
    }

    #[test]
    fn flags_keyword_init_true_after_other_pair() {
        test::<RedundantStructKeywordInit>().expect_offense(indoc! {r#"
            Struct.new(:foo, x: 1, keyword_init: true)
                                   ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
        "#});
    }

    #[test]
    fn flags_cbase_struct() {
        test::<RedundantStructKeywordInit>().expect_offense(indoc! {r#"
            ::Struct.new(:foo, keyword_init: true)
                               ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
        "#});
    }

    #[test]
    fn flags_safe_navigation() {
        test::<RedundantStructKeywordInit>().expect_offense(indoc! {r#"
            Struct&.new(:foo, keyword_init: true)
                              ^^^^^^^^^^^^^^^^^^ Remove the redundant `keyword_init: true`.
        "#});
    }

    #[test]
    fn accepts_keyword_init_false() {
        test::<RedundantStructKeywordInit>().expect_no_offenses("Struct.new(:foo, keyword_init: false)\n");
    }

    #[test]
    fn accepts_keyword_init_false_with_sibling_true() {
        // RuboCop returns early if any `keyword_init: false` is present,
        // suppressing even sibling redundant pairs.
        test::<RedundantStructKeywordInit>()
            .expect_no_offenses("Struct.new(:foo, keyword_init: false, keyword_init: true)\n");
    }

    #[test]
    fn accepts_namespaced_struct() {
        test::<RedundantStructKeywordInit>()
            .expect_no_offenses("Foo::Struct.new(:foo, keyword_init: true)\n");
    }

    #[test]
    fn accepts_plain_struct_new() {
        test::<RedundantStructKeywordInit>().expect_no_offenses("Struct.new(:foo)\n");
    }

    #[test]
    fn accepts_no_hash_argument() {
        test::<RedundantStructKeywordInit>().expect_no_offenses("Struct.new(:foo, :bar)\n");
    }

    #[test]
    fn accepts_string_keyword_init_key() {
        // Upstream matches `(sym :keyword_init)` only; a string/rocket key
        // is a different shape and is not flagged.
        test::<RedundantStructKeywordInit>()
            .expect_no_offenses("Struct.new(:foo, \"keyword_init\" => true)\n");
    }

    #[test]
    fn accepts_non_struct_receiver() {
        test::<RedundantStructKeywordInit>()
            .expect_no_offenses("Klass.new(:foo, keyword_init: true)\n");
    }

    #[test]
    fn minimum_target_ruby_version_is_set() {
        use murphy_plugin_api::{Cop, RubyVersion};
        assert_eq!(
            <RedundantStructKeywordInit as Cop>::MINIMUM_TARGET_RUBY_VERSION,
            Some(RubyVersion::new(3, 2)),
        );
    }
}

murphy_plugin_api::submit_cop!(RedundantStructKeywordInit);
