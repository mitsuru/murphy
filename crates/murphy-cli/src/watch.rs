//! `murphy watch` — resident differential lint (Phase 9 B1, murphy-fmw.2.1).
//!
//! A polling file watcher built on the A5 persistent result cache
//! (ADR 0047): the initial pass lints every discovered file (populating
//! `results/*.json`), then each tick re-lints only added/modified files.
//! Unchanged files are never re-parsed or re-dispatched in the incremental
//! pass — and a repeated full pass would hit the on-disk result cache —
//! so save-to-feedback stays at single-file lint latency (Phase 9 gate 1).
//!
//! Design notes:
//!
//! - Polling (`mtime` + length snapshots, default 0.5 s) instead of an OS
//!   file-watching crate: no new dependencies, identical behaviour on
//!   Linux/macOS/CI, and no lost-event edge cases on editors that
//!   save via atomic rename (the snapshot sees the new `mtime` either
//!   way). Sub-100 ms latency was deliberately not promised; "feels
//!   instant" means one short poll after save.
//! - Output per pass is differential: only the changed subset is linted
//!   and only its offenses are formatted (same `--format` shapes as
//!   `murphy lint`; the ADR 0006 default JSON shape is unchanged).
//! - `.murphy.yml` (cwd) and the `--baseline` file are tracked by `mtime`;
//!   a change there triggers a full re-lint with a reloaded config (the
//!   new config fingerprint misses the old result-cache keys by
//!   construction, so no stale results are ever returned). Files pulled
//!   in via `inherit_from:` are NOT tracked — restart `murphy watch`
//!   after editing those, or after adding/removing plugin packs.
//! - `murphy watch` never applies fixes (no `--fix`): a fix write-back
//!   would immediately re-trigger the watcher. Run `murphy lint --fix`
//!   separately.
//! - No `MURPHY_PLUGIN_ABI_VERSION` bump: CLI-only polling loop, the
//!   plugin ABI is untouched.
//!
//! Exit codes: `0` clean / `1` offenses (only meaningful with `--once`;
//! a resident run exits on signal), `2` setup error, `3` internal failure
//! (shared `AppError` contract in `main.rs`).

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

/// Default poll interval in seconds.
pub const DEFAULT_INTERVAL_SECS: f64 = 0.5;
/// Minimum accepted `--interval` (50 ms — below this the poll loop would
/// just burn CPU re-walking the discovery roots).
pub const MIN_INTERVAL_SECS: f64 = 0.05;
/// Maximum accepted `--interval` (60 s — longer polls miss the point of a
/// resident watcher; use `murphy lint` for one-shots instead).
pub const MAX_INTERVAL_SECS: f64 = 60.0;

/// Validate `--interval <secs>` and convert to a [`Duration`].
/// Returns the human-readable rejection reason (mapped to exit 2 by the
/// caller) on out-of-range or non-finite input.
pub fn validate_interval(secs: f64) -> Result<Duration, String> {
    if !secs.is_finite() {
        return Err(format!(
            "invalid --interval {secs:?}: must be a finite number of seconds"
        ));
    }
    if !(MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&secs) {
        return Err(format!(
            "invalid --interval {secs}: must be between {MIN_INTERVAL_SECS} and {MAX_INTERVAL_SECS} seconds"
        ));
    }
    Ok(Duration::from_secs_f64(secs))
}

/// Identity of one file for change detection: length plus `mtime`
/// (seconds + sub-second nanos). Length is included so a same-second
/// rewrite with different content is still caught on filesystems with
/// coarse timestamp granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSig {
    /// File length in bytes.
    pub len: u64,
    /// `mtime` seconds since the Unix epoch.
    pub mtime_secs: i64,
    /// `mtime` sub-second nanos.
    pub mtime_nanos: u32,
}

/// Read the current [`FileSig`] for `path`. Returns `None` when the file
/// cannot be stated (missing, permission error, non-file) — the caller
/// treats that as absent, which drives added/removed detection.
pub fn file_sig(path: &str) -> Option<FileSig> {
    let md = std::fs::metadata(Path::new(path)).ok()?;
    if !md.is_file() {
        return None;
    }
    let mtime = md.modified().ok()?;
    let elapsed = mtime.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(FileSig {
        len: md.len(),
        mtime_secs: elapsed.as_secs() as i64,
        mtime_nanos: elapsed.subsec_nanos(),
    })
}

/// Snapshot every file in `files`, skipping unstated ones (deleted or
/// unreadable between discovery and stat — they simply read as absent).
pub fn snapshot_files(files: &[String]) -> BTreeMap<String, FileSig> {
    let mut out = BTreeMap::new();
    for f in files {
        if let Some(sig) = file_sig(f) {
            out.insert(f.clone(), sig);
        }
    }
    out
}

/// What changed between two snapshots.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct WatchDiff {
    /// Present in `curr` but not in `prev`.
    pub added: Vec<String>,
    /// Present in both with a different [`FileSig`].
    pub modified: Vec<String>,
    /// Present in `prev` but not in `curr`.
    pub removed: Vec<String>,
}

impl WatchDiff {
    /// True when nothing changed (steady state — no lint pass needed).
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.removed.is_empty()
    }

    /// Files needing a lint pass (added + modified), in sorted order.
    pub fn lint_targets(&self) -> Vec<String> {
        let mut out = self.added.clone();
        out.extend(self.modified.iter().cloned());
        out.sort();
        out
    }
}

