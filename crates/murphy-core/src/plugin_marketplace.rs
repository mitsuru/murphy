//! Static-index remote pack discovery (murphy-uk7.2; ADR 0053).
//!
//! Thin extension of the C1 thin registry (ADR 0049): the index stays a
//! static TOML file, but entries may now carry a download `source-url`
//! (git repo, `http(s)` tarball, or `file://` dir/tarball). This module
//! implements the three marketplace flows over that field:
//!
//! - **search**: substring match over name/description/gem.
//! - **fetch**: materialize the source into a local dir (copy for
//!   `file://` dirs, `curl` + `tar` for tarballs, `git clone` for git).
//! - **publish**: validate a local pack dir and render the `[[packs]]`
//!   TOML snippet a publisher appends to their static index.
//!
//! No hosted service, no new Cargo deps: network fetch shells out to the
//! system `git` / `curl` / `tar` binaries (already required on dev/CI
//! hosts). No ABI bump.

use sha2::Digest as _;
use std::path::{Path, PathBuf};

use crate::pack_registry::PackEntry;

/// Env override for the fetch cache root (test hook / manual override).
/// When set, it replaces `dirs::cache_dir()/murphy/plugins/`.
pub const PLUGIN_CACHE_DIR_ENV: &str = "MURPHY_PLUGIN_CACHE_DIR";

/// Fetch cache root: `$MURPHY_PLUGIN_CACHE_DIR` when set, else
/// `dirs::cache_dir()/murphy/plugins/`. `None` when neither resolves.
pub fn plugin_cache_dir() -> Option<PathBuf> {
    if let Some(env) = std::env::var_os(PLUGIN_CACHE_DIR_ENV) {
        let p = PathBuf::from(env);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    dirs::cache_dir().map(|d| d.join("murphy/plugins"))
}

/// Cache root with a temp-dir fallback (never `None`): the cache dir when
/// available, else `<temp>/murphy-plugins/`.
pub fn plugin_cache_dir_or_temp() -> PathBuf {
    plugin_cache_dir().unwrap_or_else(|| std::env::temp_dir().join("murphy-plugins"))
}

/// Substring search over the registry index (case-insensitive).
///
/// Matches when `query` appears in the pack `name`, `description`, or
/// `gem` field. An empty/blank query returns every pack. Results sort by
/// name for stable output.
pub fn search_packs<'a>(
    index: &'a crate::pack_registry::PackRegistryIndex,
    query: &str,
) -> Vec<&'a PackEntry> {
    let q = query.trim().to_lowercase();
    let mut out: Vec<&PackEntry> = if q.is_empty() {
        index.packs.iter().collect()
    } else {
        index
            .packs
            .iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&q)
                    || p.description.to_lowercase().contains(&q)
                    || p.gem.to_lowercase().contains(&q)
            })
            .collect()
    };
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Human-readable source description for `--dry-run` / status messages.
pub fn describe_remote_source(entry: &PackEntry) -> String {
    match entry.remote_url() {
        Some(url) => {
            let mut s = format!("remote `{url}`");
            if let Some(rev) = entry.remote_rev() {
                s.push_str(&format!(" (rev `{rev}`)"));
            }
            if let Some(sub) = entry.remote_subdir() {
                s.push_str(&format!(" subdir `{sub}`"));
            }
            s
        }
        None => "no remote source (local gems / `--from` only)".to_string(),
    }
}

