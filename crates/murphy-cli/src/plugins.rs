//! `murphy plugins` subcommand — staged plugin-pack maintenance (murphy-ghxy).
//!
//! A rebuilt pack `.so` under `target/release` (or `debug`) is NOT
//! automatically propagated to a project's `.murphy/plugins/` dir, so the
//! project keeps `dlopen`-ing the stale copy until a manual
//! `cp -f target/release/libmurphy_*.so <project>/.murphy/plugins/` is run.
//! This subcommand is the tooled refresh path:
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

use std::path::PathBuf;

use murphy_core::MurphyConfig;

use super::AppError;

/// `murphy plugins sync` options (parsed via clap in `main.rs`).
#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub from: Vec<PathBuf>,
    pub check: bool,
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
