//! A5 persistent result-cache e2e (murphy-fmw.1.1).
//!
//! Exercises the compiled `murphy` binary via `assert_cmd` with an
//! isolated `XDG_CACHE_HOME` per test:
//! - default `lint` populates `results/*.json` alongside `.ast` files,
//! - a second consecutive run returns byte-identical output (hit path),
//! - `--no-cache` / `MURPHY_NO_CACHE` write neither AST nor results,
//! - a corrupt result file degrades to a miss (correct output, no panic),
//! - disabling a cop via config changes output (no stale hit),
//! - `murphy cache stat` / `murphy cache clean` work.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

const CLEAN: &str = "# frozen_string_literal: true\n\nx = 1\nlogger.info x\n";
const DIRTY: &str = "# frozen_string_literal: true\n\ndebugger\n";

fn result_files_in(root: &std::path::Path) -> Vec<std::path::PathBuf> {
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

fn run_lint(
    cache_root: &std::path::Path,
    target: &std::path::Path,
    extra_args: &[&str],
) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.env_remove("MURPHY_NO_CACHE");
    cmd.env("XDG_CACHE_HOME", cache_root);
    cmd.arg("lint").arg("--format").arg("json");
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.arg(target);
    cmd.assert()
}

#[test]
fn result_cache_populates_and_second_run_matches() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("clean.rb");
    fs::write(&file, CLEAN).expect("write");
    let cache_root = dir.path().join("cache");

    let first = run_lint(&cache_root, &file, &[]).code(0);
    let first_stdout = first.get_output().stdout.clone();

    let v1 = cache_root.join("murphy").join("v1");
    assert!(v1.is_dir(), "cache root should exist");
    assert!(
        !result_files_in(&v1).is_empty(),
        "results/*.json should be populated"
    );

    let second = run_lint(&cache_root, &file, &[]).code(0);
    assert_eq!(
        second.get_output().stdout,
        first_stdout,
        "cached second run must return identical output"
    );
}

#[test]
fn result_cache_covers_dirty_files() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("dirty.rb");
    fs::write(&file, DIRTY).expect("write");
    let cache_root = dir.path().join("cache");

    let first = run_lint(&cache_root, &file, &[]).code(1);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&first.get_output().stdout).expect("json");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["cop_name"], "Lint/Debugger");

    let second = run_lint(&cache_root, &file, &[]).code(1);
    assert_eq!(second.get_output().stdout, first.get_output().stdout);
}

#[test]
fn result_cache_disabled_by_no_cache_flag() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("clean.rb");
    fs::write(&file, CLEAN).expect("write");
    let cache_root = dir.path().join("cache");

    run_lint(&cache_root, &file, &["--no-cache"]).code(0);
    let v1 = cache_root.join("murphy").join("v1");
    assert!(
        result_files_in(&v1).is_empty(),
        "no result files under --no-cache"
    );
}

#[test]
fn result_cache_disabled_by_env_var() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("clean.rb");
    fs::write(&file, CLEAN).expect("write");
    let cache_root = dir.path().join("cache");

    Command::cargo_bin("murphy")
        .expect("builds")
        .env("MURPHY_NO_CACHE", "1")
        .env("XDG_CACHE_HOME", &cache_root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&file)
        .assert()
        .code(0);
    let v1 = cache_root.join("murphy").join("v1");
    assert!(
        result_files_in(&v1).is_empty(),
        "no result files under MURPHY_NO_CACHE"
    );
}

#[test]
fn corrupt_result_file_degrades_to_miss() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("dirty.rb");
    fs::write(&file, DIRTY).expect("write");
    let cache_root = dir.path().join("cache");

    run_lint(&cache_root, &file, &[]).code(1);
    let v1 = cache_root.join("murphy").join("v1");
    for f in result_files_in(&v1) {
        fs::write(&f, b"not json{{{").expect("corrupt");
    }
    // Must still lint correctly (miss, not panic), exit 1 with Debugger.
    let assert = run_lint(&cache_root, &file, &[]).code(1);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("json");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["cop_name"], "Lint/Debugger");
}

#[test]
fn disabling_cop_via_config_changes_output_no_stale_hit() {
    // Run from inside the project dir so `.murphy.yml` is picked up
    // (config loads from cwd). First without config (Debugger fires),
    // then with `Lint/Debugger: Enabled: false` (no offenses). A stale
    // result cache would wrongly return the old Debugger offense.
    let proj = tempdir().expect("tempdir");
    fs::write(proj.path().join("dirty.rb"), DIRTY).expect("write");
    let cache_root = proj.path().join("cache");

    let run_in = |extra_env: Option<(&str, &str)>| {
        let mut cmd = Command::cargo_bin("murphy").expect("builds");
        cmd.current_dir(proj.path());
        cmd.env_remove("MURPHY_NO_CACHE");
        cmd.env("XDG_CACHE_HOME", &cache_root);
        if let Some((k, v)) = extra_env {
            cmd.env(k, v);
        }
        cmd.arg("lint").arg("--format").arg("json").arg("dirty.rb");
        cmd.assert()
    };

    run_in(None).code(1);
    fs::write(
        proj.path().join(".murphy.yml"),
        "AllCops:\n  TargetRubyVersion: 3.4\nLint/Debugger:\n  Enabled: false\n",
    )
    .expect("write config");
    let assert = run_in(None).code(0);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("json");
    assert!(
        parsed.is_empty(),
        "disabled cop must yield no offenses (no stale hit), got {parsed:?}"
    );
}

#[test]
fn cache_stat_and_clean_work() {
    let dir = tempdir().expect("tempdir");
    let file = dir.path().join("clean.rb");
    fs::write(&file, CLEAN).expect("write");
    let cache_root = dir.path().join("cache");

    run_lint(&cache_root, &file, &[]).code(0);

    let stat = Command::cargo_bin("murphy")
        .expect("builds")
        .env_remove("MURPHY_NO_CACHE")
        .env("XDG_CACHE_HOME", &cache_root)
        .arg("cache")
        .arg("stat")
        .assert()
        .code(0);
    let out = String::from_utf8_lossy(&stat.get_output().stdout);
    assert!(out.contains("result entries:"), "stat output: {out}");

    Command::cargo_bin("murphy")
        .expect("builds")
        .env_remove("MURPHY_NO_CACHE")
        .env("XDG_CACHE_HOME", &cache_root)
        .arg("cache")
        .arg("clean")
        .assert()
        .code(0);
    let v1 = cache_root.join("murphy").join("v1");
    assert!(!v1.exists(), "clean must remove the cache root");
}