/// Fetch `entry`'s remote source into `dest_dir` and return the pack root.
///
/// `dest_dir` is removed first when it exists, so every fetch is fresh.
/// The returned path is `dest_dir` itself, `dest_dir/<subdir>`, or the
/// discovered pack root inside an extracted tarball/checkout (see
/// [`resolve_fetched_root`]).
///
/// Dispatch on the URL:
/// - ends with `.tar.gz`/`.tgz` → tarball (`curl` for `http(s)`, file
///   copy for `file://`/plain path, optional SHA-256 check, `tar` extract).
/// - otherwise git-looking (`http(s)`/`ssh`/`git@`/`.git` suffix or an
///   explicit `source-rev`) → `git clone` (+ `checkout --detach <rev>`).
/// - otherwise a local dir (`file://` or plain path): the dir itself or
///   `<dir>/<name>/` must be a pack dir; copied recursively.
///
/// Returns `Err` when the entry has no `source-url`, a required binary
/// (`git`/`curl`/`tar`) is missing, the checksum mismatches, or no pack
/// root is found after materializing.
pub fn fetch_pack(entry: &PackEntry, dest_dir: &Path) -> Result<PathBuf, String> {
    let url = entry.remote_url().ok_or_else(|| {
        format!(
            "pack `{}` has no remote source (`source-url` is unset; use `--from <dir>` or installed gems)",
            entry.name
        )
    })?;
    if url.trim().is_empty() {
        return Err(format!("pack `{}` has an empty `source-url`", entry.name));
    }
    // Reject path traversal in source-subdir early (it is joined to the
    // fetched root below).
    if let Some(sub) = entry.remote_subdir()
        && (sub.contains("..") || Path::new(sub).is_absolute())
    {
        return Err(format!(
            "pack `{}` has an unsafe `source-subdir` `{sub}` (must be a relative path without `..`)",
            entry.name
        ));
    }
    if is_tarball_url(url) {
        fetch_tarball(entry, url, dest_dir)
    } else if is_git_url(url) || entry.remote_rev().is_some() {
        fetch_git(entry, url, dest_dir)
    } else {
        // `file://` dir/tarball or plain path. A `file://` tarball file
        // still goes through the tarball flow.
        let path = strip_file_scheme(url);
        if is_tarball_url(path) {
            fetch_tarball(entry, url, dest_dir)
        } else if looks_like_git_file_source(entry, url, Path::new(path)) {
            fetch_git(entry, url, dest_dir)
        } else {
            fetch_local_dir(entry, Path::new(path), dest_dir)
        }
    }
}

/// Fetch into `<cache_root>/<name>/` (fresh) and return the pack root.
pub fn fetch_pack_to_cache(entry: &PackEntry, cache_root: &Path) -> Result<PathBuf, String> {
    let dest = cache_root.join(&entry.name);
    fetch_pack(entry, &dest)
}

fn looks_like_git_file_source(entry: &PackEntry, url: &str, path: &Path) -> bool {
    // A `file://` path that is not directly a pack dir (nor a parent of
    // `<name>/`) is probably a local git repo to clone. Probe the pack
    // shapes first; when neither hits, fall through to git so `git clone
    // <local-path>` gets a chance with a clear error when it is neither.
    if entry.remote_rev().is_some() {
        return true;
    }
    if url.trim_end().ends_with(".git") {
        return true;
    }
    if crate::plugin_manifest::is_pack_dir(path) {
        return false;
    }
    if crate::plugin_manifest::is_pack_dir(&path.join(&entry.name)) {
        return false;
    }
    // Heuristic: a dir holding a `.git` subdir is a checkout to clone
    // (rather than a pack dir to copy).
    path.join(".git").is_dir()
}

fn is_tarball_url(url: &str) -> bool {
    let base = url.split(['?', '#']).next().unwrap_or(url);
    let base = base.strip_suffix('/').unwrap_or(base);
    let lower = base.to_lowercase();
    lower.ends_with(".tar.gz") || lower.ends_with(".tgz")
}

fn is_git_url(url: &str) -> bool {
    let u = url.trim();
    u.starts_with("https://")
        || u.starts_with("http://")
        || u.starts_with("ssh://")
        || u.starts_with("git://")
        || u.starts_with("git@")
        || u.trim_end().ends_with(".git")
}

fn strip_file_scheme(url: &str) -> &str {
    url.strip_prefix("file://").unwrap_or(url)
}

