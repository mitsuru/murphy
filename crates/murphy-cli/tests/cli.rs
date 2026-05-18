//! Integration tests for the `murphy` binary (Task 7).
//!
//! These exercise the *compiled binary* via `assert_cmd` — the same surface a
//! user invokes — not a library API (the cli is a bin crate with no lib).
//!
//! Pinned contract (design §5 + plan Task 7):
//! - `murphy lint <clean.rb>`  → stdout is an empty JSON array, exit `0`.
//! - `murphy lint <dirty.rb>`  → stdout is a 1-element JSON array whose
//!   `cop_name == "Murphy/NoReceiverPuts"`, exit `1`.
//! - `murphy lint <missing>`   → exit `2` (file/setup error).
//! - `murphy lint <broken.rb>` → stdout is a 1-element JSON array whose
//!   `cop_name == "Murphy/Syntax"` (cops skipped), exit `1` (design §6).
//!
//! stdout is asserted by *decoding JSON*, never brittle string matching
//! (beyond the canonical empty-array case). Diagnostics go to stderr, so the
//! error-exit cases deliberately do not assert on stdout content.

use assert_cmd::Command;
use murphy_core::SYNTAX_COP_NAME;
use std::fs;
use tempfile::tempdir;

/// `lint` a clean file → exit 0, stdout is an empty JSON array.
#[test]
fn lint_clean_file_exits_0_with_empty_json_array() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("clean.rb");
    fs::write(&path, "x = 1\n").expect("write clean.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg(&path)
        .assert()
        .code(0);

    let stdout = &assert.get_output().stdout;
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout must be a JSON array");
    assert!(
        parsed.is_empty(),
        "clean file must yield zero offenses, got {parsed:?}"
    );
}

/// `lint` a file containing `puts` → exit 1, one NoReceiverPuts offense.
#[test]
fn lint_dirty_file_exits_1_with_one_offense() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("dirty.rb");
    fs::write(&path, "puts \"hi\"\n").expect("write dirty.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg(&path)
        .assert()
        .code(1);

    let stdout = &assert.get_output().stdout;
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        1,
        "expected exactly one offense, got {parsed:?}"
    );
    assert_eq!(
        parsed[0]["cop_name"], "Murphy/NoReceiverPuts",
        "offense must be from the NoReceiverPuts cop"
    );
}

/// `lint` a path that does not exist → exit 2 (file/setup error).
#[test]
fn lint_missing_file_exits_2() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("does_not_exist.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg(&path)
        .assert()
        .code(2);

    // Contract guard: stdout is ONLY ever JSON; error paths emit nothing on it.
    assert!(
        assert.get_output().stdout.is_empty(),
        "error path must write nothing to stdout, got {:?}",
        assert.get_output().stdout
    );
}

/// `lint` an unparseable file → exit 1, exactly one `Murphy/Syntax` offense,
/// cops skipped for that file (design §6: "1 offense, skip cops, continue").
#[test]
fn lint_syntax_error_file_exits_1_with_one_syntax_offense() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("broken.rb");
    // Genuinely unparseable Ruby that ALSO textually contains a receiver-less
    // `puts` (a line `NoReceiverPuts` WOULD flag if cops ran). Verified with
    // the built binary: prism's `parse` returns Err on this source (the
    // trailing `def (` is a hard parse error), so it remains a parse failure,
    // not a parsed file with a NoReceiverPuts offense. This pins design §6's
    // skip-cops contract: a parse failure yields ONLY the synthetic syntax
    // offense and the cop pass is genuinely skipped.
    fs::write(&path, "puts \"x\"\ndef (\n").expect("write broken.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg(&path)
        .assert()
        .code(1);

    let stdout = &assert.get_output().stdout;
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        1,
        "syntax-error file must yield exactly one offense, got {parsed:?}"
    );
    assert_eq!(
        parsed[0]["cop_name"], SYNTAX_COP_NAME,
        "the single offense must be the synthetic syntax-error offense"
    );
    // Skip-cops invariant (design §6): even though the source textually
    // contains a receiver-less `puts`, NO cop offense is emitted because the
    // cop pass is skipped on a parse failure (there is no AST).
    assert!(
        !parsed
            .iter()
            .any(|o| o["cop_name"] == "Murphy/NoReceiverPuts"),
        "cops must be skipped on a parse failure — no NoReceiverPuts offense \
         despite the receiver-less puts in source, got {parsed:?}"
    );
    assert_eq!(
        parsed[0]["severity"], "error",
        "a syntax error is an Error-severity offense"
    );
    let message = parsed[0]["message"]
        .as_str()
        .expect("syntax offense message must be a JSON string");
    assert!(
        !message.is_empty(),
        "syntax offense must carry a non-empty message, got {message:?}"
    );
    // Message-verbatim invariant (design §6): the producer uses prism's
    // first-error text directly. A refactor to the `Display` form would
    // prepend `"parse error at bytes A..B: "` — guard against that drift
    // without hard-equalling the prism string (which can vary across versions).
    assert!(
        !message.starts_with("parse error at bytes"),
        "syntax offense message must be prism's verbatim text, not the \
         ParseError Display form, got {message:?}"
    );
}

/// Missing subcommand / wrong usage → exit 2 (bad CLI usage is a setup error).
#[test]
fn bad_usage_exits_2() {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .assert()
        .code(2);

    // Contract guard: stdout is ONLY ever JSON; error paths emit nothing on it.
    assert!(
        assert.get_output().stdout.is_empty(),
        "error path must write nothing to stdout, got {:?}",
        assert.get_output().stdout
    );
}
