//! `Security/MarshalLoad` — flag `Marshal.load` / `Marshal.restore` with
//! arbitrary input.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Security/MarshalLoad
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `def_node_matcher :marshal_load`:
//!   `(send (const {nil? cbase} :Marshal) ${:load :restore}
//!   !(send (const {nil? cbase} :Marshal) :dump ...))`. The outer pattern has
//!   exactly three positional terms after `send` (receiver, method, one
//!   argument) with NO trailing `...`, so the cop fires ONLY when there is
//!   exactly one argument. `Marshal.load` (zero args) and
//!   `Marshal.load(a, b)` (two args) do NOT match — verified against rubocop
//!   1.87.0. The single argument must NOT be a `Marshal.dump(...)` call (the
//!   `!(...)` negation), so the deep-copy idiom
//!   `Marshal.load(Marshal.dump(x))` is accepted. The inner `dump` receiver
//!   must itself be the `Marshal` const (`{nil? cbase}`), matched via
//!   `is_global_const` which covers both `Marshal` and `::Marshal` (Murphy
//!   normalises `::Marshal` to a scope-less `Const`). The dump-argument check
//!   does NOT unwrap parentheses: RuboCop's node pattern matches a bare `dump`
//!   send, so a redundantly-parenthesised argument
//!   (`Marshal.load((Marshal.dump(x)))`) is a `begin` node, the `!(...)`
//!   negation succeeds, and RuboCop fires — verified against rubocop 1.87.0.
//!   The offense highlights the selector (`loc.name`), matching
//!   `node.loc.selector`. No autocorrect (parity with RuboCop).
//! ```
//!
//! ## Matched shapes
//!
//! - `Marshal.load(data)` / `Marshal.restore(data)`
//! - `::Marshal.load(data)` / `::Marshal.restore(data)`
//!
//! ## Accepted (not flagged)
//!
//! - `Marshal.dump("{}")` — `dump` is not a restricted method
//! - `Marshal.load(Marshal.dump({}))` — deep-copy idiom (arg is `Marshal.dump`)
//! - `Marshal.load` — zero args (pattern requires exactly one)
//! - `Marshal.load(a, b)` — two args (pattern requires exactly one)
//! - `obj.load(data)` — receiver is not the `Marshal` const
//!
//! ## Message
//!
//! `` Avoid using `Marshal.load`. `` / `` Avoid using `Marshal.restore`. ``
//! (matches RuboCop).

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId};

#[derive(Default)]
pub struct MarshalLoad;

#[cop(
    name = "Security/MarshalLoad",
    description = "Avoid using of Marshal.load or Marshal.restore due to potential security issues.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MarshalLoad {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `${:load :restore}` — restricted selectors.
        let method = match cx.method_name(node) {
            Some(m @ ("load" | "restore")) => m,
            _ => return,
        };
        // `(const {nil? cbase} :Marshal)` — `Marshal` / `::Marshal`.
        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };
        if !cx.is_global_const(receiver, "Marshal") {
            return;
        }
        // The outer pattern has no trailing `...`, so it matches exactly one
        // argument. Zero-arg and multi-arg calls do NOT fire.
        let args = cx.call_arguments(node);
        let [arg] = args else {
            return;
        };
        // `!(send (const {nil? cbase} :Marshal) :dump ...)` — the deep-copy
        // idiom `Marshal.load(Marshal.dump(...))` is accepted.
        if is_marshal_dump(*arg, cx) {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            &format!("Avoid using `Marshal.{method}`."),
            None,
        );
    }
}

/// True when `node` is a `Marshal.dump(...)` / `::Marshal.dump(...)` call with
/// any (or no) arguments, mirroring the inner pattern
/// `(send (const {nil? cbase} :Marshal) :dump ...)`.
///
/// Note: RuboCop's node pattern does NOT unwrap parentheses around the
/// argument, so a redundantly-parenthesised arg (`Marshal.load((Marshal.dump
/// (x)))`) is a `begin` node — not a bare `dump` send — and the `!(...)`
/// negation succeeds, making RuboCop fire. We deliberately do not unwrap here
/// to preserve exact parity (verified against rubocop 1.87.0).
fn is_marshal_dump(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.method_name(node) != Some("dump") {
        return false;
    }
    let Some(receiver) = cx.call_receiver(node).get() else {
        return false;
    };
    cx.is_global_const(receiver, "Marshal")
}

murphy_plugin_api::submit_cop!(MarshalLoad);

#[cfg(test)]
mod tests {
    use super::MarshalLoad;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_marshal_load() {
        test::<MarshalLoad>().expect_offense(indoc! {r#"
            Marshal.load(data)
                    ^^^^ Avoid using `Marshal.load`.
        "#});
    }

    #[test]
    fn flags_marshal_restore() {
        test::<MarshalLoad>().expect_offense(indoc! {r#"
            Marshal.restore(data)
                    ^^^^^^^ Avoid using `Marshal.restore`.
        "#});
    }

    #[test]
    fn flags_cbase_marshal_load() {
        test::<MarshalLoad>().expect_offense(indoc! {r#"
            ::Marshal.load(data)
                      ^^^^ Avoid using `Marshal.load`.
        "#});
    }

    #[test]
    fn flags_cbase_marshal_restore() {
        test::<MarshalLoad>().expect_offense(indoc! {r#"
            ::Marshal.restore(data)
                      ^^^^^^^ Avoid using `Marshal.restore`.
        "#});
    }

    #[test]
    fn accepts_marshal_dump() {
        test::<MarshalLoad>().expect_no_offenses("Marshal.dump(\"{}\")\n");
    }

    #[test]
    fn accepts_deep_copy_idiom() {
        // `Marshal.load(Marshal.dump(x))` — the `!(... :dump ...)` exception.
        test::<MarshalLoad>().expect_no_offenses("Marshal.load(Marshal.dump({}))\n");
    }

    #[test]
    fn accepts_deep_copy_idiom_cbase() {
        test::<MarshalLoad>().expect_no_offenses("Marshal.load(::Marshal.dump(x))\n");
    }

    #[test]
    fn flags_double_parenthesized_deep_copy_idiom() {
        // RuboCop's pattern matches a bare `dump` send and does NOT unwrap
        // parens, so a redundantly-parenthesised `((Marshal.dump(x)))` arg is a
        // `begin` node and the cop fires — verified against rubocop 1.87.0.
        test::<MarshalLoad>().expect_offense(indoc! {r#"
            Marshal.load((Marshal.dump(x)))
                    ^^^^ Avoid using `Marshal.load`.
        "#});
    }

    #[test]
    fn accepts_zero_args() {
        // Pattern requires exactly one argument — verified against rubocop 1.87.0.
        test::<MarshalLoad>().expect_no_offenses("Marshal.load\n");
    }

    #[test]
    fn accepts_multiple_args() {
        // Pattern has no trailing `...` — verified against rubocop 1.87.0.
        test::<MarshalLoad>().expect_no_offenses("Marshal.load(a, b)\n");
    }

    #[test]
    fn accepts_other_receiver() {
        test::<MarshalLoad>().expect_no_offenses("obj.load(data)\n");
    }

    #[test]
    fn accepts_implicit_receiver() {
        test::<MarshalLoad>().expect_no_offenses("load(data)\n");
    }

    #[test]
    fn accepts_non_restricted_method() {
        test::<MarshalLoad>().expect_no_offenses("Marshal.foo(data)\n");
    }
}
