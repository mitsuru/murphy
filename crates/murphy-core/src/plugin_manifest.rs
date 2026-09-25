//! `murphy-plugin.toml` pack manifest (murphy-s3s; ADR 0046).
//!
//! A plugin pack used to be addressable only as a bare cdylib
//! (`[[plugins]] path = "./libfoo.so"`, resolved to `lib<name>.so` in a
//! search dir). That left pack metadata — name, version, provided cops,
//! the required host ABI — reachable only through the cdylib's exported
//! symbols, so `murphy cops list` could not describe a pack without
//! `dlopen`ing it and multi-arch / version-coexistence distribution had
//! no agreed layout.
//!
//! This module parses the pack-level manifest that fixes that:
//!
//! ```toml
//! [plugin]
//! name = "murphy-rails"
//! version = "0.3.1"
//! murphy-api-version = 4   # must equal MURPHY_PLUGIN_ABI_VERSION
//! authors = ["..."]
//! license = "MIT"
//!
//! [cops]
//! provided = ["Rails/HttpStatus", "Rails/SaveBang"]
//! ```
//!
//! It also defines the on-disk pack-dir convention (a `murphy-plugin.toml`
//! file plus a `lib/<platform>/` binary dir) and the helpers that map a
//! pack dir to the cdylib the loader actually opens. `Detailed { path }`
//! accepts **either** a direct `.so` path (legacy, unchanged) **or** a pack
//! dir; the search path probes the pack-dir form first and falls back to
//! the legacy `lib<name>.so` file (backward compatibility, ADR 0046 §3).
//!
//! Multi-arch auto-selection (ADR 0054, `murphy-uk7.3`): the loader prefers
//! the exact host `lib/<platform>/` slot, then the current platform's
//! extension, then the first sorted entry (single-arch packs keep working).
//! Still out of scope: `cargo install` / `murphy plugin install` UX and
//! pack signing (ADR 0004 trust model).

use std::path::{Path, PathBuf};

/// Manifest file name at a pack root.
pub const MANIFEST_FILENAME: &str = "murphy-plugin.toml";

/// A parsed `murphy-plugin.toml`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    /// Optional: when absent, the provided cops are derived from the
    /// cdylib at load time (the pre-manifest behaviour).
    #[serde(default)]
    pub cops: CopsSection,
}

/// The `[plugin]` table: pack identity + host-ABI requirement.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginMeta {
    /// Distribution name, e.g. `murphy-rails`. Same alphabet as
    /// [`crate::plugin_resolver::validate_plugin_name`].
    pub name: String,
    /// Pack version (`0.3.1`). Semver recommended; the host only requires
    /// non-empty (version comparison / coexistence is install-UX scope).
    pub version: String,
    /// Required host ABI. Must equal
    /// [`murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION`] or the pack is
    /// rejected before `dlopen`.
    #[serde(rename = "murphy-api-version", alias = "murphy_api_version")]
    pub murphy_api_version: ApiVersion,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
}

/// The `[cops]` table: the statically-declared cop catalogue.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopsSection {
    /// Cop names the pack provides, e.g. `["Rails/HttpStatus"]`.
    /// Empty/absent means "derive from the cdylib at load".
    #[serde(default)]
    pub provided: Vec<String>,
}

/// The `murphy-api-version` value. TOML-native form is an integer
/// (`murphy-api-version = 4`); a string (`= "4"`) is accepted for
/// hand-written manifests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiVersion(pub u32);

impl<'de> serde::Deserialize<'de> for ApiVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ApiVersionVisitor;

        impl serde::de::Visitor<'_> for ApiVersionVisitor {
            type Value = ApiVersion;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an ABI version as an integer (e.g. `4`) or string (e.g. `\"4\"`)")
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<ApiVersion, E> {
                u32::try_from(v)
                    .map(ApiVersion)
                    .map_err(|_| E::custom(format!("ABI version out of range: {v}")))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<ApiVersion, E> {
                u32::try_from(v)
                    .map(ApiVersion)
                    .map_err(|_| E::custom(format!("ABI version out of range: {v}")))
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<ApiVersion, E> {
                v.trim()
                    .parse::<u32>()
                    .map(ApiVersion)
                    .map_err(|_| E::custom(format!("invalid ABI version: {v:?}")))
            }
        }

        deserializer.deserialize_any(ApiVersionVisitor)
    }
}