fn run_cmd(program: &str, args: &[&str], dir: Option<&Path>) -> Result<String, String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("cannot run `{program}` (is it installed?): {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let detail = format!("{stdout}{stderr}").trim().to_string();
        return Err(format!(
            "`{program} {}` failed: {}",
            args.join(" "),
            if detail.is_empty() {
                format!("exit {}", out.status)
            } else {
                detail.chars().take(1000).collect()
            }
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn ensure_clean_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)
            .map_err(|e| format!("cannot clear `{}`: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create `{}`: {e}", dir.display()))?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("cannot create `{}`: {e}", dst.display()))?;
    let entries =
        std::fs::read_dir(src).map_err(|e| format!("cannot read `{}`: {e}", src.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read `{}`: {e}", src.display()))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| {
                format!(
                    "cannot copy `{}` to `{}`: {e}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }
    Ok(())
}

/// Resolve the pack root inside a materialized `dest_dir`:
/// `<subdir>` when set, else `dest_dir` itself, else
/// `dest_dir/<name>/`, else the first sorted direct-child pack dir.
fn resolve_fetched_root(
    entry_name: &str,
    dest_dir: &Path,
    subdir: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(sub) = subdir {
        let root = dest_dir.join(sub);
        if crate::plugin_manifest::is_pack_dir(&root) {
            return Ok(root);
        }
        return Err(format!(
            "fetched pack `{entry_name}` has no manifest at `{}/<{sub}>` (checked `source-subdir` `{sub}`)",
            dest_dir.display()
        ));
    }
    if crate::plugin_manifest::is_pack_dir(dest_dir) {
        return Ok(dest_dir.to_path_buf());
    }
    let nested = dest_dir.join(entry_name);
    if crate::plugin_manifest::is_pack_dir(&nested) {
        return Ok(nested);
    }
    // Tarballs / repos often wrap the pack in one top-level dir
    // (`murphy-foo-0.1.0/murphy-plugin.toml`). Accept the first sorted
    // child pack dir so publishers do not need a flat layout.
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dest_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() && crate::plugin_manifest::is_pack_dir(&entry.path()) {
                candidates.push(entry.path());
            }
        }
    }
    candidates.sort();
    if let Some(root) = candidates.into_iter().next() {
        return Ok(root);
    }
    Err(format!(
        "fetched pack `{entry_name}` has no `murphy-plugin.toml` under `{}` (looked for `<dir>/`, `<dir>/{entry_name}/`, and one-level-deep pack dirs)",
        dest_dir.display()
    ))
}

fn fetch_local_dir(entry: &PackEntry, src: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    // `source-subdir` applies inside the source dir for local sources.
    let pack_src = if let Some(sub) = entry.remote_subdir() {
        let p = src.join(sub);
        if !crate::plugin_manifest::is_pack_dir(&p) {
            return Err(format!(
                "local source `{}` has no manifest at `<dir>/{sub}` (checked `source-subdir` `{sub}`)",
                src.display()
            ));
        }
        p
    } else if crate::plugin_manifest::is_pack_dir(src) {
        src.to_path_buf()
    } else {
        let nested = src.join(&entry.name);
        if crate::plugin_manifest::is_pack_dir(&nested) {
            nested
        } else {
            return Err(format!(
                "local source `{}` holds no pack `{}` (looked for `murphy-plugin.toml` in `<dir>/` and `<dir>/<name>/`)",
                src.display(),
                entry.name,
            ));
        }
    };
    ensure_clean_dir(dest_dir)?;
    copy_dir_recursive(&pack_src, dest_dir)?;
    // The copy itself is the pack root (subdir already applied).
    if !crate::plugin_manifest::is_pack_dir(dest_dir) {
        return Err(format!(
            "fetched pack `{}` has no manifest after copy to `{}`",
            entry.name,
            dest_dir.display()
        ));
    }
    Ok(dest_dir.to_path_buf())
}

fn fetch_git(entry: &PackEntry, url: &str, dest_dir: &Path) -> Result<PathBuf, String> {
    run_cmd("git", &["--version"], None)?;
    if dest_dir.exists() {
        std::fs::remove_dir_all(dest_dir)
            .map_err(|e| format!("cannot clear `{}`: {e}", dest_dir.display()))?;
    }
    if let Some(parent) = dest_dir.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create `{}`: {e}", parent.display()))?;
    }
    let url_owned = url.to_string();
    let dest_owned = dest_dir.to_string_lossy().into_owned();
    run_cmd("git", &["clone", "--quiet", &url_owned, &dest_owned], None)
        .map_err(|e| format!("cannot clone pack `{}` from `{url}`: {e}", entry.name))?;
    if let Some(rev) = entry.remote_rev() {
        run_cmd(
            "git",
            &["checkout", "--quiet", "--detach", rev],
            Some(dest_dir),
        )
        .map_err(|e| {
            format!(
                "pack `{}` cloned from `{url}` but rev `{rev}` failed: {e}",
                entry.name
            )
        })?;
    }
    resolve_fetched_root(&entry.name, dest_dir, entry.remote_subdir())
}

