//! Multi-file integration snapshot test (Task 9 — last Phase 1 impl task).
//!
//! Exercises `murphy lint <file>...` over a checked-in fixture project of
//! four `.rb` files (clean / dirty / broken / multibyte) and asserts the
//! aggregated JSON output exactly matches a committed, hand-verified
//! snapshot.
//!
//! ## Why this test exists
//!
//! - **Multi-file aggregation.** Proves the CLI accepts >1 file arg, runs the
//!   single-parse pipeline per file, and aggregates *across* files into one
//!   deterministic JSON array (sorted by `(file, start_offset)`).
//! - **ADR 0001 byte offsets in a real snapshot.** `multibyte.rb` opens with
//!   a self-documenting ASCII gate-comment block, then a multibyte UTF-8
//!   line (`# コメント`) immediately before a receiver-less `puts`, so its
//!   offense `range` is at byte 903..907 — NOT the char offset 893..897
//!   (they differ by 10: the multibyte line is 16 bytes / 6 chars). The
//!   committed snapshot bakes in the *byte* numbers, so a regression to
//!   char-indexing would fail this test loudly. The exact offsets are
//!   re-derived (re-blessed) from the binary's real output whenever any
//!   fixture is edited — do NOT hand-edit `tests/snapshots/sample_project.json`.
//!
//! ## Determinism / portability
//!
//! The binary is invoked with `current_dir` set to the fixtures directory
//! (located via `CARGO_MANIFEST_DIR`) and **bare filenames** are passed, so
//! the `Offense.file` field is `clean.rb` (not an absolute tempdir path) and
//! the committed snapshot is portable across machines/CI. Ordering comes
//! from `aggregate` (sorted by file then offset), NOT arg order.

use assert_cmd::Command;
use std::path::PathBuf;

/// Absolute path to `crates/murphy-cli/tests/fixtures/sample_project`.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample_project")
}

/// Absolute path to the committed expected snapshot.
fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("sample_project.json")
}

/// `murphy lint clean.rb dirty.rb broken.rb multibyte.rb` (run from the
/// fixtures dir so `file` fields are bare filenames) produces JSON that
/// exactly matches the committed snapshot, and exits `1` (offenses present).
#[test]
fn multi_file_lint_matches_committed_snapshot() {
    let dir = fixtures_dir();

    // Pass files in a deliberately non-sorted arg order to prove the snapshot
    // determinism comes from `aggregate`'s (file, start_offset) sort, NOT the
    // order the files were passed on the command line.
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(&dir)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("dirty.rb")
        .arg("multibyte.rb")
        .arg("clean.rb")
        .arg("broken.rb")
        .assert()
        // At least one fixture is dirty/broken → offenses present → exit 1.
        .code(1);

    let stdout = &assert.get_output().stdout;
    let got: serde_json::Value =
        serde_json::from_slice(stdout).expect("stdout must be a JSON array");

    let expected_bytes = std::fs::read(snapshot_path()).expect(
        "committed snapshot crates/murphy-cli/tests/snapshots/sample_project.json must exist",
    );
    let expected: serde_json::Value =
        serde_json::from_slice(&expected_bytes).expect("committed snapshot must be valid JSON");

    assert_eq!(
        got,
        expected,
        "multi-file aggregated output does not match the committed snapshot.\n\
         If this change is intentional, re-bless the snapshot at \
         crates/murphy-cli/tests/snapshots/sample_project.json.\n\
         --- EXPECTED ---\n{}\n--- GOT ---\n{}",
        serde_json::to_string_pretty(&expected).unwrap(),
        serde_json::to_string_pretty(&got).unwrap(),
    );
}
