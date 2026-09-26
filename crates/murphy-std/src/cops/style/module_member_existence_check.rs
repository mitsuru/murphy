//! `Style/ModuleMemberExistenceCheck` — prefer predicate methods over inclusion checks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ModuleMemberExistenceCheck
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Verbatim port of the outer call head `(call _ {:include? :member?} ...)`
//!   (murphy-s1yc.14): `call` covers safe-navigation
//!   (`Array.instance_methods&.include?`), mirroring RuboCop `alias on_csend
//!   on_send` plus `module_member_inclusion?` outer
//!   `(call {...} {:include? :member?} _)`; the wildcard receiver binds an
//!   absent or present receiver per murphy-if9y; trailing `...` absorbs any
//!   argument list. The receiver plus inner-method plus inner-no-args plus
//!   outer-non-empty guards below apply separately (upstream inner is
//!   `(call _ %METHODS...)` covering csend with optional inherit arg;
//!   murphy preserves Send-or-Csend inner but empty-only plus
//!   instance-methods-only plus offense-only (no autocorrect) as
//!   complementary gaps). v1 gaps: other method pairs (class_variables,
//!   etc.) not handled; inner inherit param (`instance_methods(false)`)
//!   accepts; no autocorrect to `method_defined?`.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, def_node_matcher};

// Verbatim port of the outer call head (murphy-s1yc.14):
// `(call _ {:include? :member?} ...)` — `call` = `{send csend}` covers
// safe-navigation (`Array.instance_methods&.include?`), mirroring RuboCop
// `alias on_csend on_send` plus `module_member_inclusion?` outer
// `(call {...} {:include? :member?} _)`. The `_` receiver binds an absent or
// present receiver per murphy-if9y; trailing `...` absorbs any argument list,
// so the receiver plus inner-method plus inner-no-args plus outer-non-empty
// guards below apply separately (upstream inner covers csend with optional
// inherit arg and has autocorrect; murphy preserves empty-only inner plus
// offense-only as complementary gaps).
def_node_matcher!(
    module_member_outer_call,
    "(call _ {:include? :member?} ...)"
);

const MSG: &str = "Use `method_defined?` instead.";

#[derive(Default)]
pub struct ModuleMemberExistenceCheck;

#[cop(
    name = "Style/ModuleMemberExistenceCheck",
    description = "Use predicate methods instead of inclusion checks on Module methods.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ModuleMemberExistenceCheck {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Verbatim `(call _ {:include? :member?} ...)` head: filters to
    // `include?` / `member?` calls on either send or csend
    // (safe-navigation), with any receiver (absent or present). Without
    // this, an unrelated call over an `instance_methods` receiver (e.g.
    // `Array.instance_methods.sort?(:size)`) would run check on every call
    // node instead of being rejected by the method set up front.
    if !module_member_outer_call(node, cx) {
        return;
    }

    // Must have a receiver. Mirrors the pre-port hand-rolled guard;
    // `_` binds an absent receiver, so bare `include?(:size)` is accepted
    // here (upstream requires an inner call receiver, so bare also
    // accepts upstream; murphy preserves bare-accept).
    let Some(recv_id) = cx.call_receiver(node).get() else {
        return;
    };
    let recv_id = unwrap_begin(recv_id, cx);
    // Inner must be an `instance_methods` call (send or csend, mirroring
    // upstream inner `(call _ %METHODS...)` which covers safe-navigation).
    // Other pairs (`class_variables`, etc.) stay as v1 gaps.
    if !matches!(
        cx.kind(recv_id),
        NodeKind::Send { .. } | NodeKind::Csend { .. }
    ) {
        return;
    }
    if cx.method_name(recv_id) != Some("instance_methods") {
        return;
    }
    // Inner must take no arguments. Mirrors the pre-port hand-rolled guard;
    // upstream allows an optional inherit arg (`instance_methods(false)`),
    // murphy preserves inherit-param-accept as a complementary gap.
    if !cx.call_arguments(recv_id).is_empty() {
        return;
    }
    // Trailing `...` absorbs any argument list, so the no-argument case
    // (`Array.instance_methods.include?`) is accepted here (upstream
    // requires a single arg; murphy preserves empty-accept).
    if cx.call_arguments(node).is_empty() {
        return;
    }
    cx.emit_offense(cx.range(node), MSG, None);
}

