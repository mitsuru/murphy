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
//! Within a directory the loader first probes the pack-dir form
//! `<dir>/<name>/` holding a `murphy-plugin.toml` manifest (ADR 0046 —
//! the cdylib under its `lib/` is used), then falls back to the legacy
//! `lib<sanitized>.{so,dylib}` file where `<sanitized>` is `name` with
//! `-` replaced by `_` (Cargo cdylib naming convention — `murphy-rails`
//! → `libmurphy_rails.so`). The fallback keeps pre-manifest search-path
//! layouts working unchanged.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::plugin_loader::{LoadKind, PluginLoadDiagnostic, ResolveFailure};
use crate::plugin_manifest::{is_pack_dir, resolve_pack_cdylib};
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

/// Probe one search dir for a plugin `name`: the pack-dir form first
/// (`<dir>/<name>/murphy-plugin.toml` + `lib/` cdylib, ADR 0046), then the
/// legacy `lib<sanitized>.{so,dylib}` file.
///
/// A pack dir whose manifest is broken or whose `lib/` holds no cdylib is
/// skipped (falling through to the legacy file, then the next dir) — a
/// half-installed pack must not shadow a working copy later in the path.
/// Use the `Detailed { path }` form to surface manifest errors directly.
fn find_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let pack_dir = dir.join(name);
    if is_pack_dir(&pack_dir)
        && let Ok(cdylib) = resolve_pack_cdylib(&pack_dir)
        && cdylib.is_file()
    {
        return Some(cdylib);
    }
    let legacy = dir.join(lib_filename(name));
    legacy.is_file().then_some(legacy)
}

