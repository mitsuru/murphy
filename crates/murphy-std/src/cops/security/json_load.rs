//! `Security/JSONLoad` — flag `JSON.load` / `JSON.restore`, which deserialize
//! arbitrary Ruby objects and are a security risk. Prefer `JSON.parse`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Security/JSONLoad
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `insecure_json_load` matcher:
//!   `(send (const {nil? cbase} :JSON) ${:load :restore} ...
//!   !`(pair (sym :create_additions) _))`. Fires on `JSON.load(x)` /
//!   `JSON.restore(x)` and the `::JSON` (`cbase`) forms; `Foo::JSON.load`
//!   is not matched because `is_global_const` requires the top-level
//!   `JSON`. The offense is suppressed when a `create_additions:` keyword
//!   pair appears anywhere in the call's descendants — RuboCop uses a
//!   descendant search (the backtick `` ` ``), so even
//!   `JSON.load(foo(create_additions: true))` is excluded; Murphy walks
//!   `cx.descendants` to match this exactly (verified against standalone
//!   rubocop 1.87 ground truth). The pair value is a wildcard, so both
//!   `create_additions: true` and `create_additions: false` suppress the
//!   offense. The offense highlights the selector (`loc.name`), matching
//!   `node.loc.selector`. RuboCop's autocorrect (replace selector with
//!   `parse`) is `SafeAutoCorrect: false` / `@safety` unsafe, so it is
//!   deliberately NOT ported (report-only).
//! ```
//!
//! ## Matched shapes
//!
//! - `JSON.load(x)` / `JSON.restore(x)`
//! - `::JSON.load(x)` / `::JSON.restore(x)`
//!
//! ## Accepted (not flagged)
//!
//! - `JSON.parse('{}')` / `JSON.unsafe_load('{}')` — different method
//! - `JSON.load('{}', create_additions: true)` / `create_additions: false`
//! - `JSON.load(foo(create_additions: true))` — descendant `create_additions:`
//! - `obj.load('{}')` / `load('{}')` — receiver is not the top-level `JSON`
//! - `Foo::JSON.load('{}')` — namespaced const, not the top-level `JSON`
//!
//! ## Message
//!
//! `` Prefer `JSON.parse` over `JSON.<method>`. `` (matches RuboCop).

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

#[derive(Default)]
pub struct JSONLoad;

#[cop(
    name = "Security/JSONLoad",
    description = "Prefer usage of JSON.parse over JSON.load due to potential security issues.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl JSONLoad {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `${:load :restore}` — gate on the selector first.
        let Some(method) = cx.method_name(node) else {
            return;
        };
        if method != "load" && method != "restore" {
            return;
        }
        // `(const {nil? cbase} :JSON)` — top-level `JSON` or `::JSON`.
        // `is_global_const` matches both and excludes `Foo::JSON`.
        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };
        if !cx.is_global_const(receiver, "JSON") {
            return;
        }
        // `!`(pair (sym :create_additions) _)` — descendant search: any
        // `create_additions:` keyword pair anywhere below the call (with any
        // value) suppresses the offense.
        if has_create_additions(node, cx) {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            &format!("Prefer `JSON.parse` over `JSON.{method}`."),
            None,
        );
    }
}

/// True when a `create_additions:` keyword pair (`(pair (sym :create_additions)
/// _)`) appears anywhere in the call's descendants, matching RuboCop's
/// backtick descendant search. The pair value is unconstrained.
fn has_create_additions(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.descendants(node).into_iter().any(|child| {
        let NodeKind::Pair { key, .. } = *cx.kind(child) else {
            return false;
        };
        matches!(*cx.kind(key), NodeKind::Sym(sym) if cx.symbol_str(sym) == "create_additions")
    })
}

murphy_plugin_api::submit_cop!(JSONLoad);

#[cfg(test)]
mod tests {
    use super::JSONLoad;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_json_load() {
        test::<JSONLoad>().expect_offense(indoc! {r#"
            JSON.load('{}')
                 ^^^^ Prefer `JSON.parse` over `JSON.load`.
        "#});
    }

    #[test]
    fn flags_json_restore() {
        test::<JSONLoad>().expect_offense(indoc! {r#"
            JSON.restore('{}')
                 ^^^^^^^ Prefer `JSON.parse` over `JSON.restore`.
        "#});
    }

    #[test]
    fn flags_cbase_json_load() {
        test::<JSONLoad>().expect_offense(indoc! {r#"
            ::JSON.load('{}')
                   ^^^^ Prefer `JSON.parse` over `JSON.load`.
        "#});
    }

    #[test]
    fn accepts_json_parse() {
        test::<JSONLoad>().expect_no_offenses("JSON.parse('{}')\n");
    }

    #[test]
    fn accepts_json_unsafe_load() {
        test::<JSONLoad>().expect_no_offenses("JSON.unsafe_load('{}')\n");
    }

    #[test]
    fn accepts_create_additions_true() {
        test::<JSONLoad>().expect_no_offenses("JSON.load('{}', create_additions: true)\n");
    }

    #[test]
    fn accepts_create_additions_false() {
        // The matcher's pair value is a wildcard `_`, so `false` also
        // suppresses the offense (RuboCop @example lists both as "good").
        test::<JSONLoad>().expect_no_offenses("JSON.load('{}', create_additions: false)\n");
    }

    #[test]
    fn accepts_nested_create_additions() {
        // RuboCop uses a descendant search (`` ` ``), so a `create_additions:`
        // pair nested inside a sub-call still suppresses the offense.
        test::<JSONLoad>().expect_no_offenses("JSON.load(foo(create_additions: true))\n");
    }

    #[test]
    fn accepts_other_receiver() {
        test::<JSONLoad>().expect_no_offenses("obj.load('{}')\n");
    }

    #[test]
    fn accepts_implicit_receiver() {
        test::<JSONLoad>().expect_no_offenses("load('{}')\n");
    }

    #[test]
    fn accepts_namespaced_json_const() {
        test::<JSONLoad>().expect_no_offenses("Foo::JSON.load('{}')\n");
    }
}
