//! `Rails/MigrationClassName` — migration class name must match file name.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/MigrationClassName
//! upstream_version_checked: 2.35.0
//! version_added: "2.14"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 MigrationsHelper#migration_class?:
//!   top-level const (nil/Cbase scope) with superclass
//!   `ActiveRecord::Migration[float]` (const_name gating, Float arg).
//!   Basename logic (strip .rb, gem suffix after first dot, leading
//!   `\d+_` timestamp) plus Ruby-capitalize camelize with case-insensitive
//!   (casecmp) comparison are mirrored. Offense is the short class name
//!   (trailing slice for `::` prefix); autocorrect replaces it. Upstream
//!   Include (`db/**/*.rb`) is enforced via the murphy-rails pack
//!   default.yml (engine `cop_applies_to_file` gate, verified vs
//!   rubocop-rails 2.38.0 default.yml, murphy-4gd.1.15);
//!   MigratedSchemaVersion skipping remains absent.
//! ```
//!
//! Makes sure that each migration file defines a migration class whose
//! name matches the file name.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MigrationClassName;

#[cop(
    name = "Rails/MigrationClassName",
    description = "The class name of the migration should match its file name.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl MigrationClassName {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Class { name, superclass, .. } = *cx.kind(node) else {
        return;
    };
    // `(const {nil? cbase} _)` — top-level only.
    if !is_top_level_const(cx, name) {
        return;
    }
    let Some(super_id) = superclass.get() else {
        return;
    };
    if !is_migration_superclass(cx, super_id) {
        return;
    }
    let short = short_name(cx, name);
    let basename = basename_without_timestamp_and_suffix(cx.file_path());
    let expected = camelize(&basename);
    if short.eq_ignore_ascii_case(&expected) {
        return;
    }
    let offense = short_name_range(cx, name, &short);
    let msg = format!("Replace with `{expected}` that matches the file name.");
    cx.emit_offense(offense, &msg, None);
    cx.emit_edit(offense, &expected);
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
    let NodeKind::Send { receiver, method, args } = *cx.kind(id) else {
        return false;
    };
    if cx.symbol_str(method) != "[]" {
        return false;
    }
    let Some(recv) = receiver.get() else {
        return false;
    };
    if cx.const_name(recv).as_deref() != Some("ActiveRecord::Migration") {
        return false;
    }
    let arg_ids = cx.list(args);
    if arg_ids.len() != 1 {
        return false;
    }
    matches!(*cx.kind(arg_ids[0]), NodeKind::Float(_))
}

fn short_name(cx: &Cx<'_>, id: NodeId) -> String {
    match *cx.kind(id) {
        NodeKind::Const { name, .. } => cx.symbol_str(name).to_owned(),
        _ => cx
            .const_name(id)
            .map(|f| f.rsplit("::").next().unwrap_or("").to_owned())
            .unwrap_or_default(),
    }
}

fn short_name_range(cx: &Cx<'_>, id: NodeId, short: &str) -> Range {
    let whole = cx.range(id);
    let len = short.len() as u32;
    if whole.end >= len && len > 0 {
        // `::SellBooks` keeps the `::` prefix; offense is the trailing short name.
        let src = cx.raw_source(whole);
        if src.len() >= short.len() && src.ends_with(short) {
            return Range {
                start: whole.end - len,
                end: whole.end,
            };
        }
    }
    whole
}

fn basename_without_timestamp_and_suffix(path: &str) -> String {
    // File.basename(path, '.rb')
    let base = path.rsplit('/').next().unwrap_or(path);
    let no_rb = base.strip_suffix(".rb").unwrap_or(base);
    // remove_gem_suffix: from `add_blobs.active_storage` to `add_blobs`.
    let no_gem = match no_rb.find('.') {
        Some(i) => &no_rb[..i],
        None => no_rb,
    };
    // sub(/\A\d+_/, '')
    let mut idx = 0;
    let bytes = no_gem.as_bytes();
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx > 0 && idx < bytes.len() && bytes[idx] == b'_' {
        no_gem[idx + 1..].to_owned()
    } else {
        no_gem.to_owned()
    }
}

fn camelize(word: &str) -> String {
    let mut out = String::new();
    for part in word.split('_') {
        if part.is_empty() {
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            let rest: String = chars.collect();
            out.push_str(&rest.to_ascii_lowercase());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::MigrationClassName;
    use murphy_plugin_api::test_support::{indoc, test};

    const FILE: &str = "db/migrate/20250101010101_create_users.rb";

    #[test]
    fn allows_matching_name() {
        test::<MigrationClassName>()
            .with_file_path(FILE)
            .expect_no_offenses("class CreateUsers < ActiveRecord::Migration[7.0]\nend\n");
    }

    #[test]
    fn allows_other_class_beside_migration() {
        test::<MigrationClassName>().with_file_path(FILE).expect_no_offenses(indoc! {r#"
            class Article < ActiveRecord::Base
            end

            class CreateUsers < ActiveRecord::Migration[7.0]
            end
        "#});
    }

    #[test]
    fn allows_inner_class() {
        test::<MigrationClassName>().with_file_path(FILE).expect_no_offenses(indoc! {r#"
            class CreateUsers < ActiveRecord::Migration[7.0]
              class Article < ActiveRecord::Base
              end
            end
        "#});
    }

    #[test]
    fn flags_mismatched_name() {
        test::<MigrationClassName>().with_file_path(FILE).expect_correction(
            indoc! {r#"
                class SellBooks < ActiveRecord::Migration[7.0]
                      ^^^^^^^^^ Replace with `CreateUsers` that matches the file name.
                end
            "#},
            "class CreateUsers < ActiveRecord::Migration[7.0]\nend\n",
        );
    }

    #[test]
    fn flags_cbase_mismatched_name() {
        test::<MigrationClassName>().with_file_path(FILE).expect_correction(
            indoc! {r#"
                class ::SellBooks < ActiveRecord::Migration[7.0]
                        ^^^^^^^^^ Replace with `CreateUsers` that matches the file name.
                end
            "#},
            "class ::CreateUsers < ActiveRecord::Migration[7.0]\nend\n",
        );
    }

    #[test]
    fn allows_dot_suffix_match() {
        test::<MigrationClassName>()
            .with_file_path("db/migrate/20220101050505_add_blobs.active_storage.rb")
            .expect_no_offenses("class AddBlobs < ActiveRecord::Migration[7.0]\nend\n");
    }

    #[test]
    fn allows_case_insensitive_acronym() {
        test::<MigrationClassName>()
            .with_file_path("db/migrate/20250101010101_remove_unused_oauth_scope_grants.rb")
            .expect_no_offenses(
                "class RemoveUnusedOAuthScopeGrants < ActiveRecord::Migration[7.0]\nend\n",
            );
    }

    #[test]
    fn allows_non_migration_superclass() {
        test::<MigrationClassName>()
            .with_file_path(FILE)
            .expect_no_offenses("class Foo < ActiveRecord::Base\nend\n");
    }

    #[test]
    fn allows_namespaced_class() {
        test::<MigrationClassName>()
            .with_file_path(FILE)
            .expect_no_offenses("class Foo::Bar < ActiveRecord::Migration[7.0]\nend\n");
    }
}
murphy_plugin_api::submit_cop!(MigrationClassName);
