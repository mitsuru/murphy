//! `murphy plugins` subcommand — staged plugin-pack maintenance (murphy-ghxy),
//! user-local pack install (murphy-uk7.1), and static-index remote
//! discovery (murphy-uk7.2; ADR 0053).
//!
//! A rebuilt pack `.so` under `target/release` (or `debug`) is NOT
//! automatically propagated to a project's `.murphy/plugins/` dir, so the
//! project keeps `dlopen`-ing the stale copy until a manual
//! `cp -f target/release/libmurphy_*.so <project>/.murphy/plugins/` is run.
//! `sync` is the tooled refresh path:
//!
//! - `murphy plugins sync [--from DIR...]` — copy fresher same-named builds
//!   into `<project>/.murphy/plugins/` (mtime-newer wins; missing staged
//!   files are installed). Prints what it did; exit `0` on success,
//!   `2` on config or copy errors.
//! - `murphy plugins sync --check` — dry run for CI: prints stale warnings
//!   to stderr and exits `2` when any staged pack is older/missing, `0`
//!   when everything is up to date.
//!
//! Only packs staged inside `<project>/.murphy/plugins/` are touched. A
//! `Detailed { path }` pointing elsewhere (e.g. directly at
//! `target/release/...`) is already live and needs no sync.
//!
//! `install` (murphy-uk7.1) is the artifact half complementing
//! `murphy add <pack>` (C1 config edit): it resolves `<name>` via the thin
//! registry + installed gems (or `--from <dir>`), copies the pack into the
//! user-local dir (`~/.local/share/murphy/plugins/`, search-path layer 5),
//! and verifies the installed copy before reporting success:
//!
//! - `murphy plugins install <name> [--registry PATH] [--from DIR]
//!   [--dry-run] [--force] [--no-remote] [--cache-dir DIR]` — install one
//!   pack to the user dir (uk7.2: falls back to the registry `source-url`
//!   remote fetch when local gems / `--from` miss).
//!
//! Remote discovery (uk7.2) adds three flows over the registry
//! `source-url` field (static index + `git`/`curl`/`tar`, no hosted
//! service):
//!
//! - `murphy plugins search [query] [--registry PATH]` — substring search
//!   over name/description/gem.
//! - `murphy plugins fetch <name> [--registry PATH] [--to DIR]` —
//!   materialize the remote source to the fetch cache (or `--to`) and
//!   verify it.
//! - `murphy plugins publish [--from DIR]` — validate a local pack dir
//!   and print the `[[packs]]` TOML snippet to append to a static index.
//!
//! `murphy plugin install` (singular) is accepted as an alias.

use std::path::{Path, PathBuf};

use murphy_core::MurphyConfig;

use super::AppError;

/// `murphy plugins sync` options (parsed via clap in `main.rs`).
#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub from: Vec<PathBuf>,
    pub check: bool,
}

/// `murphy plugins install` options (parsed via clap in `main.rs`).
#[derive(Debug, Clone)]
pub struct InstallPackOptions {
    pub pack: String,
    pub registry: Option<PathBuf>,
    pub from: Option<PathBuf>,
    pub dry_run: bool,
    pub force: bool,
    /// Disable the uk7.2 remote-source fallback (local gems / `--from` only).
    pub no_remote: bool,
    /// Fetch cache root override (defaults to
    /// `$MURPHY_PLUGIN_CACHE_DIR` else the system cache dir).
    pub cache_dir: Option<PathBuf>,
}

/// `murphy plugins search` options (parsed via clap in `main.rs`).
#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub query: Option<String>,
    pub registry: Option<PathBuf>,
}

/// `murphy plugins fetch` options (parsed via clap in `main.rs`).
#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub pack: String,
    pub registry: Option<PathBuf>,
    pub to: Option<PathBuf>,
}

/// `murphy plugins publish` options (parsed via clap in `main.rs`).
#[derive(Debug, Clone)]
pub struct PublishOptions {
    pub from: Option<PathBuf>,
}

