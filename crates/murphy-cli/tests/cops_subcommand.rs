//! Integration tests for `murphy cops list` (murphy-9cr.23 §12c).
//!
//! Public contract under test:
//!
//! - Table output has 4 columns (NAME / NAMESPACE / STATUS / SOURCE PACK)
//!   and exit code 0.
//! - `--format=json` emits a JSON array whose elements have the stable
//!   keys `name`, `namespace`, `status`, `source_pack` (snake_case).
//! - Three statuses are surfaced: `enabled`, `disabled: arena migration`,
//!   `disabled: user config`.
//! - A `[cops.rules."Name"] enabled = true` against a cop in the
//!   disabled registry emits a warning on stderr and lint still exits
//!   0 / cleanly (no error path).
//! - The active cop (`Lint/Debugger`) is present and reported
//!   as `enabled`.
//! - Every entry in `murphy_std::DISABLED_COPS` appears as
//!   `disabled: arena migration`. The list is empty after §12d
//!   migrated `Lint/UnreachableCode` / `Style/StringLiterals` /
//!   `Layout/TrailingWhitespace` and stays empty until murphy-au8
//!   §14a adds `murphy-rails`'s pending cops.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

#[test]
fn cops_list_default_table_includes_active_and_disabled_cops_and_exits_0() {
    let dir = tempdir().expect("create tempdir");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("cops")
        .arg("list")
        .assert()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf-8 stdout");
    // Header — exact column titles are part of the contract.
    assert!(
        stdout.contains("NAME")
            && stdout.contains("NAMESPACE")
            && stdout.contains("STATUS")
            && stdout.contains("SOURCE PACK"),
        "table header must include all four columns; got:\n{stdout}"
    );
    // Active cop is enabled and tagged with the builtin pack.
    assert!(
        stdout.contains("Lint/Debugger") && stdout.contains("enabled"),
        "Lint/Debugger must appear as enabled; got:\n{stdout}"
    );
    // Every name in `murphy_std::DISABLED_COPS` must surface as a row
    // with the arena-migration status. After §12d's third cop migrated,
    // the live list is empty until murphy-au8 §14a adds Rails — but we
    // keep the contract data-driven so it picks up automatically when
    // the list repopulates.
    for name in murphy_std::DISABLED_COPS {
        assert!(
            stdout.contains(name),
            "disabled cop {name:?} must appear in the table; got:\n{stdout}"
        );
    }
    if !murphy_std::DISABLED_COPS.is_empty() {
        assert!(
            stdout.contains("disabled: arena migration"),
            "table must surface the arena-migration status when disabled cops exist; got:\n{stdout}"
        );
    }
}

#[test]
fn cops_list_json_format_is_a_machine_readable_array() {
    let dir = tempdir().expect("create tempdir");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("cops")
        .arg("list")
        .arg("--format=json")
        .assert()
        .code(0);

    let stdout = &assert.get_output().stdout;
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout must be a JSON array");

    // Every entry has the four contract keys.
    for entry in &parsed {
        let obj = entry.as_object().expect("entry must be an object");
        for key in ["name", "namespace", "status", "source_pack"] {
            assert!(
                obj.contains_key(key),
                "every JSON entry must contain key {key:?}; entry: {entry:?}"
            );
            assert!(
                obj[key].is_string(),
                "JSON value for {key:?} must be a string; got {:?}",
                obj[key]
            );
        }
    }

    // Active cop present with status `enabled`.
    let active = parsed
        .iter()
        .find(|e| e["name"] == "Lint/Debugger")
        .expect("Lint/Debugger must appear in JSON listing");
    assert_eq!(active["status"], "enabled");
    assert_eq!(active["namespace"], "Lint");
    assert_eq!(active["source_pack"], "builtin");

    // Data-driven: every name in `DISABLED_COPS` must appear in the
    // listing with the arena-migration status. Empty list → no
    // assertions to run (currently the case post-§12d; will repopulate
    // in murphy-au8 §14a).
    for name in murphy_std::DISABLED_COPS {
        let entry = parsed
            .iter()
            .find(|e| e["name"] == *name)
            .unwrap_or_else(|| panic!("{name:?} must appear in JSON listing"));
        assert_eq!(
            entry["status"], "disabled: arena migration",
            "{name:?} must be tagged as arena migration",
        );
        assert_eq!(entry["source_pack"], "builtin");
    }
}

#[test]
fn cops_list_reports_user_disabled_status_for_active_cop() {
    // `enabled = false` against the *active* `Lint/Debugger`
    // surfaces as `disabled: user config`, distinct from the
    // arena-migration status applied to never-migrated cops.
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join(".murphy.yml"),
        "Lint/Debugger:\n  Enabled: false\n",
    )
    .expect("write .murphy.yml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("cops")
        .arg("list")
        .arg("--format=json")
        .assert()
        .code(0);

    let stdout = &assert.get_output().stdout;
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout must be a JSON array");

    let entry = parsed
        .iter()
        .find(|e| e["name"] == "Lint/Debugger")
        .expect("Lint/Debugger must appear in JSON listing");
    assert_eq!(entry["status"], "disabled: user config");
}

#[test]
fn lint_warns_and_continues_when_user_enables_a_disabled_cop() {
    // Contract: when at least one cop sits in the disabled registry,
    // enabling it explicitly must produce a diagnostic on stderr and
    // NOT fail the lint run (exit 0, no unknown-cop error). When the
    // disabled registry is empty (post-§12d, pre-§14a) the mechanism
    // has no live tenant to probe — the warning code path is
    // exercised once `DISABLED_COPS` repopulates with Rails. Skipping
    // is preferred over deleting the test so the contract stays
    // documented next to the live mechanism.
    let Some(probe) = murphy_std::DISABLED_COPS.first() else {
        return;
    };
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join(".murphy.yml"),
        format!("{probe}:\n  Enabled: true\n"),
    )
    .expect("write .murphy.yml");
    fs::write(dir.path().join("clean.rb"), "x = 1\n").expect("write clean.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("clean.rb")
        .assert()
        .code(0);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf-8 stderr");
    assert!(
        stderr.contains(probe)
            && stderr.contains("disabled registry")
            && stderr.contains("arena migration"),
        "expected warning naming {probe:?} and arena migration; got stderr:\n{stderr}",
    );
}

#[test]
fn cops_list_rejects_unknown_format_value() {
    let dir = tempdir().expect("create tempdir");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("cops")
        .arg("list")
        .arg("--format=yaml")
        .assert()
        .failure();
}

#[test]
fn cops_list_help_describes_format() {
    let dir = tempdir().expect("create tempdir");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("cops")
        .arg("list")
        .arg("--help")
        .assert()
        .code(0);

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    for expected in ["--format", "table", "json"] {
        assert!(
            stdout.contains(expected),
            "cops list help should mention {expected:?}, got:\n{stdout}"
        );
    }
}

#[test]
fn cops_subcommand_requires_a_subcommand() {
    let dir = tempdir().expect("create tempdir");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("cops")
        .assert()
        .failure();
}
