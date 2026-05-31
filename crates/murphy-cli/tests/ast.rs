//! Integration tests for `murphy ast --format sexp <path|->`.
//!
//! The printer itself is unit-tested in `murphy-ast` and the
//! `murphy-translate` golden snapshots. These tests exercise the *binary*:
//! flag parsing, file vs stdin input, exit codes, parse-error mapping.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

/// `ast --format sexp <file>` prints S-expression text and exits 0.
#[test]
fn ast_sexp_file_exits_0_with_sexp_stdout() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("expr.rb");
    fs::write(&path, "x = 1 + 2\n").expect("write expr.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg(&path)
        .assert()
        .code(0);

    let stdout =
        String::from_utf8(assert.get_output().stdout.clone()).expect("stdout must be utf-8");
    assert_eq!(
        stdout,
        "(lvasgn x\n  (send :+\n    (int 1)\n    (int 2)))\n",
    );
}

/// `ast --format sexp -` reads from stdin and prints S-expression text.
#[test]
fn ast_sexp_stdin_exits_0_with_sexp_stdout() {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg("-")
        .write_stdin("nil\n")
        .assert()
        .code(0);

    let stdout =
        String::from_utf8(assert.get_output().stdout.clone()).expect("stdout must be utf-8");
    assert_eq!(stdout, "(nil)\n");
}

/// A syntax error in the source exits 1 (consistent with the lint path's
/// `Murphy/Syntax` finding) and emits a diagnostic to stderr.
#[test]
fn ast_sexp_parse_error_exits_1() {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg("-")
        .write_stdin("def foo(\n")
        .assert()
        .code(1);

    // stderr carries the diagnostic; stdout must be empty on error.
    assert!(
        assert.get_output().stdout.is_empty(),
        "no S-expression on parse error"
    );
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("murphy:"),
        "stderr must carry a `murphy:` diagnostic, got {stderr:?}"
    );
}

/// A missing file is a setup error (exit 2), not a parse error.
#[test]
fn ast_sexp_missing_file_exits_2() {
    let dir = tempdir().expect("create tempdir");
    let missing = dir.path().join("does-not-exist.rb");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("sexp")
        .arg(&missing)
        .assert()
        .code(2);
}

/// Bad usage (missing `--format sexp`) is a setup error (exit 2).
#[test]
fn ast_bad_usage_exits_2() {
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .assert()
        .code(2);

    // Unsupported format value.
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("ast")
        .arg("--format")
        .arg("json")
        .arg("/tmp/whatever.rb")
        .assert()
        .code(2);
}

/// Re-running over identical source yields identical output (the
/// determinism flavor of "round-trip"): no hidden iteration-order
/// dependency in the printer or arena.
#[test]
fn ast_sexp_is_deterministic_across_runs() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("mix.rb");
    fs::write(
        &path,
        "class Foo < Bar\n  def baz(a, *b)\n    [a, *b].each { |n| puts n }\n  end\nend\n",
    )
    .expect("write mix.rb");

    let run = || {
        let assert = Command::cargo_bin("murphy")
            .expect("murphy binary builds")
            .arg("ast")
            .arg("--format")
            .arg("sexp")
            .arg(&path)
            .assert()
            .code(0);
        String::from_utf8(assert.get_output().stdout.clone()).expect("stdout must be utf-8")
    };

    let first = run();
    let second = run();
    assert_eq!(first, second, "output must be byte-identical across runs");
    // And the output must have at least one form (smoke).
    assert!(first.contains("(class"), "unexpected output: {first}");
}

#[test]
fn ast_sexp_exposes_useless_assignment_parity_shapes() {
    for (src, expected) in [
        ("for item in items\nend\n", "(for\n"),
        ("case value\nin {name: name}\nend\n", "(match_var :name)"),
        ("/(?<name>foo)/ =~ value\n", "(send :=~"),
    ] {
        let assert = Command::cargo_bin("murphy")
            .expect("murphy binary builds")
            .arg("ast")
            .arg("--format")
            .arg("sexp")
            .arg("-")
            .write_stdin(src)
            .assert()
            .code(0);

        let stdout =
            String::from_utf8(assert.get_output().stdout.clone()).expect("stdout must be utf-8");
        assert!(
            !stdout.starts_with("(unknown)"),
            "root must not be unknown for {src:?}: {stdout}"
        );
        assert!(
            stdout.contains(expected),
            "expected {expected:?} in AST for {src:?}: {stdout}"
        );
    }
}
