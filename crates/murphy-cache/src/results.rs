//! Persistent per-file lint-result cache (A5, murphy-fmw.1.1).
//!
//! While [`crate::Cache`] stores the parsed arena AST, [`ResultCache`]
//! stores the *analysis result* for a file: the post-inline-filter
//! offense list serialized as JSON bytes. A hit skips both prism parse
//! and cop dispatch for the file, which is what makes the second
//! consecutive `murphy lint` run 5x+ faster (Phase 8 gate #3).
//!
//! Key design (stale-cache avoidance):
//!
//! ```text
//!   $root/results/<aa>/<aabbcc...64hex>.json
//!                 ^^   ^^^^^^^^^^^^^^^^^^^^
//!                 |    sha256(content_hash || result_version_key || path) hex
//!                 2-hex shard
//! ```
//!
//! `result_version_key = sha256(version_key || extra_fingerprint)` where
//! `version_key` is the AST cache's
//! [`crate::derive_version_key`] (murphy version + target triple +
//! translate layer version) and `extra_fingerprint` is the caller's
//! lint fingerprint (cop pack version + config — see
//! `murphy_core::lint_fingerprint`). Different cop sets or configs
//! therefore miss by construction and never read stale results.
//!
//! Like [`crate::Cache`], every failure is a silent miss: missing file,
//! I/O error, corrupt JSON, or wrong key all return `None`. The cache
//! never escalates an error and never panics on cache content. Callers
//! own serialization: `Vec<u8>` here is opaque JSON bytes; the CLI
//! serializes `Vec<Offense>` with `serde_json` and validates on lookup.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Subdirectory under the v1 root holding result entries.
const RESULTS_DIR: &str = "results";

/// `FORMAT_DIR` mirror (kept local so this module stays self-contained;
/// must match `crate::FORMAT_DIR` = "v1").
const FORMAT_DIR: &str = "v1";

/// `DISABLE_ENV` mirror (must match `crate` = "MURPHY_NO_CACHE").
const DISABLE_ENV: &str = "MURPHY_NO_CACHE";

/// Maximum cached result payload accepted on lookup (8 MiB). Larger files
/// are treated as a miss so a corrupt file cannot OOM the linter.
pub const MAX_RESULT_BYTES: usize = 8 * 1024 * 1024;

/// On-disk per-file lint-result cache. Safe to share across threads —
/// all methods take `&self`.
#[derive(Debug, Clone)]
pub struct ResultCache {
    root: PathBuf,
    version_key: [u8; 32],
}

impl ResultCache {
    /// Open the result cache at `$XDG_CACHE_HOME/murphy/v1/results`
    /// (or `$HOME/.cache/...`). Returns `None` when `MURPHY_NO_CACHE`
    /// is set, when the base directory cannot be resolved, or when the
    /// directory cannot be created.
    ///
    /// `extra_fingerprint` is the caller's lint fingerprint
    /// (`murphy_core::lint_fingerprint`: cop pack version + config).
    /// `layer_version` is `murphy_translate::LAYER_VERSION`.
    pub fn open(extra_fingerprint: &[u8; 32], layer_version: u32) -> Option<ResultCache> {
        if std::env::var_os(DISABLE_ENV).is_some() {
            return None;
        }
        let root = crate::default_cache_root()?;
        Some(Self::open_in(root, layer_version, extra_fingerprint))
    }

    /// Open rooted at `root` (tests / embedders with explicit control).
    /// Creates the results directory if missing. Panics only when
    /// directory creation fails — the caller supplied the path.
    ///
    /// When `root` already ends in `v1` (the common test shape
    /// `tempdir/v1`), the results live at `<root>/results` without
    /// doubling the `v1` component; otherwise `<root>/v1/results` is
    /// used so an XDG-style base works directly.
    pub fn open_in(root: PathBuf, layer_version: u32, extra_fingerprint: &[u8; 32]) -> ResultCache {
        let version_key = derive_result_version_key(layer_version, extra_fingerprint);
        let dir = if root.file_name().is_some_and(|n| n == FORMAT_DIR) {
            root.join(RESULTS_DIR)
        } else {
            root.join(FORMAT_DIR).join(RESULTS_DIR)
        };
        std::fs::create_dir_all(&dir).expect("ResultCache::open_in: mkdir failed");
        ResultCache {
            root: dir,
            version_key,
        }
    }

