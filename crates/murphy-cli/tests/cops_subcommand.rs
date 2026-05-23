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
//! - The active cop (`Murphy/NoReceiverPuts`) is present and reported
//!   as `enabled`.
//! - The three §12d-pending cops are present as `disabled: arena
//!   migration` so the migration backlog is discoverable from the CLI.

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
        stdout.contains("Murphy/NoReceiverPuts") && stdout.contains("enabled"),
        "Murphy/NoReceiverPuts must appear as enabled; got:\n{stdout}"
    );
    // The §12d-pending cops appear with the arena-migration status.
    // `Lint/UnreachableCode` and `Style/StringLiterals` migrated in §12d;
    // `Layout/TrailingWhitespace` remains in the disabled registry until
    // its own commit lands.
    assert!(
        stdout.contains("Layout/TrailingWhitespace"),
        "disabled cop `Layout/TrailingWhitespace` must appear in the table; got:\n{stdout}"
    );
    assert!(
        stdout.contains("disabled: arena migration"),
        "table must surface the arena-migration status string; got:\n{stdout}"
    );
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
        .find(|e| e["name"] == "Murphy/NoReceiverPuts")
        .expect("Murphy/NoReceiverPuts must appear in JSON listing");
    assert_eq!(active["status"], "enabled");
    assert_eq!(active["namespace"], "Murphy");
    assert_eq!(active["source_pack"], "builtin");

    // Remaining disabled cops are tagged as arena migration. The list
    // shrinks each time a §12d cop is migrated out of DISABLED_COPS.
    let entry = parsed
        .iter()
        .find(|e| e["name"] == "Layout/TrailingWhitespace")
        .expect("Layout/TrailingWhitespace must appear in JSON listing");
    assert_eq!(
        entry["status"], "disabled: arena migration",
        "Layout/TrailingWhitespace must be tagged as arena migration",
    );
    assert_eq!(entry["source_pack"], "builtin");
}

#[test]
fn cops_list_reports_user_disabled_status_for_active_cop() {
    // `enabled = false` against the *active* `Murphy/NoReceiverPuts`
    // surfaces as `disabled: user config`, distinct from the
    // arena-migration status applied to never-migrated cops.
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join("murphy.toml"),
        "[cops.rules.\"Murphy/NoReceiverPuts\"]\nenabled = false\n",
    )
    .expect("write murphy.toml");

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
        .find(|e| e["name"] == "Murphy/NoReceiverPuts")
        .expect("Murphy/NoReceiverPuts must appear in JSON listing");
    assert_eq!(entry["status"], "disabled: user config");
}

#[test]
fn lint_warns_and_continues_when_user_enables_a_disabled_cop() {
    // Contract: enabling a cop in the disabled registry must produce a
    // diagnostic on stderr and NOT fail the lint run (exit 0, no
    // unknown-cop error). The cop itself is not dispatched.
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join("murphy.toml"),
        "[cops.rules.\"Layout/TrailingWhitespace\"]\nenabled = true\n",
    )
    .expect("write murphy.toml");
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
        stderr.contains("Layout/TrailingWhitespace")
            && stderr.contains("disabled registry")
            && stderr.contains("arena migration"),
        "expected warning naming the cop and arena migration; got stderr:\n{stderr}"
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
fn cops_subcommand_requires_a_subcommand() {
    let dir = tempdir().expect("create tempdir");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("cops")
        .assert()
        .failure();
}
