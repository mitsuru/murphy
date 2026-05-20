use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

#[test]
fn ast_sexp_reads_stdin() {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    let assert = cmd
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg("-")
        .write_stdin("x == nil")
        .assert()
        .code(0);

    assert_eq!(
        assert.get_output().stdout,
        b"s(:send, s(:lvar, :x), :==, s(:nil))\n"
    );
    assert!(assert.get_output().stderr.is_empty());
}

#[test]
fn ast_sexp_reads_file() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("sample.rb");
    fs::write(&path, "nil == x").expect("write sample");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg(&path)
        .assert()
        .code(0);

    assert_eq!(
        assert.get_output().stdout,
        b"s(:send, s(:nil), :==, s(:lvar, :x))\n"
    );
    assert!(assert.get_output().stderr.is_empty());
}

#[test]
fn ast_unknown_format_exits_2_with_empty_stdout() {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("json")
        .arg("-")
        .write_stdin("x == nil")
        .assert()
        .code(2);

    assert!(assert.get_output().stdout.is_empty());
}

#[test]
fn ast_missing_file_exits_2_with_empty_stdout() {
    let dir = tempdir().expect("create tempdir");
    let missing = dir.path().join("missing.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg(&missing)
        .assert()
        .code(2);

    assert!(assert.get_output().stdout.is_empty());
}

#[test]
fn ast_parse_error_exits_1_with_empty_stdout() {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg("-")
        .write_stdin("def (\n")
        .assert()
        .code(1);

    assert!(assert.get_output().stdout.is_empty());
}
