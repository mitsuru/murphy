//! B8 `murphy install --git-hook` integration tests (murphy-fmw.2.8).
//!
//! Phase 9 gate 7: `murphy install --git-hook` produces a working hook
//! scaffold for pre-commit / lefthook / overcommit configurations.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn install(dir: &std::path::Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.current_dir(dir);
    cmd.arg("install");
    for a in args {
        cmd.arg(a);
    }
    cmd.assert()
}

#[test]
fn install_git_hook_defaults_to_lefthook_yml() {
    let dir = tempdir().expect("tempdir");
    let assert = install(dir.path(), &["--git-hook"]).success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("lefthook.yml"), "got:\n{stdout}");

    let body = fs::read_to_string(dir.path().join("lefthook.yml")).expect("lefthook.yml written");
    assert!(body.contains("murphy lint"), "got:\n{body}");
    assert!(body.contains("{staged_files}"), "got:\n{body}");
    assert!(body.contains("*.rb"), "got:\n{body}");
}

#[test]
fn install_git_hook_pre_commit_writes_pre_commit_config() {
    let dir = tempdir().expect("tempdir");
    let assert = install(dir.path(), &["--git-hook", "--tool", "pre-commit"]).success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains(".pre-commit-config.yaml"), "got:\n{stdout}");

    let body = fs::read_to_string(dir.path().join(".pre-commit-config.yaml"))
        .expect("pre-commit config written");
    assert!(body.contains("repo: local"), "got:\n{body}");
    assert!(body.contains("murphy lint"), "got:\n{body}");
    assert!(body.contains("language: system"), "got:\n{body}");
}

#[test]
fn install_git_hook_overcommit_writes_overcommit_yml() {
    let dir = tempdir().expect("tempdir");
    let assert = install(dir.path(), &["--git-hook", "--tool", "overcommit"]).success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains(".overcommit.yml"), "got:\n{stdout}");

    let body =
        fs::read_to_string(dir.path().join(".overcommit.yml")).expect("overcommit.yml written");
    assert!(body.contains("PreCommit:"), "got:\n{body}");
    assert!(body.contains("murphy"), "got:\n{body}");
    assert!(body.contains("lint"), "got:\n{body}");
}

#[test]
fn install_git_hook_all_writes_all_three() {
    let dir = tempdir().expect("tempdir");
    install(dir.path(), &["--git-hook", "--tool", "all"]).success();

    for file in ["lefthook.yml", ".pre-commit-config.yaml", ".overcommit.yml"] {
        let body = fs::read_to_string(dir.path().join(file))
            .unwrap_or_else(|_| panic!("{file} written"));
        assert!(
            body.contains("murphy lint"),
            "{file} must run murphy lint, got:\n{body}"
        );
    }
}

#[test]
fn install_without_git_hook_flag_exits_2() {
    let dir = tempdir().expect("tempdir");
    install(dir.path(), &[]).code(2);
}

#[test]
fn install_refuses_to_clobber_without_force() {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("lefthook.yml"), "custom: true\n").expect("seed existing");
    install(dir.path(), &["--git-hook"]).code(2);
    // Existing file preserved.
    assert_eq!(
        fs::read_to_string(dir.path().join("lefthook.yml")).expect("read"),
        "custom: true\n"
    );
    // --force overwrites.
    install(dir.path(), &["--git-hook", "--force"]).success();
    let body = fs::read_to_string(dir.path().join("lefthook.yml")).expect("read");
    assert!(body.contains("murphy lint"), "got:\n{body}");
}

#[test]
fn install_all_is_atomic_without_force() {
    // With one of the three targets pre-existing, `--tool all` without
    // --force must fail before writing anything.
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join(".overcommit.yml"), "custom: true\n").expect("seed existing");
    install(dir.path(), &["--git-hook", "--tool", "all"]).code(2);
    assert!(
        !dir.path().join("lefthook.yml").exists(),
        "must not write partial scaffolds"
    );
    assert!(
        !dir.path().join(".pre-commit-config.yaml").exists(),
        "must not write partial scaffolds"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join(".overcommit.yml")).expect("read"),
        "custom: true\n"
    );
}
