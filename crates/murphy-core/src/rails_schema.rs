//! Rails `db/schema.rb` loader (murphy-s0vb).
//!
//! Mirrors `RuboCop::Rails::SchemaLoader.db_schema_path`: walks up from the
//! project root to `/` looking for `db/schema.rb`. The file is parsed once
//! per run with `murphy-translate`, tables extracted via
//! `murphy_plugin_api::rails_schema::schema_from_ast`, and serialized to the
//! JSON threaded into every cop's `CxRaw` (`rails_schema_json`). Empty string
//! means "no schema" — cops see `Cx::rails_schema() == None` and emit nothing.

use std::path::{Path, PathBuf};

/// Walk up from `start` (inclusive) to `/` looking for `db/schema.rb`,
/// mirroring RuboCop's `SchemaLoader.db_schema_path` (which walks up from
/// `Pathname.pwd`). Returns the first hit.
pub fn find_schema_path(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_file() {
        start.parent().map(|p| p.to_path_buf())
    } else {
        Some(start.to_path_buf())
    };
    while let Some(d) = dir {
        let candidate = d.join("db/schema.rb");
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent().map(Path::to_path_buf);
    }
    None
}

/// Load + extract + serialize the schema for `project_root` (usually `"."`).
/// Returns `""` when no schema file exists, it cannot be read, or it yields
/// no parseable content — all "no schema" cases where cops must stay silent.
pub fn load_schema_json(project_root: &Path) -> String {
    let Some(path) = find_schema_path(project_root) else {
        return String::new();
    };
    let Ok(src) = std::fs::read_to_string(&path) else {
        return String::new();
    };
    if src.trim().is_empty() {
        // Empty file → schema exists but has no tables. Return empty-tables
        // JSON (cops still emit nothing since table lookup fails), matching
        // RuboCop's "empty schema.rb → no offense" spec.
        return murphy_plugin_api::RailsSchema::default().to_json();
    }
    let ast = murphy_translate::translate(&src, path.clone());
    let schema = murphy_plugin_api::rails_schema::schema_from_ast(&ast);
    schema.to_json()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_schema_by_walking_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("a/b");
        std::fs::create_dir_all(sub.join("db")).expect("mkdir");
        // Schema at a/b/db/schema.rb must be found from a/b/c.
        let deep = sub.join("c");
        std::fs::create_dir_all(&deep).expect("mkdir");
        std::fs::write(sub.join("db/schema.rb"), "x = 1\n").expect("write");
        let found = find_schema_path(&deep).expect("found");
        assert_eq!(found, sub.join("db/schema.rb"));
    }

    #[test]
    fn returns_none_when_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(find_schema_path(dir.path()), None);
    }

    #[test]
    fn load_returns_empty_when_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(load_schema_json(dir.path()), "");
    }

    #[test]
    fn load_parses_tables() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("db")).expect("mkdir");
        std::fs::write(
            dir.path().join("db/schema.rb"),
            "ActiveRecord::Schema.define do\n  create_table \"users\" do |t|\n    t.string \"account\"\n    t.index [\"account\"], name: \"idx\", unique: true\n  end\nend\n",
        )
        .expect("write");
        let json = load_schema_json(dir.path());
        let schema = murphy_plugin_api::RailsSchema::from_json(&json).expect("decodes");
        let table = schema.table_by("users").expect("users");
        assert!(table.with_column("account"));
        assert_eq!(table.indices.len(), 1);
        assert!(table.indices[0].unique);
    }
}