fn unwrap_begin(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    while let NodeKind::Begin(children) = cx.kind(node) {
        let child_list = cx.list(*children);
        if child_list.len() != 1 {
            break;
        }
        node = child_list[0];
    }
    node
}

#[cfg(test)]
mod tests {
    use super::ModuleMemberExistenceCheck;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_instance_methods_include() {
        test::<ModuleMemberExistenceCheck>().expect_offense(indoc! {"
            Array.instance_methods.include?(:size)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `method_defined?` instead.
        "});
    }

    #[test]
    fn flags_parenthesized_instance_methods_include() {
        test::<ModuleMemberExistenceCheck>().expect_offense(indoc! {"
            (Array.instance_methods).include?(:size)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `method_defined?` instead.
        "});
    }

    #[test]
    fn flags_instance_methods_member() {
        test::<ModuleMemberExistenceCheck>().expect_offense(indoc! {"
            Array.instance_methods.member?(:size)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `method_defined?` instead.
        "});
    }

    #[test]
    fn accepts_plain_instance_methods() {
        test::<ModuleMemberExistenceCheck>().expect_no_offenses(
            "Array.instance_methods\n",
        );
    }

    #[test]
    fn accepts_method_defined() {
        test::<ModuleMemberExistenceCheck>().expect_no_offenses(
            "Array.method_defined?(:size)\n",
        );
    }

    #[test]
    fn accepts_instance_methods_with_arg() {
        test::<ModuleMemberExistenceCheck>().expect_no_offenses(
            "Array.instance_methods(false).include?(:size)\n",
        );
    }

    // --- Characterization (murphy-s1yc.14): pin the exact node set the
    // hand-rolled send-only dispatch matches, so the verbatim
    // `(call _ {:include? :member?} ...)` port can be proven byte-identical.
    // `call` covers safe-navigation (mirroring upstream `alias on_csend
    // on_send` plus `module_member_inclusion?` outer); trailing `...`
    // absorbs any argument list, so the receiver plus inner-method plus
    // inner-no-args plus outer-non-empty guards below apply separately
    // (upstream inner covers csend with optional inherit arg and has
    // autocorrect; murphy preserves empty-only inner plus offense-only).

    #[test]
    fn s1yc14_flags_csend_outer() {
        // Safe navigation on the outer call: `call` covers `csend` per
        // murphy-if9y, mirroring upstream `alias on_csend on_send`. (Pre-port
        // the `csend` handler is missing: `check_include_member`
        // destructures `NodeKind::Send` only and
        // `#[on_node(kind = "send", methods = [...])]` never visits csend.)
        test::<ModuleMemberExistenceCheck>().expect_offense(indoc! {"
            Array.instance_methods&.include?(:size)
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `method_defined?` instead.
        "});
    }

    #[test]
    fn s1yc14_accepts_bare_include() {
        // Bare receiver: `_` binds an absent receiver per murphy-if9y, so
        // the head matches and the complementary receiver guard accepts.
        // (Upstream also accepts bare: it requires an inner `instance_methods`
        // call receiver.)
        test::<ModuleMemberExistenceCheck>().expect_no_offenses("include?(:size)\n");
    }

    #[test]
    fn s1yc14_accepts_no_arg_outer() {
        // No arguments: trailing `...` absorbs the empty list, so the head
        // matches and the complementary non-empty-arg guard accepts.
        // (Upstream requires a single arg.)
        test::<ModuleMemberExistenceCheck>().expect_no_offenses("Array.instance_methods.include?\n");
    }

    #[test]
    fn s1yc14_accepts_unrelated_outer_method() {
        // `sort?` is outside the verbatim method set, so the head rejects.
        test::<ModuleMemberExistenceCheck>().expect_no_offenses("Array.instance_methods.sort?(:size)\n");
    }
}
murphy_plugin_api::submit_cop!(ModuleMemberExistenceCheck);
