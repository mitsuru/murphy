//! B7 `--format` CLI wiring: every new formatter is selectable and JSON stays frozen.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

const DIRTY: &str = "# frozen_string_literal: true\n\ndebugger\n";

fn lint_with_format(dir: &std::path::Path, format: &str) -> (Vec<u8>, Vec<u8>, i32) {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir)
        .arg("lint")
        .arg("--format")
        .arg(format)
        .arg("--no-cache")
        .arg("dirty.rb")
        .assert()
        .code(1);
    let out = assert.get_output();
    (
        out.stdout.clone(),
        out.stderr.clone(),
        out.status.code().unwrap_or(-1),
    )
}

fn setup_dirty() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("dirty.rb"), DIRTY).expect("write dirty.rb");
    dir
}

#[test]
fn cli_format_checkstyle_is_xml() {
    let dir = setup_dirty();
    let (stdout, _, code) = lint_with_format(dir.path(), "checkstyle");
    assert_eq!(code, 1);
    let s = String::from_utf8_lossy(&stdout);
    assert!(s.contains("<checkstyle>"), "got: {s}");
    assert!(s.contains("Lint/Debugger"), "got: {s}");
}

#[test]
fn cli_format_sarif_is_valid_sarif() {
    let dir = setup_dirty();
    let (stdout, _, code) = lint_with_format(dir.path(), "sarif");
    assert_eq!(code, 1);
    let v: serde_json::Value = serde_json::from_slice(&stdout).expect("sarif json");
    assert_eq!(v["version"], "2.1.0");
    assert_eq!(v["runs"][0]["results"][0]["ruleId"], "Lint/Debugger");
}

#[test]
fn cli_format_junit_is_xml() {
    let dir = setup_dirty();
    let (stdout, _, code) = lint_with_format(dir.path(), "junit");
    assert_eq!(code, 1);
    let s = String::from_utf8_lossy(&stdout);
    assert!(s.contains("<testsuite"), "got: {s}");
    assert!(s.contains("Lint/Debugger"), "got: {s}");
}

#[test]
fn cli_format_github_is_annotation() {
    let dir = setup_dirty();
    let (stdout, _, code) = lint_with_format(dir.path(), "github");
    assert_eq!(code, 1);
    let s = String::from_utf8_lossy(&stdout);
    assert!(s.contains("::warning file="), "got: {s}");
    assert!(s.contains("Lint/Debugger"), "got: {s}");
}

#[test]
fn cli_format_gnu_is_gnu_style() {
    let dir = setup_dirty();
    let (stdout, _, code) = lint_with_format(dir.path(), "gnu");
    assert_eq!(code, 1);
    let s = String::from_utf8_lossy(&stdout);
    assert!(s.contains("dirty.rb:"), "got: {s}");
    assert!(s.contains("[Lint/Debugger]"), "got: {s}");
}

#[test]
fn cli_format_tap_is_tap() {
    let dir = setup_dirty();
    let (stdout, _, code) = lint_with_format(dir.path(), "tap");
    assert_eq!(code, 1);
    let s = String::from_utf8_lossy(&stdout);
    assert!(s.starts_with("TAP version 13\n1.."), "got: {s}");
    assert!(s.contains("not ok 1 - "), "got: {s}");
}

#[test]
fn cli_format_html_is_report() {
    let dir = setup_dirty();
    let (stdout, _, code) = lint_with_format(dir.path(), "html");
    assert_eq!(code, 1);
    let s = String::from_utf8_lossy(&stdout);
    assert!(s.contains("<!DOCTYPE html>"), "got: {s}");
    assert!(s.contains("Murphy report"), "got: {s}");
    assert!(s.contains("Lint/Debugger"), "got: {s}");
    // B4-enriched lint output links the cop to its docs URL.
    assert!(
        s.contains("https://murphy.dev/docs/cops/Lint/Debugger"),
        "got: {s}"
    );
}

#[test]
fn cli_format_markdown_is_report() {
    let dir = setup_dirty();
    let (stdout, _, code) = lint_with_format(dir.path(), "markdown");
    assert_eq!(code, 1);
    let s = String::from_utf8_lossy(&stdout);
    assert!(s.contains("# Murphy report"), "got: {s}");
    assert!(s.contains("Lint/Debugger"), "got: {s}");
    assert!(s.contains("| dirty.rb |"), "got: {s}");
}

#[test]
fn cli_format_json_stays_frozen() {
    let dir = setup_dirty();
    let (stdout, _, code) = lint_with_format(dir.path(), "json");
    assert_eq!(code, 1);
    let parsed: Vec<serde_json::Value> = serde_json::from_slice(&stdout).expect("json array");
    assert_eq!(parsed.len(), 1);
    let o = &parsed[0];
    assert_eq!(o["cop_name"], "Lint/Debugger");
    assert!(o.get("range").is_some());
    assert_eq!(o["severity"], "warning");
    // Frozen ADR 0006 core shape + B4 extend-only enrichment
    // (murphy-fmw.2.4): real lint output carries `documentation_url` /
    // `rationale` / `fix_example`; the five core keys keep their values.
    // Exactly these keys when no autocorrect.
    let mut keys: Vec<&str> = o.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "cop_name",
            "documentation_url",
            "file",
            "fix_example",
            "message",
            "range",
            "rationale",
            "severity"
        ]
    );
}