/// Manifest-level failure: unreadable / unparsable manifest, invalid
/// identity fields, ABI mismatch, or a pack dir with no loadable cdylib.
#[derive(Debug)]
pub enum ManifestError {
    /// The manifest file could not be read.
    Io(String),
    /// TOML parse / schema error (unknown field, missing key, bad type).
    Parse(String),
    /// `plugin.name` violates the plugin-name alphabet.
    InvalidName(String),
    /// `plugin.version` is empty.
    InvalidVersion(String),
    /// `murphy-api-version` differs from the host ABI.
    ApiMismatch {
        /// What this host embeds.
        expected: u32,
        /// What the manifest declares.
        got: u32,
    },
    /// The pack dir holds no `*.so` / `*.dylib` under `lib/`.
    NoCdylib { pack_dir: PathBuf },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Io(msg) => write!(f, "{msg}"),
            ManifestError::Parse(msg) => write!(f, "invalid {MANIFEST_FILENAME}: {msg}"),
            ManifestError::InvalidName(msg) => {
                write!(f, "invalid {MANIFEST_FILENAME} plugin name: {msg}")
            }
            ManifestError::InvalidVersion(name) => write!(
                f,
                "invalid {MANIFEST_FILENAME} for plugin `{name}`: `version` must be non-empty"
            ),
            ManifestError::ApiMismatch { expected, got } => write!(
                f,
                "plugin pack requires ABI version {got}, host provides {expected}                  (rebuild the pack against murphy-plugin-api {expected})"
            ),
            ManifestError::NoCdylib { pack_dir } => write!(
                f,
                "plugin pack `{}` has no cdylib under `lib/`                  (expected `lib/<arch>/lib<name>.{{so,dylib}}`)",
                pack_dir.display()
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

impl std::str::FromStr for PluginManifest {
    type Err = ManifestError;

    /// Parse manifest text (TOML) and validate the identity fields.
    fn from_str(text: &str) -> Result<Self, ManifestError> {
        let manifest: PluginManifest =
            toml::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }
}

impl PluginManifest {
    /// Read + parse the manifest at an explicit file path.
    pub fn from_file(manifest_path: &Path) -> Result<Self, ManifestError> {
        let text = std::fs::read_to_string(manifest_path).map_err(|e| {
            ManifestError::Io(format!("cannot read `{}`: {e}", manifest_path.display()))
        })?;
        text.parse()
    }

    /// Read + parse `<pack_dir>/murphy-plugin.toml`.
    pub fn from_pack_dir(pack_dir: &Path) -> Result<Self, ManifestError> {
        Self::from_file(&pack_dir.join(MANIFEST_FILENAME))
    }

    fn validate(&self) -> Result<(), ManifestError> {
        crate::plugin_resolver::validate_plugin_name(&self.plugin.name)
            .map_err(|e| ManifestError::InvalidName(e.to_string()))?;
        if self.plugin.version.trim().is_empty() {
            return Err(ManifestError::InvalidVersion(self.plugin.name.clone()));
        }
        Ok(())
    }

    /// Reject the pack unless its `murphy-api-version` equals the host
    /// ABI — before any `dlopen` happens.
    pub fn check_api_compat(&self) -> Result<(), ManifestError> {
        let expected = murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION;
        let got = self.plugin.murphy_api_version.0;
        if got != expected {
            return Err(ManifestError::ApiMismatch { expected, got });
        }
        Ok(())
    }

    /// The statically-declared cop catalogue (`[]` when the manifest
    /// leaves `[cops]` out — the host then derives it from the cdylib).
    pub fn provided_cops(&self) -> &[String] {
        &self.cops.provided
    }
}

/// True when `path` is a directory holding a `murphy-plugin.toml`.
pub fn is_pack_dir(path: &Path) -> bool {
    path.is_dir() && path.join(MANIFEST_FILENAME).is_file()
}

/// Canonical `lib/<platform>/` slot names (ADR 0054, `murphy-uk7.3`).
///
/// `os-arch` order (`linux-x86_64`, …) matches ADR 0046's examples and the
/// template matrix. The reversed gem tags (`x86_64-linux`, …) and Rust
/// triples are accepted as aliases by [`normalize_platform_slot`].
pub const SUPPORTED_PLATFORM_SLOTS: [&str; 4] = [
    "linux-x86_64",
    "linux-aarch64",
    "darwin-x86_64",
    "darwin-arm64",
];

/// The current host's `lib/<platform>/` slot.
///
/// `darwin-arm64` uses the `arm64` spelling (Apple / RubyGems convention);
/// the Linux ARM slot uses `aarch64` (Rust / kernel convention). Unknown
/// hosts return a non-matching sentinel so resolution falls through to the
/// extension tier (the pre-`uk7.3` narrow rule).
pub fn host_platform_slot() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64" | "x86" | "amd64") => "linux-x86_64",
        ("linux", "aarch64" | "arm64") => "linux-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("macos", "aarch64" | "arm64") => "darwin-arm64",
        _ => "unknown-unknown",
    }
}

