//! E2E for builtin config presets `extends:` / `--preset` (C3; ADR 0050).
//!
//! Presets are builtin named layers (minimal / recommended / shopify /
//! rails-strict) distributed with the binary. `extends: murphy:<name>`
//! in `.murphy.yml` and `murphy lint|cops list|watch --preset <name>`
//! select them; user `Enabled:`/options always win. Unknown names fail
//! with exit 2.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn run_lint(dir: &std::path::Path, extra: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.current_dir(dir).arg("lint").arg("--format").arg("json");
    for a in extra {
        cmd.arg(a);
    }
    cmd.assert()
}

#[test]
fn lint_preset_minimal_succeeds() {
    let dir = tempdir().expect("project");
    fs::write(
        dir.path().join("a.rb"),
        "# frozen_string_literal: true\n\nx = 1\nlogger.info x\n",
    )
    .expect("write a.rb");
    run_lint(dir.path(), &["--preset", "minimal"]).code(0);
}

#[test]
fn lint_preset_prefixed_form_succeeds() {
    let dir = tempdir().expect("project");
    fs::write(
        dir.path().join("a.rb"),
        "# frozen_string_literal: true\n\nx = 1\nlogger.info x\n",
    )
    .expect("write a.rb");
    run_lint(dir.path(), &["--preset", "murphy:shopify"]).code(0);
}

#[test]
fn lint_unknown_preset_fails_exit_2_with_hint() {
    let dir = tempdir().expect("project");
    fs::write(
        dir.path().join("a.rb"),
        "# frozen_string_literal: true\n\nx = 1\nlogger.info x\n",
    )
    .expect("write a.rb");
    let assert = run_lint(dir.path(), &["--preset", "murphy:nope"]);
    assert.code(2);
    // Re-run to capture stderr text.
    let out = Command::cargo_bin("murphy")
        .expect("builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--preset")
        .arg("murphy:nope")
        .arg("a.rb")
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&out).into_owned();
    assert!(
        stderr.contains("unknown preset"),
        "stderr must name the problem: {stderr:?}"
    );
    assert!(
        stderr.contains("minimal"),
        "stderr must list presets: {stderr:?}"
    );
}

#[test]
fn lint_extends_minimal_in_file_succeeds() {
    let dir = tempdir().expect("project");
    fs::write(dir.path().join(".murphy.yml"), "extends: murphy:minimal\n").expect("config");
    fs::write(
        dir.path().join("a.rb"),
        "# frozen_string_literal: true\n\nx = 1\nlogger.info x\n",
    )
    .expect("write a.rb");
    run_lint(dir.path(), &[]).code(0);
}

#[test]
fn lint_extends_unknown_in_file_fails_exit_2() {
    let dir = tempdir().expect("project");
    fs::write(dir.path().join(".murphy.yml"), "extends: murphy:nope\n").expect("config");
    fs::write(
        dir.path().join("a.rb"),
        "# frozen_string_literal: true\n\nx = 1\nlogger.info x\n",
    )
    .expect("write a.rb");
    run_lint(dir.path(), &[]).code(2);
}

#[test]
fn cops_list_preset_minimal_disables_metrics_as_user_config() {
    let dir = tempdir().expect("project");
    let assert = Command::cargo_bin("murphy")
        .expect("builds")
        .current_dir(dir.path())
        .arg("cops")
        .arg("list")
        .arg("--format=json")
        .arg("--preset")
        .arg("minimal")
        .assert()
        .code(0);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("json array");
    let entry = parsed
        .iter()
        .find(|e| e["name"] == "Metrics/MethodLength")
        .expect("Metrics/MethodLength must be listed");
    assert_eq!(
        entry["status"], "disabled: user config",
        "preset disable surfaces as user-config disable: {entry:?}"
    );
}

#[test]
fn cops_list_user_wins_over_preset() {
    let dir = tempdir().expect("project");
    fs::write(
        dir.path().join(".murphy.yml"),
        "extends: murphy:minimal\nMetrics/MethodLength:\n  Enabled: true\n",
    )
    .expect("config");
    let assert = Command::cargo_bin("murphy")
        .expect("builds")
        .current_dir(dir.path())
        .arg("cops")
        .arg("list")
        .arg("--format=json")
        .assert()
        .code(0);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("json array");
    let entry = parsed
        .iter()
        .find(|e| e["name"] == "Metrics/MethodLength")
        .expect("listed");
    assert_eq!(entry["status"], "enabled", "user opt-in wins: {entry:?}");
}

#[test]
fn cops_list_rails_strict_enables_curated_rails_cop() {
    let dir = tempdir().expect("project");
    let assert = Command::cargo_bin("murphy")
        .expect("builds")
        .current_dir(dir.path())
        .arg("cops")
        .arg("list")
        .arg("--format=json")
        .arg("--preset")
        .arg("rails-strict")
        .assert()
        .code(0);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("json array");
    // Rails cops live in the optional murphy-rails pack; the builtin
    // catalogue may not list them. The preset must at least not error,
    // and when the cop is present it must read enabled.
    if let Some(entry) = parsed.iter().find(|e| e["name"] == "Rails/Blank") {
        assert_eq!(
            entry["status"], "enabled",
            "rails-strict enables: {entry:?}"
        );
    }
}