fn fetch_tarball(entry: &PackEntry, url: &str, dest_dir: &Path) -> Result<PathBuf, String> {
    // Stage the tarball bytes to a temp file next to the dest.
    let stage_parent = dest_dir
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&stage_parent)
        .map_err(|e| format!("cannot create `{}`: {e}", stage_parent.display()))?;
    let stage = stage_parent.join(format!("{}.tarball", entry.name));
    if stage.exists() {
        let _ = std::fs::remove_file(&stage);
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        run_cmd("curl", &["--version"], None)?;
        let url_owned = url.to_string();
        let stage_owned = stage.to_string_lossy().into_owned();
        run_cmd(
            "curl",
            &["-fsSL", "--retry", "2", "-o", &stage_owned, &url_owned],
            None,
        )
        .map_err(|e| format!("cannot download pack `{}` from `{url}`: {e}", entry.name))?;
    } else {
        let src = Path::new(strip_file_scheme(url));
        if !src.is_file() {
            return Err(format!(
                "tarball source `{}` for pack `{}` is missing (expected a `.tar.gz`/`.tgz` file)",
                src.display(),
                entry.name
            ));
        }
        std::fs::copy(src, &stage)
            .map_err(|e| format!("cannot stage tarball `{}`: {e}", src.display()))?;
    }
    if let Some(expected) = entry.remote_sha256() {
        let bytes = std::fs::read(&stage)
            .map_err(|e| format!("cannot read staged tarball `{}`: {e}", stage.display()))?;
        let digest = sha2::Sha256::digest(&bytes);
        let got = hex_bytes(&digest);
        if got.to_lowercase() != expected.trim().to_lowercase() {
            let _ = std::fs::remove_file(&stage);
            return Err(format!(
                "pack `{}` checksum mismatch (expected `{expected}`, got `{got}`)",
                entry.name
            ));
        }
    }
    ensure_clean_dir(dest_dir)?;
    run_cmd("tar", &["--version"], None)?;
    let stage_owned = stage.to_string_lossy().into_owned();
    let dest_owned = dest_dir.to_string_lossy().into_owned();
    let res = run_cmd("tar", &["xzf", &stage_owned, "-C", &dest_owned], None)
        .map_err(|e| format!("cannot extract pack `{}` tarball: {e}", entry.name));
    let _ = std::fs::remove_file(&stage);
    res?;
    resolve_fetched_root(&entry.name, dest_dir, entry.remote_subdir())
}

fn hex_bytes(digest: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

/// What `check_publish_source` validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishReport {
    /// Pack name from the manifest.
    pub name: String,
    /// Pack version from the manifest.
    pub version: String,
    /// Resolved cdylib inside the pack.
    pub cdylib: PathBuf,
}