/// Map a `Detailed { path }` (project-root-joined) to the cdylib the
/// loader opens: a pack dir (`murphy-plugin.toml` + `lib/`, ADR 0046)
/// resolves to its cdylib; anything else passes through unchanged (the
/// legacy direct-`.so` reference).
fn normalize_pack_path(path: &Path) -> Result<PathBuf, ConfigError> {
    if !is_pack_dir(path) {
        return Ok(path.to_path_buf());
    }
    resolve_pack_cdylib(path)
        .map_err(|e| ConfigError::Io(format!("plugin pack `{}`: {e}", path.display())))
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
) -> Result<PathBuf, ResolveFailure> {
    if let Some(path) = overrides.get(name) {
        return Ok(path.clone());
    }
    let filename = lib_filename(name);
    for dir in search_dirs {
        if let Some(pack_cdylib) = find_in_dir(dir, name) {
            return Ok(pack_cdylib);
        }
    }
    Err(ResolveFailure {
        filename,
        searched_dirs: search_dirs.to_vec(),
    })
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
    resolve_plugin_name_with_search_dirs(name, overrides, &search_dirs).map_err(|failure| {
        ConfigError::PluginLoad(PluginLoadDiagnostic {
            plugin_name: name.to_string(),
            attempted_path: None,
            kind: LoadKind::Resolve(failure),
        })
    })
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
        let raw = match plugin {
            // For a Detailed entry we know `overrides` has the resolved
            // path — same data we just inserted above.
            PluginConfig::Detailed(_) => overrides[name].clone(),
            PluginConfig::Name(_) => resolve_plugin_name(name, project_root, &overrides)?,
        };
        // A `Detailed` path may point at a pack dir instead of a direct
        // `.so` (ADR 0046); a `Name` pinned via overrides may too. The
        // loader below only ever opens a cdylib, so normalize here.
        let path = normalize_pack_path(&raw)?;
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
    fn resolve_emits_structured_failure_carrying_filename_and_search_dirs() {
        // Missing plugin: returns ResolveFailure with the filename it
        // looked for and the dirs it probed, structurally — the user-
        // facing rendering happens in PluginLoadDiagnostic (murphy-tvh),
        // not here.
        let empty = tempfile::tempdir().expect("tempdir");
        let dir = empty.path().to_path_buf();
        let failure = resolve_plugin_name_with_search_dirs(
            "missing",
            &BTreeMap::new(),
            std::slice::from_ref(&dir),
        )
        .expect_err("missing plugin must fail");
        assert_eq!(failure.filename, lib_filename("missing"));
        assert_eq!(failure.searched_dirs, vec![dir]);
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

    /// Build a pack dir `<parent>/<name>/` with a manifest + one cdylib
    /// under `lib/<arch>/`. Returns the cdylib path.
    fn write_pack_dir(parent: &Path, name: &str) -> PathBuf {
        let pack = parent.join(name);
        let arch = pack.join("lib").join("linux-x86_64");
        std::fs::create_dir_all(&arch).expect("mkdir lib/<arch>");
        std::fs::write(
            pack.join(crate::plugin_manifest::MANIFEST_FILENAME),
            format!("[plugin]\nname = \"{name}\"\nversion = \"0.1.0\"\nmurphy-api-version = 4\n"),
        )
        .expect("write manifest");
        let cdylib = arch.join(format!("lib{}.so", name.replace('-', "_")));
        std::fs::write(&cdylib, b"").expect("write fake .so");
        cdylib
    }

    #[test]
    fn search_prefers_pack_dir_over_legacy_so() {
        // ADR 0046 §3: `<dir>/<name>/` (manifest + lib/) wins over the
        // legacy `<dir>/lib<sanitized>.so` in the same dir.
        let dir = tempfile::tempdir().expect("tempdir");
        let pack_cdylib = write_pack_dir(dir.path(), "murphy-rails");
        std::fs::write(dir.path().join(lib_filename("murphy-rails")), b"")
            .expect("write legacy .so");

        let got = resolve_plugin_name_with_search_dirs(
            "murphy-rails",
            &BTreeMap::new(),
            &[dir.path().to_path_buf()],
        )
        .unwrap();
        assert_eq!(got, pack_cdylib);
    }

    #[test]
    fn search_falls_back_to_legacy_when_pack_dir_is_broken() {
        // A half-installed pack (manifest but no cdylib) must not shadow
        // the working legacy file in the same dir.
        let dir = tempfile::tempdir().expect("tempdir");
        let pack = dir.path().join("murphy-rails");
        std::fs::create_dir_all(pack.join("lib")).expect("mkdir");
        std::fs::write(
            pack.join(crate::plugin_manifest::MANIFEST_FILENAME),
            "[plugin]\nname = \"murphy-rails\"\nversion = \"1\"\nmurphy-api-version = 4\n",
        )
        .expect("write manifest");
        let legacy = dir.path().join(lib_filename("murphy-rails"));
        std::fs::write(&legacy, b"").expect("write legacy .so");

        let got = resolve_plugin_name_with_search_dirs(
            "murphy-rails",
            &BTreeMap::new(),
            &[dir.path().to_path_buf()],
        )
        .unwrap();
        assert_eq!(got, legacy);
    }

    #[test]
    fn plan_normalizes_detailed_pack_dir_to_its_cdylib() {
        // `Detailed { path }` accepts a pack dir (ADR 0046 §3): the plan
        // carries the resolved cdylib, so the loader below is unchanged.
        let project = tempfile::tempdir().expect("tempdir");
        let cdylib = write_pack_dir(project.path(), "vendor-pack");
        let plugins = vec![PluginConfig::Detailed(PluginDetailed {
            name: "vendor-pack".to_string(),
            path: PathBuf::from("./vendor-pack"),
        })];
        let plan = plan_plugin_loads(project.path(), &plugins).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].0, "vendor-pack");
        assert_eq!(plan[0].1, cdylib);
    }

    #[test]
    fn plan_passes_detailed_direct_so_through_unchanged() {
        // Backward compatibility: a direct `.so` path is untouched.
        let project = tempfile::tempdir().expect("tempdir");
        let so = project.path().join("libfoo.so");
        std::fs::write(&so, b"").expect("write fake .so");
        let plugins = vec![PluginConfig::Detailed(PluginDetailed {
            name: "foo".to_string(),
            path: PathBuf::from("./libfoo.so"),
        })];
        let plan = plan_plugin_loads(project.path(), &plugins).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].1, so);
    }

    #[test]
    fn plan_errors_on_pack_dir_without_cdylib() {
        // A `Detailed` pack dir with an empty `lib/` is a user error with
        // a manifest-shaped message — not a silent dlopen of a directory.
        let project = tempfile::tempdir().expect("tempdir");
        let pack = project.path().join("broken-pack");
        std::fs::create_dir_all(pack.join("lib")).expect("mkdir");
        std::fs::write(
            pack.join(crate::plugin_manifest::MANIFEST_FILENAME),
            "[plugin]\nname = \"broken-pack\"\nversion = \"1\"\nmurphy-api-version = 4\n",
        )
        .expect("write manifest");
        let plugins = vec![PluginConfig::Detailed(PluginDetailed {
            name: "broken-pack".to_string(),
            path: PathBuf::from("./broken-pack"),
        })];
        let err = plan_plugin_loads(project.path(), &plugins).expect_err("must fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("no cdylib"),
            "error must explain the pack dir holds no cdylib: {msg}"
        );
    }

    #[test]
    fn plan_name_pinned_by_detailed_pack_dir_override_loads_once() {
        // `Name` + same-name `Detailed`-as-pack-dir dedups to one load of
        // the pack's cdylib (override pin flows through normalization).
        //
        // Safety: see `resolve_wrapper_finds_lib_in_project_local_dot_murphy_plugins`.
        unsafe { std::env::remove_var("MURPHY_PLUGIN_PATH") };

        let project = tempfile::tempdir().expect("tempdir");
        let cdylib = write_pack_dir(project.path(), "murphy-rails");
        let plugins = vec![
            PluginConfig::Name("murphy-rails".to_string()),
            PluginConfig::Detailed(PluginDetailed {
                name: "murphy-rails".to_string(),
                path: PathBuf::from("./murphy-rails"),
            }),
        ];
        let plan = plan_plugin_loads(project.path(), &plugins).unwrap();
        assert_eq!(plan.len(), 1, "single load expected: {plan:?}");
        assert_eq!(plan[0].1, cdylib);
    }
}
