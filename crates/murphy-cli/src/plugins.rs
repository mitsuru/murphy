//! `murphy plugins` subcommand — staged plugin-pack maintenance (murphy-ghxy)
//! plus user-local pack install (murphy-uk7.1).
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
//!   [--dry-run] [--force]` — install one pack to the user dir.
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
    let source = murphy_core::plugin_install::find_install_source(&opts.pack, opts.from.as_deref())
        .map_err(AppError::setup)?;
    let user_dir = murphy_core::plugin_install::user_plugins_dir().ok_or_else(|| {
        AppError::setup(
            "cannot locate user plugins dir (no home dir; set $MURPHY_USER_PLUGINS_DIR)",
        )
    })?;
    if opts.dry_run {
        println!(
            "dry run: would install pack `{}` from {} to `{}`",
            opts.pack,
            source.describe(),
            user_dir.join(&opts.pack).display(),
        );
        if let Some(entry) = &registry_pack {
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
        &opts.pack, &source, &user_dir, opts.force,
    )
    .map_err(AppError::setup)?;
    let cdylib = murphy_core::plugin_install::verify_installed(&opts.pack, &user_dir)
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
}