/// Diff `prev` against `curr`. All vectors are sorted for deterministic
/// output (iteration order of the input maps is already sorted, but
/// sorting keeps the contract explicit for callers passing other shapes).
pub fn diff_snapshots(
    prev: &BTreeMap<String, FileSig>,
    curr: &BTreeMap<String, FileSig>,
) -> WatchDiff {
    let mut diff = WatchDiff::default();
    for (path, sig) in curr {
        match prev.get(path) {
            None => diff.added.push(path.clone()),
            Some(old) if old != sig => diff.modified.push(path.clone()),
            _ => {}
        }
    }
    for path in prev.keys() {
        if !curr.contains_key(path) {
            diff.removed.push(path.clone());
        }
    }
    diff.added.sort();
    diff.modified.sort();
    diff.removed.sort();
    diff
}

/// One-line stderr summary for a pass with changes, e.g.
/// `2 changed (a.rb, b.rb), 1 removed`. Path lists longer than
/// [`MAX_LISTED_PATHS`] are truncated with an `… and N more` suffix so a
/// mass rename never floods stderr.
pub const MAX_LISTED_PATHS: usize = 5;

pub fn format_change_summary(diff: &WatchDiff) -> String {
    let total = diff.added.len() + diff.modified.len();
    let mut parts = Vec::new();
    let mut changed: Vec<&str> = diff
        .added
        .iter()
        .chain(diff.modified.iter())
        .map(String::as_str)
        .collect();
    changed.sort();
    if total > 0 {
        let listed: Vec<&str> = changed.into_iter().take(MAX_LISTED_PATHS).collect();
        let mut s = format!("{} changed ({})", total, listed.join(", "));
        if total > MAX_LISTED_PATHS {
            s.push_str(&format!(" … and {} more", total - MAX_LISTED_PATHS));
        }
        parts.push(s);
    }
    if !diff.removed.is_empty() {
        parts.push(format!("{} removed", diff.removed.len()));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(len: u64, secs: i64) -> FileSig {
        FileSig {
            len,
            mtime_secs: secs,
            mtime_nanos: 0,
        }
    }

    #[test]
    fn interval_accepts_default_and_rejects_garbage() {
        assert!(validate_interval(DEFAULT_INTERVAL_SECS).is_ok());
        assert!(validate_interval(0.05).is_ok());
        assert!(validate_interval(60.0).is_ok());
        assert!(validate_interval(0.0).is_err());
        assert!(validate_interval(-1.0).is_err());
        assert!(validate_interval(61.0).is_err());
        assert!(validate_interval(f64::NAN).is_err());
        assert!(validate_interval(f64::INFINITY).is_err());
    }

    #[test]
    fn diff_detects_added_modified_removed() {
        let prev: BTreeMap<String, FileSig> = [
            ("a.rb".to_string(), sig(10, 1)),
            ("b.rb".to_string(), sig(10, 1)),
            ("c.rb".to_string(), sig(10, 1)),
        ]
        .into_iter()
        .collect();
        let curr: BTreeMap<String, FileSig> = [
            ("a.rb".to_string(), sig(10, 1)),
            ("b.rb".to_string(), sig(20, 2)),
            ("d.rb".to_string(), sig(5, 3)),
        ]
        .into_iter()
        .collect();
        let diff = diff_snapshots(&prev, &curr);
        assert_eq!(diff.added, vec!["d.rb".to_string()]);
        assert_eq!(diff.modified, vec!["b.rb".to_string()]);
        assert_eq!(diff.removed, vec!["c.rb".to_string()]);
        assert!(!diff.is_empty());
        assert_eq!(
            diff.lint_targets(),
            vec!["b.rb".to_string(), "d.rb".to_string()]
        );
    }

    #[test]
    fn diff_steady_state_is_empty() {
        let snap: BTreeMap<String, FileSig> =
            [("a.rb".to_string(), sig(10, 1))].into_iter().collect();
        let diff = diff_snapshots(&snap, &snap);
        assert!(diff.is_empty());
        assert!(diff.lint_targets().is_empty());
    }

    #[test]
    fn same_second_rewrite_with_new_length_counts_as_modified() {
        let prev: BTreeMap<String, FileSig> =
            [("a.rb".to_string(), sig(10, 1))].into_iter().collect();
        let curr: BTreeMap<String, FileSig> =
            [("a.rb".to_string(), sig(11, 1))].into_iter().collect();
        let diff = diff_snapshots(&prev, &curr);
        assert_eq!(diff.modified, vec!["a.rb".to_string()]);
    }

    #[test]
    fn change_summary_truncates_long_lists() {
        let diff = WatchDiff {
            added: (0..8).map(|i| format!("f{i}.rb")).collect(),
            modified: vec![],
            removed: vec!["old.rb".to_string()],
        };
        let s = format_change_summary(&diff);
        assert!(s.contains("8 changed"), "summary: {s}");
        assert!(s.contains("and 3 more"), "summary: {s}");
        assert!(s.contains("1 removed"), "summary: {s}");
    }

    #[test]
    fn change_summary_empty_diff_is_empty_string() {
        assert_eq!(format_change_summary(&WatchDiff::default()), "");
    }

    #[test]
    fn snapshot_skips_missing_files() {
        let snap = snapshot_files(&["/definitely/not/here/missing.rb".to_string()]);
        assert!(snap.is_empty());
    }

    #[test]
    fn file_sig_round_trips_a_real_file() {
        let dir = std::env::temp_dir().join(format!("murphy-watch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let p = dir.join("sig.rb");
        std::fs::write(&p, "x = 1\n").expect("write");
        let key = p.to_string_lossy().into_owned();
        let first = file_sig(&key).expect("sig");
        assert_eq!(first.len, 6);
        // Rewriting with different content changes the sig (len differs).
        std::fs::write(&p, "x = 1\nx = 2\n").expect("rewrite");
        let second = file_sig(&key).expect("sig2");
        assert_ne!(first, second);
        std::fs::remove_dir_all(&dir).ok();
    }
}
