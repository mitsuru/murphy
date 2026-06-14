//! CLI e2e for murphy-bgd8: a user `.murphy.yml` `Layout/IndentationWidth:
//! Width: N` must flow through `Config::load` -> `allcops_context` -> dispatch
//! and change `Layout/ClosingParenthesisIndentation`'s expected column. This
//! seals the real composition seam the in-process unit tests do not exercise
//! (those inject the resolved width directly via `Tester::with_indentation_width`).

use std::fs;

use assert_cmd::Command;
use tempfile::tempdir;

fn lint_json_with_config(config: &str, source: &str) -> Vec<serde_json::Value> {
    let dir = tempdir().expect("create tempdir");
    fs::write(dir.path().join(".murphy.yml"), config).expect("write .murphy.yml");
    fs::write(dir.path().join("t.rb"), source).expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("t.rb")
        .assert();
    serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be JSON")
}

fn closing_paren_messages(offenses: &[serde_json::Value]) -> Vec<&str> {
    offenses
        .iter()
        .filter(|o| o["cop_name"] == "Layout/ClosingParenthesisIndentation")
        .filter_map(|o| o["message"].as_str())
        .collect()
}

/// `Layout/IndentationWidth: Width: 4` set in `.murphy.yml` reaches the cop
/// through the real load path: first arg indented 4, `)` at column 4 -> the
/// line-break branch expects `4 - 4 = 0`.
#[test]
fn closing_paren_honors_configured_layout_indentation_width() {
    let offenses = lint_json_with_config(
        "Layout/IndentationWidth:\n  Width: 4\n",
        "some_method(\n    a\n    )\n",
    );
    assert_eq!(
        closing_paren_messages(&offenses),
        vec!["Indent `)` to column 0 (not 4)"],
        "configured Width 4 must flow through load() to the cop; got {offenses:?}",
    );
}

/// With no config the cop uses the default width 2: same source -> expected
/// `4 - 2 = 2`. Guards that the threading does not silently force the width to
/// 0 (uninitialised) or to a stale value.
#[test]
fn closing_paren_default_layout_indentation_width_is_two() {
    let offenses = lint_json_with_config("", "some_method(\n    a\n    )\n");
    assert_eq!(
        closing_paren_messages(&offenses),
        vec!["Indent `)` to column 2 (not 4)"],
        "default width 2 expected; got {offenses:?}",
    );
}
