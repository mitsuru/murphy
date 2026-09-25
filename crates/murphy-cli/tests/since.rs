//! End-to-end tests for `murphy lint --since <ref>` (Phase 9 B5).
//!
//! Diff-driven flow per `docs/guides/ci.md`: commit a clean tree, then only
//! files changed since the ref (modified + untracked) are linted. Plus error
//! cases (unknown ref / non-git directory → exit 2).

use assert_cmd::Command;
use std::fs;
use std::process::Command as StdCommand;
use tempfile::TempDir;

const CLEAN: &str = "# frozen_string_literal: true\n";
const DIRTY: &str = "# frozen_string_literal: true\n\ndebugger\n";

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("git must run");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

fn init_repo() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.email", "murphy@test.invalid"]);
    git(dir.path(), &["config", "user.name", "murphy test"]);
    fs::write(dir.path().join("clean.rb"), CLEAN).expect("write clean.rb");
    git(dir.path(), &["add", "clean.rb"]);
    git(dir.path(), &["commit", "-m", "initial"]);
    dir
}

fn lint_since_json(
    dir: &std::path::Path,
    git_ref: &str,
    extra: &[&str],
) -> (i32, Vec<serde_json::Value>, String) {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.arg("lint")
        .arg("--format")
        .arg("json")
        .arg("--since")
        .arg(git_ref);
    for a in extra {
        cmd.arg(a);
    }
    cmd.current_dir(dir);
    let assert = cmd.assert();
    let code = assert.get_output().status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    let parsed: Vec<serde_json::Value> = if code == 2 {
        Vec::new()
    } else {
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array")
    };
    (code, parsed, stderr)
}

fn files_of(parsed: &[serde_json::Value]) -> Vec<String> {
    // Discovery from `.` yields `./`-prefixed paths; strip for assertions.
    let mut files: Vec<String> = parsed
        .iter()
        .map(|o| {
            o["file"]
                .as_str()
                .unwrap_or("?")
                .trim_start_matches("./")
                .to_string()
        })
        .collect();
    files.sort();
    files.dedup();
    files
}

#[test]
fn since_lints_only_modified_and_untracked_files() {
    let dir = init_repo();
    // Modified tracked file + brand-new untracked file, both dirty.
    fs::write(dir.path().join("clean.rb"), DIRTY).expect("dirty clean.rb");
    fs::write(dir.path().join("new.rb"), DIRTY).expect("write new.rb");
    // Untracked but clean: must not surface, and must not break the run.
    fs::write(dir.path().join("other.rb"), CLEAN).expect("write other.rb");

    let (code, parsed, _) = lint_since_json(dir.path(), "HEAD", &[]);
    assert_eq!(code, 1);
    assert_eq!(files_of(&parsed), vec!["clean.rb", "new.rb"]);
    assert!(parsed.iter().all(|o| o["cop_name"] == "Lint/Debugger"));
}

#[test]
fn since_with_no_changes_is_green() {
    let dir = init_repo();
    let (code, parsed, _) = lint_since_json(dir.path(), "HEAD", &[]);
    assert_eq!(code, 0, "unchanged tree must exit 0");
    assert!(parsed.is_empty());
}

#[test]
fn since_intersects_with_explicit_paths() {
    let dir = init_repo();
    fs::write(dir.path().join("clean.rb"), DIRTY).expect("dirty clean.rb");
    // Explicitly naming an unchanged-at-HEAD file yields the empty set.
    fs::write(dir.path().join("steady.rb"), CLEAN).expect("write steady.rb");
    git(dir.path(), &["add", "steady.rb"]);
    git(dir.path(), &["commit", "-m", "steady"]);

    let (code, parsed, _) = lint_since_json(dir.path(), "HEAD", &["steady.rb"]);
    assert_eq!(code, 0, "unchanged explicit path must be skipped");
    assert!(parsed.is_empty());
}

#[test]
fn since_unknown_ref_exits_2() {
    let dir = init_repo();
    let (code, _, stderr) = lint_since_json(dir.path(), "no-such-ref-xyz", &[]);
    assert_eq!(code, 2);
    assert!(
        stderr.contains("unknown git ref"),
        "stderr must guide the fix, got: {stderr}"
    );
}

#[test]
fn since_outside_git_exits_2() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("a.rb"), DIRTY).expect("write a.rb");
    let (code, _, stderr) = lint_since_json(dir.path(), "HEAD", &[]);
    assert_eq!(code, 2);
    assert!(
        stderr.contains("not a git repository"),
        "stderr must guide the fix, got: {stderr}"
    );
}

#[test]
fn since_sees_committed_changes_on_top_of_ref() {
    let dir = init_repo();
    // A new commit on top of HEAD~1: diffing against HEAD~1 must see it.
    fs::write(dir.path().join("feature.rb"), DIRTY).expect("write feature.rb");
    git(dir.path(), &["add", "feature.rb"]);
    git(dir.path(), &["commit", "-m", "feature"]);

    let (code, parsed, _) = lint_since_json(dir.path(), "HEAD~1", &[]);
    assert_eq!(code, 1);
    assert_eq!(files_of(&parsed), vec!["feature.rb"]);
}
