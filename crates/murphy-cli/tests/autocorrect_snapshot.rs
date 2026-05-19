//! Autocorrect integration snapshot + idempotency end-to-end tests (Task 7).
//!
//! Exercises `murphy lint` and `murphy lint --fix` over a checked-in fixture
//! project (`tests/fixtures/autocorrect_project/`) that contains three `.rb`
//! sources and two mruby cops emitting real autocorrect edits (via
//! `Murphy::Fix#replace` and `Murphy::Fix#remove`).
//!
//! ## Why this test exists
//!
//! - **SNAPSHOT** (§a): `murphy lint` stdout matches `autocorrect_project.json`
//!   byte-for-byte, proving the `autocorrect:{edits:[...]}` payload is in the
//!   offense contract end-to-end and is deterministic across runs.
//! - **IDEMPOTENCY E2E** (§b, design §7 冪等性必須): a copy of the fixture is
//!   fixed once; then fixed again; the second run writes zero files and emits
//!   `murphy: fixed 0 of N files` — the true binary-level idempotency property.
//! - **POST-FIX DETERMINISM** (§c): two independent tempdir copies fixed in
//!   the same run yield byte-identical corrected outputs.
//! - **RESIDUAL OFFENSE** (§d): the fixture contains a `print` that
//!   `Murphy/NoReceiverPuts` (native, no autocorrect) fires on.  After `--fix`,
//!   that offense survives and the exit code is `1`; conversely a fully-fixable
//!   file exits `0` with an empty offense array.
//!
//! ## Re-blessing the snapshot
//!
//! `tests/snapshots/autocorrect_project.json` is GENERATED from the real
//! binary output and MUST NOT be hand-edited.  To regenerate it after an
//! intentional change to the fixture sources or cops:
//!
//! ```sh
//! cd crates/murphy-cli/tests/fixtures/autocorrect_project
//! cargo run -p murphy-cli -- lint replace_me.rb delete_me.rb mixed.rb \
//!   > ../../snapshots/autocorrect_project.json
//! ```
//!
//! Inspect the regenerated snapshot for correctness (verify `autocorrect.edits`
//! byte offsets match the fixture sources) and then commit it.  The test
//! compares byte-for-byte so a re-bless that changes whitespace or field order
//! will catch any serialisation regression.
//!
//! ## Determinism / portability
//!
//! The binary is invoked with `current_dir` set to the fixtures directory
//! (located via `CARGO_MANIFEST_DIR`) and **bare filenames** are passed, so
//! the `Offense.file` field is `replace_me.rb` (not an absolute tempdir path)
//! and the committed snapshot is portable across machines/CI.
//!
//! ## Frozen invariants
//!
//! `tests/fixtures/sample_project/` and `tests/snapshots/sample_project.json`
//! are NOT touched here (ADR 0006/0007/0012).  `integration_snapshot.rs` must
//! remain green and byte-identical.

use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Absolute path to `crates/murphy-cli/tests/fixtures/autocorrect_project`.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("autocorrect_project")
}

/// Absolute path to the committed expected snapshot.
fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("autocorrect_project.json")
}

/// The three fixture source files (bare names, in deliberate non-sorted order
/// to prove determinism comes from `aggregate`'s sort, not arg order).
const FIXTURE_FILES: &[&str] = &["mixed.rb", "replace_me.rb", "delete_me.rb"];

// ---------------------------------------------------------------------------
// §a — SNAPSHOT
// ---------------------------------------------------------------------------

/// `murphy lint replace_me.rb delete_me.rb mixed.rb` (run from the fixtures
/// dir so `file` fields are bare filenames) produces JSON that exactly matches
/// the committed snapshot byte-for-byte.
#[test]
fn lint_stdout_matches_committed_snapshot() {
    let dir = fixtures_dir();

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(&dir)
        .arg("lint")
        .args(FIXTURE_FILES)
        .assert()
        // Offenses present (mixed.rb has unfixable print + fixable puts,
        // replace_me.rb and delete_me.rb have fixable offenses) → exit 1.
        .code(1);

    let stdout = assert.get_output().stdout.clone();

    let expected_bytes = fs::read(snapshot_path()).expect(
        "committed snapshot crates/murphy-cli/tests/snapshots/autocorrect_project.json must exist",
    );

    assert_eq!(
        stdout,
        expected_bytes,
        "lint output does not match the committed snapshot (byte-for-byte).\n\
         If this change is intentional, re-bless the snapshot:\n\
         cd crates/murphy-cli/tests/fixtures/autocorrect_project\n\
         cargo run -p murphy-cli -- lint replace_me.rb delete_me.rb mixed.rb \\\n\
           > ../../snapshots/autocorrect_project.json\n\
         --- EXPECTED (committed) ---\n{}\n--- GOT ---\n{}",
        String::from_utf8_lossy(&expected_bytes),
        String::from_utf8_lossy(&stdout),
    );
}