/// Normalize a `lib/<platform>/` dir name (or gem tag / Rust triple) to its
/// canonical slot, or `None` when it names no known platform.
pub fn normalize_platform_slot(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    let s = lower.as_str();
    Some(match s {
        "linux-x86_64"
        | "linux-x86-64"
        | "x86_64-linux"
        | "x86_64-unknown-linux-gnu"
        | "x86_64-unknown-linux-musl" => "linux-x86_64",
        "linux-aarch64"
        | "linux-arm64"
        | "aarch64-linux"
        | "arm64-linux"
        | "aarch64-unknown-linux-gnu"
        | "aarch64-unknown-linux-musl" => "linux-aarch64",
        "darwin-x86_64" | "x86_64-darwin" | "x86_64-apple-darwin" => "darwin-x86_64",
        "darwin-arm64"
        | "darwin-aarch64"
        | "arm64-darwin"
        | "aarch64-darwin"
        | "aarch64-apple-darwin" => "darwin-arm64",
        _ => return None,
    })
}

/// The cdylib extension for a canonical slot (`dylib` on `darwin-*`).
fn platform_slot_extension(slot: &str) -> &'static str {
    if slot.starts_with("darwin-") {
        "dylib"
    } else {
        "so"
    }
}

/// The `lib/<platform>/` slot a candidate cdylib was staged under: the
/// first path component below `<pack>/lib/`, or `None` when the file sits
/// directly under `lib/` (legacy single-file staging).
fn candidate_slot(pack_dir: &Path, candidate: &Path) -> Option<String> {
    let lib_dir = pack_dir.join("lib");
    let rel = candidate.strip_prefix(&lib_dir).ok()?;
    rel.components().next().and_then(|first| {
        // A bare `lib/<file>.so` has no slot dir (single component).
        if rel.components().count() < 2 {
            return None;
        }
        match first {
            std::path::Component::Normal(os) => Some(os.to_string_lossy().into_owned()),
            _ => None,
        }
    })
}

/// Pick the loadable binary for `slot` from sorted `candidates` (three
/// tiers: exact slot → platform extension → sorted first). Pure helper so
/// tests can pin a slot without depending on the build host.
fn pick_cdylib_for_slot(pack_dir: &Path, candidates: &[PathBuf], slot: &str) -> Option<PathBuf> {
    let wanted = normalize_platform_slot(slot).unwrap_or(slot);
    if let Some(hit) = candidates.iter().find(|p| {
        candidate_slot(pack_dir, p)
            .as_deref()
            .and_then(normalize_platform_slot)
            == Some(wanted)
    }) {
        return Some(hit.clone());
    }
    let platform_ext = platform_slot_extension(wanted);
    if let Some(hit) = candidates
        .iter()
        .find(|p| p.extension().is_some_and(|e| e == platform_ext))
    {
        return Some(hit.clone());
    }
    candidates.first().cloned()
}

/// Map a pack dir to the cdylib the loader opens: parse (and validate)
/// the manifest, check host-ABI compatibility, then pick the loadable
/// binary under `<pack_dir>/lib/`.
///
/// Selection is three tiers, all deterministic (candidates are sorted):
/// the exact host `lib/<platform>/` slot wins; otherwise the first entry
/// with the current platform's extension wins (the pre-`uk7.3` narrow
/// rule); otherwise the first sorted entry wins so single-arch packs load
/// anywhere. Slot-dir spellings accept gem-tag and Rust-triple aliases
/// (see [`normalize_platform_slot`]).
pub fn resolve_pack_cdylib(pack_dir: &Path) -> Result<PathBuf, ManifestError> {
    let manifest = PluginManifest::from_pack_dir(pack_dir)?;
    manifest.check_api_compat()?;
    let lib_dir = pack_dir.join("lib");
    let mut candidates: Vec<PathBuf> = Vec::new();
    collect_cdylibs(&lib_dir, &mut candidates);
    candidates.sort();
    if candidates.is_empty() {
        return Err(ManifestError::NoCdylib {
            pack_dir: pack_dir.to_path_buf(),
        });
    }
    Ok(
        pick_cdylib_for_slot(pack_dir, &candidates, host_platform_slot())
            .expect("non-empty checked above"),
    )
}

