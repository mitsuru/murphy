//! File discovery: `.murphy.yml` `AllCops` include/exclude + `.murphyignore`
//! (Phase 2 Task 6, Scope Fence 2 — discovery-only config).
//!
//! `murphy lint <dir>` / `murphy lint` (no paths) needs the file *list* to
//! come from walking a directory tree, pruned by an optional `.murphyignore`
//! (gitignore syntax) and an optional `.murphy.yml` `AllCops` section:
//!
//! ```yaml
//! AllCops:
//!   Include: ["**/*.rb"]    # globs; default `["**/*.rb"]` if absent
//!   Exclude: ["vendor/**"]  # globs; applied AFTER include
//! ```
//!
//! Scope Fence 2 (decided): `.murphy.yml` `AllCops` drives include/exclude.
//! A malformed `.murphy.yml`, an unreadable directory, or a bad glob is a
//! structured [`ConfigError`] (the CLI maps it to exit `2`) — never a panic,
//! never silently ignored.
//!
//! ## Crate choice (ADR-style one-liner)
//!
//! Directory walking + `.murphyignore` uses ripgrep's **`ignore`** crate: it
//! natively supports a custom-named ignore file with gitignore semantics
//! (`add_custom_ignore_filename(".murphyignore")`), is battle-tested, and
//! avoids hand-rolling gitignore matching. We deliberately **disable** all of
//! `ignore`'s ambient sources (`.gitignore`/`.ignore`/global git ignore,
//! hidden-file skipping, parent traversal) so pruning is *exactly*
//! `.murphyignore` + the `Exclude` globs — predictable and not perturbed by
//! an ambient `.gitignore`. Include/exclude glob matching uses **`globset`**
//! (also from the ripgrep family) as two explicit `GlobSet`s
//! (`include.is_match(p) && !exclude.is_match(p)`), which expresses the
//! plan's "exclude applied after include" directly. `.murphy.yml` is parsed
//! with **`serde_yaml`** + a hand-rolled open-map parser. Pure-Rust deps,
//! caret-pinned (project convention; exact `=` pins are reserved for the
//! contract-affecting native bindings only).

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::MurphyConfig;

/// A discovery/config setup failure. The CLI maps every variant to exit `2`
/// (config/cop/file-setup error, design §6) via its `AppError::setup`.
#[derive(Debug)]
pub enum ConfigError {
    /// `.murphy.yml` exists but is not valid YAML or violates the schema.
    BadYaml(String),
    /// An `Include`/`Exclude` entry is not a valid glob.
    BadGlob(String),
    /// The discovery root is unreadable / the walk hit an I/O error.
    Io(String),
    /// A `plugins:` entry could not be resolved or loaded. Renders
    /// a rustc-style `error:` / `cause:` / `hint:` block (murphy-tvh).
    PluginLoad(crate::plugin_loader::PluginLoadDiagnostic),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::BadYaml(m) => write!(f, "invalid .murphy.yml: {m}"),
            ConfigError::BadGlob(m) => write!(f, "invalid glob in .murphy.yml: {m}"),
            ConfigError::Io(m) => write!(f, "{m}"),
            ConfigError::PluginLoad(d) => write!(f, "{d}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::PluginLoad(d) => Some(d),
            _ => None,
        }
    }
}

/// Build a [`GlobSet`] from glob strings, surfacing a bad pattern as a
/// structured [`ConfigError::BadGlob`].
fn build_globset(patterns: &[String]) -> Result<GlobSet, ConfigError> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        let glob = Glob::new(p).map_err(|e| ConfigError::BadGlob(format!("{p:?}: {e}")))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|e| ConfigError::BadGlob(e.to_string()))
}