// ---------------------------------------------------------------------------
// §b — IDEMPOTENCY E2E (design §7 冪等性必須)
// ---------------------------------------------------------------------------

/// Copy the fixture into a tempdir; run `murphy lint --fix`; capture corrected
/// bytes; run `murphy lint --fix` again; assert the second run:
///   1. writes ZERO files (files bytes identical to after first run), and
///   2. emits `murphy: fixed 0 of N files` on stderr.
#[test]
fn fix_is_idempotent_binary_level() {
    let dir = tempdir().expect("create tempdir");
    copy_fixture_to(dir.path());

    // First --fix pass.
    let first_out = run_fix(dir.path(), FIXTURE_FILES);
    let stderr_first = String::from_utf8_lossy(&first_out.stderr);
    // Summary must appear and report at least one fixed file.
    let summary_first = stderr_first
        .lines()
        .find(|l| l.starts_with("murphy: fixed "))
        .expect("first --fix must emit a summary line");
    assert!(
        !summary_first.contains("fixed 0 of"),
        "first pass must fix at least one file, got: {summary_first:?}"
    );

    // Capture file bytes after first pass.
    let bytes_after_first: Vec<(String, Vec<u8>)> = FIXTURE_FILES
        .iter()
        .map(|f| {
            (
                f.to_string(),
                fs::read(dir.path().join(f)).expect("read after first fix"),
            )
        })
        .collect();

    // Second --fix pass (must be a no-op if cops are idempotent).
    let second_out = run_fix(dir.path(), FIXTURE_FILES);
    let stderr_second = String::from_utf8_lossy(&second_out.stderr);
    let summary_second = stderr_second
        .lines()
        .find(|l| l.starts_with("murphy: fixed "))
        .expect("second --fix must emit a summary line");

    // "fixed 0 of N" — N is the file count we passed.
    let n = FIXTURE_FILES.len();
    assert!(
        summary_second.contains(&format!("fixed 0 of {n}")),
        "second --fix pass must report fixed 0 of {n}, got: {summary_second:?}"
    );

    // File bytes must be identical after the second pass.
    for (name, bytes_first) in &bytes_after_first {
        let bytes_second = fs::read(dir.path().join(name)).expect("read after second fix");
        assert_eq!(
            *bytes_first, bytes_second,
            "second --fix must not change {name} (idempotency violated)"
        );
    }
}

// ---------------------------------------------------------------------------
// §c — POST-FIX DETERMINISM
// ---------------------------------------------------------------------------

/// Two independent tempdir copies fixed in the same run must produce
/// byte-identical corrected outputs.
#[test]
fn fix_two_copies_produce_identical_corrected_bytes() {
    let dir_a = tempdir().expect("create tempdir A");
    let dir_b = tempdir().expect("create tempdir B");
    copy_fixture_to(dir_a.path());
    copy_fixture_to(dir_b.path());

    run_fix(dir_a.path(), FIXTURE_FILES);
    run_fix(dir_b.path(), FIXTURE_FILES);

    for name in FIXTURE_FILES {
        let a = fs::read(dir_a.path().join(name)).expect("read A after fix");
        let b = fs::read(dir_b.path().join(name)).expect("read B after fix");
        assert_eq!(
            a, b,
            "two independent copies of {name} must produce identical corrected bytes"
        );
    }
}

// ---------------------------------------------------------------------------
// §d — RESIDUAL OFFENSE and FULLY-FIXABLE EXIT CODES
// ---------------------------------------------------------------------------

