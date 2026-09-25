//! Gem / Bundler pack discovery for `[[plugins]]` `Name(String)` shorthand
//! (murphy-fmw.3.2, C2; ADR 0048).
//!
//! A `murphy-*` cop pack can ship as a Ruby gem: the installed gem
//! directory holds a `murphy-plugin.toml` manifest (ADR 0046 pack root)
//! plus per-arch cdylibs under `lib/`, or — for legacy single-file packs —
//! a `lib<sanitized>.{so,dylib}` file under the gem's `lib/`. When the
//! user declares `plugins = ["murphy-rails"]` in `.murphy.yml` and runs
//! under `bundle exec` (or with RubyGems env set), the resolver finds the
//! pack inside the already-installed gems, so the `Gemfile` is the version
//! manager (RuboCop parity: `Gemfile` owns gem versions, `.murphy.yml`
//! owns cop configuration).
//!
//! ## Sources (in order)
//!
//! 1. `MURPHY_GEM_PATH` — explicit override, `split_paths` list. Each entry
//!    is either a gemdir (holding a `gems/` subdir) or a `gems/` dir
//!    itself. Test hook + manual override.
//! 2. `GEM_HOME` — single RubyGems home dir.
//! 3. `GEM_PATH` — `split_paths` list of additional gemdirs.
//!
//! Under `bundle exec`, Bundler sets `GEM_HOME`/`GEM_PATH` to the bundle's
//! paths, so no `bundle show` subprocess is needed: the same env the Ruby
//! wrapper (`exe/murphy`) inherits is what the native binary reads. The
//! wrapper also exports `MURPHY_GEM_PATH` from `gem environment gemdir`
//! as a fallback when neither `GEM_HOME` nor `GEM_PATH` is set (plain
//! `gem install murphy` outside Bundler).
//!
//! ## Gem layout probing
//!
//! For plugin `name`, candidate gem dirs are `<gems>/<name>-<version>/`
//! sorted by version descending (highest wins). Each candidate is probed:
//!
//! 1. `<gem>/` as pack dir (`murphy-plugin.toml` + `lib/` cdylib).
//! 2. `<gem>/murphy/` as pack dir (avoids `lib/` clashing with Ruby files).
//! 3. legacy `<gem>/lib/<lib_filename>` single file.
//!
//! A candidate that probes clean is returned; otherwise the next version
//! is tried. A half-installed gem (manifest but no cdylib) never shadows
//! a working copy later in the search path — the caller (`plugin_resolver`)
//! treats a `None` here as "try the next search layer".

use std::path::{Path, PathBuf};

use crate::plugin_manifest::{is_pack_dir, resolve_pack_cdylib};
use crate::plugin_resolver::lib_filename;

/// Env var for an explicit gem search path (colon-separated, `split_paths`).
pub const GEM_PATH_ENV: &str = "MURPHY_GEM_PATH";

/// Process-global lock serializing env-touching tests.
///
/// Exposed `pub(crate)` so `plugin_resolver`'s gem-layer tests can share
/// the same critical section: `std::env` is process-global and parallel
/// `set_var` / `remove_var` would otherwise interleave across modules.
#[cfg(test)]
pub(crate) static GEM_ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
    std::sync::OnceLock::new();

/// Acquire the gem-env critical section. Test-only.
#[cfg(test)]
pub(crate) fn lock_gem_env() -> std::sync::MutexGuard<'static, ()> {
    GEM_ENV_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .expect("gem env lock")
}