/// Validate `pack_dir` as a publishable pack: manifest parses, name is
/// well-formed, version is non-empty, ABI matches the host, and a cdylib
/// resolves and exists.
pub fn check_publish_source(pack_dir: &Path) -> Result<PublishReport, String> {
    if !crate::plugin_manifest::is_pack_dir(pack_dir) {
        return Err(format!(
            "pack dir `{}` holds no `{}`",
            pack_dir.display(),
            crate::plugin_manifest::MANIFEST_FILENAME
        ));
    }
    let manifest = crate::plugin_manifest::PluginManifest::from_pack_dir(pack_dir)
        .map_err(|e| format!("pack `{}`: {e}", pack_dir.display()))?;
    crate::plugin_resolver::validate_plugin_name(&manifest.plugin.name)
        .map_err(|e| e.to_string())?;
    if manifest.plugin.version.trim().is_empty() {
        return Err(format!(
            "pack `{}` has an empty `version`",
            pack_dir.display()
        ));
    }
    manifest
        .check_api_compat()
        .map_err(|e| format!("pack `{}`: {e}", manifest.plugin.name))?;
    let cdylib = crate::plugin_manifest::resolve_pack_cdylib(pack_dir)
        .map_err(|e| format!("pack `{}`: {e}", pack_dir.display()))?;
    if !cdylib.is_file() {
        return Err(format!(
            "pack `{}` cdylib `{}` is missing",
            manifest.plugin.name,
            cdylib.display()
        ));
    }
    Ok(PublishReport {
        name: manifest.plugin.name.clone(),
        version: manifest.plugin.version.clone(),
        cdylib,
    })
}

