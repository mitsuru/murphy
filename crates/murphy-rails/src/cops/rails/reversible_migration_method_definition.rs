//! `Rails/ReversibleMigrationMethodDefinition` — migration must have `change` or `up`+`down`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ReversibleMigrationMethodDefinition
//! upstream_version_checked: 2.35.0
//! version_added: "2.10"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 MigrationsHelper#migration_class?:
//!   top-level const with `ActiveRecord::Migration[float]` superclass.
//!   Flags when neither a zero-arg `change` def nor both zero-arg `up`
//!   and `down` defs are present among class descendants. Offense range
//!   is the class header (`class` to superclass end) matching upstream's
//!   first-line highlight. No autocorrect. `MigratedSchemaVersion`
//!   skipping remains absent (cf. MigrationClassName). Upstream Include
//!   (`db/**/*.rb`) is enforced via the murphy-rails pack default.yml.
//!   Upstream ships `Enabled: false`; Murphy maps that to
//!   `default_enabled = false`.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ReversibleMigrationMethodDefinition;

#[cop(
    name = "Rails/ReversibleMigrationMethodDefinition",
    description = "Migrations must contain either a `change` method, or both an `up` and a `down` method.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl ReversibleMigrationMethodDefinition {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Class {
        name,
        superclass,
        body,
    } = *cx.kind(node)
    else {
        return;
    };
    if !is_top_level_const(cx, name) {
        return;
    }
    let Some(super_id) = superclass.get() else {
        return;
    };
    if !is_migration_superclass(cx, super_id) {
        return;
    }
    let Some(body_id) = body.get() else {
        // Empty class body — no change/up/down.
        cx.emit_offense(
            header_range(cx, node, super_id),
            "Migrations must contain either a `change` method, or both an `up` and a `down` method.",
            None,
        );
        return;
    };
    let mut has_change = false;
    let mut has_up = false;
    let mut has_down = false;
    collect_defs(cx, body_id, &mut has_change, &mut has_up, &mut has_down);
    if has_change || (has_up && has_down) {
        return;
    }
    cx.emit_offense(
        header_range(cx, node, super_id),
        "Migrations must contain either a `change` method, or both an `up` and a `down` method.",
        None,
    );
}

fn is_top_level_const(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Const { scope, .. } = *cx.kind(id) else {
        return false;
    };
    match scope.get() {
        None => true,
        Some(s) => matches!(*cx.kind(s), NodeKind::Cbase),
    }
}

fn is_migration_superclass(cx: &Cx<'_>, id: NodeId) -> bool {
    // `(send (const (const {nil? cbase} :ActiveRecord) :Migration) :[] (float _))`
    if !matches!(*cx.kind(id), NodeKind::Send { .. }) {
        return false;
    }
    if cx.method_name(id) != Some("[]") {
        return false;
    }
    let Some(recv) = cx.call_receiver(id).get() else {
        return false;
    };
    if cx.const_name(recv).as_deref() != Some("ActiveRecord::Migration") {
        return false;
    }
    let args = cx.call_arguments(id);
    if args.len() != 1 {
        return false;
    }
    matches!(*cx.kind(args[0]), NodeKind::Float(_))
}

fn header_range(cx: &Cx<'_>, class_id: NodeId, super_id: NodeId) -> murphy_plugin_api::Range {
    let start = cx.range(class_id).start;
    let end = cx.range(super_id).end;
    murphy_plugin_api::Range { start, end }
}

