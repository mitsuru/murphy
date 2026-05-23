//! Cop registry: the single source of the cop set for a run.
//!
//! Post-reboot (ADR 0038): everything dispatched against the arena AST
//! goes through a `&[&'static PluginCopV1]` slice. The registry assembles
//! that slice from
//!
//! - `crate::builtin::BUILTINS` (the host's built-in cops), and
//! - any `cop_packs` configured in `murphy.toml`, loaded via
//!   `crate::plugin_loader::load_plugin_pack` (the single-symbol ABI
//!   loader, murphy-9cr.4).
//!
//! Per-cop enable/disable from `[cops.rules."Name"]` is applied here, so
//! the dispatch host sees the post-config cop set.
//!
//! ## `cops/*.rb` enumeration (deferred to murphy-9cr.24)
//!
//! v1 does not load `.rb` user cops; the C-backend matcher
//! (murphy-9cr.24) reintroduces the path. Under `--features
//! mruby-user-cops` the registry still enumerates `<root>/cops/*.rb`
//! (preserving the ADR 0004 mitigation-2 enumeration contract) so the
//! follow-up loader has its inputs ready. Without the feature, the path
//! list is empty.

#[cfg(feature = "mruby-user-cops")]
use std::path::PathBuf;
use std::path::Path;

use murphy_plugin_api::PluginCopV1;

use crate::ConfigError;
use crate::MurphyConfig;
use crate::builtin::BUILTINS;
#[cfg(not(target_os = "windows"))]
use crate::plugin_loader::{LoadedPluginPack, load_plugin_pack};

/// The cop set for a run: builtins + cops contributed by `.so` plugin
/// packs, filtered by `[cops.rules."Name".enabled]`.
///
/// `Send + Sync` so the slice can cross a rayon `par_iter` boundary
/// (the CLI's memoized lint phase); `LoadedPluginPack`'s
/// `libloading::Library` is `Send + Sync` on POSIX.
pub struct CopRegistry {
    /// Borrowed cop table assembled at construction. Each entry is one of:
    ///
    /// - A built-in `PluginCopV1` (`&'static`, embedded in this crate), or
    /// - A pointer into a `LoadedPluginPack.cops` slice (lifetime tied to
    ///   the corresponding `_packs` entry).
    ///
    /// The slice is declared `&'static` because every `PluginCopV1` we
    /// dispatch against today is a `&'static` from `register_cops!`; for
    /// `.so`-loaded packs the static-ness is upheld by holding the
    /// `LoadedPluginPack`s alive on the registry (the libraries unload
    /// only when the registry is dropped).
    cops: Vec<&'static PluginCopV1>,
    /// Friendly pack names, in registration order: `"builtin"` followed
    /// by each configured `cop_packs[i].name`. Surfaced in `--explain` /
    /// progress reporting.
    pack_names: Vec<String>,
    /// Owns the `dlopen` handles for the lifetime of the registry; the
    /// borrows in [`Self::cops`] are valid for as long as this field is
    /// alive.
    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code)]
    packs: Vec<LoadedPluginPack>,
    /// Enumerated `cops/*.rb` paths (sorted). Always empty when the
    /// `mruby-user-cops` feature is off.
    #[cfg(feature = "mruby-user-cops")]
    mruby_cop_paths: Vec<PathBuf>,
}

impl CopRegistry {
    /// Built-in cop names, in registration order. Available without a
    /// project root for callers that just want the static catalog.
    pub fn native_cop_names() -> Vec<String> {
        BUILTINS
            .iter()
            .map(|c| String::from_utf8_lossy(unsafe { c.name.as_bytes() }).into_owned())
            .collect()
    }

    /// Registry with builtins only — no `cop_packs`, no `cops/*.rb`.
    /// Useful for callers that don't have a project root (e.g. tests).
    pub fn native_only() -> Self {
        CopRegistry {
            cops: BUILTINS.to_vec(),
            pack_names: vec!["builtin".to_string()],
            #[cfg(not(target_os = "windows"))]
            packs: Vec::new(),
            #[cfg(feature = "mruby-user-cops")]
            mruby_cop_paths: Vec::new(),
        }
    }

    /// Build a registry for the project rooted at `root`: builtins plus
    /// every configured `[[cop_packs]]` entry, with `[cops.rules."Name"]`
    /// applied.
    pub fn discover(root: &Path) -> Result<Self, ConfigError> {
        let config = MurphyConfig::load(root)?;
        Self::discover_with_config(root, &config)
    }

