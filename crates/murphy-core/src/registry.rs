//! Cop registry: the single source of the cop set for a run.
//!
//! Post-reboot (ADR 0038): everything dispatched against the arena AST
//! goes through a `&[&PluginCopV1]` slice. The registry assembles that
//! slice from
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

use std::path::Path;
#[cfg(feature = "mruby-user-cops")]
use std::path::PathBuf;
use std::ptr::NonNull;

use murphy_plugin_api::PluginCopV1;

use crate::ConfigError;
use crate::MurphyConfig;
use crate::builtin::BUILTINS;
#[cfg(not(target_os = "windows"))]
use crate::plugin_loader::{LoadedPluginPack, load_plugin_pack};

/// The cop set for a run: builtins + cops contributed by `.so` plugin
/// packs, filtered by `[cops.rules."Name".enabled]`.
///
/// ## Lifetime safety
///
/// Each entry in [`Self::cops_ptrs`] points to either a true `&'static`
/// built-in or into a [`LoadedPluginPack`] kept alive in [`Self::packs`].
/// Storing the pointers as [`NonNull`] (raw, not `&'static`) makes it
/// impossible for safe code to outlive the pack: borrowed views are
/// reconstructed on demand by [`Self::cops`], whose return lifetime is
/// bound to `&self`. Drop order is `cops_ptrs` before `packs`, so a
/// pack's `dlclose` never races with a borrow.
///
/// `Send + Sync` because every `NonNull<PluginCopV1>` points to
/// immutable data and `LoadedPluginPack` itself is `Send + Sync`; the
/// registry never lets a caller mutate either through a shared `&self`.
pub struct CopRegistry {
    /// Raw pointers to cops in dispatch order. Borrowed access is only
    /// available via [`Self::cops`] (whose return lifetime is `&self`).
    cops_ptrs: Vec<NonNull<PluginCopV1>>,
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

// Safety: every `NonNull<PluginCopV1>` in `cops_ptrs` points to immutable
// `PluginCopV1` data (built-in `'static` or a pack-owned table whose
// memory mapping is kept alive by `_library`). Sharing or sending the
// registry across threads only allows shared reads.
unsafe impl Send for CopRegistry {}
unsafe impl Sync for CopRegistry {}

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
            cops_ptrs: BUILTINS.iter().map(|c| NonNull::from(*c)).collect(),
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

        // Built-ins first; their pointer lifetime is `'static`. Pack cops
        // are appended below as `NonNull<PluginCopV1>` keyed to a
        // `LoadedPluginPack` in `packs` (same drop ordering rule).
        let mut cops_ptrs: Vec<NonNull<PluginCopV1>> =
            BUILTINS.iter().map(|c| NonNull::from(*c)).collect();
        let mut pack_names: Vec<String> = vec!["builtin".to_string()];

        #[cfg(not(target_os = "windows"))]
        let mut packs: Vec<LoadedPluginPack> = Vec::new();

        #[cfg(not(target_os = "windows"))]
        for pack in &config.cop_packs {
            let path = root.join(&pack.path);
            let loaded = load_plugin_pack(&path)
                .map_err(|e| ConfigError::Io(format!("cannot load cop pack {}: {e}", pack.name)))?;
            // Name-collision check against the already-registered cops.
            // `loaded.cops()` borrows from `loaded` for the loop body.
            for cop in loaded.cops() {
                let name = unsafe { cop.name.as_bytes() };
                let already = cops_ptrs.iter().any(|existing| {
                    // Safety: each `existing` is a live pointer to an
                    // immutable `PluginCopV1` (built-in `'static` or in
                    // an earlier pack); the borrow ends inside this
                    // closure. `RawSlice::as_bytes` is `unsafe` because
                    // the slice's pointer/length must be valid — the
                    // cop tables either come from `register_cops!` or
                    // the in-crate `&'static` `BUILTINS`, both of which
                    // satisfy that.
                    let existing_name = unsafe { existing.as_ref().name.as_bytes() };
                    existing_name == name
                });
                if already {
                    let name_str = String::from_utf8_lossy(name).into_owned();
                    return Err(ConfigError::Io(format!(
                        "cop pack {} attempts to register `{name_str}` but a cop with that name \
                         is already registered (built-in or earlier pack)",
                        pack.name
                    )));
                }
                cops_ptrs.push(NonNull::from(cop));
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

        // Per-cop enablement filter. Closure borrows the cop briefly to
        // read its name; the pointer itself is retained.
        cops_ptrs.retain(|cop| {
            // Safety: same as the collision check above.
            let name_bytes = unsafe { cop.as_ref().name.as_bytes() };
            let name = String::from_utf8_lossy(name_bytes);
            config.cop_enabled(&name)
        });

        Ok(CopRegistry {
            cops_ptrs,
            pack_names,
            #[cfg(not(target_os = "windows"))]
            packs,
            #[cfg(feature = "mruby-user-cops")]
            mruby_cop_paths,
        })
    }

    /// The dispatch input view. Order is `BUILTINS` first, then each
    /// configured `cop_pack`'s cops in pack-registration order, with any
    /// `enabled = false` rule excluded.
    ///
    /// The returned `Vec<&PluginCopV1>` borrows from `&self`: pack cops
    /// stay valid for as long as the registry is alive. Allocating a
    /// fresh `Vec` per call is intentional — it's the bridge between
    /// the registry's raw-pointer storage and the dispatch host's safe
    /// `&[&PluginCopV1]` interface, and the cop list is small (a few
    /// builtins + plugin cops, dozens at most in v1+).
    pub fn cops(&self) -> Vec<&PluginCopV1> {
        // Safety: each `NonNull` points to immutable `PluginCopV1` data
        // valid for at least as long as `&self` (built-ins are `'static`;
        // pack cops are anchored to `self.packs`).
        self.cops_ptrs
            .iter()
            .map(|p| unsafe { p.as_ref() })
            .collect()
    }

    /// Cop names in dispatch order. Surfaced in progress reports.
    pub fn cop_names(&self) -> Vec<String> {
        self.cops()
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