fn collect_defs(cx: &Cx<'_>, id: NodeId, has_change: &mut bool, has_up: &mut bool, has_down: &mut bool) {
    // Do not descend into inner classes/modules — only the migration's own defs count.
    if matches!(
        *cx.kind(id),
        NodeKind::Class { .. } | NodeKind::Module { .. }
    ) {
        return;
    }
    if let NodeKind::Def { name, args, .. } = *cx.kind(id) {
        let method = cx.symbol_str(name);
        if matches!(method, "change" | "up" | "down") && args_empty(cx, args) {
            match method {
                "change" => *has_change = true,
                "up" => *has_up = true,
                "down" => *has_down = true,
                _ => {}
            }
        }
        // Still walk body for nested defs? `def` bodies cannot contain `def`
        // at the same level except via define_method; walk children anyway
        // except inner classes (handled above).
    }
    for child in cx.children(id) {
        collect_defs(cx, child, has_change, has_up, has_down);
    }
}

fn args_empty(cx: &Cx<'_>, args_id: NodeId) -> bool {
    match *cx.kind(args_id) {
        NodeKind::Args(list) => cx.list(list).is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::ReversibleMigrationMethodDefinition;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn allows_change() {
        test::<ReversibleMigrationMethodDefinition>().expect_no_offenses(indoc! {r#"
            class SomeMigration < ActiveRecord::Migration[6.0]
              def change
                add_column :users, :email, :text, null: false
              end
            end
        "#});
    }

    #[test]
    fn flags_only_up() {
        test::<ReversibleMigrationMethodDefinition>().expect_offense(indoc! {r#"
            class SomeMigration < ActiveRecord::Migration[6.0]
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Migrations must contain either a `change` method, or both an `up` and a `down` method.
              def up
                add_column :users, :email, :text, null: false
              end
            end
        "#});
    }

    #[test]
    fn flags_only_down() {
        test::<ReversibleMigrationMethodDefinition>().expect_offense(indoc! {r#"
            class SomeMigration < ActiveRecord::Migration[6.0]
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Migrations must contain either a `change` method, or both an `up` and a `down` method.
              def down
                remove_column :users, :email
              end
            end
        "#});
    }

    #[test]
    fn allows_up_and_down() {
        test::<ReversibleMigrationMethodDefinition>().expect_no_offenses(indoc! {r#"
            class SomeMigration < ActiveRecord::Migration[6.0]
              def up
                add_column :users, :email, :text, null: false
              end

              def down
                remove_column :users, :email
              end
            end
        "#});
    }

    #[test]
    fn flags_typo_change() {
        test::<ReversibleMigrationMethodDefinition>().expect_offense(indoc! {r#"
            class SomeMigration < ActiveRecord::Migration[6.0]
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Migrations must contain either a `change` method, or both an `up` and a `down` method.
              def chance
                add_column :users, :email, :text, null: false
              end
            end
        "#});
    }

    #[test]
    fn allows_helper_methods_with_change() {
        test::<ReversibleMigrationMethodDefinition>().expect_no_offenses(indoc! {r#"
            class SomeMigration < ActiveRecord::Migration[6.0]
              def change
                add_users_column :email, :text, null: false
              end

              private

              def add_users_column(column_name, null: false)
                add_column :users, column_name, type, null: null
              end
            end
        "#});
    }

    #[test]
    fn allows_inner_class_with_change() {
        test::<ReversibleMigrationMethodDefinition>().expect_no_offenses(indoc! {r#"
            class SomeMigration < ActiveRecord::Migration[6.0]
              class Foo
              end

              def change
                add_column :users, :email, :text, null: false
              end
            end
        "#});
    }

    #[test]
    fn allows_non_migration() {
        test::<ReversibleMigrationMethodDefinition>().expect_no_offenses(indoc! {r#"
            class Foo < ActiveRecord::Base
              def up
              end
            end
        "#});
    }

    #[test]
    fn flags_cbase_migration_with_only_up() {
        test::<ReversibleMigrationMethodDefinition>().expect_offense(indoc! {r#"
            class ::SomeMigration < ActiveRecord::Migration[6.0]
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Migrations must contain either a `change` method, or both an `up` and a `down` method.
              def up
                add_column :users, :email, :text, null: false
              end
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(ReversibleMigrationMethodDefinition);
