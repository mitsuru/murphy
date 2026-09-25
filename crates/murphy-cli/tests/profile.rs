//! B6 profiler integration tests (murphy-fmw.2.6).
//!
//! - `murphy lint --profile` prints the Phase 9 gate 5 JSON (per-cop wall
//!   time + p95 + cop x file matrix + hot files) to stdout INSTEAD of lint
//!   output; the exit code still reflects lint offenses.
//! - `murphy lint --profile --profile-format speedscope` prints the
//!   Speedscope trace shape.
//! - `--profile-format` without `--profile` exits 2; unknown format exits 2.
//! - `--format` is ignored with `--profile`; `--debug` still goes to stderr
//!   and never pollutes the profile JSON on stdout.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

const CLEAN_SOURCE: &str = "# frozen_string_literal: true\n\nx = 1\nlogger.info x\n";
const DIRTY_DEBUGGER_SOURCE: &str = "# frozen_string_literal: true\n\ndebugger\n";

/// Gate 5 shape: stdout is an object with cop wall times, p95, the cop x
/// file matrix, hot files, and invocation counts.
#[test]
fn lint_profile_outputs_gate_json() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("clean.rb");
    fs::write(&path, CLEAN_SOURCE).expect("write clean.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile")
        .arg(&path)
        .assert()
        .code(0);

    let parsed: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be profile JSON");
    assert!(
        parsed.is_object(),
        "profile must be an object, got {parsed:?}"
    );
    assert!(
        parsed
            .get("cop_wall_micros")
            .and_then(|v| v.as_object())
            .is_some(),
        "missing cop_wall_micros object, got {parsed:?}"
    );
    assert!(
        parsed
            .get("cop_file_micros")
            .and_then(|v| v.as_object())
            .is_some(),
        "missing cop_file_micros matrix, got {parsed:?}"
    );
    assert!(
        parsed.get("p95_micros").and_then(|v| v.as_u64()).is_some(),
        "missing p95_micros, got {parsed:?}"
    );
    assert!(
        parsed.get("hot_files").and_then(|v| v.as_array()).is_some(),
        "missing hot_files, got {parsed:?}"
    );
    assert!(
        parsed
            .get("invocation_count")
            .and_then(|v| v.as_object())
            .is_some(),
        "missing invocation_count, got {parsed:?}"
    );
}

/// Offenses still drive the exit code with `--profile` (1 here), while
/// stdout is the profile object — not the offense array.
#[test]
fn lint_profile_dirty_file_keeps_offense_exit_code() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("dirty.rb");
    fs::write(&path, DIRTY_DEBUGGER_SOURCE).expect("write dirty.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile")
        .arg(&path)
        .assert()
        .code(1);

    let parsed: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be profile JSON");
    assert!(
        parsed.is_object(),
        "profile must be an object, got {parsed:?}"
    );
    assert!(
        parsed.get("cop_wall_micros").is_some(),
        "missing cop_wall_micros, got {parsed:?}"
    );
    assert!(
        parsed.get("cop_name").is_none(),
        "profile must not be the offense array shape, got {parsed:?}"
    );
}

#[test]
fn lint_profile_outputs_speedscope_json() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("clean.rb");
    fs::write(&path, CLEAN_SOURCE).expect("write clean.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile")
        .arg("--profile-format")
        .arg("speedscope")
        .arg(&path)
        .assert()
        .code(0);

    let parsed: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be profile JSON");

    assert_eq!(
        parsed.get("process_name").and_then(|v| v.as_str()),
        Some("murphy-lint")
    );
    let events = parsed
        .get("traceEvents")
        .and_then(|v| v.as_array())
        .expect("traceEvents must be an array");
    assert_eq!(
        events.len(),
        parsed
            .get("event_count")
            .and_then(|v| v.as_u64())
            .unwrap_or_default() as usize
    );
    // Parse is always recorded, so the trace is non-empty even for a clean file.
    assert!(
        events
            .iter()
            .any(|e| e.get("name").and_then(|v| v.as_str()) == Some("parse")),
        "trace must contain a parse event, got {parsed:?}"
    );
    for event in events {
        assert!(event.get("ts").and_then(|v| v.as_u64()).is_some());
        assert!(event.get("name").and_then(|v| v.as_str()).is_some());
    }
}

#[test]
fn lint_profile_speedscope_thread_ids_follow_sorted_file_order() {
    let dir = tempdir().expect("create tempdir");
    fs::write(dir.path().join("a.rb"), CLEAN_SOURCE).expect("write a.rb");
    fs::write(dir.path().join("z.rb"), DIRTY_DEBUGGER_SOURCE).expect("write z.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--profile")
        .arg("--profile-format")
        .arg("speedscope")
        .arg(".")
        .assert()
        .code(1);

    let parsed: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be profile JSON");
    let events = parsed
        .get("traceEvents")
        .and_then(|v| v.as_array())
        .expect("traceEvents must be an array");

    let mut tid_files: Vec<(u64, String)> = events
        .iter()
        .filter_map(|event| {
            let tid = event.get("tid").and_then(|v| v.as_u64())?;
            let thread_name = event
                .get("args")?
                .get("thread_name")
                .and_then(|v| v.as_str())?;
            Some((tid, thread_name.to_owned()))
        })
        .collect();

    tid_files.sort_by_key(|(tid, _)| *tid);
    tid_files.dedup_by_key(|(tid, _)| *tid);
    assert!(!tid_files.is_empty());

    let actual_files: Vec<String> = tid_files.iter().map(|(_, file)| file.clone()).collect();
    let mut expected_files = actual_files.clone();
    expected_files.sort_unstable();

    assert_eq!(actual_files, expected_files);
}

#[test]
fn lint_profile_format_requires_profile_flag() {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile-format")
        .arg("speedscope")
        .assert()
        .code(2);

    assert!(
        String::from_utf8_lossy(&assert.get_output().stderr)
            .contains("--profile-format requires --profile")
    );
}

#[test]
fn lint_profile_unknown_profile_format_exits_2() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("clean.rb");
    fs::write(&path, CLEAN_SOURCE).expect("write clean.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile")
        .arg("--profile-format")
        .arg("not-a-format")
        .arg(&path)
        .assert()
        .code(2);

    assert!(
        String::from_utf8_lossy(&assert.get_output().stderr).contains("profile-format"),
        "clap must reject the unknown format, got: {:?}",
        String::from_utf8_lossy(&assert.get_output().stderr)
    );
}

/// `--format` is ignored with `--profile`: stdout stays the profile object.
#[test]
fn lint_profile_ignores_format_flag() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("dirty.rb");
    fs::write(&path, DIRTY_DEBUGGER_SOURCE).expect("write dirty.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(1);

    let parsed: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be profile JSON");
    assert!(
        parsed.is_object(),
        "profile must be an object, got {parsed:?}"
    );
    assert!(parsed.get("cop_wall_micros").is_some());
}

/// `--debug` diagnostics stay on stderr; stdout remains pure profile JSON.
#[test]
fn lint_profile_with_debug_keeps_json_stdout() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("dirty.rb");
    fs::write(&path, DIRTY_DEBUGGER_SOURCE).expect("write dirty.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile")
        .arg("--format")
        .arg("json")
        .arg("--debug")
        .arg(&path)
        .assert()
        .code(1);

    let parsed: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be profile JSON");
    assert!(parsed.get("cop_wall_micros").is_some());

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("murphy: debug:"),
        "--debug must still emit to stderr, got: {stderr:?}"
    );
}