/// Collect the `gems/` directories to scan, in priority order.
///
/// Each env entry may be a gemdir (containing `gems/`) or a `gems/` dir
/// itself; both shapes are probed so `GEM_HOME=/usr/local/bundle`,
/// `GEM_HOME=/usr/local/bundle/gems`, and `MURPHY_GEM_PATH=<gems>` all work.
/// Duplicates are removed, order preserved.
pub fn gem_gems_dirs() -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(env) = std::env::var_os(GEM_PATH_ENV) {
        bases.extend(std::env::split_paths(&env));
    }
    if let Some(home) = std::env::var_os("GEM_HOME") {
        bases.push(PathBuf::from(home));
    }
    if let Some(path) = std::env::var_os("GEM_PATH") {
        bases.extend(std::env::split_paths(&path));
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for base in bases {
        // Prefer `<base>/gems` when it exists (gemdir shape), else treat
        // `<base>` itself as the gems dir (explicit `.../gems` shape).
        let gems = base.join("gems");
        let candidate = if gems.is_dir() { gems } else { base };
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

/// Probe one installed gem directory for plugin `name`.
///
/// Returns the cdylib path when the gem holds a usable pack, else `None`.
fn probe_gem_dir(gem_dir: &Path, name: &str) -> Option<PathBuf> {
    // 1. Gem root as pack dir.
    if is_pack_dir(gem_dir)
        && let Ok(cdylib) = resolve_pack_cdylib(gem_dir)
        && cdylib.is_file()
    {
        return Some(cdylib);
    }
    // 2. `<gem>/murphy/` subdir as pack dir.
    let sub = gem_dir.join("murphy");
    if is_pack_dir(&sub)
        && let Ok(cdylib) = resolve_pack_cdylib(&sub)
        && cdylib.is_file()
    {
        return Some(cdylib);
    }
    // 3. Legacy single file under the gem's `lib/`.
    let legacy = gem_dir.join("lib").join(lib_filename(name));
    legacy.is_file().then_some(legacy)
}

/// Compare gem version strings (`"0.3.1"`) for descending order.
///
/// Splits on `.`, compares numeric segments numerically, non-numeric
/// segments lexically; missing segments count as zero. Good enough for
/// picking the newest installed `murphy-*` gem — exact semver precedence
/// (pre-release ordering) is Bundler's job, not the resolver's.
fn compare_versions_desc(a: &str, b: &str) -> std::cmp::Ordering {
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Seg {
        Num(u64),
        Str(String),
    }
    let parse = |v: &str| {
        v.split('.')
            .map(|s| match s.parse::<u64>() {
                Ok(n) => Seg::Num(n),
                Err(_) => Seg::Str(s.to_string()),
            })
            .collect::<Vec<_>>()
    };
    let av = parse(a);
    let bv = parse(b);
    // Descending: compare b vs a so the newest sorts first.
    bv.cmp(&av).then_with(|| b.cmp(a))
}

/// Find plugin `name` inside the given `gems/` directories.
///
/// Scans each dir for entries starting with `"<name>-"`, sorts candidates
/// by version descending (highest first), and returns the first candidate
/// whose probe yields a cdylib. Returns `None` when nothing matches.
pub fn find_gem_pack_with_dirs(name: &str, gems_dirs: &[PathBuf]) -> Option<PathBuf> {
    let prefix = format!("{name}-");
    for gems in gems_dirs {
        let entries = std::fs::read_dir(gems).ok()?;
        let mut candidates: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if let Some(version) = file_name.strip_prefix(&prefix)
                && !version.is_empty()
                && entry.path().is_dir()
            {
                candidates.push((version.to_string(), entry.path()));
            }
        }
        candidates.sort_by(|a, b| compare_versions_desc(&a.0, &b.0));
        for (_, gem_dir) in candidates {
            if let Some(cdylib) = probe_gem_dir(&gem_dir, name) {
                return Some(cdylib);
            }
        }
    }
    None
}

/// Find plugin `name` inside the installed gems from the environment
/// ([`gem_gems_dirs`]). Pure-env wrapper over [`find_gem_pack_with_dirs`].
pub fn find_gem_pack(name: &str) -> Option<PathBuf> {
    let dirs = gem_gems_dirs();
    if dirs.is_empty() {
        return None;
    }
    find_gem_pack_with_dirs(name, &dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clear gem env vars. Callers must hold [`crate::gem_discovery::lock_gem_env`] across the
    /// whole clear → set → assert → clear sequence.
    ///
    /// Safety: see above; holding the lock makes the sequence atomic
    /// w.r.t. the other env-touching tests in this module.
    fn clear_gem_env() {
        unsafe {
            std::env::remove_var(GEM_PATH_ENV);
            std::env::remove_var("GEM_HOME");
            std::env::remove_var("GEM_PATH");
        }
    }

    fn write_manifest(pack_dir: &Path, name: &str) {
        std::fs::create_dir_all(pack_dir).expect("mkdir pack");
        std::fs::write(
            pack_dir.join(crate::plugin_manifest::MANIFEST_FILENAME),
            format!("[plugin]\nname = \"{name}\"\nversion = \"0.1.0\"\nmurphy-api-version = 4\n"),
        )
        .expect("write manifest");
    }

    fn write_cdylib(pack_dir: &Path, name: &str) -> PathBuf {
        let arch = pack_dir.join("lib").join("linux-x86_64");
        std::fs::create_dir_all(&arch).expect("mkdir lib/<arch>");
        let cdylib = arch.join(lib_filename(name));
        std::fs::write(&cdylib, b"").expect("write fake .so");
        cdylib
    }

    #[test]
    fn empty_env_yields_no_dirs() {
        let _guard = lock_gem_env();
        clear_gem_env();
        assert!(gem_gems_dirs().is_empty());
        clear_gem_env();
    }

    #[test]
    fn murphy_gem_path_points_directly_at_gems_dir() {
        let _guard = lock_gem_env();
        clear_gem_env();
        let gems = tempfile::tempdir().expect("tempdir");
        unsafe {
            std::env::set_var(GEM_PATH_ENV, gems.path());
        }
        assert_eq!(gem_gems_dirs(), vec![gems.path().to_path_buf()]);
        clear_gem_env();
    }

    #[test]
    fn gem_home_with_gems_subdir_resolves_to_it() {
        let _guard = lock_gem_env();
        clear_gem_env();
        let home = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(home.path().join("gems")).expect("mkdir gems");
        unsafe {
            std::env::set_var("GEM_HOME", home.path());
        }
        assert_eq!(gem_gems_dirs(), vec![home.path().join("gems")]);
        clear_gem_env();
    }

    #[test]
    fn finds_pack_at_gem_root() {
        let gems = tempfile::tempdir().expect("tempdir");
        let gem_dir = gems.path().join("murphy-rails-0.3.1");
        write_manifest(&gem_dir, "murphy-rails");
        let cdylib = write_cdylib(&gem_dir, "murphy-rails");
        let got = find_gem_pack_with_dirs("murphy-rails", &[gems.path().to_path_buf()]);
        assert_eq!(got, Some(cdylib));
    }

    #[test]
    fn finds_pack_in_murphy_subdir() {
        let gems = tempfile::tempdir().expect("tempdir");
        let gem_dir = gems.path().join("murphy-rails-0.3.1");
        std::fs::create_dir_all(gem_dir.join("lib")).expect("mkdir lib");
        let sub = gem_dir.join("murphy");
        write_manifest(&sub, "murphy-rails");
        let cdylib = write_cdylib(&sub, "murphy-rails");
        let got = find_gem_pack_with_dirs("murphy-rails", &[gems.path().to_path_buf()]);
        assert_eq!(got, Some(cdylib));
    }

    #[test]
    fn finds_legacy_so_under_gem_lib() {
        let gems = tempfile::tempdir().expect("tempdir");
        let gem_dir = gems.path().join("murphy-rails-0.3.1");
        let legacy = gem_dir.join("lib").join(lib_filename("murphy-rails"));
        std::fs::create_dir_all(legacy.parent().unwrap()).expect("mkdir lib");
        std::fs::write(&legacy, b"").expect("write fake .so");
        let got = find_gem_pack_with_dirs("murphy-rails", &[gems.path().to_path_buf()]);
        assert_eq!(got, Some(legacy));
    }

    #[test]
    fn picks_highest_version_first() {
        let gems = tempfile::tempdir().expect("tempdir");
        for v in ["0.3.1", "0.9.0", "0.10.0"] {
            let gem_dir = gems.path().join(format!("murphy-rails-{v}"));
            write_manifest(&gem_dir, "murphy-rails");
            write_cdylib(&gem_dir, "murphy-rails");
        }
        // Lexical descending would pick `0.9.0`; numeric-aware must pick `0.10.0`.
        let got = find_gem_pack_with_dirs("murphy-rails", &[gems.path().to_path_buf()]).unwrap();
        assert!(
            got.to_string_lossy().contains("0.10.0"),
            "highest version must win, got: {}",
            got.display()
        );
    }

    #[test]
    fn skips_broken_gem_and_falls_to_next_version() {
        let gems = tempfile::tempdir().expect("tempdir");
        // Newest gem is half-installed (manifest, no cdylib).
        let broken = gems.path().join("murphy-rails-0.4.0");
        write_manifest(&broken, "murphy-rails");
        std::fs::create_dir_all(broken.join("lib")).expect("mkdir lib");
        // Older gem is complete.
        let good = gems.path().join("murphy-rails-0.3.1");
        write_manifest(&good, "murphy-rails");
        let cdylib = write_cdylib(&good, "murphy-rails");
        let got = find_gem_pack_with_dirs("murphy-rails", &[gems.path().to_path_buf()]);
        assert_eq!(got, Some(cdylib));
    }

    #[test]
    fn ignores_unrelated_gems() {
        let gems = tempfile::tempdir().expect("tempdir");
        let other = gems.path().join("rails-8.0.0");
        write_manifest(&other, "rails");
        write_cdylib(&other, "rails");
        let got = find_gem_pack_with_dirs("murphy-rails", &[gems.path().to_path_buf()]);
        assert_eq!(got, None);
    }

    #[test]
    fn find_gem_pack_reads_murphy_gem_path_env() {
        let _guard = lock_gem_env();
        clear_gem_env();
        let gems = tempfile::tempdir().expect("tempdir");
        let gem_dir = gems.path().join("murphy-foo-1.0.0");
        write_manifest(&gem_dir, "murphy-foo");
        let cdylib = write_cdylib(&gem_dir, "murphy-foo");
        unsafe {
            std::env::set_var(GEM_PATH_ENV, gems.path());
        }
        assert_eq!(find_gem_pack("murphy-foo"), Some(cdylib));
        clear_gem_env();
    }
}
