//! Stale plugin-pack detection + refresh into project `.murphy/plugins/`.
//!
//! Background (murphy-ghxy): a rebuilt pack `.so` under `target/release`
//! (or `debug`) is NOT automatically propagated to a project's
//! `.murphy/plugins/` dir. The project keeps `dlopen`-ing the stale copy,
//! so pack-bundled config/cop changes silently don't take effect until a
//! manual `cp -f target/release/libmurphy_*.so <project>/.murphy/plugins/`
//! is run.
//!
//! This module provides the small, testable core for the fix:
//!
//! - [`candidate_dirs`] — where a fresher build of the same `lib<name>.so`
//!   might live (`--from` dirs, `MURPHY_PLUGIN_PATH`, the `murphy` binary's
//!   own dir such as `target/debug/`, plus `<project>/target/{debug,release}`).
//! - [`is_stale`] — "fresh is newer than staged" (mtime, with an
//!   equal-mtime-but-different-bytes fallback for coarse filesystems).
//! - [`find_fresh_candidate`] — newest stale-capable `lib<name>.so` for one plugin.
//! - [`check_project_with_config`] — per-plugin stale/missing report for a
//!   project (only for packs staged inside `<project>/.murphy/plugins/`;
//!   `Detailed` paths pointing elsewhere are already live and need no sync).
//! - [`refresh_staged`] — copy fresh over staged (creating parent dirs).
//!
//! The CLI wires this as `murphy plugins sync [--from DIR] [--check]` and
//! as a non-failing stderr warning at lint time. The native plugin ABI is
//! untouched.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::MurphyConfig;
use crate::PluginConfig;
use crate::plugin_resolver::{lib_filename, plan_plugin_loads};

/// One plugin whose staged copy in `<project>/.murphy/plugins/` is older
/// than (or missing while) a fresher build exists elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleReport {
    /// Plugin name as in `plugins = [...]` (e.g. `murphy-rails`).
    pub name: String,
    /// Where the project currently loads it from (inside
    /// `<project>/.murphy/plugins/`; may not exist yet when `missing`).
    pub staged: PathBuf,
    /// The fresher same-named artifact found in a candidate dir.
    pub fresh: PathBuf,
    /// True when `staged` does not exist yet (fresh install, not refresh).
    pub missing: bool,
}

impl StaleReport {
    /// Human-readable one-line warning, e.g. for lint-time stderr.
    pub fn warning(&self) -> String {
        if self.missing {
            format!(
                "warning: plugin `{}` is not staged in `.murphy/plugins/`; found build at `{}` — run `murphy plugins sync --from <dir>` to install it",
                self.name,
                self.fresh.display()
            )
        } else {
            format!(
                "warning: plugin `{}` loaded from `{}` is older than `{}` — run `murphy plugins sync --from <dir>` (or `cp -f {} {}`) to refresh it",
                self.name,
                self.staged.display(),
                self.fresh.display(),
                self.fresh.display(),
                self.staged.display()
            )
        }
    }
}

