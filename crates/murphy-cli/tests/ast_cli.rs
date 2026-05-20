use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn ast_sexp_reads_stdin() {
    let _ = tempdir;
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
