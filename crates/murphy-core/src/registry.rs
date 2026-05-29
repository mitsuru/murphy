//! Cop registry: the single source of the cop set for a run.
//!
//! Post-reboot (ADR 0038): everything dispatched against the arena AST
//! goes through a `&[&PluginCopV1]` slice. The registry assembles that
//! slice from
//!
//! - a caller-supplied **built-in pack** (a `&[&'static PluginCopV1]`
//!   that the host obtained from `murphy-std` via its `mode = static`
//!   `register_cops!`, or an empty slice for tests / minimal embedders),
//!   and
//! - any `plugins:` configured in `.murphy.yml`, loaded via
//!   `crate::plugin_loader::load_plugin_pack` (the single-symbol ABI
//!   loader, murphy-9cr.4).
//!
//! Built-in and dynamic packs flow through the *same* registration code
//! path (design §5) — the registry never special-cases "builtin". Per-cop
//! enable/disable from cop rule sections in `.murphy.yml` are applied here, so the
//! dispatch host sees the post-config cop set.
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
#[cfg(target_os = "windows")]
use crate::PluginConfig;
#[cfg(not(target_os = "windows"))]
use crate::plugin_loader::{LoadKind, LoadedPluginPack, PluginLoadDiagnostic, load_plugin_pack};
#[cfg(not(target_os = "windows"))]
use crate::plugin_resolver::plan_plugin_loads;