fn collect_cdylibs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_cdylibs(&path, out);
        } else if path.is_file() && path.extension().is_some_and(|e| e == "so" || e == "dylib") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    const EXAMPLE_MANIFEST: &str = r#"
[plugin]
name = "murphy-rails"
version = "0.3.1"
murphy-api-version = 4
authors = ["Murphy contributors"]
license = "MIT"
description = "Rails cops for Murphy"
homepage = "https://example.invalid/murphy-rails"

[cops]
provided = ["Rails/HttpStatus", "Rails/SaveBang"]
"#;

    #[test]
    fn parses_full_manifest_with_integer_api_version() {
        let m = PluginManifest::from_str(EXAMPLE_MANIFEST).expect("parses");
        assert_eq!(m.plugin.name, "murphy-rails");
        assert_eq!(m.plugin.version, "0.3.1");
        assert_eq!(m.plugin.murphy_api_version, ApiVersion(4));
        assert_eq!(m.plugin.authors, vec!["Murphy contributors"]);
        assert_eq!(m.plugin.license.as_deref(), Some("MIT"));
        assert_eq!(
            m.provided_cops(),
            &["Rails/HttpStatus".to_string(), "Rails/SaveBang".to_string()]
        );
        m.check_api_compat().expect("ABI 4 matches host");
    }

    #[test]
    fn accepts_string_api_version_and_underscore_alias() {
        let m = PluginManifest::from_str(
            "[plugin]\nname = \"x\"\nversion = \"0.1.0\"\nmurphy_api_version = \"4\"\n",
        )
        .expect("string + alias form parses");
        assert_eq!(m.plugin.murphy_api_version, ApiVersion(4));
        assert!(m.provided_cops().is_empty());
    }

    #[test]
    fn minimal_manifest_gets_defaults() {
        let m = PluginManifest::from_str(
            "[plugin]\nname = \"murphy-foo\"\nversion = \"1.0\"\nmurphy-api-version = 4\n",
        )
        .expect("minimal parses");
        assert!(m.plugin.authors.is_empty());
        assert_eq!(m.plugin.license, None);
        assert!(m.provided_cops().is_empty());
    }

    #[test]
    fn rejects_unknown_field_missing_name_bad_name_and_empty_version() {
        let unknown = "[plugin]\nname = \"x\"\nversion = \"1\"\nmurphy-api-version = 4\ntypo = 1\n";
        assert!(
            matches!(
                PluginManifest::from_str(unknown),
                Err(ManifestError::Parse(_))
            ),
            "unknown field must fail"
        );

        let missing = "[plugin]\nversion = \"1\"\nmurphy-api-version = 4\n";
        assert!(
            matches!(
                PluginManifest::from_str(missing),
                Err(ManifestError::Parse(_))
            ),
            "missing name must fail"
        );

        let bad_name = "[plugin]\nname = \"../evil\"\nversion = \"1\"\nmurphy-api-version = 4\n";
        assert!(
            matches!(
                PluginManifest::from_str(bad_name),
                Err(ManifestError::InvalidName(_))
            ),
            "path-traversal name must fail"
        );

        let empty_version = "[plugin]\nname = \"x\"\nversion = \"  \"\nmurphy-api-version = 4\n";
        assert!(
            matches!(
                PluginManifest::from_str(empty_version),
                Err(ManifestError::InvalidVersion(_))
            ),
            "blank version must fail"
        );
    }

    #[test]
    fn api_mismatch_reports_expected_and_got() {
        let m = PluginManifest::from_str(
            "[plugin]\nname = \"x\"\nversion = \"1\"\nmurphy-api-version = 999\n",
        )
        .expect("parses (compat is a separate check)");
        match m.check_api_compat() {
            Err(ManifestError::ApiMismatch { expected, got }) => {
                assert_eq!(expected, murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION);
                assert_eq!(got, 999);
            }
            other => panic!("expected ApiMismatch, got {other:?}"),
        }
    }

    #[test]
    fn is_pack_dir_and_resolve_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!is_pack_dir(dir.path()));
        let pack = dir.path().join("murphy-foo");
        let arch = pack.join("lib").join("linux-x86_64");
        std::fs::create_dir_all(&arch).expect("mkdir lib/<arch>");
        std::fs::write(
            pack.join(MANIFEST_FILENAME),
            "[plugin]\nname = \"murphy-foo\"\nversion = \"0.1.0\"\nmurphy-api-version = 4\n",
        )
        .expect("write manifest");
        let cdylib = arch.join("libmurphy_foo.so");
        std::fs::write(&cdylib, b"").expect("write fake .so");
        assert!(is_pack_dir(&pack));
        assert_eq!(resolve_pack_cdylib(&pack).expect("resolves"), cdylib);
    }

    #[test]
    fn resolve_prefers_platform_extension_and_sorts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pack = dir.path().join("p");
        std::fs::create_dir_all(pack.join("lib").join("b")).expect("mkdir");
        std::fs::create_dir_all(pack.join("lib").join("a")).expect("mkdir");
        std::fs::write(
            pack.join(MANIFEST_FILENAME),
            "[plugin]\nname = \"p\"\nversion = \"1\"\nmurphy-api-version = 4\n",
        )
        .expect("write manifest");
        // Both extensions present: the current platform's wins regardless
        // of sort order (`a/*.dylib` sorts before `b/*.so`).
        std::fs::write(pack.join("lib").join("b").join("libp.so"), b"").expect("write");
        std::fs::write(pack.join("lib").join("a").join("libp.dylib"), b"").expect("write");
        let got = resolve_pack_cdylib(&pack).expect("resolves");
        let expected_ext = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        assert_eq!(
            got.extension().unwrap(),
            expected_ext,
            "platform extension wins: {got:?}"
        );
    }

    #[test]
    fn resolve_errors_when_lib_has_no_cdylib() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pack = dir.path().join("p");
        std::fs::create_dir_all(pack.join("lib")).expect("mkdir");
        std::fs::write(
            pack.join(MANIFEST_FILENAME),
            "[plugin]\nname = \"p\"\nversion = \"1\"\nmurphy-api-version = 4\n",
        )
        .expect("write manifest");
        match resolve_pack_cdylib(&pack) {
            Err(ManifestError::NoCdylib { pack_dir }) => assert_eq!(pack_dir, pack),
            other => panic!("expected NoCdylib, got {other:?}"),
        }
    }

    #[test]
    fn example_pack_reference_manifest_parses_and_lists_both_cops() {
        // The reference pack doubles as the manifest format example: this
        // test keeps `murphy-plugin.toml` next to the pack's `Cargo.toml`
        // in sync with the parser (and the cop list with the cdylib).
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../murphy-example-pack")
            .join(MANIFEST_FILENAME);
        let m = PluginManifest::from_file(&path)
            .unwrap_or_else(|e| panic!("reference manifest {}: {e}", path.display()));
        assert_eq!(m.plugin.name, "murphy-example-pack");
        m.check_api_compat()
            .expect("reference pack targets host ABI");
        assert_eq!(
            m.provided_cops(),
            &[
                "Example/NoEval".to_string(),
                "Example/TodoFormat".to_string()
            ]
        );
    }

    #[test]
    fn supported_slots_cover_the_template_matrix() {
        assert_eq!(
            SUPPORTED_PLATFORM_SLOTS,
            [
                "linux-x86_64",
                "linux-aarch64",
                "darwin-x86_64",
                "darwin-arm64"
            ]
        );
        assert!(
            SUPPORTED_PLATFORM_SLOTS.contains(&host_platform_slot())
                || host_platform_slot() == "unknown-unknown"
        );
    }

    #[test]
    fn normalize_accepts_gem_tags_and_rust_triples() {
        let cases = [
            ("linux-x86_64", "linux-x86_64"),
            ("x86_64-linux", "linux-x86_64"),
            ("x86_64-unknown-linux-gnu", "linux-x86_64"),
            ("linux-aarch64", "linux-aarch64"),
            ("aarch64-linux", "linux-aarch64"),
            ("aarch64-unknown-linux-gnu", "linux-aarch64"),
            ("darwin-x86_64", "darwin-x86_64"),
            ("x86_64-darwin", "darwin-x86_64"),
            ("x86_64-apple-darwin", "darwin-x86_64"),
            ("darwin-arm64", "darwin-arm64"),
            ("arm64-darwin", "darwin-arm64"),
            ("aarch64-apple-darwin", "darwin-arm64"),
            ("darwin-aarch64", "darwin-arm64"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                normalize_platform_slot(input),
                Some(expected),
                "alias {input:?}"
            );
        }
        assert_eq!(normalize_platform_slot("windows-x86_64"), None);
        assert_eq!(normalize_platform_slot("linux"), None);
    }

    /// Build a pack dir with one fake cdylib per slot name in `slots`.
    fn multi_slot_pack(slots: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let pack = dir.path().join("p");
        for slot in slots {
            let slot_dir = pack.join("lib").join(slot);
            std::fs::create_dir_all(&slot_dir).expect("mkdir lib/<slot>");
            let ext = if slot.starts_with("darwin-") {
                "dylib"
            } else {
                "so"
            };
            std::fs::write(slot_dir.join(format!("libp.{ext}")), b"").expect("write");
        }
        std::fs::write(
            pack.join(MANIFEST_FILENAME),
            "[plugin]\nname = \"p\"\nversion = \"1\"\nmurphy-api-version = 4\n",
        )
        .expect("write manifest");
        dir
    }

    #[test]
    fn exact_slot_beats_extension_and_sort_order() {
        // All four slots ship: the pinned slot must win even when another
        // slot's path sorts first (e.g. `darwin-arm64` < `linux-x86_64`).
        let dir = multi_slot_pack(&SUPPORTED_PLATFORM_SLOTS);
        let pack = dir.path().join("p");
        let lib = pack.join("lib");
        let mut candidates = Vec::new();
        collect_cdylibs(&lib, &mut candidates);
        candidates.sort();
        assert_eq!(candidates.len(), SUPPORTED_PLATFORM_SLOTS.len());
        for slot in SUPPORTED_PLATFORM_SLOTS {
            let got = pick_cdylib_for_slot(&pack, &candidates, slot).expect("picks");
            let got_slot = candidate_slot(&pack, &got).expect("has slot");
            assert_eq!(
                normalize_platform_slot(&got_slot),
                normalize_platform_slot(slot),
                "slot {slot} must resolve to its own dir, got {got:?}"
            );
        }
    }

    #[test]
    fn gem_tag_alias_resolves_to_canonical_slot() {
        let dir = multi_slot_pack(&["linux-x86_64", "linux-aarch64"]);
        let pack = dir.path().join("p");
        let lib = pack.join("lib");
        let mut candidates = Vec::new();
        collect_cdylibs(&lib, &mut candidates);
        candidates.sort();
        let got = pick_cdylib_for_slot(&pack, &candidates, "aarch64-linux").expect("picks");
        assert!(
            got.to_string_lossy().contains("linux-aarch64"),
            "gem tag must hit canonical slot, got {got:?}"
        );
    }

    #[test]
    fn missing_slot_falls_back_to_extension_then_sorted_first() {
        // Pack ships only foreign slots: no exact match, so the platform
        // extension tier (then sorted-first) applies — single-arch packs
        // keep loading anywhere (backward compat with the narrow rule).
        let dir = multi_slot_pack(&["darwin-arm64"]);
        let pack = dir.path().join("p");
        let lib = pack.join("lib");
        let mut candidates = Vec::new();
        collect_cdylibs(&lib, &mut candidates);
        candidates.sort();
        assert_eq!(candidates.len(), 1);
        let got = pick_cdylib_for_slot(&pack, &candidates, "linux-x86_64").expect("picks");
        assert_eq!(got, candidates[0]);
    }

    #[test]
    fn template_ci_matrix_covers_all_supported_slots() {
        // The template's `ci.yml.liquid` is the matrix-config source of
        // truth: every supported `lib/<platform>/` slot needs a runner +
        // Rust target + staging step, or cross-built packs silently miss a
        // slot. This guard keeps the matrix and `SUPPORTED_PLATFORM_SLOTS`
        // in step.
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/plugins/template/.github/workflows/ci.yml.liquid");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read template CI {}: {e}", path.display()));
        for slot in SUPPORTED_PLATFORM_SLOTS {
            assert!(
                text.contains(slot),
                "template CI must stage slot `{slot}` ({})",
                path.display()
            );
        }
        for target in [
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
        ] {
            assert!(
                text.contains(target),
                "template CI must build Rust target `{target}`"
            );
        }
    }
}
