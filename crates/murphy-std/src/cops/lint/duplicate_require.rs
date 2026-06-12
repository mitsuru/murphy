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

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

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
    let method = cx.method_name(node)?;
    if method != "require" && method != "require_relative" {
        return None;
    }
    match cx.call_receiver(node).get() {
        None => Some(method),
        Some(receiver) if cx.is_global_const(receiver, "Kernel") => Some(method),
        Some(_) => None,
    }
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
}