    /// Open rooted at an explicit *results* dir (no `v1`/`results`
    /// suffixing). Used when the caller already resolved the full
    /// results directory (e.g. `Cache::root().join("results")`).
    pub fn open_in_results_dir(
        results_dir: PathBuf,
        layer_version: u32,
        extra_fingerprint: &[u8; 32],
    ) -> ResultCache {
        let version_key = derive_result_version_key(layer_version, extra_fingerprint);
        std::fs::create_dir_all(&results_dir)
            .expect("ResultCache::open_in_results_dir: mkdir failed");
        ResultCache {
            root: results_dir,
            version_key,
        }
    }

    /// The results directory. Useful for diagnostics (`cache stat`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The derived result version key.
    pub fn version_key(&self) -> &[u8; 32] {
        &self.version_key
    }

    /// Look up the cached result JSON for (`content_hash`, `file_path`).
    /// Returns `None` on any failure — missing file, I/O error, oversize
    /// payload. Callers validate the JSON shape (failed deserialize ⇒
    /// miss).
    ///
    /// The key mixes the file path so identical content at different
    /// paths (which can yield different offenses under per-cop
    /// `Include`/`Exclude` scopes, e.g. pack-bundled defaults) never
    /// shares an entry. Same path + same content + same
    /// cop-pack/config fingerprint ⇒ hit.
    pub fn lookup(&self, content_hash: &[u8; 32], file_path: &str) -> Option<Vec<u8>> {
        let path = self.path_for(content_hash, file_path);
        let bytes = std::fs::read(&path).ok()?;
        if bytes.is_empty() || bytes.len() > MAX_RESULT_BYTES {
            return None;
        }
        Some(bytes)
    }

    /// Persist result JSON `payload` under (`content_hash`, `file_path`).
    /// Best-effort; failures are silently swallowed. Empty or oversize
    /// payloads are rejected without I/O.
    pub fn put(&self, content_hash: &[u8; 32], file_path: &str, payload: &[u8]) {
        if payload.is_empty() || payload.len() > MAX_RESULT_BYTES {
            return;
        }
        let _ = self.put_impl(content_hash, file_path, payload);
    }

    fn put_impl(
        &self,
        content_hash: &[u8; 32],
        file_path: &str,
        payload: &[u8],
    ) -> std::io::Result<()> {
        let path = self.path_for(content_hash, file_path);
        let dir = path.parent().expect("path_for always has a parent");
        std::fs::create_dir_all(dir)?;
        let tmp = tmp_path(dir);
        std::fs::write(&tmp, payload)?;
        std::fs::rename(&tmp, &path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
        Ok(())
    }

    fn path_for(&self, content_hash: &[u8; 32], file_path: &str) -> PathBuf {
        let mut h = Sha256::new();
        h.update(content_hash);
        h.update(self.version_key);
        h.update((file_path.len() as u64).to_le_bytes());
        h.update(file_path.as_bytes());
        let combined: [u8; 32] = h.finalize().into();
        let hex = hex(&combined);
        let shard = &hex[..2];
        let mut p = self.root.clone();
        p.push(shard);
        p.push(format!("{hex}.json"));
        p
    }
}

/// `sha256(version_key || extra_fingerprint)`. `version_key` already
/// binds murphy version + target triple + translate layer version, so
/// the result key misses whenever the binary, the platform, the AST
/// layer, the cop set, or the config changes.
pub fn derive_result_version_key(layer_version: u32, extra_fingerprint: &[u8; 32]) -> [u8; 32] {
    let base = crate::derive_version_key(layer_version);
    let mut h = Sha256::new();
    h.update(base);
    h.update(extra_fingerprint);
    h.finalize().into()
}

fn tmp_path(dir: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!(".murphy-results.{}-{n}.tmp", std::process::id());
    dir.join(name)
}

fn hex(bytes: &[u8]) -> String {
    const CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(CHARS[(b >> 4) as usize] as char);
        s.push(CHARS[(b & 0xF) as usize] as char);
    }
    s
}
