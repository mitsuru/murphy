//! `murphy plugins install <name>` artifact install (murphy-uk7.1).
//!
//! `murphy add <pack>` (C1; ADR 0049) edits `.murphy.yml` only — Bundler
//! owns download + install. That leaves non-Bundler users (cargo-installed
//! `murphy`, no Ruby) with no way to materialize a pack into the search
//! path. This module is the artifact half:
//!
//! 1. resolve `<name>` via the thin registry (compat metadata) + gem
//!    discovery (installed artifact) or an explicit `--from` dir,
//! 2. copy the pack into the user-local pack dir
//!    (`dirs::data_dir()/murphy/plugins/`, layer 5 of ADR 0042), and
//! 3. verify the installed copy (manifest + ABI + cdylib, then resolver
//!    visibility) before reporting success.
//!
//! The marketplace (remote download) and multi-arch auto-selection are
//! follow-ups; this module only copies from already-local sources
//! (installed gems, `--from` dirs). No network, no `bundle`/`gem`
//! subprocess, no ABI bump.

use std::path::{Path, PathBuf};

/// Env override for the user-local pack dir (test hook / manual override).
/// When set, it replaces `dirs::data_dir()/murphy/plugins/`.
pub const USER_PLUGINS_DIR_ENV: &str = "MURPHY_USER_PLUGINS_DIR";

/// User-local pack dir: `$MURPHY_USER_PLUGINS_DIR` when set, else
/// `dirs::data_dir()/murphy/plugins/`. `None` when neither resolves
/// (e.g. no home dir and no override).
pub fn user_plugins_dir() -> Option<PathBuf> {
    if let Some(env) = std::env::var_os(USER_PLUGINS_DIR_ENV) {
        let p = PathBuf::from(env);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    dirs::data_dir().map(|d| d.join("murphy/plugins"))
}

/// Where the installable artifact was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallSource {
    /// A pack-dir root (`murphy-plugin.toml` + `lib/`).
    PackDir(PathBuf),
    /// A legacy single-file cdylib (`lib<sanitized>.{so,dylib}`).
    LegacyFile(PathBuf),
}

impl InstallSource {
    /// Human-readable description for `--dry-run` / success messages.
    pub fn describe(&self) -> String {
        match self {
            InstallSource::PackDir(p) => format!("pack dir `{}`", p.display()),
            InstallSource::LegacyFile(p) => format!("legacy file `{}`", p.display()),
        }
    }
}

