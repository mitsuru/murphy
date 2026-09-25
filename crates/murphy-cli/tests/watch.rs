//! B1 watch/daemon e2e (murphy-fmw.2.1).
//!
//! Exercises the compiled `murphy` binary:
//! - `watch --once` exits 0/1 like `lint` and prints byte-identical
//!   `--format json` output (same pipeline, ADR 0006 unchanged),
//! - `watch --once` populates `results/*.json` (built on the A5
//!   incremental cache) while `--no-cache` writes nothing,
//! - `--baseline` filtering and bad `--interval` handling work,
//! - a resident `watch` process picks up a file save and prints the
//!   differential re-lint (Phase 9 gate 1).

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::tempdir;

const CLEAN: &str = "# frozen_string_literal: true\n\nx = 1\nlogger.info x\n";
const DIRTY: &str = "# frozen_string_literal: true\n\ndebugger\n";

fn murphy() -> Command {
    Command::cargo_bin("murphy").expect("murphy binary builds")
}

fn result_files_in(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(root) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                out.extend(result_files_in(&p));
            } else if p.extension().is_some_and(|e| e == "json") {
                out.push(p);
            }
        }
    }
    out
}

#[test]
fn watch_once_clean_exits_zero_with_empty_json() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("clean.rb");
    fs::write(&file, CLEAN).expect("write");
    let cache_root = dir.path().join("cache");

    let assert = murphy()
        .env_remove("MURPHY_NO_CACHE")
        .env("XDG_CACHE_HOME", &cache_root)
        .arg("watch")
        .arg("--once")
        .arg("--format")
        .arg("json")
        .arg(&file)
        .assert()
        .code(0);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        !stdout.contains("Lint/Debugger"),
        "clean file must print no offenses, got: {stdout}"
    );
}

#[test]
fn watch_once_matches_lint_json_byte_for_byte() {
    // Same file, same config → the watch pipeline must print exactly what
    // `murphy lint --format json` prints (differential only narrows the
    // file subset, never per-file semantics).
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("dirty.rb");
    fs::write(&file, DIRTY).expect("write");
    let cache_root = dir.path().join("cache");

    let lint_out = murphy()
        .env_remove("MURPHY_NO_CACHE")
        .env("XDG_CACHE_HOME", &cache_root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&file)
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();

    let watch_out = murphy()
        .env_remove("MURPHY_NO_CACHE")
        .env("XDG_CACHE_HOME", &cache_root)
        .arg("watch")
        .arg("--once")
        .arg("--format")
        .arg("json")
        .arg(&file)
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();

    assert_eq!(watch_out, lint_out, "watch --once must match lint output");
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&watch_out).expect("stdout is JSON");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["cop_name"], "Lint/Debugger");
}

#[test]
fn watch_once_warms_result_cache_but_no_cache_writes_nothing() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("clean.rb");
    fs::write(&file, CLEAN).expect("write");
    let cache_root = dir.path().join("cache");
    let v1 = cache_root.join("murphy").join("v1");

    murphy()
        .env_remove("MURPHY_NO_CACHE")
        .env("XDG_CACHE_HOME", &cache_root)
        .arg("watch")
        .arg("--once")
        .arg("--format")
        .arg("json")
        .arg(&file)
        .assert()
        .code(0);
    assert!(
        !result_files_in(&v1).is_empty(),
        "watch --once must populate results/*.json (A5)"
    );

    let dir2 = tempdir().expect("tempdir");
    let file2 = dir2.path().join("clean.rb");
    fs::write(&file2, CLEAN).expect("write");
    let cache_root2 = dir2.path().join("cache");
    murphy()
        .env_remove("MURPHY_NO_CACHE")
        .env("XDG_CACHE_HOME", &cache_root2)
        .arg("watch")
        .arg("--once")
        .arg("--no-cache")
        .arg(&file2)
        .assert()
        .code(0);
    assert!(
        result_files_in(&cache_root2.join("murphy").join("v1")).is_empty(),
        "watch --once --no-cache must write no result entries"
    );
}

#[test]
fn watch_once_honours_baseline() {
    // Freeze the Debugger offense, then watch --once with the baseline:
    // the frozen offense is suppressed (exit 0).
    let proj = tempdir().expect("tempdir");
    fs::write(proj.path().join("dirty.rb"), DIRTY).expect("write");
    let cache_root = proj.path().join("cache");

    murphy()
        .current_dir(proj.path())
        .env_remove("MURPHY_NO_CACHE")
        .env("XDG_CACHE_HOME", &cache_root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("--generate-baseline")
        .arg("baseline.toml")
        .arg("dirty.rb")
        .assert()
        .code(1);

    murphy()
        .current_dir(proj.path())
        .env_remove("MURPHY_NO_CACHE")
        .env("XDG_CACHE_HOME", &cache_root)
        .arg("watch")
        .arg("--once")
        .arg("--format")
        .arg("json")
        .arg("--baseline")
        .arg("baseline.toml")
        .arg("dirty.rb")
        .assert()
        .code(0);
}

#[test]
fn watch_rejects_bad_interval() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("clean.rb");
    fs::write(&file, CLEAN).expect("write");

    for bad in ["0", "-1", "61", "nan", "inf"] {
        murphy()
            .env_remove("MURPHY_NO_CACHE")
            .env("XDG_CACHE_HOME", dir.path().join("cache"))
            .arg("watch")
            .arg("--once")
            .arg("--interval")
            .arg(bad)
            .arg(&file)
            .assert()
            .code(2);
    }
}

/// Wait until `path` contains `needle` (or the deadline passes).
fn poll_contains(path: &Path, needle: &str, deadline: Instant) -> bool {
    while Instant::now() < deadline {
        if let Ok(text) = fs::read_to_string(path)
            && text.contains(needle)
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn resident_watch_re_lints_on_save() {
    // Phase 9 gate 1: resident startup, then save → differential lint.
    // Stdout/stderr go to files so the test can poll without blocking on
    // pipes; the child is always killed before asserting.
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("a.rb");
    fs::write(&file, CLEAN).expect("write");
    let out_log = dir.path().join("stdout.log");
    let err_log = dir.path().join("stderr.log");
    let out_file = fs::File::create(&out_log).expect("out log");
    let err_file = fs::File::create(&err_log).expect("err log");

    let bin = assert_cmd::cargo::cargo_bin("murphy");
    let mut child = std::process::Command::new(bin)
        .env_remove("MURPHY_NO_CACHE")
        .env("XDG_CACHE_HOME", dir.path().join("cache"))
        .arg("watch")
        .arg("--interval")
        .arg("0.1")
        .arg("--format")
        .arg("json")
        .arg(&file)
        .stdout(out_file)
        .stderr(err_file)
        .spawn()
        .expect("spawn murphy watch");

    // Initial pass: the clean file has no Debugger offense, but the
    // resident loop must still announce itself on stderr.
    let announced = poll_contains(
        &err_log,
        "murphy watch: watching",
        Instant::now() + Duration::from_secs(30),
    );
    // Save a dirty file → the differential pass must print Debugger.
    fs::write(&file, DIRTY).expect("rewrite dirty");
    let relinted = poll_contains(
        &out_log,
        "Lint/Debugger",
        Instant::now() + Duration::from_secs(30),
    );

    let _ = child.kill();
    let _ = child.wait();

    assert!(announced, "watch must announce resident startup on stderr");
    assert!(
        relinted,
        "watch must re-lint the saved file and print the new offense"
    );
    let err = fs::read_to_string(&err_log).unwrap_or_default();
    assert!(
        err.contains("1 changed"),
        "watch must log the differential summary, got: {err}"
    );
}