/// Candidate dirs that may hold a fresher build of a staged pack.
///
/// Order (highest to lowest): explicit `--from` dirs, `MURPHY_PLUGIN_PATH`
/// entries, the `murphy` binary's own directory (typically
/// `target/{debug,release}/` when run via `cargo run`, where freshly built
/// packs sit alongside the binary), then `<project>/target/{debug,release}/`.
/// Missing dirs are kept (callers skip non-existent files); no I/O here
/// except reading the current exe path.
pub fn candidate_dirs(project_root: &Path, extra_from: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for d in extra_from {
        if !out.contains(d) {
            out.push(d.clone());
        }
    }
    if let Some(env) = std::env::var_os("MURPHY_PLUGIN_PATH") {
        for d in std::env::split_paths(&env) {
            if !out.contains(&d) {
                out.push(d);
            }
        }
    }
    // Fresh packs alongside the murphy binary (cargo run / cargo install
    // layouts). Best-effort: ignore errors.
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let dir = dir.to_path_buf();
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    for profile in ["debug", "release"] {
        let d = project_root.join("target").join(profile);
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// True when `staged` and `fresh` are different files and `fresh` should
/// replace `staged`.
///
/// - Same canonical path → false (same file, e.g. `Detailed` pointing at
///   `target/release/...` or a symlink into a candidate dir).
/// - `staged` missing → true when `fresh` exists (install case).
/// - Otherwise true when `fresh` mtime is strictly newer than `staged`
///   mtime, or mtimes are equal but bytes differ (coarse-filesystem
///   fallback; avoids missing a rebuild that landed within the same
///   mtime tick).
/// - Any I/O error reading metadata/bytes → false (conservative: never
///   report stale on unreadable files).
pub fn is_stale(staged: &Path, fresh: &Path) -> bool {
    // Same-file fast path (covers symlinked staging, which the e2e tests
    // use: staged is a symlink to the fresh artifact).
    if paths_same_file(staged, fresh) {
        return false;
    }
    let fresh_meta = match std::fs::metadata(fresh) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if !fresh_meta.is_file() {
        return false;
    }
    let staged_meta = match std::fs::metadata(staged) {
        Ok(m) => m,
        // Staged missing → install case.
        Err(_) => return true,
    };
    if !staged_meta.is_file() {
        return false;
    }
    // Identical bytes → never stale, even when the fresh file carries a
    // newer mtime (a rebuild that produced byte-identical output needs no
    // refresh; this also keeps `cp -p`-style timestamp-only churn quiet).
    if !bytes_differ(staged, fresh) {
        return false;
    }
    let staged_mtime = staged_meta.modified().ok();
    let fresh_mtime = fresh_meta.modified().ok();
    match (staged_mtime, fresh_mtime) {
        (Some(s), Some(f)) => f >= s,
        // Without mtimes we cannot order; differing bytes imply stale.
        _ => true,
    }
}

fn paths_same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    // Canonicalize both; a staged symlink into a candidate dir (the e2e
    // staging helper) canonicalizes to the same target.
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

fn bytes_differ(a: &Path, b: &Path) -> bool {
    let (Ok(ab), Ok(bb)) = (std::fs::read(a), std::fs::read(b)) else {
        return false;
    };
    ab != bb
}

/// Newest fresher `lib<name>.so` for `staged` among `search_dirs`, or
/// `None` when no candidate is stale-newer.
///
/// Skips candidates that are the same file as `staged` and candidates
/// that do not exist. When several stale candidates exist, the one with
/// the newest mtime wins (mtime-unavailable candidates sort last).
pub fn find_fresh_candidate(name: &str, staged: &Path, search_dirs: &[PathBuf]) -> Option<PathBuf> {
    let filename = lib_filename(name);
    let mut best: Option<(PathBuf, Option<std::time::SystemTime>)> = None;
    for dir in search_dirs {
        let cand = dir.join(&filename);
        if !cand.exists() {
            continue;
        }
        if paths_same_file(staged, &cand) {
            continue;
        }
        if !is_stale(staged, &cand) {
            continue;
        }
        let mtime = std::fs::metadata(&cand)
            .ok()
            .and_then(|m| m.modified().ok());
        let replace = match (&best, mtime) {
            (None, _) => true,
            (Some((_, best_m)), Some(m)) => match best_m {
                Some(b) => m > *b,
                None => true,
            },
            (Some(_), None) => false,
        };
        if replace {
            best = Some((cand, mtime));
        }
    }
    best.map(|(p, _)| p)
}

/// Copy `fresh` over `staged`, creating parent dirs as needed.
///
/// Returns `Ok(true)` when a copy happened. Uses `fs::copy` (content
/// copy; the new `staged` gets a fresh mtime, so a subsequent
/// [`is_stale`] check reports up-to-date).
pub fn refresh_staged(staged: &Path, fresh: &Path) -> std::io::Result<bool> {
    if let Some(parent) = staged.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // If staged is a symlink, replace the link target path itself would
    // leave the link pointing at the old bytes; remove first so the copy
    // writes a regular file. Best-effort: ignore `symlink_metadata` errors
    // and let `fs::copy` surface real failures.
    if std::fs::symlink_metadata(staged)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        let _ = std::fs::remove_file(staged);
    }
    std::fs::copy(fresh, staged)?;
    Ok(true)
}

/// Project plugin dir: `<project_root>/.murphy/plugins/`.
pub fn project_plugin_dir(project_root: &Path) -> PathBuf {
    project_root.join(".murphy/plugins")
}

/// Per-plugin stale/missing report for a project.
///
/// Only packs whose staged path lives inside `<project>/.murphy/plugins/`
/// are considered: a `Detailed { path }` pointing elsewhere (e.g.
/// directly at `target/release/...`) is already live and needs no sync,
/// and a `Name` currently resolving outside the project dir (env /
/// user-local) is left alone so `sync` never shadows a higher-priority
/// search dir with a copy. Missing staged files (no project-local copy
/// yet, but a fresh build exists in a candidate dir) are reported with
/// `missing: true` so `sync` can install them.
///
/// `extra_from` are explicit `--from` dirs, prepended to
/// [`candidate_dirs`]. Never fails on resolve errors: unresolvable names
/// are still checked for installable candidates.
pub fn check_project_with_config(
    project_root: &Path,
    config: &MurphyConfig,
    extra_from: &[PathBuf],
) -> Vec<StaleReport> {
    let dirs = candidate_dirs(project_root, extra_from);
    let plugin_dir = project_plugin_dir(project_root);

    // Dedup names preserving first-seen order (mirrors plan_plugin_loads).
    let mut names: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for p in &config.plugins {
        let n = match p {
            PluginConfig::Name(n) => n.clone(),
            PluginConfig::Detailed(d) => d.name.clone(),
        };
        if seen.insert(n.clone()) {
            names.push(n);
        }
    }

    // Resolved plan (best-effort: on total failure fall back to assuming
    // every name stages into the project plugin dir).
    let plan_ok = plan_plugin_loads(project_root, &config.plugins).ok();

    let mut out = Vec::new();
    for name in names {
        // Staged path for sync purposes: the project-local slot when the
        // plan resolves there, else skip (live path elsewhere).
        let staged_in_project = plugin_dir.join(lib_filename(&name));
        let resolved: Option<PathBuf> = plan_ok.as_ref().and_then(|plan| {
            plan.iter()
                .find(|(n, _)| n == &name)
                .map(|(_, p)| p.clone())
        });
        let staged: PathBuf = match resolved {
            Some(r) => {
                // Only sync packs staged inside the project plugin dir.
                if r != staged_in_project {
                    continue;
                }
                r
            }
            // Unresolvable (missing everywhere): offer install into the
            // project slot when a candidate exists.
            None => staged_in_project.clone(),
        };
        // When staged exists but equals a candidate file (symlink), skip
        // quickly via find_fresh_candidate (same-file guard).
        if let Some(fresh) = find_fresh_candidate(&name, &staged, &dirs) {
            let missing = !staged.exists();
            out.push(StaleReport {
                name,
                staged,
                fresh,
                missing,
            });
        }
    }
    out
}

/// Convenience wrapper that loads `.murphy.yml` from `project_root`
/// first. Missing config → empty report (nothing to sync).
pub fn check_project(project_root: &Path, extra_from: &[PathBuf]) -> Vec<StaleReport> {
    let config = MurphyConfig::load(project_root).unwrap_or_default();
    check_project_with_config(project_root, &config, extra_from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PluginConfig, PluginDetailed};
    use std::fs;

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).expect("mkdir parents");
        }
        fs::write(path, bytes).expect("write file");
    }

    fn set_mtime(path: &Path, secs: u64) {
        // Portable mtime setter without extra deps: use filetime-ish via
        // std only? std has no setter, so shell out to `touch -d`.
        // Tests run on Unix CI; fall back silently otherwise.
        let stamp = format!(
            "19700101{:02}.{:02}.{:02}",
            secs / 3600,
            (secs / 60) % 60,
            secs % 60
        );
        let _ = std::process::Command::new("touch")
            .arg("-t")
            .arg(format!(
                "2020010100{:02}.{:02}",
                (secs / 60) % 60,
                secs % 60
            ))
            .arg(path)
            .status();
        let _ = stamp;
    }

    #[test]
    fn is_stale_false_for_same_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let f = dir.path().join("libfoo.so");
        write(&f, b"v1");
        assert!(!is_stale(&f, &f));
    }

    #[test]
    fn is_stale_false_for_symlink_to_same_target() {
        // The e2e staging helper symlinks the project copy at the fresh
        // artifact: canonicalizes equal → never stale.
        let dir = tempfile::tempdir().expect("tempdir");
        let fresh = dir.path().join("fresh").join("libfoo.so");
        write(&fresh, b"v1");
        let staged = dir.path().join("staged").join("libfoo.so");
        fs::create_dir_all(staged.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&fresh, &staged).expect("symlink");
        assert!(
            !is_stale(&staged, &fresh),
            "symlinked staged copy must not report stale"
        );
    }

    #[test]
    fn is_stale_true_when_fresh_newer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let staged = dir.path().join("staged.so");
        let fresh = dir.path().join("fresh.so");
        write(&staged, b"old");
        // Ensure a strictly newer mtime for fresh (sleep beats coarse ticks).
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write(&fresh, b"new-different-bytes");
        assert!(
            is_stale(&staged, &fresh),
            "newer fresh build must report stale"
        );
        assert!(
            !is_stale(&fresh, &staged),
            "reverse direction must not report stale"
        );
    }

    #[test]
    fn is_stale_true_when_staged_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let staged = dir.path().join("missing.so");
        let fresh = dir.path().join("fresh.so");
        write(&fresh, b"bytes");
        assert!(is_stale(&staged, &fresh));
    }

    #[test]
    fn is_stale_false_when_bytes_equal_and_mtime_not_newer() {
        // Same bytes written in quick succession: not stale regardless of
        // tick granularity (equal-mtime branch requires differing bytes).
        let dir = tempfile::tempdir().expect("tempdir");
        let staged = dir.path().join("a.so");
        let fresh = dir.path().join("b.so");
        write(&staged, b"same");
        write(&fresh, b"same");
        // Fresh may be marginally newer by mtime (written second); force
        // equal content to dominate only when mtimes tie. If fresh mtime
        // is strictly newer, is_stale is true by design — so normalize by
        // copying fresh over staged equivalent: rewrite staged after fresh
        // so staged is newest.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write(&staged, b"same");
        assert!(!is_stale(&staged, &fresh));
    }

    #[test]
    fn find_fresh_candidate_picks_newest_stale() {
        let dir = tempfile::tempdir().expect("tempdir");
        let staged = dir.path().join("proj").join(lib_filename("murphy-rails"));
        write(&staged, b"old");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let d1 = dir.path().join("from1");
        let d2 = dir.path().join("from2");
        write(&d1.join(lib_filename("murphy-rails")), b"new1");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write(&d2.join(lib_filename("murphy-rails")), b"new2-longer");
        let got = find_fresh_candidate("murphy-rails", &staged, &[d1, d2.clone()])
            .expect("should find fresh");
        assert_eq!(got, d2.join(lib_filename("murphy-rails")));
    }

    #[test]
    fn find_fresh_candidate_none_when_up_to_date() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cand_dir = dir.path().join("cand");
        write(&cand_dir.join(lib_filename("foo")), b"v1-old");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        // Staged is newest: written after the candidate so its mtime wins.
        let staged = dir.path().join("proj").join(lib_filename("foo"));
        write(&staged, b"v2");
        // staged (v2, newer mtime) vs candidate (v1, older mtime) → not stale.
        assert!(find_fresh_candidate("foo", &staged, &[cand_dir]).is_none());
    }

    #[test]
    fn refresh_staged_copies_and_clears_staleness() {
        let dir = tempfile::tempdir().expect("tempdir");
        let staged = dir.path().join(".murphy/plugins").join(lib_filename("foo"));
        write(&staged, b"old");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let fresh = dir.path().join("target/release").join(lib_filename("foo"));
        write(&fresh, b"new-bytes");
        assert!(is_stale(&staged, &fresh));
        refresh_staged(&staged, &fresh).expect("copy");
        assert_eq!(fs::read(&staged).unwrap(), b"new-bytes");
        assert!(
            !is_stale(&staged, &fresh),
            "after refresh staged must be up-to-date"
        );
    }

    #[test]
    fn candidate_dirs_includes_extra_env_exe_and_target() {
        unsafe { std::env::remove_var("MURPHY_PLUGIN_PATH") };
        let project = tempfile::tempdir().expect("tempdir");
        let extra = vec![PathBuf::from("/extra/dir")];
        let dirs = candidate_dirs(project.path(), &extra);
        assert_eq!(dirs.first(), Some(&PathBuf::from("/extra/dir")));
        assert!(dirs.contains(&project.path().join("target/debug")));
        assert!(dirs.contains(&project.path().join("target/release")));
        // exe dir best-effort: present on normal test binaries.
        if let Ok(exe) = std::env::current_exe()
            && let Some(d) = exe.parent()
        {
            assert!(dirs.contains(&d.to_path_buf()));
        }
    }

    #[test]
    fn check_project_reports_stale_name_plugin() {
        unsafe { std::env::remove_var("MURPHY_PLUGIN_PATH") };
        let project = tempfile::tempdir().expect("tempdir");
        let staged = project
            .path()
            .join(".murphy/plugins")
            .join(lib_filename("murphy-rails"));
        write(&staged, b"old-staged");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let from = project.path().join("build-out");
        write(
            &from.join(lib_filename("murphy-rails")),
            b"fresh-build-bytes",
        );
        let config = MurphyConfig {
            target_ruby_version: murphy_plugin_api::RubyVersion::new(3, 4),
            target_rails_version: None,
            active_support_extensions_enabled: false,
            user_set_active_support_extensions_enabled: false,
            files: crate::config::FilesConfig {
                include: vec!["**/*.rb".to_string()],
                exclude: vec![],
            },
            cops: crate::config::CopsConfig {
                path: PathBuf::from("cops"),
                rules: Default::default(),
            },
            plugins: vec![PluginConfig::Name("murphy-rails".to_string())],
            base_defaults: Default::default(),
            user_exclude: None,
            exclude_merge: false,
        };
        // Write a .murphy.yml too so check_project (load path) agrees.
        write(
            &project.path().join(".murphy.yml"),
            b"plugins:\n  - murphy-rails\n",
        );
        let reports =
            check_project_with_config(project.path(), &config, std::slice::from_ref(&from));
        assert_eq!(reports.len(), 1, "one stale report: {reports:?}");
        assert_eq!(reports[0].name, "murphy-rails");
        assert_eq!(reports[0].fresh, from.join(lib_filename("murphy-rails")));
        assert!(!reports[0].missing);
    }

    #[test]
    fn check_project_skips_detailed_pointing_elsewhere() {
        // A Detailed path directly at target/release is live — no sync.
        unsafe { std::env::remove_var("MURPHY_PLUGIN_PATH") };
        let project = tempfile::tempdir().expect("tempdir");
        let live = project
            .path()
            .join("target/release")
            .join(lib_filename("foo"));
        write(&live, b"live");
        let config = MurphyConfig {
            target_ruby_version: murphy_plugin_api::RubyVersion::new(3, 4),
            target_rails_version: None,
            active_support_extensions_enabled: false,
            user_set_active_support_extensions_enabled: false,
            files: crate::config::FilesConfig {
                include: vec!["**/*.rb".to_string()],
                exclude: vec![],
            },
            cops: crate::config::CopsConfig {
                path: PathBuf::from("cops"),
                rules: Default::default(),
            },
            plugins: vec![PluginConfig::Detailed(PluginDetailed {
                name: "foo".to_string(),
                path: PathBuf::from("target/release").join(lib_filename("foo")),
            })],
            base_defaults: Default::default(),
            user_exclude: None,
            exclude_merge: false,
        };
        let reports = check_project_with_config(project.path(), &config, &[]);
        assert!(
            reports.is_empty(),
            "live Detailed path needs no sync: {reports:?}"
        );
    }

    #[test]
    fn stale_report_warning_mentions_sync_command() {
        let r = StaleReport {
            name: "murphy-rails".to_string(),
            staged: PathBuf::from("/proj/.murphy/plugins/libmurphy_rails.so"),
            fresh: PathBuf::from("/b/target/release/libmurphy_rails.so"),
            missing: false,
        };
        let w = r.warning();
        assert!(w.contains("murphy-rails"), "{w}");
        assert!(w.contains("plugins sync"), "{w}");
        assert!(w.contains("cp -f"), "{w}");
    }

    #[test]
    fn set_mtime_helper_is_dead_code_guard() {
        // Keeps the mtime helper referenced so clippy/warnings stay quiet
        // if the suite ever switches to deterministic mtimes.
        let dir = tempfile::tempdir().expect("tempdir");
        let f = dir.path().join("x.so");
        write(&f, b"x");
        set_mtime(&f, 61);
        assert!(f.exists());
    }
}
