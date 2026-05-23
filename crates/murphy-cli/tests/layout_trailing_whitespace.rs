//! End-to-end tests for `Layout/TrailingWhitespace` against the
//! compiled `murphy` binary (murphy-9cr.23 §12d).
//!
//! Contract anchored:
//! - flags trailing space / tab characters on any line.
//! - flags trailing whitespace on the *last* line even without a final
//!   `\n` (file-EOL edge case).
//! - flags whitespace-only lines (the whole line is trailing).
//! - autocorrect deletes the offending range (replacement is `""`).
//! - CRLF: a stray `\r` before `\n` counts as trailing whitespace.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn lint_json(source: &str) -> (i32, Vec<serde_json::Value>) {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("t.rb");
    fs::write(&path, source).expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert();
    let output = assert.get_output().clone();
    let code = output.status.code().unwrap_or(-1);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).expect("stdout must be JSON");
    (code, parsed)
}

fn offenses_named<'a>(offenses: &'a [serde_json::Value], cop: &str) -> Vec<&'a serde_json::Value> {
    offenses.iter().filter(|o| o["cop_name"] == cop).collect()
}

#[test]
fn flags_trailing_space_on_a_line() {
    // `x = 1` followed by two spaces then `\n`.
    let (code, offs) = lint_json("x = 1  \ny = 2\n");
    assert_eq!(code, 1, "offense → exit 1");
    let ws = offenses_named(&offs, "Layout/TrailingWhitespace");
    assert_eq!(
        ws.len(),
        1,
        "exactly one trailing-whitespace offense; got {offs:?}",
    );
    let edit = ws[0]["autocorrect"]["edits"]
        .as_array()
        .expect("autocorrect must emit an edit");
    assert_eq!(edit.len(), 1);
    assert_eq!(
        edit[0]["replacement"], "",
        "edit deletes the trailing whitespace"
    );
}

#[test]
fn flags_trailing_tab_on_a_line() {
    let (code, offs) = lint_json("x = 1\t\ny = 2\n");
    assert_eq!(code, 1);
    assert_eq!(
        offenses_named(&offs, "Layout/TrailingWhitespace").len(),
        1,
        "tab counts as trailing whitespace; got {offs:?}",
    );
}

#[test]
fn does_not_flag_clean_lines() {
    let (_code, offs) = lint_json("x = 1\ny = 2\n");
    assert!(
        offenses_named(&offs, "Layout/TrailingWhitespace").is_empty(),
        "no whitespace at any line end; got {offs:?}",
    );
}

#[test]
fn flags_trailing_whitespace_on_unterminated_last_line() {
    // Last line has trailing space and no final `\n`.
    let (code, offs) = lint_json("x = 1 ");
    assert_eq!(code, 1);
    assert_eq!(
        offenses_named(&offs, "Layout/TrailingWhitespace").len(),
        1,
        "last-line trailing whitespace counts even without a final newline; got {offs:?}",
    );
}

#[test]
fn flags_whitespace_only_line() {
    // The middle line is just spaces — the whole line is trailing
    // whitespace and reported once.
    let (code, offs) = lint_json("x = 1\n   \ny = 2\n");
    assert_eq!(code, 1);
    assert_eq!(
        offenses_named(&offs, "Layout/TrailingWhitespace").len(),
        1,
        "whitespace-only line is one offense; got {offs:?}",
    );
}

#[test]
fn flags_carriage_return_before_newline() {
    // CRLF line ending — the `\r` before `\n` is "trailing
    // whitespace" by the cop's definition so editors that auto-strip
    // CRs get pointed at it.
    let (code, offs) = lint_json("x = 1\r\ny = 2\n");
    assert_eq!(code, 1);
    assert_eq!(
        offenses_named(&offs, "Layout/TrailingWhitespace").len(),
        1,
        "stray `\\r` before `\\n` is trailing whitespace; got {offs:?}",
    );
}

#[test]
fn flags_multiple_trailing_whitespace_lines() {
    // Three offending lines; expect three offenses.
    let (code, offs) = lint_json("a  \nb  \nc  \n");
    assert_eq!(code, 1);
    assert_eq!(
        offenses_named(&offs, "Layout/TrailingWhitespace").len(),
        3,
        "one offense per trailing-whitespace line; got {offs:?}",
    );
}

#[test]
fn config_disabled_silences_the_cop() {
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join("murphy.toml"),
        "[cops.rules.\"Layout/TrailingWhitespace\"]\nenabled = false\n",
    )
    .expect("write murphy.toml");
    fs::write(dir.path().join("t.rb"), "x = 1  \n").expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("t.rb")
        .assert();
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be JSON");
    assert!(
        offenses_named(&parsed, "Layout/TrailingWhitespace").is_empty(),
        "user-disabled cop must produce no offenses; got {parsed:?}",
    );
}