    /// Like [`Self::discover`] but the config is already in hand.
    pub fn discover_with_config(root: &Path, config: &MurphyConfig) -> Result<Self, ConfigError> {
        #[cfg(feature = "mruby-user-cops")]
        let mruby_cop_paths = enumerate_cop_paths(root, &config.cops.path)?;

        let mut cops: Vec<&'static PluginCopV1> = BUILTINS.to_vec();
        let mut pack_names: Vec<String> = vec!["builtin".to_string()];

        #[cfg(not(target_os = "windows"))]
        let mut packs: Vec<LoadedPluginPack> = Vec::new();

        #[cfg(not(target_os = "windows"))]
        for pack in &config.cop_packs {
            let path = root.join(&pack.path);
            let loaded = load_plugin_pack(&path).map_err(|e| {
                ConfigError::Io(format!("cannot load cop pack {}: {e}", pack.name))
            })?;
            // Name-collision check against the already-registered cops.
            for cop in loaded.cops {
                let name = unsafe { cop.name.as_bytes() };
                if cops.iter().any(|existing| unsafe { existing.name.as_bytes() } == name) {
                    let name_str = String::from_utf8_lossy(name).into_owned();
                    return Err(ConfigError::Io(format!(
                        "cop pack {} attempts to register `{name_str}` but a cop with that name \
                         is already registered (built-in or earlier pack)",
                        pack.name
                    )));
                }
                cops.push(cop);
            }
            pack_names.push(pack.name.clone());
            packs.push(loaded);
        }

        #[cfg(target_os = "windows")]
        if let Some(pack) = config.cop_packs.first() {
            return Err(ConfigError::Io(format!(
                "cop packs (`.so` plugins) are not supported on Windows: {}",
                pack.name
            )));
        }

        // Per-cop enablement filter.
        cops.retain(|cop| {
            let name = String::from_utf8_lossy(unsafe { cop.name.as_bytes() });
            config.cop_enabled(&name)
        });

        Ok(CopRegistry {
            cops,
            pack_names,
            #[cfg(not(target_os = "windows"))]
            packs,
            #[cfg(feature = "mruby-user-cops")]
            mruby_cop_paths,
        })
    }

    /// The dispatch input slice. Order is `BUILTINS` first, then each
    /// configured `cop_pack`'s cops in pack-registration order, with any
    /// `enabled = false` rule excluded.
    pub fn cops(&self) -> &[&'static PluginCopV1] {
        &self.cops
    }

    /// Cop names in dispatch order. Surfaced in progress reports.
    pub fn cop_names(&self) -> Vec<String> {
        self.cops
            .iter()
            .map(|c| String::from_utf8_lossy(unsafe { c.name.as_bytes() }).into_owned())
            .collect()
    }

    /// Pack registration order: `"builtin"` first, then configured packs.
    pub fn pack_names(&self) -> &[String] {
        &self.pack_names
    }

    /// Enumerated `cops/*.rb` paths. Always empty without the
    /// `mruby-user-cops` feature.
    #[cfg(feature = "mruby-user-cops")]
    pub fn mruby_cop_paths(&self) -> &[PathBuf] {
        &self.mruby_cop_paths
    }
}

/// Enumerate `<root>/<cops_path>/*.rb` (flat, non-recursive), filtered
/// to regular files with a `.rb` extension, sorted. Absent dir → empty
/// vec (no error); a real I/O error → [`ConfigError::Io`]. The contract
/// matches the pre-reboot enumeration exactly (ADR 0004 mitigation 2).
#[cfg(feature = "mruby-user-cops")]
fn enumerate_cop_paths(root: &Path, cops_path: &Path) -> Result<Vec<PathBuf>, ConfigError> {
    let cops_dir = root.join(cops_path);
    let entries = match std::fs::read_dir(&cops_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(ConfigError::Io(format!(
                "cannot read cops directory {}: {e}",
                cops_dir.display()
            )));
        }
    };

    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| {
            ConfigError::Io(format!(
                "cannot read an entry in cops directory {}: {e}",
                cops_dir.display()
            ))
        })?;
        let path = entry.path();
        let is_file = entry.file_type().map(|ft| ft.is_file()).unwrap_or(false);
        if is_file && path.extension().and_then(|e| e.to_str()) == Some("rb") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CopRegistry>();
    }

    #[test]
    fn native_only_yields_builtins_in_registration_order() {
        let reg = CopRegistry::native_only();
        let names = reg.cop_names();
        assert_eq!(names, vec!["Murphy/NoReceiverPuts".to_string()]);
        assert_eq!(reg.pack_names(), &["builtin".to_string()]);
    }

    #[test]
    fn discover_with_empty_root_yields_builtins_only() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let reg = CopRegistry::discover(dir.path()).expect("absent murphy.toml is fine");
        let names = reg.cop_names();
        assert_eq!(names, vec!["Murphy/NoReceiverPuts".to_string()]);
        assert_eq!(reg.pack_names(), &["builtin".to_string()]);
    }

    #[test]
    fn discover_respects_config_disabled_builtin() {
        let dir = tempfile::tempdir().expect("create tempdir");
        std::fs::write(
            dir.path().join("murphy.toml"),
            "[cops.rules.\"Murphy/NoReceiverPuts\"]\nenabled = false\n",
        )
        .expect("write murphy.toml");

        let reg = CopRegistry::discover(dir.path()).expect("discover Ok");
        let names = reg.cop_names();
        assert!(
            names.is_empty(),
            "an enabled = false rule must exclude the built-in: got {names:?}"
        );
    }
}