pub fn run_sync(opts: &SyncOptions) -> Result<u8, AppError> {
    let project_root = std::path::Path::new(".");
    let config = MurphyConfig::load(project_root).map_err(|e| AppError::setup(e.to_string()))?;
    if config.plugins.is_empty() {
        println!("no plugins configured in `.murphy.yml` — nothing to sync");
        return Ok(super::EXIT_OK);
    }
    let reports =
        murphy_core::plugin_sync::check_project_with_config(project_root, &config, &opts.from);
    if reports.is_empty() {
        println!("plugins up to date in `.murphy/plugins/`");
        return Ok(super::EXIT_OK);
    }
    if opts.check {
        for r in &reports {
            eprintln!("{}", r.warning());
        }
        eprintln!(
            "plugins stale: {} pack(s) need refresh — run `murphy plugins sync --from <dir>`",
            reports.len()
        );
        return Err(AppError::setup(format!(
            "{} plugin pack(s) staged in `.murphy/plugins/` are stale or missing",
            reports.len()
        )));
    }
    let mut refreshed = 0usize;
    for r in &reports {
        murphy_core::plugin_sync::refresh_staged(&r.staged, &r.fresh).map_err(|e| {
            AppError::setup(format!(
                "cannot refresh plugin `{}` from `{}` to `{}`: {e}",
                r.name,
                r.fresh.display(),
                r.staged.display()
            ))
        })?;
        if r.missing {
            println!(
                "installed `{}` from `{}` to `{}`",
                r.name,
                r.fresh.display(),
                r.staged.display()
            );
        } else {
            println!(
                "refreshed `{}` from `{}` to `{}`",
                r.name,
                r.fresh.display(),
                r.staged.display()
            );
        }
        refreshed += 1;
    }
    println!("synced {refreshed} plugin pack(s) into `.murphy/plugins/`");
    Ok(super::EXIT_OK)
}

pub fn run_install_pack(opts: &InstallPackOptions) -> Result<u8, AppError> {
    run_install_pack_in(Path::new("."), opts)
}

fn run_install_pack_in(project_root: &Path, opts: &InstallPackOptions) -> Result<u8, AppError> {
    murphy_core::plugin_resolver::validate_plugin_name(&opts.pack)
        .map_err(|e| AppError::setup(e.to_string()))?;
    // Registry compat first (same gate as `murphy add`): unknown packs fall
    // through to manifest-only verification so third-party / local packs
    // remain installable; registry-listed packs must match host ABI + core.
    let registry_pack = match murphy_core::pack_registry::PackRegistryIndex::load_with_override(
        opts.registry.as_deref(),
    ) {
        Ok(index) => match index.find(&opts.pack) {
            Some(entry) => {
                murphy_core::pack_registry::check_compat(
                    entry,
                    murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
                    murphy_core::version(),
                )
                .map_err(|e| AppError::setup(e.to_string()))?;
                Some(entry.clone())
            }
            None => None,
        },
        Err(e) => return Err(AppError::setup(e)),
    };
    let user_dir = murphy_core::plugin_install::user_plugins_dir().ok_or_else(|| {
        AppError::setup(
            "cannot locate user plugins dir (no home dir; set $MURPHY_USER_PLUGINS_DIR)",
        )
    })?;
    // Local sources first (`--from` / installed gems, the uk7.1 path).
    // When they miss and the registry entry carries a uk7.2 remote source,
    // fall back to fetching it into the fetch cache (unless `--no-remote`).
    match murphy_core::plugin_install::find_install_source(&opts.pack, opts.from.as_deref()) {
        Ok(source) => install_from_source(project_root, opts, &registry_pack, &source, &user_dir),
        Err(local_err) => {
            let entry = match &registry_pack {
                Some(e) => e,
                None => return Err(AppError::setup(local_err)),
            };
            if opts.no_remote || !entry.has_remote_source() {
                return Err(AppError::setup(local_err));
            }
            if opts.dry_run {
                println!(
                    "dry run: would fetch pack `{}` from {} to the fetch cache, then install to `{}`",
                    opts.pack,
                    murphy_core::plugin_marketplace::describe_remote_source(entry),
                    user_dir.join(&opts.pack).display(),
                );
                return Ok(super::EXIT_OK);
            }
            let cache_root = opts
                .cache_dir
                .clone()
                .unwrap_or_else(murphy_core::plugin_marketplace::plugin_cache_dir_or_temp);
            println!(
                "fetching pack `{}` from {}",
                opts.pack,
                murphy_core::plugin_marketplace::describe_remote_source(entry),
            );
            let fetched = murphy_core::plugin_marketplace::fetch_pack_to_cache(entry, &cache_root)
                .map_err(AppError::setup)?;
            let source = murphy_core::plugin_install::find_install_source(
                &opts.pack,
                Some(fetched.as_path()),
            )
            .map_err(AppError::setup)?;
            install_from_source(project_root, opts, &registry_pack, &source, &user_dir)
        }
    }
}