/// The cop set for a run: builtins + cops contributed by `.so` plugin
/// packs, filtered by `Enabled:` in cop rule sections of `.murphy.yml`.
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
    /// Raw pointers to cops in **dispatch order**, post-`enabled = false`
    /// filter. Borrowed access is only available via [`Self::cops`]
    /// (whose return lifetime is `&self`).
    cops_ptrs: Vec<NonNull<PluginCopV1>>,
    /// Raw pointers to **every** registered cop (builtin + every loaded
    /// pack), in registration order, **before** the `enabled = false`
    /// filter is applied. Exists so the host's catalogue view
    /// (`murphy cops list`) can show user-disabled cops with a
    /// `disabled: user config` status — the post-filter `cops_ptrs`
    /// alone cannot distinguish "user disabled" from "not registered".
    all_cops_ptrs: Vec<NonNull<PluginCopV1>>,
    /// Pack index for each entry of [`Self::all_cops_ptrs`]. Indexes
    /// into [`Self::pack_names`].
    all_pack_indices: Vec<usize>,
    /// Friendly pack names, in registration order: `"builtin"` followed
    /// by each configured `plugins[i]` name. Surfaced in `--explain` /
    /// progress reporting and `murphy cops list`.
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
    /// Built-in cop names, in registration order. Pure projection of the
    /// caller-supplied static pack — the registry no longer owns one.
    pub fn native_cop_names(builtins: &[&'static PluginCopV1]) -> Vec<String> {
        builtins
            .iter()
            .map(|c| String::from_utf8_lossy(unsafe { c.name.as_bytes() }).into_owned())
            .collect()
    }

    /// Registry with the caller-supplied built-in pack only — no
    /// `plugins`, no `cops/*.rb`. Useful for callers that don't have a
    /// project root (e.g. tests).
    pub fn native_only(builtins: &[&'static PluginCopV1]) -> Self {
        let all_cops_ptrs: Vec<NonNull<PluginCopV1>> =
            builtins.iter().map(|c| NonNull::from(*c)).collect();
        let all_pack_indices: Vec<usize> = (0..all_cops_ptrs.len()).map(|_| 0).collect();
        CopRegistry {
            cops_ptrs: all_cops_ptrs.clone(),
            all_cops_ptrs,
            all_pack_indices,
            pack_names: vec!["builtin".to_string()],
            #[cfg(not(target_os = "windows"))]
            packs: Vec::new(),
            #[cfg(feature = "mruby-user-cops")]
            mruby_cop_paths: Vec::new(),
        }
    }

    /// Build a registry for the project rooted at `root`: the caller's
    /// built-in pack plus every configured `[[plugins]]` entry, with
    /// cop rule sections of `.murphy.yml` applied.
    pub fn discover(root: &Path, builtins: &[&'static PluginCopV1]) -> Result<Self, ConfigError> {
        let config = MurphyConfig::load(root)?;
        Self::discover_with_config(root, &config, builtins)
    }

    /// Like [`Self::discover`] but the config is already in hand.
    pub fn discover_with_config(
        root: &Path,
        config: &MurphyConfig,
        builtins: &[&'static PluginCopV1],
    ) -> Result<Self, ConfigError> {
        #[cfg(feature = "mruby-user-cops")]
        let mruby_cop_paths = enumerate_cop_paths(root, &config.cops.path)?;

        // Built-ins first; their pointer lifetime is `'static`. Pack cops
        // are appended below as `NonNull<PluginCopV1>` keyed to a
        // `LoadedPluginPack` in `packs` (same drop ordering rule).
        let mut all_cops_ptrs: Vec<NonNull<PluginCopV1>> =
            builtins.iter().map(|c| NonNull::from(*c)).collect();
        // Parallel to `all_cops_ptrs`: the pack index each cop came from.
        // The builtin slot is index 0.
        let mut all_pack_indices: Vec<usize> = (0..all_cops_ptrs.len()).map(|_| 0).collect();
        let mut pack_names: Vec<String> = vec!["builtin".to_string()];

        #[cfg(not(target_os = "windows"))]
        let mut packs: Vec<LoadedPluginPack> = Vec::new();

        // `plan_plugin_loads` (murphy-9cr.10.2 / ADR 0042) resolves any
        // `Name(String)` shorthand against the search path, applies the
        // `Detailed → env → project → user` priority, and dedups so a
        // same-name `Name` + `Detailed` pair loads at most once.
        #[cfg(not(target_os = "windows"))]
        let plan = plan_plugin_loads(root, &config.plugins)?;
        #[cfg(not(target_os = "windows"))]
        for (pack_name, path) in plan {
            let pack_index = pack_names.len();
            let loaded = load_plugin_pack(&path).map_err(|e| {
                ConfigError::PluginLoad(PluginLoadDiagnostic {
                    plugin_name: pack_name.clone(),
                    attempted_path: Some(path.clone()),
                    kind: LoadKind::Load(e),
                })
            })?;
            // Name-collision check against the already-registered cops.
            // `loaded.cops()` borrows from `loaded` for the loop body.
            for cop in loaded.cops() {
                let name = unsafe { cop.name.as_bytes() };
                let already = all_cops_ptrs.iter().any(|existing| {
                    // Safety: each `existing` is a live pointer to an
                    // immutable `PluginCopV1` (a `'static` built-in from
                    // the caller-supplied pack, or a cop in an earlier
                    // loaded pack); the borrow ends inside this closure.
                    // `RawSlice::as_bytes` is `unsafe` because the
                    // slice's pointer/length must be valid — every
                    // source (a `register_cops!` static or a loaded
                    // pack's table) satisfies that.
                    let existing_name = unsafe { existing.as_ref().name.as_bytes() };
                    existing_name == name
                });
                if already {
                    let name_str = String::from_utf8_lossy(name).into_owned();
                    return Err(ConfigError::Io(format!(
                        "plugin {pack_name} attempts to register `{name_str}` but a cop with that name \
                         is already registered (built-in or earlier plugin)"
                    )));
                }
                all_cops_ptrs.push(NonNull::from(cop));
                all_pack_indices.push(pack_index);
            }
            pack_names.push(pack_name);
            packs.push(loaded);
        }

        #[cfg(target_os = "windows")]
        if let Some(plugin) = config.plugins.first() {
            let name = match plugin {
                PluginConfig::Detailed(d) => &d.name,
                PluginConfig::Name(name) => name,
            };
            return Err(ConfigError::Io(format!(
                "plugins (`.so`/`.dylib` packs) are not supported on Windows: {name}"
            )));
        }

        // Apply the `Enabled: false` filter from `.murphy.yml` cop rules to
        // build the dispatch view, leaving `all_cops_ptrs` unfiltered
        // for the catalogue (`murphy cops list`).
        let cops_ptrs: Vec<NonNull<PluginCopV1>> = all_cops_ptrs
            .iter()
            .filter(|cop| {
                // Safety: same as the collision check above.
                let name_bytes = unsafe { cop.as_ref().name.as_bytes() };
                let name = String::from_utf8_lossy(name_bytes);
                config.cop_enabled(&name)
            })
            .copied()
            .collect();

        Ok(CopRegistry {
            cops_ptrs,
            all_cops_ptrs,
            all_pack_indices,
            pack_names,
            #[cfg(not(target_os = "windows"))]
            packs,
            #[cfg(feature = "mruby-user-cops")]
            mruby_cop_paths,
        })
    }

    /// The dispatch input view. Order is the supplied built-in pack
    /// first, then each configured `cop_pack`'s cops in pack-registration
    /// order, with any `enabled = false` rule excluded.
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

    /// **Every** registered cop paired with its source pack name, in
    /// registration order, **including** entries that the
    /// `[cops.rules."Name"].enabled = false` filter would drop from
    /// dispatch. This is the catalogue view (`murphy cops list`) — it
    /// is intentionally pre-filter so the host can attach a
    /// `disabled: user config` status to entries the dispatch view
    /// omits.
    pub fn all_cops_with_packs(&self) -> Vec<(&PluginCopV1, &str)> {
        self.all_cops_ptrs
            .iter()
            .zip(self.all_pack_indices.iter())
            .map(|(cop, &idx)| {
                // Safety: each `NonNull` points to immutable
                // `PluginCopV1` data valid for at least as long as
                // `&self` (built-ins are `'static`; pack cops are
                // anchored to `self.packs`).
                (unsafe { cop.as_ref() }, self.pack_names[idx].as_str())
            })
            .collect()
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
    use murphy_ast::NodeId;
    use murphy_plugin_api::{Cop, Cx, NoOptions, NodeCop, NodeKindTag};

    /// Synthetic cop used to exercise the registry's plumbing without
    /// reaching into `murphy-std`. The registry's own tests must not
    /// depend on which specific cops the standard pack ships (single-
    /// surface ABI: the registry treats all cops uniformly through
    /// `PluginCopV1`, design §5).
    #[derive(Default)]
    struct StubBuiltin;

    impl Cop for StubBuiltin {
        type Options = NoOptions;
        const NAME: &'static str = "Stub/Builtin";
    }

    impl NodeCop for StubBuiltin {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
    }

    static STUB_BUILTIN_COP: PluginCopV1 =
        murphy_plugin_api::__internal::build_cop::<StubBuiltin>();
    static STUB_BUILTINS: &[&PluginCopV1] = &[&STUB_BUILTIN_COP];

    #[test]
    fn registry_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CopRegistry>();
    }

    #[test]
    fn native_only_yields_builtins_in_registration_order() {
        let reg = CopRegistry::native_only(STUB_BUILTINS);
        let names = reg.cop_names();
        assert_eq!(names, vec!["Stub/Builtin".to_string()]);
        assert_eq!(reg.pack_names(), &["builtin".to_string()]);
    }

    #[test]
    fn discover_with_empty_root_yields_builtins_only() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let reg =
            CopRegistry::discover(dir.path(), STUB_BUILTINS).expect("absent .murphy.yml is fine");
        let names = reg.cop_names();
        assert_eq!(names, vec!["Stub/Builtin".to_string()]);
        assert_eq!(reg.pack_names(), &["builtin".to_string()]);
    }

    #[test]
    fn discover_respects_config_disabled_builtin() {
        let dir = tempfile::tempdir().expect("create tempdir");
        std::fs::write(
            dir.path().join(".murphy.yml"),
            "Stub/Builtin:\n  Enabled: false\n",
        )
        .expect("write .murphy.yml");

        let reg = CopRegistry::discover(dir.path(), STUB_BUILTINS).expect("discover Ok");
        let names = reg.cop_names();
        assert!(
            names.is_empty(),
            "an enabled = false rule must exclude the built-in: got {names:?}"
        );
    }
}
