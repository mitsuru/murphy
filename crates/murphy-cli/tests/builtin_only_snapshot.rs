//! Post-reboot JSON-contract anchor.
//!
//! The legacy `sample_project` / `autocorrect_project` snapshots were
//! tied to the 14 standard cops (Layout / Lint / Style) and the spike
//! plugin ABI — all retired in murphy-9cr.22. This file replaces them.
//!
//! `builtin_only_project` is the smallest fixture that exercises:
//!
//! - the host's only v1 built-in (`Murphy/NoReceiverPuts`),
//! - multibyte source (ADR 0001 byte-offset semantics — the
//!   `multibyte.rb` row's offsets are u8 positions),
//! - directory discovery (no path args = discover-cwd),
//! - the JSON wire shape (ADR 0006 + `aggregator.rs`'s content-based
//!   sort).
//!
//! The snapshot is normalized through `serde_json::Value` rather than
//! compared byte-for-byte; CLI output is compact `serde_json::to_string`
//! while the committed file is pretty-printed for diff readability. The
//! aggregator's deterministic sort is what makes equality meaningful —
//! see `murphy_core::aggregator`.

use assert_cmd::Command;

const FIXTURE_DIR: &str = "tests/fixtures/builtin_only_project";
const SNAPSHOT_PATH: &str = "tests/snapshots/builtin_only_project.json";

#[test]
fn builtin_only_project_matches_committed_snapshot() {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(FIXTURE_DIR)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .assert()
        .code(1); // dirty.rb + multibyte.rb trigger NoReceiverPuts → exit 1.

    let actual: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be valid JSON");

    let snapshot_text = std::fs::read_to_string(SNAPSHOT_PATH).expect("snapshot file exists");
    let expected: serde_json::Value =
        serde_json::from_str(&snapshot_text).expect("snapshot is valid JSON");

    assert_eq!(
        actual, expected,
        "builtin_only_project offense JSON drifted; update \
         `{SNAPSHOT_PATH}` if the change is intentional"
    );
}

/// `clean.rb` on its own must lint to a zero-offense JSON response —
/// the bare-defaults wire shape (`[]` plus newline) is part of the
/// ADR 0006 contract. The whole-directory test above captures the
/// surface for *non-empty* output; this one is its zero-offense pair.
/// If the JSON shape ever regresses to `null` / `{}` / `""`, the
/// directory test would still pass (offenses elsewhere mask it), but
/// this one will catch it.
#[test]
fn clean_file_alone_yields_empty_json_array_and_exit_zero() {
    use assert_cmd::Command;
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(std::path::Path::new(FIXTURE_DIR).join("clean.rb"))
        .assert()
        .code(0);

    let parsed: serde_json::Value =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be valid JSON");
    assert!(
        parsed.is_array(),
        "zero-offense response must be a JSON array, got {parsed:?}"
    );
    let arr = parsed.as_array().unwrap();
    assert!(
        arr.is_empty(),
        "clean.rb on its own must produce zero offenses, got {arr:?}"
    );
}

/// A subset that asserts the `range.start_offset` of the multibyte file's
/// `puts "日本語"` offense lands on a u8 byte boundary, not a char index.
/// Byte 162 here is the index of `p` in `puts` after a leading multibyte
/// comment header — confirming ADR 0001 ("offsets are byte offsets, never
/// char indices") survives the arena rewrite.
#[test]
fn multibyte_offsets_are_byte_indices_not_char_indices() {
    let source = std::fs::read_to_string(std::path::Path::new(FIXTURE_DIR).join("multibyte.rb"))
        .expect("read multibyte fixture");

    // Find the `puts` call's byte offset in the source.
    let puts_byte_idx = source
        .find("puts \"")
        .expect("multibyte fixture must contain a `puts` call");

    // Tolerate either offset = puts_byte_idx (a future cop with a
    // selector-only range) or the broader Send-range start (post-reboot
    // default). Both are byte-indexed; what we reject is a char-index
    // value that diverges from both.
    let snapshot_text = std::fs::read_to_string(SNAPSHOT_PATH).expect("read snapshot");
    let snapshot: serde_json::Value = serde_json::from_str(&snapshot_text).unwrap();
    let multibyte_offense = snapshot
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["file"] == "./multibyte.rb")
        .expect("multibyte file must produce an offense");
    let start = multibyte_offense["range"]["start_offset"]
        .as_u64()
        .expect("start_offset is u64");
    assert_eq!(
        start as usize, puts_byte_idx,
        "the snapshot's start_offset for the multibyte file must be the \
         BYTE index of `puts` in the source (ADR 0001), not a char index"
    );
}