/// After `--fix`, `mixed.rb` still has `Murphy/NoReceiverPuts` on the `print`
/// call (native cop, no autocorrect) → exit 1, exactly one residual offense.
#[test]
fn post_fix_mixed_has_residual_offense_and_exits_1() {
    let dir = tempdir().expect("create tempdir");
    copy_fixture_to(dir.path());

    run_fix(dir.path(), FIXTURE_FILES);

    // Lint mixed.rb alone after fix.
    let out = run_lint(dir.path(), &["mixed.rb"]);
    let exit_code = out.status.code().expect("exit code");
    assert_eq!(
        exit_code, 1,
        "post-fix mixed.rb still has residual → exit 1"
    );

    let offenses: Vec<serde_json::Value> =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON array");
    assert_eq!(
        offenses.len(),
        1,
        "exactly one residual offense (print), got: {offenses:?}"
    );
    assert_eq!(
        offenses[0]["cop_name"], "Murphy/NoReceiverPuts",
        "residual is the native NoReceiverPuts on print"
    );
}

/// After `--fix`, `replace_me.rb` (only a fixable puts) has zero offenses
/// → exit 0, empty offense array.
#[test]
fn post_fix_replace_me_fully_fixed_exits_0() {
    let dir = tempdir().expect("create tempdir");
    copy_fixture_to(dir.path());

    run_fix(dir.path(), &["replace_me.rb"]);

    let out = run_lint(dir.path(), &["replace_me.rb"]);
    let exit_code = out.status.code().expect("exit code");
    assert_eq!(exit_code, 0, "post-fix replace_me.rb fully fixed → exit 0");

    let offenses: Vec<serde_json::Value> =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON array");
    assert!(
        offenses.is_empty(),
        "post-fix replace_me.rb must have zero offenses, got: {offenses:?}"
    );
}

/// After `--fix`, `delete_me.rb` (only a fixable pp call) has zero offenses
/// → exit 0, empty offense array.
#[test]
fn post_fix_delete_me_fully_fixed_exits_0() {
    let dir = tempdir().expect("create tempdir");
    copy_fixture_to(dir.path());

    run_fix(dir.path(), &["delete_me.rb"]);

    let out = run_lint(dir.path(), &["delete_me.rb"]);
    let exit_code = out.status.code().expect("exit code");
    assert_eq!(exit_code, 0, "post-fix delete_me.rb fully fixed → exit 0");

    let offenses: Vec<serde_json::Value> =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON array");
    assert!(
        offenses.is_empty(),
        "post-fix delete_me.rb must have zero offenses, got: {offenses:?}"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Copy all fixture sources and cops/ into `dest`.
fn copy_fixture_to(dest: &std::path::Path) {
    let src = fixtures_dir();
    for entry in fs::read_dir(&src).expect("read fixtures dir") {
        let entry = entry.expect("dir entry");
        let ft = entry.file_type().expect("file type");
        let name = entry.file_name();
        if ft.is_file() {
            fs::copy(entry.path(), dest.join(&name)).expect("copy source file");
        } else if ft.is_dir() {
            let sub_src = entry.path();
            let sub_dst = dest.join(&name);
            fs::create_dir(&sub_dst).expect("create subdir");
            for sub_entry in fs::read_dir(&sub_src).expect("read subdir") {
                let sub_entry = sub_entry.expect("sub dir entry");
                fs::copy(sub_entry.path(), sub_dst.join(sub_entry.file_name()))
                    .expect("copy cop file");
            }
        }
    }
}

/// Run `murphy lint --fix <files>` from `dir`. Panics if the binary fails to
/// start; non-zero exit codes are expected and not treated as errors.
fn run_fix(dir: &std::path::Path, files: &[&str]) -> std::process::Output {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.current_dir(dir).arg("lint").arg("--fix");
    for f in files {
        cmd.arg(f);
    }
    cmd.assert().get_output().clone()
}

/// Run `murphy lint <files>` (no --fix) from `dir`.
fn run_lint(dir: &std::path::Path, files: &[&str]) -> std::process::Output {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.current_dir(dir).arg("lint");
    for f in files {
        cmd.arg(f);
    }
    cmd.assert().get_output().clone()
}
