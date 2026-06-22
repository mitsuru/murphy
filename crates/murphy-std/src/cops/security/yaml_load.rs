//! `Security/YAMLLoad` — flag `YAML.load` and recommend `YAML.safe_load`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Security/YAMLLoad
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's `def_node_matcher :yaml_load`:
//!   `(send (const {nil? cbase} :YAML) :load ...)`. Fires on `YAML.load(x)`
//!   and `::YAML.load(x)` — Murphy normalises `::YAML` to a scope-less
//!   `Const`, so `is_global_const(receiver, "YAML")` matches both forms and
//!   correctly rejects nested constants like `Foo::YAML`. `RESTRICT_ON_SEND
//!   = %i[load]` maps to `methods = ["load"]`. The `...` in the pattern means
//!   zero-or-more arguments, so bare `YAML.load` (no parens) also fires; we
//!   do NOT gate on argument presence. Offense highlights the `load` selector
//!   (`loc.name`), matching `node.loc.selector`. Message is the upstream MSG
//!   constant verbatim: ``Prefer using `YAML.safe_load` over `YAML.load`.``
//!
//!   Autocorrect: single surgical selector rename `load` -> `safe_load`.
//!   Upstream config marks this `SafeAutoCorrect: false`, but Murphy does not
//!   model safe/unsafe autocorrect — users opt in via `--fix`, so the rewrite
//!   ships (precedent: `Rails/NegateInclude`). Fixpoint is automatic: the
//!   renamed selector `safe_load` no longer matches `methods = ["load"]`.
//!
//!   TARGET RUBY: RuboCop declares `maximum_target_ruby_version 3.0` on this
//!   cop — it runs ONLY when the target Ruby is <= 3.0, because Psych defaults
//!   to safe behaviour from Ruby 3.1. Murphy's `#[cop]` macro has no
//!   maximum-version gate (only `minimum_target_ruby_version`), so this is
//!   enforced at runtime by a `cx.target_ruby_version()` guard in `check_send`:
//!   the cop fires only when the resolved target Ruby is <= 3.0. With murphy's
//!   default target (3.1) — and when the target is unset — it stays silent,
//!   matching RuboCop on a default codebase. Follow-up murphy-n0ua tracks
//!   adding a `maximum_target_ruby_version` macro attribute so this can move to
//!   host-side gating like the minimum gate.
//! ```
//!
//! ## Matched shapes
//!
//! - `YAML.load(x)`
//! - `::YAML.load(x)`
//! - `YAML.load` (no arguments — `...` matches zero args)
//!
//! ## Accepted (not flagged)
//!
//! - `YAML.safe_load(x)` — already the recommended form (also the fixpoint
//!   target)
//! - `YAML.dump(x)` — wrong method
//! - `Foo::YAML.load(x)` — receiver is a nested const, not `{nil? cbase}`
//! - `obj.load(x)` — receiver is not the `YAML` constant
//!
//! ## Message
//!
//! `` Prefer using `YAML.safe_load` over `YAML.load`. `` (matches RuboCop).

use murphy_plugin_api::{Cx, NoOptions, NodeId, RubyVersion, cop};

#[derive(Default)]
pub struct YamlLoad;

#[cop(
    name = "Security/YAMLLoad",
    description = "Prefer usage of `YAML.safe_load` over `YAML.load` due to potential security issues.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl YamlLoad {
    // `methods = ["load"]` mirrors upstream `RESTRICT_ON_SEND = %i[load]` —
    // dispatch only on `load` sends. The receiver check is the parity surface.
    #[on_node(kind = "send", methods = ["load"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop declares `maximum_target_ruby_version 3.0`: the cop runs only
        // when the target Ruby is <= 3.0, because Psych is safe-by-default from
        // Ruby 3.1. Murphy's `#[cop]` macro has no maximum-version gate (only
        // `minimum_target_ruby_version`, host-gated in the registry), so we gate
        // at runtime via `cx.target_ruby_version()`. `None` (unset) resolves to
        // murphy's default floor (Ruby 3.1), which is above the max, so it does
        // not fire — matching RuboCop on a default codebase. murphy-n0ua tracks
        // adding a `maximum_target_ruby_version` macro attribute so this can
        // move to host-side gating like the minimum gate.
        if !matches!(cx.target_ruby_version(), Some(v) if v <= RubyVersion::new(3, 0)) {
            return;
        }
        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };
        // `(const {nil? cbase} :YAML)` — `YAML` / `::YAML`. Murphy normalises
        // `::YAML` to a scope-less `Const`, so `is_global_const` matches both
        // and rejects nested consts (`Foo::YAML`).
        if !cx.is_global_const(receiver, "YAML") {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            "Prefer using `YAML.safe_load` over `YAML.load`.",
            None,
        );
        // Autocorrect: rename the selector `load` -> `safe_load`. `loc.name`
        // is the parser-gem-style selector range, so this overwrites exactly
        // the four bytes of `load`. Receiver and argument source pass through
        // untouched. The renamed call no longer matches `methods = ["load"]`,
        // so the rewrite reaches fixpoint on the next pass.
        cx.emit_edit(cx.node(node).loc.name, "safe_load");
    }
}