fn install_from_source(
    project_root: &Path,
    opts: &InstallPackOptions,
    registry_pack: &Option<murphy_core::pack_registry::PackEntry>,
    source: &murphy_core::plugin_install::InstallSource,
    user_dir: &Path,
) -> Result<u8, AppError> {
    if opts.dry_run {
        println!(
            "dry run: would install pack `{}` from {} to `{}`",
            opts.pack,
            source.describe(),
            user_dir.join(&opts.pack).display(),
        );
        if let Some(entry) = registry_pack {
            println!(
                "registry: {} {} ({})",
                entry.name, entry.version, entry.description
            );
        } else {
            println!(
                "registry: `{}` is not listed (manifest-only verification)",
                opts.pack
            );
        }
        return Ok(super::EXIT_OK);
    }
    let report = murphy_core::plugin_install::install_source_to_user_dir(
        &opts.pack, source, user_dir, opts.force,
    )
    .map_err(AppError::setup)?;
    let cdylib = murphy_core::plugin_install::verify_installed(&opts.pack, user_dir)
        .map_err(AppError::setup)?;
    if report.overwrote {
        println!(
            "reinstalled pack `{}` to `{}` (verified: {})",
            opts.pack,
            report.dest.display(),
            cdylib.display(),
        );
    } else {
        println!(
            "installed pack `{}` to `{}` (verified: {})",
            opts.pack,
            report.dest.display(),
            cdylib.display(),
        );
    }
    // Config hint: the user dir is search-path layer 5, so a
    // `plugins = ["<name>"]` entry now resolves with no `path:` pin.
    let config_path = project_root.join(".murphy.yml");
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    if !murphy_core::pack_registry::config_text_has_plugin(&existing, &opts.pack) {
        println!(
            "next: add `plugins = [\"{}\"]` to `{}` (or run `murphy add {}`)",
            opts.pack,
            config_path.display(),
            opts.pack,
        );
    }
    Ok(super::EXIT_OK)
}

pub fn run_search(opts: &SearchOptions) -> Result<u8, AppError> {
    let index =
        murphy_core::pack_registry::PackRegistryIndex::load_with_override(opts.registry.as_deref())
            .map_err(AppError::setup)?;
    let query = opts.query.as_deref().unwrap_or("");
    let hits = murphy_core::plugin_marketplace::search_packs(&index, query);
    if hits.is_empty() {
        println!(
            "no packs matching `{query}` ({} pack(s) in index)",
            index.packs.len()
        );
        return Ok(super::EXIT_OK);
    }
    for entry in hits {
        let remote = if entry.has_remote_source() {
            " [remote]"
        } else {
            ""
        };
        if entry.description.is_empty() {
            println!("{} {}{}", entry.name, entry.version, remote);
        } else {
            println!(
                "{} {} - {}{}",
                entry.name, entry.version, entry.description, remote
            );
        }
    }
    Ok(super::EXIT_OK)
}

pub fn run_fetch(opts: &FetchOptions) -> Result<u8, AppError> {
    murphy_core::plugin_resolver::validate_plugin_name(&opts.pack)
        .map_err(|e| AppError::setup(e.to_string()))?;
    let index =
        murphy_core::pack_registry::PackRegistryIndex::load_with_override(opts.registry.as_deref())
            .map_err(AppError::setup)?;
    let entry = index.find(&opts.pack).ok_or_else(|| {
        AppError::setup(format!(
            "unknown pack `{}` (available: {})",
            opts.pack,
            index
                .pack_names()
                .join(", ")
                .chars()
                .take(500)
                .collect::<String>()
        ))
    })?;
    murphy_core::pack_registry::check_compat(
        entry,
        murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
        murphy_core::version(),
    )
    .map_err(|e| AppError::setup(e.to_string()))?;
    if !entry.has_remote_source() {
        return Err(AppError::setup(format!(
            "pack `{}` has no remote source (`source-url` is unset; use `--from <dir>` or installed gems)",
            opts.pack
        )));
    }
    let dest = match &opts.to {
        Some(to) => {
            murphy_core::plugin_marketplace::fetch_pack(entry, to).map_err(AppError::setup)?
        }
        None => {
            let cache = murphy_core::plugin_marketplace::plugin_cache_dir_or_temp();
            println!(
                "fetching pack `{}` from {}",
                opts.pack,
                murphy_core::plugin_marketplace::describe_remote_source(entry),
            );
            murphy_core::plugin_marketplace::fetch_pack_to_cache(entry, &cache)
                .map_err(AppError::setup)?
        }
    };
    // Verify the fetched copy (manifest + ABI + cdylib) before reporting.
    let manifest = murphy_core::plugin_manifest::PluginManifest::from_pack_dir(&dest)
        .map_err(|e| AppError::setup(format!("fetched pack `{}`: {e}", dest.display())))?;
    manifest
        .check_api_compat()
        .map_err(|e| AppError::setup(format!("fetched pack `{}`: {e}", opts.pack)))?;
    let cdylib = murphy_core::plugin_manifest::resolve_pack_cdylib(&dest)
        .map_err(|e| AppError::setup(format!("fetched pack `{}`: {e}", dest.display())))?;
    println!(
        "fetched pack `{}` {} to `{}` (verified: {})",
        opts.pack,
        manifest.plugin.version,
        dest.display(),
        cdylib.display(),
    );
    Ok(super::EXIT_OK)
}

