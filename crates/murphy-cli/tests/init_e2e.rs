//! E2E for `murphy init` setup UX (C5; ADR 0051).
//!
//! `murphy init` scaffolds `.murphy.yml` + `.murphyignore` so the next
//! `murphy lint` returns a sensible result fast (Phase 10 gate 4).
//! Existing files are never overwritten without `--force` (atomic
//! pre-flight); `--preset` selects a builtin preset (C3); `--from`
//! migrates a `.rubocop.yml`; `--hook` adds a B8 git-hook scaffold.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn init(dir: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.current_dir(dir).arg("init");
    for a in args {
        cmd.arg(a);
    }
    cmd.assert()
}

fn lint_json(dir: &std::path::Path) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.current_dir(dir).arg("lint").arg("--format").arg("json");
    cmd.assert()
}

#[test]
fn init_generates_config_and_ignore() {
    let dir = tempdir().expect("tempdir");
    init(dir.path(), &[]).success();
    let cfg = fs::read_to_string(dir.path().join(".murphy.yml")).expect("config written");
    assert!(cfg.contains("extends: murphy:recommended"), "got:\n{cfg}");
    assert!(cfg.contains("AllCops"), "got:\n{cfg}");
    // Generated config must parse.
    let parsed = murphy_core::MurphyConfig::from_yaml_str(&cfg).expect("parses");
    let _ = parsed;
    let ignore = fs::read_to_string(dir.path().join(".murphyignore")).expect("ignore written");
    assert!(ignore.contains("db/schema.rb"), "got:\n{ignore}");
}

#[test]
fn init_then_lint_returns_valid_output() {
    let dir = tempdir().expect("tempdir");
    init(dir.path(), &[]).success();
    fs::write(
        dir.path().join("clean.rb"),
        "# frozen_string_literal: true\n\nlogger.info x\n",
    )
    .expect("write rb");
    // Clean file: exit 0, empty array.
    let assert = lint_json(dir.path()).code(0);
    assert_eq!(assert.get_output().stdout, b"[]\n");
}

#[test]
fn init_then_lint_reports_offenses_not_setup_error() {
    let dir = tempdir().expect("tempdir");
    init(dir.path(), &[]).success();
    fs::write(dir.path().join("dirty.rb"), "puts \"hi\"\n").expect("write rb");
    // Offenses => exit 1 (not exit 2 setup error).
    let assert = lint_json(dir.path()).code(1);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("json array");
    assert!(!parsed.is_empty(), "dirty file must produce offenses");
}

#[test]
fn init_refuses_to_clobber_without_force() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join(".murphy.yml"), "custom: true\n").expect("seed");
    init(dir.path(), &[]).code(2);
    assert_eq!(
        fs::read_to_string(dir.path().join(".murphy.yml")).expect("read"),
        "custom: true\n"
    );
    // --force overwrites with the template.
    init(dir.path(), &["--force"]).success();
    let cfg = fs::read_to_string(dir.path().join(".murphy.yml")).expect("read");
    assert!(cfg.contains("extends: murphy:recommended"), "got:\n{cfg}");
}

#[test]
fn init_preset_minimal_seeds_extends() {
    let dir = tempdir().expect("tempdir");
    init(dir.path(), &["--preset", "minimal"]).success();
    let cfg = fs::read_to_string(dir.path().join(".murphy.yml")).expect("read");
    assert!(cfg.contains("extends: murphy:minimal"), "got:\n{cfg}");
    murphy_core::MurphyConfig::from_yaml_str(&cfg).expect("parses");
}

#[test]
fn init_unknown_preset_exits_2() {
    let dir = tempdir().expect("tempdir");
    let assert = init(dir.path(), &["--preset", "murphy:nope"]).code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(stderr.contains("unknown preset"), "got: {stderr:?}");
    assert!(stderr.contains("minimal"), "lists presets: {stderr:?}");
    assert!(
        !dir.path().join(".murphy.yml").exists(),
        "failed init must not write"
    );
}

#[test]
fn init_from_migrates_rubocop_yml() {
    let dir = tempdir().expect("tempdir");
    fs::write(
        dir.path().join(".rubocop.yml"),
        "Lint/Debugger:\n  Enabled: false\n",
    )
    .expect("seed rubocop");
    init(dir.path(), &["--from", ".rubocop.yml"]).success();
    let cfg = fs::read_to_string(dir.path().join(".murphy.yml")).expect("read");
    assert!(cfg.contains("Lint/Debugger"), "migrated rule kept:\n{cfg}");
    assert!(cfg.contains("extends: murphy:recommended"), "got:\n{cfg}");
    // Migrated config disables the cop: lint a debugger file => clean.
    fs::write(
        dir.path().join("d.rb"),
        "# frozen_string_literal: true\n\ndebugger\n",
    )
    .expect("write rb");
    lint_json(dir.path()).code(0);
}

#[test]
fn init_hook_writes_hook_scaffold() {
    let dir = tempdir().expect("tempdir");
    init(dir.path(), &["--hook"]).success();
    let body = fs::read_to_string(dir.path().join("lefthook.yml")).expect("hook written");
    assert!(body.contains("murphy lint"), "got:\n{body}");
    // Config + ignore still written alongside.
    assert!(dir.path().join(".murphy.yml").exists());
    assert!(dir.path().join(".murphyignore").exists());
}

#[test]
fn init_hook_atomic_without_force() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("lefthook.yml"), "custom: true\n").expect("seed hook");
    init(dir.path(), &["--hook"]).code(2);
    assert!(
        !dir.path().join(".murphy.yml").exists(),
        "must not write partial scaffolds"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("lefthook.yml")).expect("read"),
        "custom: true\n"
    );
}