/// Render the `[[packs]]` TOML snippet a publisher appends to their static
/// index for this pack. `gem` defaults to the manifest name; `base_url`
/// (when given) seeds a commented `source-url` template.
pub fn render_index_snippet(
    manifest: &crate::plugin_manifest::PluginManifest,
    gem: Option<&str>,
    base_url: Option<&str>,
) -> String {
    let gem = gem.unwrap_or(&manifest.plugin.name);
    let mut s = String::from("[[packs]]\n");
    s.push_str(&format!("name = \"{}\"\n", manifest.plugin.name));
    s.push_str(&format!("gem = \"{gem}\"\n"));
    s.push_str(&format!("version = \"{}\"\n", manifest.plugin.version));
    let desc = manifest.plugin.description.as_deref().unwrap_or("");
    s.push_str(&format!("description = \"{desc}\"\n"));
    let homepage = manifest
        .plugin
        .homepage
        .as_deref()
        .unwrap_or("https://example.invalid");
    s.push_str(&format!("homepage = \"{homepage}\"\n"));
    s.push_str(&format!(
        "murphy-api-version = {}\n",
        manifest.plugin.murphy_api_version.0
    ));
    s.push_str(&format!("min-murphy-version = \"{}\"\n", crate::version()));
    match base_url {
        Some(base) => {
            let base = base.trim_end_matches('/');
            s.push_str(&format!(
                "source-url = \"{base}/{}-{}.tar.gz\"\n",
                manifest.plugin.name, manifest.plugin.version
            ));
            s.push_str("# source-sha256 = \"<sha256 of the tarball>\"\n");
        }
        None => {
            s.push_str(
                "# source-url = \"https://example.invalid/packs/<name>-<version>.tar.gz\"\n",
            );
            s.push_str("# source-rev = \"v<version>\"  # git sources only\n");
            s.push_str("# source-subdir = \"pack\"  # when the pack root is not the checkout/extract root\n");
            s.push_str("# source-sha256 = \"<sha256 of the tarball>\"  # tarball sources only\n");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    fn write_manifest(pack_dir: &Path, name: &str) {
        std::fs::create_dir_all(pack_dir).expect("mkdir pack");
        std::fs::write(
            pack_dir.join(crate::plugin_manifest::MANIFEST_FILENAME),
            format!("[plugin]\nname = \"{name}\"\nversion = \"0.1.0\"\nmurphy-api-version = 4\n"),
        )
        .expect("write manifest");
    }

    fn write_cdylib(pack_dir: &Path) {
        let arch = pack_dir.join("lib").join("linux-x86_64");
        std::fs::create_dir_all(&arch).expect("mkdir lib/<arch>");
        let ext = if cfg!(target_os = "macos") {
            "dylib"
        } else {
            "so"
        };
        std::fs::write(arch.join(format!("libpack.{ext}")), b"fake").expect("cdylib");
    }

    fn entry_with_source(name: &str, url: &str) -> PackEntry {
        PackEntry {
            name: name.to_string(),
            gem: name.to_string(),
            version: "0.1.0".to_string(),
            description: format!("{name} pack"),
            homepage: "https://example.invalid".to_string(),
            murphy_api_version: murphy_plugin_api::MURPHY_PLUGIN_ABI_VERSION,
            min_murphy_version: "0.1.0".to_string(),
            source_url: Some(url.to_string()),
            source_rev: None,
            source_subdir: None,
            source_sha256: None,
        }
    }

    #[test]
    fn search_matches_name_description_and_gem() {
        let idx = crate::pack_registry::PackRegistryIndex::from_str(
            "[registry]\nversion = 1\n\n\
             [[packs]]\nname = \"murphy-rails\"\ngem = \"murphy-rails\"\nversion = \"0.1.0\"\n\
             description = \"Rails cops\"\nhomepage = \"https://example.invalid\"\n\
             murphy-api-version = 4\nmin-murphy-version = \"0.1.0\"\n\n\
             [[packs]]\nname = \"murphy-rspec\"\ngem = \"murphy-rspec\"\nversion = \"0.1.0\"\n\
             description = \"RSpec cops\"\nhomepage = \"https://example.invalid\"\n\
             murphy-api-version = 4\nmin-murphy-version = \"0.1.0\"\n",
        )
        .expect("parses");
        assert_eq!(
            search_packs(&idx, "rails")
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["murphy-rails"]
        );
        assert_eq!(search_packs(&idx, "RSPEC").len(), 1);
        assert_eq!(search_packs(&idx, "cops").len(), 2);
        assert_eq!(search_packs(&idx, "").len(), 2);
        assert!(search_packs(&idx, "nope").is_empty());
    }

    #[test]
    fn parses_source_fields_and_helpers() {
        let idx = crate::pack_registry::PackRegistryIndex::from_str(
            "[registry]\nversion = 1\n\n[[packs]]\nname = \"murphy-foo\"\ngem = \"murphy-foo\"\n\
             version = \"0.1.0\"\ndescription = \"x\"\nhomepage = \"https://example.invalid\"\n\
             murphy-api-version = 4\nmin-murphy-version = \"0.1.0\"\n\
             source-url = \"https://example.invalid/murphy-foo.tar.gz\"\n\
             source-sha256 = \"abc\"\n",
        )
        .expect("parses");
        let e = idx.find("murphy-foo").expect("found");
        assert!(e.has_remote_source());
        assert_eq!(
            e.remote_url(),
            Some("https://example.invalid/murphy-foo.tar.gz")
        );
        assert_eq!(e.remote_sha256(), Some("abc"));
        assert_eq!(e.remote_rev(), None);
    }

    #[test]
    fn missing_source_url_means_no_remote() {
        let idx = crate::pack_registry::PackRegistryIndex::from_str(
            "[registry]\nversion = 1\n\n[[packs]]\nname = \"murphy-foo\"\ngem = \"murphy-foo\"\n\
             version = \"0.1.0\"\nmurphy-api-version = 4\n",
        )
        .expect("parses");
        let e = idx.find("murphy-foo").expect("found");
        assert!(!e.has_remote_source());
        assert_eq!(e.remote_url(), None);
    }

    #[test]
    fn fetch_local_dir_copies_pack() {
        let src_parent = tempfile::tempdir().expect("src");
        let pack = src_parent.path().join("murphy-foo");
        write_manifest(&pack, "murphy-foo");
        write_cdylib(&pack);
        let url = format!("file://{}", src_parent.path().display());
        let entry = entry_with_source("murphy-foo", &url);
        let dest_parent = tempfile::tempdir().expect("dest parent");
        let dest = dest_parent.path().join("out");
        let root = fetch_pack(&entry, &dest).expect("fetch");
        assert_eq!(root, dest);
        assert!(
            dest.join(crate::plugin_manifest::MANIFEST_FILENAME)
                .is_file()
        );
    }

    #[test]
    fn fetch_local_dir_itself_as_pack_root() {
        let pack = tempfile::tempdir().expect("pack");
        write_manifest(pack.path(), "murphy-foo");
        write_cdylib(pack.path());
        let url = format!("file://{}", pack.path().display());
        let entry = entry_with_source("murphy-foo", &url);
        let dest_parent = tempfile::tempdir().expect("dest parent");
        let dest = dest_parent.path().join("out");
        fetch_pack(&entry, &dest).expect("fetch");
        assert!(
            dest.join(crate::plugin_manifest::MANIFEST_FILENAME)
                .is_file()
        );
    }

    #[test]
    fn fetch_rejects_unsafe_subdir() {
        let src = tempfile::tempdir().expect("src");
        let mut entry =
            entry_with_source("murphy-foo", &format!("file://{}", src.path().display()));
        entry.source_subdir = Some("../evil".to_string());
        let dest = src.path().join("out");
        let err = fetch_pack(&entry, &dest).expect_err("must fail");
        assert!(err.contains("source-subdir"), "got: {err}");
    }

    #[test]
    fn fetch_tarball_file_extracts_top_level_pack() {
        let pack = tempfile::tempdir().expect("pack");
        write_manifest(pack.path(), "murphy-foo");
        write_cdylib(pack.path());
        // Build a tarball with `tar` (system binary, same one fetch uses).
        let tarball = pack.path().join("murphy-foo.tar.gz");
        run_cmd(
            "tar",
            &[
                "czf",
                &tarball.to_string_lossy(),
                "-C",
                &pack.path().to_string_lossy(),
                "murphy-plugin.toml",
                "lib",
            ],
            None,
        )
        .expect("build tarball");
        let url = format!("file://{}", tarball.display());
        let entry = entry_with_source("murphy-foo", &url);
        let dest_parent = tempfile::tempdir().expect("dest parent");
        let dest = dest_parent.path().join("out");
        let root = fetch_pack(&entry, &dest).expect("fetch tarball");
        assert!(
            root.join(crate::plugin_manifest::MANIFEST_FILENAME)
                .is_file()
        );
    }

    #[test]
    fn fetch_tarball_checksum_mismatch_fails() {
        let pack = tempfile::tempdir().expect("pack");
        write_manifest(pack.path(), "murphy-foo");
        write_cdylib(pack.path());
        let tarball = pack.path().join("p.tar.gz");
        run_cmd(
            "tar",
            &[
                "czf",
                &tarball.to_string_lossy(),
                "-C",
                &pack.path().to_string_lossy(),
                "murphy-plugin.toml",
                "lib",
            ],
            None,
        )
        .expect("build tarball");
        let url = format!("file://{}", tarball.display());
        let mut entry = entry_with_source("murphy-foo", &url);
        entry.source_sha256 = Some("0".repeat(64));
        let dest = pack.path().join("out");
        let err = fetch_pack(&entry, &dest).expect_err("must fail");
        assert!(err.contains("checksum"), "got: {err}");
    }

    #[test]
    fn publish_check_validates_and_renders_snippet() {
        let pack = tempfile::tempdir().expect("pack");
        write_manifest(pack.path(), "murphy-foo");
        write_cdylib(pack.path());
        let report = check_publish_source(pack.path()).expect("valid");
        assert_eq!(report.name, "murphy-foo");
        let manifest =
            crate::plugin_manifest::PluginManifest::from_pack_dir(pack.path()).expect("manifest");
        let snippet = render_index_snippet(&manifest, None, None);
        assert!(snippet.contains("murphy-foo"), "got:\n{snippet}");
        assert!(snippet.contains("source-url"), "got:\n{snippet}");
    }

    #[test]
    fn publish_check_rejects_broken_pack() {
        let dir = tempfile::tempdir().expect("dir");
        let err = check_publish_source(dir.path()).expect_err("must fail");
        assert!(err.contains("murphy-plugin.toml"), "got: {err}");
    }
}
