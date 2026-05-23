//! On-disk arena binary cache for Murphy (murphy-9cr.26).
//!
//! Lookup and persistence of [`murphy_ast::Ast`] keyed by
//! `content_hash(source)` + a derived version key. The cache is mach-local
//! and bestiowed effortfully on the next compile of the same source —
//! any version-key or format-version mismatch silently misses and rolls
//! over to a regenerate, never panicking the linter on a stale or
//! corrupted file. Lookup and write failures (I/O errors, malformed
//! header, bounds-check failure) are all treated as cache misses; cache
//! never escalates an error to the caller.
//!
//! ```text
//!   $root/<aa>/<aabbcc...64hex>.ast
//!         ^^   ^^^^^^^^^^^^^^^^^^^^
//!         |    sha256(content_hash || version_key) hex
//!         2-hex shard
//! ```
//!
//! `version_key = sha256(murphy_version_bytes || target_triple_bytes ||
//!                       layer_version_le_bytes)`.

use murphy_ast::Ast;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Environment variable that, when set to any value, disables the cache.
const DISABLE_ENV: &str = "MURPHY_NO_CACHE";

/// Path subcomponent for the binary format version, included so future
/// formats can coexist on disk without clobbering current entries.
const FORMAT_DIR: &str = "v1";

/// An on-disk arena binary cache. Cheap to clone via `Arc`; safe to share
/// across threads — all methods take `&self`.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
    version_key: [u8; 32],
}

impl Cache {
    /// Open the cache at `$XDG_CACHE_HOME/murphy/v1` (or, when unset,
    /// `$HOME/.cache/murphy/v1`). Returns `None` if `MURPHY_NO_CACHE` is
    /// set, if neither base directory can be resolved, or if the root
    /// directory cannot be created.
    pub fn open(layer_version: u32) -> Option<Cache> {
        if std::env::var_os(DISABLE_ENV).is_some() {
            return None;
        }
        let base = xdg_cache_home()?;
        let root = base.join("murphy").join(FORMAT_DIR);
        std::fs::create_dir_all(&root).ok()?;
        Some(Cache {
            root,
            version_key: derive_version_key(layer_version),
        })
    }

    /// Open the cache rooted at `root`. Used in tests and by embedders that
    /// want explicit control over the cache location. The directory is
    /// created if missing. Panics only if directory creation fails — a
    /// stricter contract than [`Cache::open`] precisely because the caller
    /// supplied the path.
    pub fn open_in(root: PathBuf, layer_version: u32) -> Cache {
        std::fs::create_dir_all(&root).expect("Cache::open_in: mkdir failed");
        Cache {
            root,
            version_key: derive_version_key(layer_version),
        }
    }

    /// The cache root directory. Mostly useful for diagnostics.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The derived version key. Two caches that differ in any of the
    /// inputs (Murphy version, target triple, layer version) will return
    /// different keys here.
    pub fn version_key(&self) -> &[u8; 32] {
        &self.version_key
    }

    /// Look up the cached AST for a source whose `content_hash` is known.
    /// Returns `None` on any failure — missing file, I/O error, header
    /// mismatch, bounds-check failure. Never panics, never returns `Err`.
    pub fn lookup(&self, content_hash: &[u8; 32]) -> Option<Ast> {
        let path = self.path_for(content_hash);
        let bytes = std::fs::read(&path).ok()?;
        Ast::from_bytes(&bytes).ok()
    }

    /// Persist `ast` under the given `content_hash`. Failures (I/O, mkdir
    /// of the shard directory, rename collision) are silently swallowed —
    /// the cache is best-effort, never load-bearing.
    pub fn put(&self, content_hash: &[u8; 32], ast: &Ast) {
        let _ = self.put_impl(content_hash, ast);
    }

    fn put_impl(&self, content_hash: &[u8; 32], ast: &Ast) -> std::io::Result<()> {
        // Cache writes are best-effort: a non-UTF-8 `source.path` (which
        // `to_bytes` now rejects rather than lossy-converting) just means
        // we skip caching this AST.
        let bytes = ast
            .to_bytes()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e:?}")))?;
        let path = self.path_for(content_hash);
        let dir = path.parent().expect("path_for always has a parent");
        std::fs::create_dir_all(dir)?;
        // Tempfile in the same directory → atomic rename onto target.
        let tmp = tmp_path(dir);
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path).inspect_err(|_| {
            // If rename failed, do not orphan the tempfile.
            let _ = std::fs::remove_file(&tmp);
        })?;
        Ok(())
    }

    /// `$root/<aa>/<aa...>.ast` where the hash combines `content_hash`
    /// with the cache's `version_key` so different keys never collide on
    /// disk.
    fn path_for(&self, content_hash: &[u8; 32]) -> PathBuf {
        let mut h = Sha256::new();
        h.update(content_hash);
        h.update(self.version_key);
        let combined: [u8; 32] = h.finalize().into();
        let hex = hex(&combined);
        let shard = &hex[..2];
        let mut p = self.root.clone();
        p.push(shard);
        p.push(format!("{hex}.ast"));
        p
    }
}

/// Build the version key from the running binary's identity plus the
/// layer version. Exposed so downstream tooling (e.g. `murphy cache stat`)
/// can derive the same key without owning a `Cache`.
pub fn derive_version_key(layer_version: u32) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(env!("CARGO_PKG_VERSION").as_bytes());
    h.update(b"\0");
    h.update(env!("MURPHY_CACHE_TARGET_TRIPLE").as_bytes());
    h.update(b"\0");
    h.update(layer_version.to_le_bytes());
    h.finalize().into()
}

fn xdg_cache_home() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("XDG_CACHE_HOME") {
        let p = PathBuf::from(v);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home);
    if p.as_os_str().is_empty() {
        return None;
    }
    Some(p.join(".cache"))
}

/// Tempfile name unique across processes and concurrent writers within a
/// process. Same-directory by construction so `rename` is atomic on POSIX.
fn tmp_path(dir: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!(".murphy-cache.{}-{n}.tmp", std::process::id());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_key_is_deterministic() {
        assert_eq!(derive_version_key(7), derive_version_key(7));
    }

    #[test]
    fn version_key_changes_with_layer_version() {
        assert_ne!(derive_version_key(1), derive_version_key(2));
    }

    #[test]
    fn hex_round_trips_a_known_value() {
        assert_eq!(hex(&[0x00, 0x10, 0xff]), "0010ff");
    }
}
