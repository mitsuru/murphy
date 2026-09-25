//! `Rails/TopLevelHashWithIndifferentAccess` — flag top-level `HashWithIndifferentAccess`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/TopLevelHashWithIndifferentAccess
//! upstream_version_checked: 2.35.0
//! version_added: "2.16"
//! version_changed: "2.18"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_const` behind
//!   `minimum_target_rails_version 5.1` (unset means newest): flags a bare
//!   or `::`-prefixed `HashWithIndifferentAccess` const (`is_global_const`
//!   matches upstream `(const {nil? cbase} :HashWithIndifferentAccess)`;
//!   Murphy folds the `::` prefix into the const range with scope `None`
//!   either way, so the correction detects the prefix from source),
//!   except a class definition nested in a module (parent is `Class` with a
//!   `Module` ancestor — e.g. `module CoreExt; class
//!   HashWithIndifferentAccess; end; end` defines a namespaced class rather
//!   than referencing the top-level constant). A top-level `class
//!   HashWithIndifferentAccess` still flags, matching upstream. Offense
//!   covers the whole const (including `::`); autocorrect inserts
//!   `ActiveSupport::` before the name. Disabled upstream by default
//!   (`Enabled: pending`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct TopLevelHashWithIndifferentAccess;

const MSG: &str = "Avoid top-level `HashWithIndifferentAccess`.";

#[cop(
    name = "Rails/TopLevelHashWithIndifferentAccess",
    description = "Identifies top-level `HashWithIndifferentAccess`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl TopLevelHashWithIndifferentAccess {
    // Mirrors upstream `on_const`.
    #[on_node(kind = "const")]
    fn check_const(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Upstream `(const {nil? cbase} :HashWithIndifferentAccess)`.
    if !cx.is_global_const(node, "HashWithIndifferentAccess") {
        return;
    }
    // Upstream `minimum_target_rails_version 5.1` — unset means newest.
    if !cx.rails_version_at_least(5, 1) {
        return;
    }
    // Upstream `return if node.parent&.class_type? &&
    // node.parent.ancestors.any?(&:module_type?)`: a class definition nested
    // in a module defines a namespaced class, not a top-level reference.
    if cx.parent(node).get().is_some_and(|parent| {
        matches!(*cx.kind(parent), NodeKind::Class { .. })
            && cx
                .ancestors(node)
                .any(|anc| matches!(*cx.kind(anc), NodeKind::Module { .. }))
    }) {
        return;
    }
    cx.emit_offense(cx.range(node), MSG, None);
    // Upstream `corrector.insert_before(node.location.name,
    // 'ActiveSupport::')`: before the name itself, i.e. after the `::`
    // prefix for a cbase const (`::Foo` -> `::ActiveSupport::Foo`).
    // Upstream inserts before `location.name`; Murphy folds a `::` prefix
    // into the const range (scope is `None` either way), so detect the
    // prefix from source: `::Foo` -> `::ActiveSupport::Foo`.
    let expr = cx.range(node);
    let src_text = cx.raw_source(expr);
    let pos = if src_text.starts_with("::") {
        expr.start + 2
    } else {
        expr.start
    };
    cx.emit_edit(
        Range {
            start: pos,
            end: pos,
        },
        "ActiveSupport::",
    );
}

#[cfg(test)]
mod tests {
    use super::TopLevelHashWithIndifferentAccess;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_const_with_call() {
        test::<TopLevelHashWithIndifferentAccess>().expect_correction(
            indoc! {r#"
                HashWithIndifferentAccess.new(foo: 'bar')
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid top-level `HashWithIndifferentAccess`.
            "#},
            "ActiveSupport::HashWithIndifferentAccess.new(foo: 'bar')\n",
        );
    }

    #[test]
    fn flags_cbase_const_with_call() {
        test::<TopLevelHashWithIndifferentAccess>().expect_correction(
            indoc! {r#"
                ::HashWithIndifferentAccess.new(foo: 'bar')
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid top-level `HashWithIndifferentAccess`.
            "#},
            "::ActiveSupport::HashWithIndifferentAccess.new(foo: 'bar')\n",
        );
    }

    #[test]
    fn flags_bare_const_without_call() {
        test::<TopLevelHashWithIndifferentAccess>().expect_correction(
            indoc! {r#"
                HashWithIndifferentAccess
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid top-level `HashWithIndifferentAccess`.
            "#},
            "ActiveSupport::HashWithIndifferentAccess\n",
        );
    }

    #[test]
    fn flags_top_level_class_definition() {
        // Upstream flags this too: the parent is `Class` but no ancestor is
        // a `Module` (verified against rubocop-rails 2.35.0).
        test::<TopLevelHashWithIndifferentAccess>().expect_offense(indoc! {r#"
            class HashWithIndifferentAccess
                  ^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid top-level `HashWithIndifferentAccess`.
        "#});
    }

    #[test]
    fn does_not_flag_namespaced_const() {
        test::<TopLevelHashWithIndifferentAccess>()
            .expect_no_offenses("ActiveSupport::HashWithIndifferentAccess.new(foo: 'bar')\n");
    }

    #[test]
    fn does_not_flag_scoped_const() {
        test::<TopLevelHashWithIndifferentAccess>()
            .expect_no_offenses("Foo::HashWithIndifferentAccess.new\n");
    }

    #[test]
    fn does_not_flag_class_definition_inside_module() {
        test::<TopLevelHashWithIndifferentAccess>().expect_no_offenses(indoc! {r#"
            module CoreExt
              class HashWithIndifferentAccess
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_const_assignment() {
        // `on_const` never fires for `Casgn`.
        test::<TopLevelHashWithIndifferentAccess>()
            .expect_no_offenses("HashWithIndifferentAccess = 1\n");
    }

    #[test]
    fn does_not_flag_below_rails_51() {
        test::<TopLevelHashWithIndifferentAccess>()
            .with_target_rails_version(5, 0)
            .expect_no_offenses("HashWithIndifferentAccess.new(foo: 'bar')\n");
    }

    #[test]
    fn fires_at_rails_51() {
        test::<TopLevelHashWithIndifferentAccess>()
            .with_target_rails_version(5, 1)
            .expect_offense(indoc! {r#"
                HashWithIndifferentAccess.new(foo: 'bar')
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid top-level `HashWithIndifferentAccess`.
            "#});
    }
}
murphy_plugin_api::submit_cop!(TopLevelHashWithIndifferentAccess);
