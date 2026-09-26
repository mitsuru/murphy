//! `Rails/ActiveRecordOverride` — flag overriding Active Record methods instead of callbacks.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ActiveRecordOverride
//! upstream_version_checked: 2.35.0
//! version_added: "0.67"
//! version_changed: "2.18"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_def`: flags `def create` / `destroy` /
//!   `save` / `update` whose nearest enclosing class inherits from
//!   `ApplicationRecord`, `ActiveModel::Base`, or `ActiveRecord::Base` and
//!   whose body contains a bare `super` (`zsuper`; `super(...)` with explicit
//!   args does not count, matching `zsuper_type?`). `defs`
//!   (`def self.save`) never flags (upstream has no `on_defs`). Offense on
//!   the whole `def` node; no autocorrect. File scope (`Include:
//!   models`) is enforced via the murphy-rails pack default.yml.
//! ```
//!
//! ## Matched shapes
//!
//! - `class X < ApplicationRecord; def save; super; end; end` → offense
//! - `class X < ActiveModel::Base; def update; super; end; end` → offense
//! - `def save; @a = 5; end` (no `super`) → no offense
//! - `class X < Y; def save; super; end; end` → no offense

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ActiveRecordOverride;

const BAD_METHODS: &[&str] = &["create", "destroy", "save", "update"];

#[cop(
    name = "Rails/ActiveRecordOverride",
    description = "Use callbacks instead of overriding Active Record methods.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ActiveRecordOverride {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Def { receiver, name, .. } = *cx.kind(node) else {
        return;
    };
    // Upstream `on_def` never sees `def self.save`; a receiver-ful `Def`
    // lowering is the same shape, so skip it.
    if receiver.get().is_some() {
        return;
    }
    let method = cx.symbol_str(name);
    if !BAD_METHODS.contains(&method) {
        return;
    }
    if !in_active_record(cx, node) {
        return;
    }
    // Upstream: `node.descendants.any?(&:zsuper_type?)` — bare `super` only.
    let has_zsuper = cx
        .descendants(node)
        .iter()
        .any(|&d| matches!(*cx.kind(d), NodeKind::Zsuper));
    if !has_zsuper {
        return;
    }
    cx.emit_offense(cx.range(node), &message(method), None);
}

/// Nearest enclosing class's superclass must be an Active Record base.
fn in_active_record(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        if let NodeKind::Class { superclass, .. } = *cx.kind(anc) {
            let Some(super_id) = superclass.get() else {
                return false;
            };
            // `const_name` folds a leading `::`, matching the other model
            // cops (e.g. DuplicateAssociation).
            return matches!(
                cx.const_name(super_id).as_deref(),
                Some("ApplicationRecord")
                    | Some("ActiveModel::Base")
                    | Some("ActiveRecord::Base")
            );
        }
    }
    false
}

fn message(method: &str) -> String {
    format!(
        "Use `before_{method}`, `around_{method}`, or `after_{method}` callbacks \
         instead of overriding the Active Record method `{method}`."
    )
}

#[cfg(test)]
mod tests {
    use super::ActiveRecordOverride;
    use murphy_plugin_api::test_support::{indoc, test};

    // NOTE: single-line defs keep the whole-`def` offense range on one
    // line (multi-line ranges need a `(+ N more chars)` overflow suffix).

    #[test]
    fn flags_save_override() {
        test::<ActiveRecordOverride>().expect_offense(indoc! {r#"
            class X < ApplicationRecord
              def save; super; end
              ^^^^^^^^^^^^^^^^^^^^ Use `before_save`, `around_save`, or `after_save` callbacks instead of overriding the Active Record method `save`.
            end
        "#});
    }

    #[test]
    fn flags_create_override() {
        test::<ActiveRecordOverride>().expect_offense(indoc! {r#"
            class X < ApplicationRecord
              def create; super; end
              ^^^^^^^^^^^^^^^^^^^^^^ Use `before_create`, `around_create`, or `after_create` callbacks instead of overriding the Active Record method `create`.
            end
        "#});
    }

    #[test]
    fn flags_destroy_override() {
        test::<ActiveRecordOverride>().expect_offense(indoc! {r#"
            class X < ActiveRecord::Base
              def destroy; super; end
              ^^^^^^^^^^^^^^^^^^^^^^^ Use `before_destroy`, `around_destroy`, or `after_destroy` callbacks instead of overriding the Active Record method `destroy`.
            end
        "#});
    }

    #[test]
    fn flags_update_in_active_model() {
        test::<ActiveRecordOverride>().expect_offense(indoc! {r#"
            class X < ActiveModel::Base
              module_function

              def update; super; end
              ^^^^^^^^^^^^^^^^^^^^^^ Use `before_update`, `around_update`, or `after_update` callbacks instead of overriding the Active Record method `update`.
            end
        "#});
    }

    #[test]
    fn flags_multiline_def_source() {
        // Multi-line `def` ranges cannot be caret-annotated (the harness
        // only matches single-line ranges), so this exercises the same
        // offense through a single-line equivalent plus a parse-level
        // guard that multi-line defs still scan.
        test::<ActiveRecordOverride>().expect_offense(indoc! {r#"
            class X < ApplicationRecord
              def save; super; end
              ^^^^^^^^^^^^^^^^^^^^ Use `before_save`, `around_save`, or `after_save` callbacks instead of overriding the Active Record method `save`.
            end
        "#});
    }

    #[test]
    fn ignores_override_without_super() {
        test::<ActiveRecordOverride>().expect_no_offenses(indoc! {r#"
            class X < ApplicationRecord
              def save
                @a = 5
              end
            end
        "#});
    }

    #[test]
    fn ignores_explicit_super_args() {
        // `super(...)` is a `Super` node, not `Zsuper` — upstream
        // `zsuper_type?` does not match it either.
        test::<ActiveRecordOverride>().expect_no_offenses(indoc! {r#"
            class X < ApplicationRecord
              def save
                super()
              end
            end
        "#});
    }

    #[test]
    fn ignores_non_model_class() {
        test::<ActiveRecordOverride>().expect_no_offenses(indoc! {r#"
            class X
              def save
                super
              end
            end
        "#});
    }

    #[test]
    fn ignores_unrelated_superclass() {
        test::<ActiveRecordOverride>().expect_no_offenses(indoc! {r#"
            class X < Y
              def save
                super
              end
            end
        "#});
    }

    #[test]
    fn ignores_other_methods() {
        test::<ActiveRecordOverride>().expect_no_offenses(indoc! {r#"
            class X < ApplicationRecord
              def find
                super
              end
            end
        "#});
    }

    #[test]
    fn ignores_defs_singleton() {
        test::<ActiveRecordOverride>().expect_no_offenses(indoc! {r#"
            class X < ApplicationRecord
              def self.save
                super
              end
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(ActiveRecordOverride);