murphy_plugin_api::submit_cop!(YamlLoad);

#[cfg(test)]
mod tests {
    use super::YamlLoad;
    use murphy_plugin_api::test_support::{indoc, test};

    // RuboCop's `maximum_target_ruby_version 3.0` means this cop only runs when
    // the target Ruby is <= 3.0. Every firing / autocorrect case therefore pins
    // `with_target_ruby_version(3, 0)`; the no-hit receiver/method cases also
    // pin 3.0 so the version gate doesn't mask the real discrimination.

    // === target-ruby gate ===

    #[test]
    fn silent_on_default_target() {
        // No target set → murphy's default floor (Ruby 3.1) → above the
        // `maximum_target_ruby_version 3.0` gate → no offense (RuboCop parity).
        test::<YamlLoad>().expect_no_offenses("YAML.load(arg)\n");
    }

    #[test]
    fn silent_on_ruby_3_1() {
        // Explicit Ruby 3.1 is above the max gate → no offense.
        test::<YamlLoad>()
            .with_target_ruby_version(3, 1)
            .expect_no_offenses("YAML.load(arg)\n");
    }

    // === hit cases (target Ruby <= 3.0) ===

    #[test]
    fn flags_yaml_load() {
        test::<YamlLoad>()
            .with_target_ruby_version(3, 0)
            .expect_offense(indoc! {r#"
            YAML.load(arg)
                 ^^^^ Prefer using `YAML.safe_load` over `YAML.load`.
        "#});
    }

    #[test]
    fn flags_cbase_yaml_load() {
        test::<YamlLoad>()
            .with_target_ruby_version(3, 0)
            .expect_offense(indoc! {r#"
            ::YAML.load(arg)
                   ^^^^ Prefer using `YAML.safe_load` over `YAML.load`.
        "#});
    }

    #[test]
    fn flags_bare_yaml_load_no_args() {
        // `...` matches zero-or-more args, so a no-arg `YAML.load` still fires.
        test::<YamlLoad>()
            .with_target_ruby_version(3, 0)
            .expect_offense(indoc! {r#"
            YAML.load
                 ^^^^ Prefer using `YAML.safe_load` over `YAML.load`.
        "#});
    }

    // === no-hit cases (target Ruby <= 3.0 so the gate doesn't mask them) ===

    #[test]
    fn accepts_safe_load() {
        // Already the recommended form (and the fixpoint target).
        test::<YamlLoad>()
            .with_target_ruby_version(3, 0)
            .expect_no_offenses("YAML.safe_load(arg)\n");
    }

    #[test]
    fn accepts_yaml_dump() {
        // Wrong method.
        test::<YamlLoad>()
            .with_target_ruby_version(3, 0)
            .expect_no_offenses("YAML.dump(arg)\n");
    }

    #[test]
    fn accepts_nested_const_yaml_load() {
        // `Foo::YAML` is `(const (const nil :Foo) :YAML)` — neither `nil?`
        // nor `cbase`, so RuboCop does NOT fire. Pins that `is_global_const`
        // rejects nested consts (no over-match).
        test::<YamlLoad>()
            .with_target_ruby_version(3, 0)
            .expect_no_offenses("Foo::YAML.load(arg)\n");
    }

    #[test]
    fn accepts_other_receiver_load() {
        // Receiver is not the `YAML` constant.
        test::<YamlLoad>()
            .with_target_ruby_version(3, 0)
            .expect_no_offenses("obj.load(arg)\n");
    }

    #[test]
    fn accepts_implicit_receiver_load() {
        // Bare `load(arg)` has a nil receiver — the pattern requires the
        // `YAML` const receiver.
        test::<YamlLoad>()
            .with_target_ruby_version(3, 0)
            .expect_no_offenses("load(arg)\n");
    }

    // === autocorrect (target Ruby <= 3.0) ===

    #[test]
    fn corrects_yaml_load() {
        test::<YamlLoad>()
            .with_target_ruby_version(3, 0)
            .expect_correction(
                indoc! {r#"
                YAML.load(arg)
                     ^^^^ Prefer using `YAML.safe_load` over `YAML.load`.
            "#},
                "YAML.safe_load(arg)\n",
            );
    }

    #[test]
    fn corrects_cbase_yaml_load() {
        // `::YAML` prefix is preserved byte-for-byte.
        test::<YamlLoad>()
            .with_target_ruby_version(3, 0)
            .expect_correction(
                indoc! {r#"
                ::YAML.load(arg)
                       ^^^^ Prefer using `YAML.safe_load` over `YAML.load`.
            "#},
                "::YAML.safe_load(arg)\n",
            );
    }

    #[test]
    fn correction_reaches_fixpoint() {
        // After the `load` -> `safe_load` rename, re-running on the result
        // produces zero offenses — the renamed selector no longer matches
        // `methods = ["load"]`. (Pinned at the firing target so the gate is
        // not what makes it silent.)
        test::<YamlLoad>()
            .with_target_ruby_version(3, 0)
            .expect_no_offenses("YAML.safe_load(arg)\n");
    }
}
