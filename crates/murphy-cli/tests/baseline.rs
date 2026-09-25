//! End-to-end tests for `.murphy-baseline.toml` (Phase 9 B3).
//!
//! Legacy-adoption flow per `docs/guides/baseline.md`:
//! lint → `--generate-baseline` → `--baseline` (green) → new violation
//! surfaces. Plus error cases (missing/malformed baseline → exit 2).

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

const TWO_DEBUGGERS: &str = "# frozen_string_literal: true\n\ndebugger\ndebugger\n";
const THREE_DEBUGGERS: &str = "# frozen_string_literal: true\n\ndebugger\ndebugger\ndebugger\n";

fn lint_json(dir: &std::path::Path, extra: &[&str]) -> (i32, Vec<serde_json::Value>) {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.arg("lint").arg("--format").arg("json");
    for a in extra {
        cmd.arg(a);
    }
    cmd.arg(dir.join("legacy.rb"));
    cmd.current_dir(dir);
    let assert = cmd.assert();
    let code = assert.get_output().status.code().unwrap_or(-1);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array");
    (code, parsed)
}

#[test]
fn baseline_freeze_then_green_then_new_offense_surfaces() {
    let dir = tempdir().expect("create tempdir");
    fs::write(dir.path().join("legacy.rb"), TWO_DEBUGGERS).expect("write legacy.rb");
    let baseline = dir.path().join(".murphy-baseline.toml");

    // 1. Legacy lint: 2 offenses, exit 1.
    let (code, parsed) = lint_json(dir.path(), &[]);
    assert_eq!(code, 1);
    assert_eq!(parsed.len(), 2);

    // 2. Freeze: --generate-baseline writes valid TOML; run still reports.
    let baseline_str = baseline.to_string_lossy().into_owned();
    let gen_assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("--generate-baseline")
        .arg(&baseline_str)
        .arg(dir.path().join("legacy.rb"))
        .current_dir(dir.path())
        .assert()
        .code(1);
    let gen_parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&gen_assert.get_output().stdout)
            .expect("stdout must be a JSON array");
    assert_eq!(
        gen_parsed.len(),
        2,
        "generate must still report all offenses"
    );
    let text = fs::read_to_string(&baseline).expect("baseline file must exist");
    assert!(
        text.contains("version = 1"),
        "baseline must declare version, got: {text:?}"
    );
    assert!(
        text.contains("Lint/Debugger"),
        "baseline must freeze the cop, got: {text:?}"
    );

    // 3. Re-lint with --baseline: green, exit 0.
    let (code, parsed) = lint_json(dir.path(), &["--baseline", &baseline_str]);
    assert_eq!(code, 0, "baselined run must exit 0");
    assert!(
        parsed.is_empty(),
        "baselined run must report nothing, got {parsed:?}"
    );

    // 4. A NEW offense of the same cop in the same file surfaces (count = 2).
    fs::write(dir.path().join("legacy.rb"), THREE_DEBUGGERS).expect("add third debugger");
    let (code, parsed) = lint_json(dir.path(), &["--baseline", &baseline_str]);
    assert_eq!(code, 1);
    assert_eq!(
        parsed.len(),
        1,
        "only the new offense must surface, got {parsed:?}"
    );
    assert_eq!(parsed[0]["cop_name"], "Lint/Debugger");
}

#[test]
fn baseline_does_not_hide_other_cops() {
    let dir = tempdir().expect("create tempdir");
    fs::write(dir.path().join("legacy.rb"), TWO_DEBUGGERS).expect("write legacy.rb");
    let baseline = dir.path().join(".murphy-baseline.toml");
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--generate-baseline")
        .arg(&baseline)
        .arg(dir.path().join("legacy.rb"))
        .current_dir(dir.path())
        .assert()
        .code(1);
    // A syntax error (Murphy/Syntax) is a different cop: it must surface.
    fs::write(dir.path().join("legacy.rb"), "def broken(\n").expect("write broken.rb");
    let baseline_str = baseline.to_string_lossy();
    let (code, parsed) = lint_json(dir.path(), &["--baseline", &baseline_str]);
    assert_eq!(code, 1);
    assert!(
        parsed.iter().any(|o| o["cop_name"] == "Murphy/Syntax"),
        "unbaselined cop must surface, got {parsed:?}"
    );
}

#[test]
fn baseline_missing_file_exits_2() {
    let dir = tempdir().expect("create tempdir");
    fs::write(dir.path().join("legacy.rb"), TWO_DEBUGGERS).expect("write legacy.rb");
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("--baseline")
        .arg(dir.path().join("no-such-baseline.toml"))
        .arg(dir.path().join("legacy.rb"))
        .current_dir(dir.path())
        .assert()
        .code(2);
}

#[test]
fn baseline_malformed_toml_exits_2() {
    let dir = tempdir().expect("create tempdir");
    fs::write(dir.path().join("legacy.rb"), TWO_DEBUGGERS).expect("write legacy.rb");
    let bad = dir.path().join("bad.toml");
    fs::write(&bad, "not toml [[[\n").expect("write bad.toml");
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("--baseline")
        .arg(&bad)
        .arg(dir.path().join("legacy.rb"))
        .current_dir(dir.path())
        .assert()
        .code(2);
}

#[test]
fn baseline_flags_conflict() {
    let dir = tempdir().expect("create tempdir");
    fs::write(dir.path().join("legacy.rb"), TWO_DEBUGGERS).expect("write legacy.rb");
    // clap conflict → exit 2 (usage error).
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--baseline")
        .arg("a.toml")
        .arg("--generate-baseline")
        .arg("b.toml")
        .arg(dir.path().join("legacy.rb"))
        .current_dir(dir.path())
        .assert()
        .code(2);
}