/// Walk `cdylib`'s ancestors for the nearest dir holding
/// `murphy-plugin.toml`. Returns the pack root when the cdylib came from
/// a pack-dir layout (`<root>/lib/<arch>/...`), else `None` (legacy).
pub fn find_pack_root_for_cdylib(cdylib: &Path) -> Option<PathBuf> {
    let mut cur = cdylib.parent()?;
    loop {
        if crate::plugin_manifest::is_pack_dir(cur) {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
}

/// Resolve the install source for `name`.
///
/// - When `from_dir` is given: probe `from_dir` itself as a pack dir,
///   then `from_dir/<name>` as a pack dir, then
///   `from_dir/<lib_filename>` as a legacy file.
/// - Otherwise: gem discovery (`crate::gem_discovery::find_gem_pack`);
///   a pack-dir cdylib maps back to its pack root, a legacy cdylib maps
///   to itself.
///
/// Returns `Err` with an actionable message (where it looked, what to do
/// next) when nothing is found.
pub fn find_install_source(name: &str, from_dir: Option<&Path>) -> Result<InstallSource, String> {
    if let Some(dir) = from_dir {
        if crate::plugin_manifest::is_pack_dir(dir) {
            return Ok(InstallSource::PackDir(dir.to_path_buf()));
        }
        let nested = dir.join(name);
        if crate::plugin_manifest::is_pack_dir(&nested) {
            return Ok(InstallSource::PackDir(nested));
        }
        let legacy = dir.join(crate::plugin_resolver::lib_filename(name));
        if legacy.is_file() {
            return Ok(InstallSource::LegacyFile(legacy));
        }
        return Err(format!(
            "no pack `{name}` in `--from {}` (looked for `murphy-plugin.toml` in `<dir>/` and `<dir>/{name}/`, and `{}`)",
            dir.display(),
            crate::plugin_resolver::lib_filename(name),
        ));
    }
    match crate::gem_discovery::find_gem_pack(name) {
        Some(cdylib) => {
            if let Some(root) = find_pack_root_for_cdylib(&cdylib) {
                Ok(InstallSource::PackDir(root))
            } else {
                Ok(InstallSource::LegacyFile(cdylib))
            }
        }
        None => Err(format!(
            "pack `{name}` not found in installed gems \
             (run `bundle install` / `gem install {name}`, or pass `--from <dir>` with a local pack)"
        )),
    }
}

/// What `install_source_to_user_dir` copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    /// Destination: `<user_dir>/<name>/` (pack dir) or
    /// `<user_dir>/<lib_filename>` (legacy).
    pub dest: PathBuf,
    /// True when an existing install was overwritten (`--force`).
    pub overwrote: bool,
}

/// Copy `source` into `user_dir` for `name`.
///
/// - Pack dir → `<user_dir>/<name>/` (recursive copy).
/// - Legacy file → `<user_dir>/<lib_filename>` (single copy).
///
/// Refuses to clobber an existing destination unless `force` is set.
/// Validates the source manifest (pack-dir form: parse + name match +
/// ABI compat) before copying so a broken source never half-installs.
pub fn install_source_to_user_dir(
    name: &str,
    source: &InstallSource,
    user_dir: &Path,
    force: bool,
) -> Result<InstallReport, String> {
    crate::plugin_resolver::validate_plugin_name(name).map_err(|e| e.to_string())?;
    match source {
        InstallSource::PackDir(root) => {
            let manifest = crate::plugin_manifest::PluginManifest::from_pack_dir(root)
                .map_err(|e| format!("source pack `{}`: {e}", root.display()))?;
            if manifest.plugin.name != name {
                return Err(format!(
                    "source pack `{}` holds manifest name `{}` (expected `{name}`)",
                    root.display(),
                    manifest.plugin.name,
                ));
            }
            manifest
                .check_api_compat()
                .map_err(|e| format!("source pack `{name}`: {e}"))?;
            // Confirm a loadable cdylib exists before touching the dest.
            crate::plugin_manifest::resolve_pack_cdylib(root)
                .map_err(|e| format!("source pack `{}`: {e}", root.display()))?;
            let dest = user_dir.join(name);
            let existed = dest.exists();
            if existed && !force {
                return Err(format!(
                    "pack `{name}` is already installed at `{}` (pass `--force` to overwrite)",
                    dest.display(),
                ));
            }
            if existed {
                let _ = std::fs::remove_dir_all(&dest);
            }
            copy_dir_recursive(root, &dest).map_err(|e| {
                format!(
                    "cannot copy pack `{}` to `{}`: {e}",
                    root.display(),
                    dest.display()
                )
            })?;
            Ok(InstallReport {
                dest,
                overwrote: existed && force,
            })
        }
        InstallSource::LegacyFile(file) => {
            if !file.is_file() {
                return Err(format!("source file `{}` is missing", file.display()));
            }
            let dest = user_dir.join(crate::plugin_resolver::lib_filename(name));
            let existed = dest.exists();
            if existed && !force {
                return Err(format!(
                    "pack `{name}` is already installed at `{}` (pass `--force` to overwrite)",
                    dest.display(),
                ));
            }
            std::fs::create_dir_all(user_dir)
                .map_err(|e| format!("cannot create `{}`: {e}", user_dir.display()))?;
            std::fs::copy(file, &dest).map_err(|e| {
                format!(
                    "cannot copy `{}` to `{}`: {e}",
                    file.display(),
                    dest.display()
                )
            })?;
            Ok(InstallReport {
                dest,
                overwrote: existed && force,
            })
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Verify the installed copy at `<user_dir>/<name>` (pack dir) or the
/// legacy file: manifest parses, ABI matches the host, a cdylib resolves,
/// and the name resolves via the same probe the loader uses.
pub fn verify_installed(name: &str, user_dir: &Path) -> Result<PathBuf, String> {
    let pack_dest = user_dir.join(name);
    if crate::plugin_manifest::is_pack_dir(&pack_dest) {
        let manifest = crate::plugin_manifest::PluginManifest::from_pack_dir(&pack_dest)
            .map_err(|e| format!("installed pack `{}`: {e}", pack_dest.display()))?;
        manifest
            .check_api_compat()
            .map_err(|e| format!("installed pack `{name}`: {e}"))?;
        let cdylib = crate::plugin_manifest::resolve_pack_cdylib(&pack_dest)
            .map_err(|e| format!("installed pack `{}`: {e}", pack_dest.display()))?;
        if !cdylib.is_file() {
            return Err(format!(
                "installed pack `{name}` cdylib `{}` is missing",
                cdylib.display()
            ));
        }
        return Ok(cdylib);
    }
    let legacy = user_dir.join(crate::plugin_resolver::lib_filename(name));
    if legacy.is_file() {
        return Ok(legacy);
    }
    Err(format!(
        "pack `{name}` not found in `{}` after install (looked for `<dir>/{name}/` pack dir and `{}`)",
        user_dir.display(),
        crate::plugin_resolver::lib_filename(name),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(pack_dir: &Path, name: &str) {
        std::fs::create_dir_all(pack_dir).expect("mkdir pack");
        std::fs::write(
            pack_dir.join(crate::plugin_manifest::MANIFEST_FILENAME),
            format!("[plugin]\nname = \"{name}\"\nversion = \"0.1.0\"\nmurphy-api-version = 4\n"),
        )
        .expect("write manifest");
    }

    fn write_cdylib(pack_dir: &Path) -> PathBuf {
        let arch = pack_dir.join("lib").join("linux-x86_64");
        std::fs::create_dir_all(&arch).expect("mkdir lib/<arch>");
        // Use the platform-correct extension so resolve_pack_cdylib picks it.
        let ext = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        let cdylib = arch.join(format!("libtest_pack.{ext}"));
        std::fs::write(&cdylib, b"fake").expect("write fake cdylib");
        cdylib
    }

    #[test]
    fn user_plugins_dir_prefers_env_override() {
        let dir = tempfile::tempdir().expect("tempdir");
        unsafe {
            std::env::set_var(USER_PLUGINS_DIR_ENV, dir.path());
        }
        assert_eq!(user_plugins_dir(), Some(dir.path().to_path_buf()));
        unsafe {
            std::env::remove_var(USER_PLUGINS_DIR_ENV);
        }
    }

    #[test]
    fn find_source_from_pack_dir_itself() {
        let src = tempfile::tempdir().expect("tempdir");
        write_manifest(src.path(), "murphy-foo");
        let got = find_install_source("murphy-foo", Some(src.path())).expect("source");
        assert_eq!(got, InstallSource::PackDir(src.path().to_path_buf()));
    }

    #[test]
    fn find_source_from_parent_holding_named_pack() {
        let parent = tempfile::tempdir().expect("tempdir");
        let pack = parent.path().join("murphy-foo");
        write_manifest(&pack, "murphy-foo");
        let got = find_install_source("murphy-foo", Some(parent.path())).expect("source");
        assert_eq!(got, InstallSource::PackDir(pack));
    }

    #[test]
    fn find_source_from_legacy_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir
            .path()
            .join(crate::plugin_resolver::lib_filename("murphy-foo"));
        std::fs::write(&file, b"fake").expect("write fake so");
        let got = find_install_source("murphy-foo", Some(dir.path())).expect("source");
        assert_eq!(got, InstallSource::LegacyFile(file));
    }

    #[test]
    fn find_source_missing_from_dir_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = find_install_source("murphy-foo", Some(dir.path())).expect_err("must fail");
        assert!(err.contains("murphy-foo"), "got: {err}");
    }

    #[test]
    fn install_pack_dir_and_verify_roundtrip() {
        let src = tempfile::tempdir().expect("tempdir");
        write_manifest(src.path(), "murphy-foo");
        write_cdylib(src.path());
        let user = tempfile::tempdir().expect("userdir");
        let source = InstallSource::PackDir(src.path().to_path_buf());
        let report =
            install_source_to_user_dir("murphy-foo", &source, user.path(), false).expect("install");
        assert_eq!(report.dest, user.path().join("murphy-foo"));
        let cdylib = verify_installed("murphy-foo", user.path()).expect("verify");
        assert!(cdylib.is_file(), "cdylib: {}", cdylib.display());
    }

    #[test]
    fn install_refuses_clobber_without_force() {
        let src = tempfile::tempdir().expect("tempdir");
        write_manifest(src.path(), "murphy-foo");
        write_cdylib(src.path());
        let user = tempfile::tempdir().expect("userdir");
        let source = InstallSource::PackDir(src.path().to_path_buf());
        install_source_to_user_dir("murphy-foo", &source, user.path(), false).expect("first");
        let err = install_source_to_user_dir("murphy-foo", &source, user.path(), false)
            .expect_err("second must fail");
        assert!(err.contains("--force"), "got: {err}");
        // With force it succeeds.
        install_source_to_user_dir("murphy-foo", &source, user.path(), true).expect("force");
    }

    #[test]
    fn install_rejects_manifest_name_mismatch() {
        let src = tempfile::tempdir().expect("tempdir");
        write_manifest(src.path(), "murphy-other");
        write_cdylib(src.path());
        let user = tempfile::tempdir().expect("userdir");
        let source = InstallSource::PackDir(src.path().to_path_buf());
        let err = install_source_to_user_dir("murphy-foo", &source, user.path(), false)
            .expect_err("must fail");
        assert!(err.contains("murphy-other"), "got: {err}");
    }

    #[test]
    fn find_pack_root_walks_up_from_cdylib() {
        let root = tempfile::tempdir().expect("tempdir");
        write_manifest(root.path(), "murphy-foo");
        let cdylib = write_cdylib(root.path());
        assert_eq!(
            find_pack_root_for_cdylib(&cdylib),
            Some(root.path().to_path_buf())
        );
    }

    #[test]
    fn verify_missing_errors() {
        let user = tempfile::tempdir().expect("userdir");
        let err = verify_installed("murphy-foo", user.path()).expect_err("must fail");
        assert!(err.contains("murphy-foo"), "got: {err}");
    }
}