pub fn run_publish(opts: &PublishOptions) -> Result<u8, AppError> {
    let pack_dir = opts.from.clone().unwrap_or_else(|| PathBuf::from("."));
    let report = murphy_core::plugin_marketplace::check_publish_source(&pack_dir)
        .map_err(AppError::setup)?;
    let manifest = murphy_core::plugin_manifest::PluginManifest::from_pack_dir(&pack_dir)
        .map_err(|e| AppError::setup(format!("pack `{}`: {e}", pack_dir.display())))?;
    let snippet = murphy_core::plugin_marketplace::render_index_snippet(&manifest, None, None);
    println!(
        "pack `{}` {} is publishable (verified: {})",
        report.name,
        report.version,
        report.cdylib.display(),
    );
    println!("--- append to your static index (`registry/index.toml`) ---");
    println!("{snippet}---");
    println!(
        "next: `tar czf {}-{}.tar.gz -C {} murphy-plugin.toml lib`, upload it, set `source-url`/`source-sha256`, and share the index (see `docs/guides/plugin-marketplace.md`)",
        report.name,
        report.version,
        pack_dir.display(),
    );
    Ok(super::EXIT_OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_text(name: &str) -> String {
        format!("[plugin]\nname = \"{name}\"\nversion = \"0.1.0\"\nmurphy-api-version = 4\n")
    }

    fn make_pack_dir(parent: &Path, name: &str) -> PathBuf {
        let pack = parent.join(name);
        std::fs::create_dir_all(&pack).expect("mkdir pack");
        std::fs::write(
            pack.join(murphy_core::plugin_manifest::MANIFEST_FILENAME),
            manifest_text(name),
        )
        .expect("write manifest");
        let arch = pack.join("lib").join("linux-x86_64");
        std::fs::create_dir_all(&arch).expect("mkdir lib/<arch>");
        let ext = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        std::fs::write(arch.join(format!("libtest_tmp.{ext}")), b"fake").expect("cdylib");
        pack
    }

    fn install_opts(pack: &str, from: Option<PathBuf>) -> InstallPackOptions {
        InstallPackOptions {
            pack: pack.to_string(),
            registry: None,
            from,
            dry_run: false,
            force: false,
            no_remote: false,
            cache_dir: None,
        }
    }

    static INSTALL_ENV_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

    fn with_user_dir(dir: &Path, f: impl FnOnce()) {
        let _guard = INSTALL_ENV_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .expect("install env lock");
        unsafe {
            std::env::set_var(murphy_core::plugin_install::USER_PLUGINS_DIR_ENV, dir);
        }
        f();
        unsafe {
            std::env::remove_var(murphy_core::plugin_install::USER_PLUGINS_DIR_ENV);
        }
    }

    fn unwrap_code(r: Result<u8, AppError>) -> u8 {
        match r {
            Ok(code) => code,
            Err(e) => panic!("expected Ok, got setup error: {}", e.message),
        }
    }

    fn unwrap_err_msg(r: Result<u8, AppError>) -> String {
        match r {
            Ok(code) => panic!("expected Err, got Ok({code})"),
            Err(e) => e.message,
        }
    }

    #[test]
    fn install_from_dir_roundtrip() {
        let src_parent = tempfile::tempdir().expect("src");
        make_pack_dir(src_parent.path(), "murphy-foo");
        let user = tempfile::tempdir().expect("user");
        let project = tempfile::tempdir().expect("project");
        let opts = install_opts("murphy-foo", Some(src_parent.path().to_path_buf()));
        with_user_dir(user.path(), || {
            let code = unwrap_code(run_install_pack_in(project.path(), &opts));
            assert_eq!(code, super::super::EXIT_OK);
        });
        assert!(user.path().join("murphy-foo").is_dir());
    }

    #[test]
    fn dry_run_does_not_write() {
        let src_parent = tempfile::tempdir().expect("src");
        make_pack_dir(src_parent.path(), "murphy-foo");
        let user = tempfile::tempdir().expect("user");
        let project = tempfile::tempdir().expect("project");
        let mut opts = install_opts("murphy-foo", Some(src_parent.path().to_path_buf()));
        opts.dry_run = true;
        with_user_dir(user.path(), || {
            unwrap_code(run_install_pack_in(project.path(), &opts));
        });
        assert!(!user.path().join("murphy-foo").exists());
    }

    #[test]
    fn second_install_needs_force() {
        let src_parent = tempfile::tempdir().expect("src");
        make_pack_dir(src_parent.path(), "murphy-foo");
        let user = tempfile::tempdir().expect("user");
        let project = tempfile::tempdir().expect("project");
        let opts = install_opts("murphy-foo", Some(src_parent.path().to_path_buf()));
        with_user_dir(user.path(), || {
            unwrap_code(run_install_pack_in(project.path(), &opts));
            let msg = unwrap_err_msg(run_install_pack_in(project.path(), &opts));
            assert!(msg.contains("--force"), "got: {msg}");
        });
    }

    #[test]
    fn invalid_name_rejected_before_io() {
        let project = tempfile::tempdir().expect("project");
        let opts = install_opts("../evil", None);
        let msg = unwrap_err_msg(run_install_pack_in(project.path(), &opts));
        assert!(
            msg.contains("invalid character") || msg.contains(".."),
            "got: {msg}"
        );
    }

    fn write_mirror_registry(dir: &Path, source_url: &str) -> PathBuf {
        let path = dir.join("mirror.toml");
        std::fs::write(
            &path,
            format!(
                "[registry]\nversion = 1\n\n\
                 [[packs]]\nname = \"murphy-foo\"\ngem = \"murphy-foo\"\n\
                 version = \"0.1.0\"\ndescription = \"Foo cops\"\n\
                 homepage = \"https://example.invalid\"\n\
                 murphy-api-version = 4\nmin-murphy-version = \"0.1.0\"\n\
                 source-url = \"{source_url}\"\n"
            ),
        )
        .expect("write mirror");
        path
    }

    fn write_search_registry(dir: &Path) -> PathBuf {
        let path = dir.join("search.toml");
        std::fs::write(
            &path,
            "[registry]\nversion = 1\n\n\
             [[packs]]\nname = \"murphy-rails\"\ngem = \"murphy-rails\"\n\
             version = \"0.1.0\"\ndescription = \"Rails cops\"\n\
             homepage = \"https://example.invalid\"\n\
             murphy-api-version = 4\nmin-murphy-version = \"0.1.0\"\n\
             source-url = \"https://example.invalid/murphy-rails.tar.gz\"\n\n\
             [[packs]]\nname = \"murphy-rspec\"\ngem = \"murphy-rspec\"\n\
             version = \"0.1.0\"\ndescription = \"RSpec cops\"\n\
             homepage = \"https://example.invalid\"\n\
             murphy-api-version = 4\nmin-murphy-version = \"0.1.0\"\n",
        )
        .expect("write search registry");
        path
    }

    #[test]
    fn search_lists_all_and_filters() {
        let reg_dir = tempfile::tempdir().expect("regdir");
        let reg = write_search_registry(reg_dir.path());
        // Empty query lists both (no panic = success).
        unwrap_code(run_search(&SearchOptions {
            query: None,
            registry: Some(reg.clone()),
        }));
        unwrap_code(run_search(&SearchOptions {
            query: Some("rails".to_string()),
            registry: Some(reg.clone()),
        }));
        // No match is still exit 0.
        unwrap_code(run_search(&SearchOptions {
            query: Some("nope".to_string()),
            registry: Some(reg),
        }));
    }

    #[test]
    fn fetch_from_file_dir_to_custom_to() {
        let src_parent = tempfile::tempdir().expect("src");
        make_pack_dir(src_parent.path(), "murphy-foo");
        let reg_dir = tempfile::tempdir().expect("regdir");
        let url = format!("file://{}", src_parent.path().display());
        let reg = write_mirror_registry(reg_dir.path(), &url);
        let dest_parent = tempfile::tempdir().expect("dest");
        let to = dest_parent.path().join("fetched");
        unwrap_code(run_fetch(&FetchOptions {
            pack: "murphy-foo".to_string(),
            registry: Some(reg),
            to: Some(to.clone()),
        }));
        assert!(
            to.join(murphy_core::plugin_manifest::MANIFEST_FILENAME)
                .is_file(),
            "fetched pack must hold a manifest"
        );
    }

    #[test]
    fn fetch_without_remote_errors() {
        let reg_dir = tempfile::tempdir().expect("regdir");
        let path = reg_dir.path().join("plain.toml");
        std::fs::write(
            &path,
            "[registry]\nversion = 1\n\n[[packs]]\nname = \"murphy-foo\"\n\
             gem = \"murphy-foo\"\nversion = \"0.1.0\"\n\
             murphy-api-version = 4\nmin-murphy-version = \"0.1.0\"\n",
        )
        .expect("write registry");
        let msg = unwrap_err_msg(run_fetch(&FetchOptions {
            pack: "murphy-foo".to_string(),
            registry: Some(path),
            to: None,
        }));
        assert!(msg.contains("no remote source"), "got: {msg}");
    }

    #[test]
    fn publish_validates_pack_dir() {
        let src_parent = tempfile::tempdir().expect("src");
        let pack = make_pack_dir(src_parent.path(), "murphy-foo");
        unwrap_code(run_publish(&PublishOptions { from: Some(pack) }));
    }

    #[test]
    fn install_falls_back_to_remote_when_local_misses() {
        let src_parent = tempfile::tempdir().expect("src");
        make_pack_dir(src_parent.path(), "murphy-foo");
        let reg_dir = tempfile::tempdir().expect("regdir");
        let url = format!("file://{}", src_parent.path().display());
        let reg = write_mirror_registry(reg_dir.path(), &url);
        let user = tempfile::tempdir().expect("user");
        let cache = tempfile::tempdir().expect("cache");
        let project = tempfile::tempdir().expect("project");
        let opts = InstallPackOptions {
            pack: "murphy-foo".to_string(),
            registry: Some(reg),
            from: None,
            dry_run: false,
            force: false,
            no_remote: false,
            cache_dir: Some(cache.path().to_path_buf()),
        };
        with_user_dir(user.path(), || {
            unwrap_code(run_install_pack_in(project.path(), &opts));
        });
        assert!(user.path().join("murphy-foo").is_dir());
        assert!(cache.path().join("murphy-foo").is_dir());
    }

    #[test]
    fn install_no_remote_disables_fallback() {
        let src_parent = tempfile::tempdir().expect("src");
        make_pack_dir(src_parent.path(), "murphy-foo");
        let reg_dir = tempfile::tempdir().expect("regdir");
        let url = format!("file://{}", src_parent.path().display());
        let reg = write_mirror_registry(reg_dir.path(), &url);
        let user = tempfile::tempdir().expect("user");
        let cache = tempfile::tempdir().expect("cache");
        let project = tempfile::tempdir().expect("project");
        let opts = InstallPackOptions {
            pack: "murphy-foo".to_string(),
            registry: Some(reg),
            from: None,
            dry_run: false,
            force: false,
            no_remote: true,
            cache_dir: Some(cache.path().to_path_buf()),
        };
        with_user_dir(user.path(), || {
            let msg = unwrap_err_msg(run_install_pack_in(project.path(), &opts));
            assert!(msg.contains("installed gems"), "got: {msg}");
        });
        assert!(!user.path().join("murphy-foo").exists());
    }

    #[test]
    fn install_dry_run_with_remote_does_not_fetch() {
        let src_parent = tempfile::tempdir().expect("src");
        make_pack_dir(src_parent.path(), "murphy-foo");
        let reg_dir = tempfile::tempdir().expect("regdir");
        let url = format!("file://{}", src_parent.path().display());
        let reg = write_mirror_registry(reg_dir.path(), &url);
        let user = tempfile::tempdir().expect("user");
        let cache = tempfile::tempdir().expect("cache");
        let project = tempfile::tempdir().expect("project");
        let opts = InstallPackOptions {
            pack: "murphy-foo".to_string(),
            registry: Some(reg),
            from: None,
            dry_run: true,
            force: false,
            no_remote: false,
            cache_dir: Some(cache.path().to_path_buf()),
        };
        with_user_dir(user.path(), || {
            unwrap_code(run_install_pack_in(project.path(), &opts));
        });
        assert!(!user.path().join("murphy-foo").exists());
        assert!(
            !cache.path().join("murphy-foo").exists(),
            "dry run must not fetch"
        );
    }
}
