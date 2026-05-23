//! Plugin name → path resolution for `[[plugins]]` `Name(String)` shorthand
//! (murphy-9cr.10.2; ADR 0042). The `Detailed { name, path }` form bypasses
//! this module entirely.
//!
//! Search-path priority (highest to lowest):
//! 1. `Detailed { name, path }` overrides supplied by the caller — lets a
//!    user pin a specific build of a name-only entry without removing the
//!    shorthand from `plugins = ["..."]`.
//! 2. `MURPHY_PLUGIN_PATH` env (parsed via `std::env::split_paths`).
//! 3. project-local `<root>/.murphy/plugins/`.
//! 4. user-local `dirs::data_dir()/murphy/plugins/`
//!    (XDG `$XDG_DATA_HOME/murphy/plugins` on Linux).
//!
//! Within a directory the loader looks for `lib<sanitized>.{so,dylib}`
//! where `<sanitized>` is `name` with `-` replaced by `_` (Cargo cdylib
//! naming convention — `murphy-rails` → `libmurphy_rails.so`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{ConfigError, PluginConfig};

const MAX_PLUGIN_NAME_LEN: usize = 64;

/// Validate that a plugin name is well-formed for path construction.
///
/// Allowed: ASCII letters, digits, `_`, `-`, `.`. Length 1..=64.
///
/// Rejects path-traversal chars (`/`, `\`), whitespace, control chars, and
/// anything outside the allowed alphabet so a malicious `murphy.toml` can't
/// turn `plugins = ["../../../etc/passwd"]` into a `find_in_dir` lookup.
pub fn validate_plugin_name(name: &str) -> Result<(), ConfigError> {
    if name.is_empty() {
        return Err(ConfigError::Io(
            "Plugin name is empty: `plugins = [...]` entries must be non-empty strings".to_string(),
        ));
    }
    if name.len() > MAX_PLUGIN_NAME_LEN {
        // `take` on chars (not bytes) keeps us off `&str` byte-slicing,
        // which panics if the byte index lands inside a multi-byte char.
        let preview: String = name.chars().take(MAX_PLUGIN_NAME_LEN).collect();
        return Err(ConfigError::Io(format!(
            "Plugin name `{preview}…` exceeds {MAX_PLUGIN_NAME_LEN}-char limit"
        )));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
    {
        return Err(ConfigError::Io(format!(
            "Plugin name `{name}` contains invalid character {bad:?}: \
             only ASCII letters, digits, `_`, `-`, `.` are allowed"
        )));
    }
    // `..` is alphabet-valid but would let a `find_in_dir` lookup escape
    // the search directory via `dir/lib...so`. Reject any occurrence.
    if name.contains("..") {
        return Err(ConfigError::Io(format!(
            "Plugin name `{name}` must not contain `..`"
        )));
    }
    Ok(())
}

/// Cargo cdylib filename for a plugin `name`.
///
/// Mirrors Cargo's `crate-name → lib<crate_name>.{so,dylib}` convention,
/// including the `-` → `_` substitution: `murphy-rails` →
/// `libmurphy_rails.so`. Plugin packs in this project are always built as
/// Cargo cdylibs, so a single canonical name keeps the resolver and the
/// build artifact in sync (no two-name fallback search).
pub fn lib_filename(name: &str) -> String {
    let sanitized = name.replace('-', "_");
    let ext = if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    format!("lib{sanitized}.{ext}")
}

/// Resolve a plugin `name` into an absolute path using the given
/// `overrides` map (from `Detailed { name, path }` entries) and the
/// ordered `search_dirs` list. Pure function — used directly in tests; the
/// production wrapper [`resolve_plugin_name`] supplies the env / project /
/// user search dirs.
pub fn resolve_plugin_name_with_search_dirs(
    name: &str,
    overrides: &BTreeMap<String, PathBuf>,
    search_dirs: &[PathBuf],
) -> Result<PathBuf, ConfigError> {
    if let Some(path) = overrides.get(name) {
        return Ok(path.clone());
    }
    let filename = lib_filename(name);
    for dir in search_dirs {
        let candidate = dir.join(&filename);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    let searched = if search_dirs.is_empty() {
        "<none>".to_string()
    } else {
        search_dirs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    Err(ConfigError::Io(format!(
        "Plugin `{name}` not found (looked for `{filename}`). \
         Searched: {searched}. To pin an explicit path, use the detailed form: \
         `[[plugins]] name = \"{name}\" path = \"...\"`."
    )))
}

/// Resolve a plugin `name` against the standard search path: env
/// (`MURPHY_PLUGIN_PATH`), then `<project_root>/.murphy/plugins/`, then
/// the user data dir (`dirs::data_dir()/murphy/plugins/`). `overrides`
/// come from any `Detailed { name, path }` entries declared in the same
/// `[[plugins]]` array.
///
/// The name is validated first ([`validate_plugin_name`]); invalid names
/// are rejected before any I/O happens.
pub fn resolve_plugin_name(
    name: &str,
    project_root: &Path,
    overrides: &BTreeMap<String, PathBuf>,
) -> Result<PathBuf, ConfigError> {
    validate_plugin_name(name)?;
    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if let Some(env) = std::env::var_os("MURPHY_PLUGIN_PATH") {
        search_dirs.extend(std::env::split_paths(&env));
    }
    search_dirs.push(project_root.join(".murphy/plugins"));
    if let Some(data) = dirs::data_dir() {
        search_dirs.push(data.join("murphy/plugins"));
    }
    resolve_plugin_name_with_search_dirs(name, overrides, &search_dirs)
}

/// Pre-pass that turns a `[[plugins]]` array into the ordered `(name,
/// resolved_path)` list the loader will actually open. Each name is
/// loaded **at most once**: a same-named `Detailed` entry always pins the
/// path (even when written after a `Name(String)` shorthand), and any
/// later duplicate of an already-seen name is silently dropped.
///
/// This is the dedup layer that prevents `plugins = ["foo", { name =
/// "foo", path = "./vendor.so" }]` from triggering the registry's
/// name-collision check by trying to load `foo` twice.
pub fn plan_plugin_loads(
    project_root: &Path,
    plugins: &[PluginConfig],
) -> Result<Vec<(String, PathBuf)>, ConfigError> {
    let overrides: BTreeMap<String, PathBuf> = plugins
        .iter()
        .filter_map(|p| match p {
            PluginConfig::Detailed(d) => Some((d.name.clone(), project_root.join(&d.path))),
            PluginConfig::Name(_) => None,
        })
        .collect();

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut plan: Vec<(String, PathBuf)> = Vec::new();
    for plugin in plugins {
        let name = match plugin {
            PluginConfig::Detailed(d) => &d.name,
            PluginConfig::Name(name) => name,
        };
        if !seen.insert(name.clone()) {
            continue;
        }
        let path = match plugin {
            // For a Detailed entry we know `overrides` has the resolved
            // path — same data we just inserted above.
            PluginConfig::Detailed(_) => overrides[name].clone(),
            PluginConfig::Name(_) => resolve_plugin_name(name, project_root, &overrides)?,
        };
        plan.push((name.clone(), path));
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PluginDetailed;

    #[test]
    fn validate_rejects_empty_name() {
        let err = validate_plugin_name("").expect_err("empty must be rejected");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("empty") || msg.contains("invalid"),
            "error should explain why empty is rejected, got: {msg}"
        );
    }

    #[test]
    fn validate_rejects_path_separator_and_parent_dir() {
        // path traversal — `..`, `/`, `\` must all be rejected so a
        // malicious `murphy.toml` can't turn `plugins = ["../../../etc/passwd"]`
        // into a `dir/lib../../../etc/passwd.so` lookup.
        for bad in ["../foo", "/abs/path", "foo/bar", "foo\\bar", ".."] {
            let err = match validate_plugin_name(bad) {
                Ok(()) => panic!("{bad:?} should be rejected, got Ok"),
                Err(e) => e,
            };
            let msg = format!("{err:?}");
            assert!(
                msg.contains("invalid character") || msg.contains(".."),
                "{bad:?}: error should mention invalid character or `..`, got: {msg}"
            );
        }
    }

    #[test]
    fn validate_does_not_panic_on_long_multibyte_name() {
        // Regression: the error path used `&name[..MAX_PLUGIN_NAME_LEN]`
        // for the "too long" message. With a name whose 64th byte falls
        // inside a multi-byte UTF-8 char (e.g. 63 ASCII + `あ`), that
        // slice would panic — turning a *validation* call into a crash.
        let mut name = "a".repeat(63);
        name.push('あ'); // 3-byte UTF-8 char straddling byte 64
        let err = validate_plugin_name(&name).expect_err("must reject without panicking");
        let msg = format!("{err:?}");
        // Either the length or the invalid-character branch fires; either
        // way the call returns cleanly instead of panicking.
        assert!(
            msg.contains("exceeds") || msg.contains("invalid character"),
            "expected length / charset error, got: {msg}"
        );
    }

    #[test]
    fn validate_accepts_rubocop_style_names() {
        // RuboCop plugin naming convention: `rubocop-X` becomes `murphy-X` in
        // Murphy. Allowed alphabet must include ASCII letters, digits, `_`,
        // `-`, `.`.
        for ok in ["murphy-rails", "murphy_rspec", "murphy.local", "M9", "x"] {
            validate_plugin_name(ok).unwrap_or_else(|err| panic!("{ok:?} rejected: {err:?}"));
        }
    }

    #[test]
    fn lib_filename_applies_cargo_cdylib_hyphen_to_underscore() {
        // Cargo cdylib for crate `murphy-example-pack` outputs
        // `libmurphy_example_pack.{so,dylib}`. The resolver must use the
        // same convention or RuboCop-style `plugins = ["murphy-rails"]`
        // shorthand cannot find the built artifact.
        let name = lib_filename("murphy-example-pack");
        let expected_ext = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        assert_eq!(name, format!("libmurphy_example_pack.{expected_ext}"));
    }

    #[test]
    fn resolve_returns_detailed_override_without_touching_search_dirs() {
        // `overrides` is treated as a hard pin: a Detailed entry's path
        // wins even when a same-named lib exists in a search dir below.
        let dir = tempfile::tempdir().expect("tempdir");
        let search_dir = dir.path().to_path_buf();
        std::fs::write(search_dir.join(lib_filename("foo")), b"").expect("write fake .so");

        let pinned = PathBuf::from("/explicit/override.so");
        let overrides: BTreeMap<String, PathBuf> =
            std::iter::once(("foo".to_string(), pinned.clone())).collect();

        let got = resolve_plugin_name_with_search_dirs("foo", &overrides, &[search_dir]).unwrap();
        assert_eq!(got, pinned);
    }

    #[test]
    fn resolve_finds_in_first_search_dir_that_contains_the_lib() {
        // search_dirs are tried in order; the first hit wins. Skip empty
        // dirs and find `libfoo.{so,dylib}` in the second.
        let empty = tempfile::tempdir().expect("tempdir 1");
        let populated = tempfile::tempdir().expect("tempdir 2");
        let expected = populated.path().join(lib_filename("foo"));
        std::fs::write(&expected, b"").expect("write fake .so");

        let overrides = BTreeMap::new();
        let dirs = vec![empty.path().to_path_buf(), populated.path().to_path_buf()];

        let got = resolve_plugin_name_with_search_dirs("foo", &overrides, &dirs).unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn resolve_applies_hyphen_to_underscore_when_searching() {
        // `plugins = ["murphy-rails"]` must locate Cargo's
        // `libmurphy_rails.{so,dylib}` artifact.
        let dir = tempfile::tempdir().expect("tempdir");
        let expected = dir.path().join(lib_filename("murphy-rails"));
        std::fs::write(&expected, b"").expect("write fake .so");

        let got = resolve_plugin_name_with_search_dirs(
            "murphy-rails",
            &BTreeMap::new(),
            &[dir.path().to_path_buf()],
        )
        .unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn resolve_emits_not_found_with_search_path_hint() {
        // Missing plugin: error must list the dirs we searched and point
        // the user at the `Detailed` escape hatch.
        let empty = tempfile::tempdir().expect("tempdir");
        let dir = empty.path().to_path_buf();
        let err = resolve_plugin_name_with_search_dirs(
            "missing",
            &BTreeMap::new(),
            std::slice::from_ref(&dir),
        )
        .expect_err("missing plugin must fail");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("missing"),
            "error must echo plugin name: {msg}"
        );
        assert!(msg.contains("not found"), "error must say not found: {msg}");
        assert!(
            msg.contains(&dir.display().to_string()),
            "error must list searched dir {}: {msg}",
            dir.display()
        );
        assert!(
            msg.contains("[[plugins]]") && msg.contains("path"),
            "error must point user at the detailed form: {msg}"
        );
    }

    #[test]
    fn resolve_wrapper_finds_lib_in_project_local_dot_murphy_plugins() {
        // The production wrapper adds `<project_root>/.murphy/plugins/` to
        // the search path. Build a project root where that dir exists,
        // unset `MURPHY_PLUGIN_PATH` to keep the env-derived dir out of
        // the way, and confirm the wrapper finds the artifact.
        //
        // Safety: the unsafe `remove_var` call is the standard Rust 2024
        // idiom for env mutation in tests. Concurrent env mutation in
        // other tests is the documented hazard — but no other test in
        // this module touches `MURPHY_PLUGIN_PATH`.
        unsafe { std::env::remove_var("MURPHY_PLUGIN_PATH") };

        let project = tempfile::tempdir().expect("tempdir");
        let plugin_dir = project.path().join(".murphy/plugins");
        std::fs::create_dir_all(&plugin_dir).expect("create .murphy/plugins");
        let expected = plugin_dir.join(lib_filename("murphy-example-pack"));
        std::fs::write(&expected, b"").expect("write fake .so");

        let got =
            resolve_plugin_name("murphy-example-pack", project.path(), &BTreeMap::new()).unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn resolve_wrapper_validates_name_before_io() {
        // The wrapper must reject path-traversal names before any
        // directory lookup runs, regardless of what `overrides` contains.
        let project = tempfile::tempdir().expect("tempdir");
        let err = resolve_plugin_name("../bad", project.path(), &BTreeMap::new())
            .expect_err("path-traversal name must fail");
        assert!(
            format!("{err:?}").contains("invalid character") || format!("{err:?}").contains(".."),
            "wrapper must surface validate_plugin_name error: {err:?}"
        );
    }

    #[test]
    fn plan_dedupes_name_followed_by_detailed_same_name() {
        // `["foo", { name = "foo", path = "./vendor.so" }]` must result in
        // a single load using the `Detailed` path — array order does not
        // matter, the explicit form always wins.
        let project = tempfile::tempdir().expect("tempdir");
        let plugins = vec![
            PluginConfig::Name("foo".to_string()),
            PluginConfig::Detailed(PluginDetailed {
                name: "foo".to_string(),
                path: PathBuf::from("./vendor.so"),
            }),
        ];
        let plan = plan_plugin_loads(project.path(), &plugins).unwrap();
        assert_eq!(plan.len(), 1, "single load expected: {plan:?}");
        let (name, path) = &plan[0];
        assert_eq!(name, "foo");
        assert_eq!(path, &project.path().join("vendor.so"));
    }

    #[test]
    fn plan_dedupes_detailed_followed_by_name_same_name() {
        // Same as above but the `Detailed` comes first. Still 1 load.
        let project = tempfile::tempdir().expect("tempdir");
        let plugins = vec![
            PluginConfig::Detailed(PluginDetailed {
                name: "foo".to_string(),
                path: PathBuf::from("./vendor.so"),
            }),
            PluginConfig::Name("foo".to_string()),
        ];
        let plan = plan_plugin_loads(project.path(), &plugins).unwrap();
        assert_eq!(plan.len(), 1, "single load expected: {plan:?}");
        assert_eq!(plan[0].1, project.path().join("vendor.so"));
    }

    #[test]
    fn plan_passes_through_detailed_only_entries_unchanged() {
        // Pure Detailed input round-trips with paths resolved against the
        // project root. Multiple distinct names produce multiple entries.
        let project = tempfile::tempdir().expect("tempdir");
        let plugins = vec![
            PluginConfig::Detailed(PluginDetailed {
                name: "a".to_string(),
                path: PathBuf::from("./a.so"),
            }),
            PluginConfig::Detailed(PluginDetailed {
                name: "b".to_string(),
                path: PathBuf::from("./b.so"),
            }),
        ];
        let plan = plan_plugin_loads(project.path(), &plugins).unwrap();
        let names: Vec<&str> = plan.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(plan[0].1, project.path().join("a.so"));
        assert_eq!(plan[1].1, project.path().join("b.so"));
    }

    #[test]
    fn plan_resolves_name_via_search_path_when_no_override() {
        // A bare `Name` with no matching `Detailed` must hit the search
        // path. Use project-local `.murphy/plugins/` (env-independent) so
        // the test stays hermetic.
        //
        // Safety: see `resolve_wrapper_finds_lib_in_project_local_dot_murphy_plugins`.
        unsafe { std::env::remove_var("MURPHY_PLUGIN_PATH") };

        let project = tempfile::tempdir().expect("tempdir");
        let plugin_dir = project.path().join(".murphy/plugins");
        std::fs::create_dir_all(&plugin_dir).expect("create dir");
        let expected = plugin_dir.join(lib_filename("murphy-rails"));
        std::fs::write(&expected, b"").expect("write fake .so");

        let plugins = vec![PluginConfig::Name("murphy-rails".to_string())];
        let plan = plan_plugin_loads(project.path(), &plugins).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].0, "murphy-rails");
        assert_eq!(plan[0].1, expected);
    }
}
