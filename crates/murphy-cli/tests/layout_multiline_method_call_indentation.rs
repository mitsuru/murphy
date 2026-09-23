//! End-to-end config coverage for `Layout/MultilineMethodCallIndentation`.

use std::fs;

use assert_cmd::Command;
use tempfile::tempdir;

fn lint_with_config(config: &str, source: &str) -> (i32, Vec<serde_json::Value>) {
    let dir = tempdir().expect("create tempdir");
    fs::write(dir.path().join(".murphy.yml"), config).expect("write .murphy.yml");
    fs::write(dir.path().join("t.rb"), source).expect("write source");

    let output = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("t.rb")
        .assert()
        .get_output()
        .clone();
    let code = output.status.code().unwrap_or(-1);
    let offenses = serde_json::from_slice(&output.stdout).expect("stdout must be JSON");
    (code, offenses)
}

fn offenses_for<'a>(offenses: &'a [serde_json::Value], cop: &str) -> Vec<&'a serde_json::Value> {
    offenses
        .iter()
        .filter(|offense| offense["cop_name"] == cop)
        .collect()
}

#[test]
fn indented_style_uses_configured_shared_indentation_width() {
    let (code, offenses) = lint_with_config(
        "Layout/IndentationWidth:\n  Width: 4\nLayout/MultilineMethodCallIndentation:\n  EnforcedStyle: indented\n",
        "Thing.a\n.c\n",
    );

    assert_eq!(code, 1, "the misindented chain should report an offense");
    let cop_offenses = offenses_for(&offenses, "Layout/MultilineMethodCallIndentation");
    assert_eq!(
        cop_offenses.len(),
        1,
        "expected one cop offense: {offenses:?}"
    );
    assert_eq!(
        cop_offenses[0]["message"],
        "Use 4 (not 0) spaces for indenting an expression spanning multiple lines."
    );
}

#[test]
fn relative_style_uses_cop_indentation_width() {
    let (code, offenses) = lint_with_config(
        "Layout/MultilineMethodCallIndentation:\n  EnforcedStyle: indented_relative_to_receiver\n  IndentationWidth: 4\n",
        "x = Thing.a\n      .c\n",
    );

    assert_eq!(code, 1, "the misindented chain should report an offense");
    let cop_offenses = offenses_for(&offenses, "Layout/MultilineMethodCallIndentation");
    assert_eq!(
        cop_offenses.len(),
        1,
        "expected one cop offense: {offenses:?}"
    );
    assert_eq!(
        cop_offenses[0]["message"],
        "Indent `.c` 4 spaces more than `Thing` on line 1."
    );
}