/// Discover the `.rb` files under `root`, honoring an optional
/// `<root>/.murphy.yml` `AllCops` include/exclude and an optional
/// `.murphyignore` (gitignore syntax) anywhere in the tree.
///
/// The returned `Vec<PathBuf>` is **sorted** (deterministic input to the
/// parallel pipeline — defense in depth even though the aggregator re-sorts
/// output) and deduplicated. Paths are rooted at `root` exactly as the walker
/// yields them (so `discover(Path::new("."))` yields `./foo.rb`-shaped paths
/// and `discover(some_dir)` yields `some_dir/foo.rb`-shaped paths).
///
/// Errors (malformed `.murphy.yml`, bad glob, unreadable root) are a
/// structured [`ConfigError`]; the CLI maps these to exit `2`.
pub fn discover(root: &Path) -> Result<Vec<PathBuf>, ConfigError> {
    let config = MurphyConfig::load(root)?;
    discover_with_config(root, &config)
}

pub fn discover_with_config(
    root: &Path,
    config: &MurphyConfig,
) -> Result<Vec<PathBuf>, ConfigError> {
    let include = build_globset(&config.files.include)?;
    let exclude = build_globset(&config.files.exclude)?;

    // Walk: ONLY `.murphyignore` prunes (gitignore semantics). Every ambient
    // source is disabled so pruning is exactly `.murphyignore` + `Exclude`.
    // CORRECTNESS-CRITICAL: if upgrading the `ignore` crate, re-audit
    // standard_filters() membership — any new ambient filter must be explicitly
    // disabled here (see ambient_gitignore_is_not_honored test).
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .hidden(false)
        .parents(false);
    builder.add_custom_ignore_filename(".murphyignore");

    let mut out: Vec<PathBuf> = Vec::new();
    for entry in builder.build() {
        let entry = entry.map_err(|e| ConfigError::Io(format!("cannot discover files: {e}")))?;
        // Only regular files; the walker yields directories too.
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        // Glob-match against the path relative to `root` so Include/Exclude
        // globs (`vendor/**`, `**/*.rb`) match user-meaningful paths, not the
        // `./` / `<root>/` prefix the walker carries.
        let rel = path.strip_prefix(root).unwrap_or(path);
        if rel.starts_with(&config.cops.path) {
            continue;
        }
        if include.is_match(rel) && !exclude.is_match(rel) {
            out.push(path.to_path_buf());
        }
    }

    out.sort();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Helper: write `contents` to `<root>/<rel>`, creating parent dirs.
    fn write(root: &Path, rel: &str, contents: &str) {
        let p = root.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(&p, contents).expect("write fixture file");
    }

    /// Discover, then return the discovered paths **relative to `root`** as
    /// sorted `/`-joined strings — portable assertions independent of the
    /// tempdir prefix.
    fn discover_rel(root: &Path) -> Vec<String> {
        let mut v: Vec<String> = discover(root)
            .expect("discover must succeed")
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn no_murphy_yml_defaults_to_all_rb_recursively() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        write(root, "a.rb", "x = 1\n");
        write(root, "sub/b.rb", "y = 2\n");
        write(root, "notes.txt", "ignore me\n");

        assert_eq!(discover_rel(root), vec!["a.rb", "sub/b.rb"]);
    }

    #[test]
    fn murphy_yml_include_narrows_the_set() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        write(root, "lib/a.rb", "x = 1\n");
        write(root, "test/b.rb", "y = 2\n");
        write(
            root,
            ".murphy.yml",
            "AllCops:\n  Include:\n    - 'lib/**/*.rb'\n",
        );

        assert_eq!(discover_rel(root), vec!["lib/a.rb"]);
    }

    #[test]
    fn murphy_yml_exclude_applied_after_include() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        write(root, "app.rb", "x = 1\n");
        write(root, "vendor/dep.rb", "y = 2\n");
        write(
            root,
            ".murphy.yml",
            "AllCops:\n  Include:\n    - '**/*.rb'\n  Exclude:\n    - 'vendor/**'\n",
        );

        assert_eq!(discover_rel(root), vec!["app.rb"]);
    }

    #[test]
    fn murphyignore_line_prunes_a_matching_file() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        write(root, "keep.rb", "x = 1\n");
        write(root, "skip.rb", "y = 2\n");
        write(root, ".murphyignore", "skip.rb\n");

        // No .murphy.yml → default `**/*.rb`, still honors `.murphyignore`.
        assert_eq!(discover_rel(root), vec!["keep.rb"]);
    }

    #[test]
    fn murphyignore_directory_glob_prunes_subtree() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        write(root, "src/a.rb", "x = 1\n");
        write(root, "build/gen.rb", "y = 2\n");
        write(root, ".murphyignore", "build/\n");

        assert_eq!(discover_rel(root), vec!["src/a.rb"]);
    }

    #[test]
    fn ambient_gitignore_is_not_honored() {
        // CORRECTNESS-CRITICAL: only `.murphyignore` + .murphy.yml `Exclude`
        // prune. An ambient `.gitignore` must NOT (the WalkBuilder disables all
        // ambient ignore sources). Guards against a regression / `ignore`
        // crate upgrade silently re-enabling git-ignore semantics.
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        write(root, "kept.rb", "x = 1\n");
        // A `.gitignore` that WOULD exclude `kept.rb` if honored.
        write(root, ".gitignore", "kept.rb\n");
        // Make this look like a git repo so the `ignore` crate's git-ignore
        // machinery would activate on a single `.git_ignore(false)→true` slip
        // (without this it needs `require_git(false)` too, weakening the guard).
        fs::create_dir(root.join(".git")).expect("simulate git repo");

        assert!(
            discover_rel(root).contains(&"kept.rb".to_string()),
            "ambient .gitignore must NOT prune; only .murphyignore + Exclude do"
        );
    }

    #[test]
    fn murphyignore_and_exclude_both_prune() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        write(root, "a.rb", "1\n");
        write(root, "b.rb", "2\n");
        write(root, "c.rb", "3\n");
        write(root, ".murphyignore", "b.rb\n");
        write(
            root,
            ".murphy.yml",
            "AllCops:\n  Include:\n    - '**/*.rb'\n  Exclude:\n    - 'c.rb'\n",
        );

        assert_eq!(discover_rel(root), vec!["a.rb"]);
    }

    #[test]
    fn malformed_murphy_yml_is_a_config_error_not_a_panic() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        write(root, "a.rb", "1\n");
        // Invalid YAML: unclosed flow sequence
        write(root, ".murphy.yml", "AllCops: [unclosed\n");

        match discover(root) {
            Err(ConfigError::BadYaml(_)) => {}
            other => panic!("expected ConfigError::BadYaml, got {other:?}"),
        }
    }

    #[test]
    fn unknown_top_level_keys_become_cop_rules_not_errors() {
        // `.murphy.yml` is open-keyed at the top level: any key that is not
        // `AllCops` or `plugins` is treated as a cop rule (RuboCop compatibility).
        // This means typos in `AllCops` (e.g. `AlCops`) silently become cop rules.
        // The trade-off is accepted for RuboCop format compatibility.
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        write(root, "a.rb", "1\n");
        write(
            root,
            ".murphy.yml",
            "AllCops:\n  Include:\n    - '**/*.rb'\nZzz/DefinitelyNotReal:\n  Enabled: false\n",
        );

        // Should succeed (open-keyed), not error.
        match discover(root) {
            Ok(_) => {}
            Err(e) => panic!("expected success for unknown cop key, got: {e}"),
        }
    }

    #[test]
    fn bad_glob_in_murphy_yml_is_a_config_error() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        write(root, "a.rb", "1\n");
        write(
            root,
            ".murphy.yml",
            "AllCops:\n  Include:\n    - '[unterminated'\n",
        );

        match discover(root) {
            Err(ConfigError::BadGlob(_)) => {}
            other => panic!("expected ConfigError::BadGlob, got {other:?}"),
        }
    }

    #[test]
    fn discovered_paths_are_sorted() {
        // The walker cannot emit duplicate entries from its own traversal, so
        // dedup() of discover()'s output is a no-op and asserting it would be
        // vacuous. Real dedup (e.g. `lint d/ d/x.rb`) is exercised at the CLI
        // `BTreeSet` union layer, not here.
        let dir = tempdir().expect("tempdir");
        let root = dir.path();
        for name in ["z.rb", "a.rb", "m.rb"] {
            write(root, name, "1\n");
        }
        let got = discover(root).expect("discover");
        let mut sorted = got.clone();
        sorted.sort();
        assert_eq!(got, sorted, "discover() output must be sorted");
    }
}
