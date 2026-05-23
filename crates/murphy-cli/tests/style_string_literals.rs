//! End-to-end tests for `Style/StringLiterals` against the compiled
//! `murphy` binary (murphy-9cr.23 §12d).
//!
//! Contract anchored:
//! - default `preferred_quote = "single"` flags `"x"` (double) but not
//!   `'x'` (single).
//! - interpolated strings (`"a#{b}"`) never produce a
//!   `Style/StringLiterals` offense (no quote style can host
//!   interpolation other than double).
//! - autocorrect emits an edit only when the body is safe to swap
//!   (no backslash, no `#`, no conflicting quote).

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
fn flags_double_quoted_literal_when_default_is_single() {
    let (code, offs) = lint_json("x = \"hello\"\n");
    assert_eq!(code, 1, "offense → exit 1");
    let style = offenses_named(&offs, "Style/StringLiterals");
    assert_eq!(
        style.len(),
        1,
        "must flag one double-quoted literal; got {offs:?}"
    );
}

#[test]
fn does_not_flag_single_quoted_literal_at_default() {
    let (_code, offs) = lint_json("x = 'hello'\n");
    assert!(
        offenses_named(&offs, "Style/StringLiterals").is_empty(),
        "single-quoted is the default; must not be flagged; got {offs:?}"
    );
}

#[test]
fn does_not_flag_interpolated_string() {
    // Interpolated strings need double quotes — they cannot be expressed
    // as `'…'`, so the cop must not subscribe to `Dstr` and must produce
    // no offense for `"a#{b}"`.
    let (_code, offs) = lint_json("b = 1\nx = \"a#{b}\"\n");
    assert!(
        offenses_named(&offs, "Style/StringLiterals").is_empty(),
        "interpolated strings must not be flagged; got {offs:?}",
    );
}

#[test]
fn safe_double_to_single_emits_autocorrect() {
    let (_code, offs) = lint_json("x = \"hello\"\n");
    let style = offenses_named(&offs, "Style/StringLiterals");
    assert_eq!(style.len(), 1);
    let edit = style[0]["autocorrect"]["edits"]
        .as_array()
        .expect("safe-swap case must include an autocorrect edit");
    assert_eq!(edit.len(), 1, "exactly one edit for the literal");
    assert_eq!(
        edit[0]["replacement"], "'hello'",
        "edit must rewrite the literal to single quotes",
    );
}

#[test]
fn double_with_backslash_emits_offense_but_skips_autocorrect() {
    // `"foo\n"` contains `\n` — semantics differ between quote styles so
    // the cop must NOT autocorrect. The offense still fires.
    let (_code, offs) = lint_json("x = \"foo\\n\"\n");
    let style = offenses_named(&offs, "Style/StringLiterals");
    assert_eq!(style.len(), 1, "offense still fires; got {offs:?}");
    assert!(
        style[0]["autocorrect"].is_null(),
        "must not autocorrect a backslash-bearing literal; got {:?}",
        style[0]["autocorrect"]
    );
}

#[test]
fn config_disabled_silences_the_cop() {
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join("murphy.toml"),
        "[cops.rules.\"Style/StringLiterals\"]\nenabled = false\n",
    )
    .expect("write murphy.toml");
    fs::write(dir.path().join("t.rb"), "x = \"hello\"\n").expect("write source");

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
        offenses_named(&parsed, "Style/StringLiterals").is_empty(),
        "user-disabled cop must produce no offenses; got {parsed:?}",
    );
}

#[test]
fn options_schema_advertises_enum_values() {
    // The §12c `cops list --format=json` exposes only NAME/NAMESPACE/STATUS/
    // SOURCE_PACK — not the schema. The schema lives on `PluginCopV1`
    // (consumed at runtime by the future config-validation gate,
    // murphy-9cr.9). For now, anchor that the cop is at least visible
    // as enabled, so future tests building on the schema have a
    // recognisable name to look up.
    let dir = tempdir().expect("create tempdir");
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("cops")
        .arg("list")
        .arg("--format=json")
        .assert()
        .code(0);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be JSON");
    let entry = parsed
        .iter()
        .find(|e| e["name"] == "Style/StringLiterals")
        .expect("cops list must include Style/StringLiterals");
    assert_eq!(entry["status"], "enabled");
    assert_eq!(entry["namespace"], "Style");
}
