//! `Gemspec/AddRuntimeDependency` — prefer `add_dependency` over
//! `add_runtime_dependency` in a gemspec. The cop runs only on `*.gemspec`
//! files; the host applies the per-cop `Include` from `config/default.yml`, so
//! this cop never inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Gemspec/AddRuntimeDependency
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop v1.87.0 (`gemspec/add_runtime_dependency.rb`) exactly.
//!   `RESTRICT_ON_SEND = %i[add_runtime_dependency]`; `on_send` body is
//!   `return if !node.receiver || node.arguments.empty?` then
//!   `add_offense(node.loc.selector) { |c| c.replace(node.loc.selector,
//!   'add_dependency') }`. So the offense fires only on a `Send` (not a
//!   safe-navigation `&.` call — RuboCop's `on_send` does not dispatch on
//!   csend, and Murphy parses `spec&.add_runtime_dependency` as a distinct
//!   `CSend` node that `#[on_node(kind = "send")]` does not match) whose
//!   selector is `add_runtime_dependency`, that HAS a receiver, and that HAS at
//!   least one argument.
//!
//!   Both the offense range and the autocorrect target are `node.loc.selector`
//!   (Murphy's `cx.node(node).loc.name`): the caret underlines only the
//!   `add_runtime_dependency` selector, not the receiver or arguments, and the
//!   fix replaces just that selector with `add_dependency` (receiver and args
//!   pass through byte-for-byte). Message is RuboCop's `MSG` verbatim:
//!   ``Use `add_dependency` instead of `add_runtime_dependency`.``
//!
//!   `Enabled: pending` in `config/default.yml` → `default_enabled = false`.
//!   Behaviour, message text, selector caret column, and autocorrect output all
//!   verified against standalone rubocop 1.87.0 on a sample gemspec
//!   (`spec.add_runtime_dependency "rake"` flags at the selector;
//!   `spec&.add_runtime_dependency`, bare `add_runtime_dependency "x"`, and
//!   `spec.add_runtime_dependency` with no args do not flag).
//!
//!   The bd issue note "Safe: safe / no-autocorrect" is reconciled here:
//!   "safe" is correct, but "no-autocorrect" is wrong — the 1.87.0 source
//!   `extend AutoCorrector` and ships a selector-rename correction, which this
//!   port reproduces.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct AddRuntimeDependency;

const MSG: &str = "Use `add_dependency` instead of `add_runtime_dependency`.";

#[cop(
    name = "Gemspec/AddRuntimeDependency",
    description = "Prefer `add_dependency` over `add_runtime_dependency`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl AddRuntimeDependency {
    // `methods = ["add_runtime_dependency"]` mirrors upstream
    // `RESTRICT_ON_SEND = %i[add_runtime_dependency]`. Dispatching on
    // `kind = "send"` excludes the safe-navigation `&.` form (a `CSend`
    // node), matching RuboCop's `on_send`, which does not fire on csend.
    #[on_node(kind = "send", methods = ["add_runtime_dependency"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop: `return if !node.receiver || node.arguments.empty?`.
        if cx.call_receiver(node).get().is_none() {
            return;
        }
        if cx.call_arguments(node).is_empty() {
            return;
        }

        // Both offense range and autocorrect target are the selector
        // (`node.loc.selector`). `loc.name` is the parser-gem-style selector
        // range, so this is exactly the `add_runtime_dependency` bytes.
        let selector = cx.node(node).loc.name;
        cx.emit_offense(selector, MSG, None);
        cx.emit_edit(selector, "add_dependency");
    }
}

murphy_plugin_api::submit_cop!(AddRuntimeDependency);

#[cfg(test)]
mod tests {
    use super::AddRuntimeDependency;
    use murphy_plugin_api::test_support::{indoc, test};

    // === hit cases ===

    #[test]
    fn flags_add_runtime_dependency_with_receiver_and_arg() {
        test::<AddRuntimeDependency>().expect_offense(indoc! {r#"
            spec.add_runtime_dependency "rake"
                 ^^^^^^^^^^^^^^^^^^^^^^ Use `add_dependency` instead of `add_runtime_dependency`.
        "#});
    }

    #[test]
    fn flags_add_runtime_dependency_with_parens_and_version() {
        test::<AddRuntimeDependency>().expect_offense(indoc! {r#"
            spec.add_runtime_dependency("rspec", "~> 3.0")
                 ^^^^^^^^^^^^^^^^^^^^^^ Use `add_dependency` instead of `add_runtime_dependency`.
        "#});
    }

    #[test]
    fn flags_add_runtime_dependency_inside_block() {
        test::<AddRuntimeDependency>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.add_runtime_dependency "rake"
                   ^^^^^^^^^^^^^^^^^^^^^^ Use `add_dependency` instead of `add_runtime_dependency`.
            end
        "#});
    }

    // === no-hit cases ===

    #[test]
    fn does_not_flag_add_dependency() {
        // Already the recommended form.
        test::<AddRuntimeDependency>().expect_no_offenses("spec.add_dependency \"rake\"\n");
    }

    #[test]
    fn does_not_flag_bare_call_without_receiver() {
        // RuboCop's `return if !node.receiver`: no receiver → no offense.
        test::<AddRuntimeDependency>().expect_no_offenses("add_runtime_dependency \"rake\"\n");
    }

    #[test]
    fn does_not_flag_without_arguments() {
        // RuboCop's `return if ... node.arguments.empty?`: no args → no offense.
        test::<AddRuntimeDependency>().expect_no_offenses("spec.add_runtime_dependency\n");
    }

    #[test]
    fn does_not_flag_safe_navigation() {
        // `spec&.add_runtime_dependency "x"` parses as a `CSend`, not `Send`.
        // RuboCop's `on_send` does not dispatch on csend, so neither do we.
        test::<AddRuntimeDependency>().expect_no_offenses("spec&.add_runtime_dependency \"rake\"\n");
    }

    // === autocorrect ===

    #[test]
    fn corrects_selector_preserving_receiver_and_arg() {
        test::<AddRuntimeDependency>().expect_correction(
            indoc! {r#"
                spec.add_runtime_dependency "rake"
                     ^^^^^^^^^^^^^^^^^^^^^^ Use `add_dependency` instead of `add_runtime_dependency`.
            "#},
            "spec.add_dependency \"rake\"\n",
        );
    }

    #[test]
    fn corrects_selector_preserving_parens_and_version() {
        test::<AddRuntimeDependency>().expect_correction(
            indoc! {r#"
                spec.add_runtime_dependency("rspec", "~> 3.0")
                     ^^^^^^^^^^^^^^^^^^^^^^ Use `add_dependency` instead of `add_runtime_dependency`.
            "#},
            "spec.add_dependency(\"rspec\", \"~> 3.0\")\n",
        );
    }

    #[test]
    fn correction_reaches_fixpoint() {
        // After renaming the selector to `add_dependency`, re-running the cop
        // produces zero offenses (idempotent).
        test::<AddRuntimeDependency>().expect_no_offenses("spec.add_dependency \"rake\"\n");
    }
}
