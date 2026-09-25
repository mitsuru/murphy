//! `Lint/DuplicateRequire` — flag a `require`/`require_relative` of a path
//! that was already required earlier in the same statement sequence.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DuplicateRequire
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   RuboCop accumulates required paths per `node.parent` (identity-keyed) on
//!   `on_send`. Murphy cops are `&self`-only and run in parallel, so we cannot
//!   accumulate per-file state. Instead we dispatch on the container (`begin`,
//!   which is also the program root for a multi-statement file) and dedup the
//!   direct `require`/`require_relative` children by `method + first_argument
//!   source`. Each require has exactly one parent `begin`, so scoping to direct
//!   children reproduces RuboCop's per-parent keying exactly: a require inside a
//!   nested block/begin is a child of *that* `begin`, not the outer one. Both a
//!   bare receiver (`require`) and a `Kernel` receiver (`Kernel.require`) are
//!   matched, mirroring RuboCop's `{nil? (const _ :Kernel)}` pattern. Autocorrect
//!   removes the whole duplicate line including its trailing newline (unsafe in
//!   RuboCop because it may reorder dependencies).
//! ```

use std::collections::HashSet;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, def_node_matcher};

// RuboCop parity: `Lint/DuplicateRequire` receiver guards are bare (`nil?`)
// and `Kernel` top-level (`(const {nil? cbase} :Kernel)`), methods
// `:require` / `:require_relative`.
// In Murphy `::Kernel` collapses to `Const{scope:None}`: `nil?` covers bare +
// `::` for `Kernel` (namespaced `Foo::Kernel` still accepts — pinned by
// `boundary_accepts_namespaced_kernel_require`). `call` covers both `Send`
// and `Csend` (pinned by `boundary_flags_csend_kernel_require`), matching the
// generic `method_name` + `call_receiver` dispatch below. Argument dedup
// (`method + first_argument source`) stays hand-rolled below.
def_node_matcher!(
    require_bare,
    "(call nil? {:require :require_relative} ...)"
);
def_node_matcher!(
    require_kernel,
    "(call (const nil? :Kernel) {:require :require_relative} ...)"
);

#[derive(Default)]
pub struct DuplicateRequire;

#[cop(
    name = "Lint/DuplicateRequire",
    description = "Flag duplicate `require`/`require_relative` statements.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicateRequire {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Begin(list) = *cx.kind(node) else {
            return;
        };
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for &child in cx.list(list) {
            let Some(method) = require_method(cx, child) else {
                continue;
            };
            let [arg] = cx.call_arguments(child) else {
                continue;
            };
            let key = (method.to_string(), cx.raw_source(cx.range(*arg)).to_string());
            if !seen.insert(key) {
                let message = format!("Duplicate `{method}` detected.");
                cx.emit_offense(cx.range(child), &message, None);
                // Autocorrect: remove the whole duplicate line incl. its newline.
                cx.emit_edit(cx.range_by_whole_lines(cx.range(child), true), "");
            }
        }
    }
}

/// Returns the require-method name (`require` / `require_relative`) if `node`
/// is a bare or `Kernel`-receiver call to one of them, else `None`.
fn require_method<'a>(cx: &Cx<'a>, node: NodeId) -> Option<&'a str> {
    // `(call nil? {:require :require_relative} ...)` (bare) or
    // `(call (const nil? :Kernel) {:require :require_relative} ...)`
    // (`Kernel` / `::Kernel`, top-level only). `call` covers `Send` + `Csend`.
    if !require_bare(node, cx) && !require_kernel(node, cx) {
        return None;
    }
    // The matcher already constrains the method to `require` /
    // `require_relative`; return it for the dedup key.
    cx.method_name(node)
}

murphy_plugin_api::submit_cop!(DuplicateRequire);

#[cfg(test)]
mod tests {
    use super::DuplicateRequire;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_require() {
        test::<DuplicateRequire>().expect_offense(indoc! {r#"
            require 'foo'
            require 'bar'
            require 'foo'
            ^^^^^^^^^^^^^ Duplicate `require` detected.
        "#});
    }

    #[test]
    fn flags_duplicate_require_relative() {
        test::<DuplicateRequire>().expect_offense(indoc! {r#"
            require_relative 'foo'
            require_relative 'foo'
            ^^^^^^^^^^^^^^^^^^^^^^ Duplicate `require_relative` detected.
        "#});
    }

    #[test]
    fn allows_distinct_requires() {
        test::<DuplicateRequire>().expect_no_offenses(indoc! {r#"
            require 'foo'
            require 'bar'
        "#});
    }

    #[test]
    fn require_and_require_relative_of_same_path_are_distinct() {
        test::<DuplicateRequire>().expect_no_offenses(indoc! {r#"
            require 'foo'
            require_relative 'foo'
        "#});
    }

    #[test]
    fn flags_kernel_require() {
        test::<DuplicateRequire>().expect_offense(indoc! {r#"
            Kernel.require 'foo'
            Kernel.require 'foo'
            ^^^^^^^^^^^^^^^^^^^^ Duplicate `require` detected.
        "#});
    }

    #[test]
    fn same_path_in_different_scopes_is_not_duplicate() {
        // Each `begin` (here: the top level and the method body) tracks its own
        // requires, mirroring RuboCop's per-parent keying.
        test::<DuplicateRequire>().expect_no_offenses(indoc! {r#"
            require 'foo'
            def setup
              require 'foo'
            end
        "#});
    }

    #[test]
    fn autocorrects_by_removing_duplicate_line() {
        test::<DuplicateRequire>().expect_correction(
            indoc! {r#"
                require 'foo'
                require 'bar'
                require 'foo'
                ^^^^^^^^^^^^^ Duplicate `require` detected.
            "#},
            indoc! {r#"
                require 'foo'
                require 'bar'
            "#},
        );
    }

    // --- Boundary characterization (murphy-ft88.7): pin the exact node set
    // the hand-rolled `require` guard matches, so the verbatim
    // `(call nil? {:require :require_relative} ...)` +
    // `(call (const nil? :Kernel) {:require :require_relative} ...)` refactor
    // can be proven equivalent. `::Kernel` collapses to `Const{scope:None}`:
    // `nil?` covers bare + `::`. `call` covers `Send` + `Csend`.

    #[test]
    fn boundary_flags_cbase_kernel_require() {
        // `::Kernel` collapses to scope-less Const: `nil?` covers bare + `::`.
        test::<DuplicateRequire>().expect_offense(indoc! {r#"
            ::Kernel.require 'foo'
            ::Kernel.require 'foo'
            ^^^^^^^^^^^^^^^^^^^^^^ Duplicate `require` detected.
        "#});
    }

    #[test]
    fn boundary_accepts_namespaced_kernel_require() {
        // `Foo::Kernel` is scoped: `is_global_const` rejects, no dedup.
        test::<DuplicateRequire>().expect_no_offenses(indoc! {r#"
            Foo::Kernel.require 'foo'
            Foo::Kernel.require 'foo'
        "#});
    }

    #[test]
    fn boundary_flags_csend_kernel_require() {
        // `&.` is `csend`; `call` covers `Send` + `Csend`, and the
        // hand-rolled `method_name` + `call_receiver` is generic over both.
        test::<DuplicateRequire>().expect_offense(indoc! {r#"
            Kernel&.require 'foo'
            Kernel&.require 'foo'
            ^^^^^^^^^^^^^^^^^^^^^ Duplicate `require` detected.
        "#});
    }
}
