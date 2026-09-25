//! E2E for `murphy plugins sync` (murphy-ghxy).
//!
//! A rebuilt pack `.so` is not auto-propagated to `.murphy/plugins/`; the
//! sync subcommand is the tooled refresh path, and lint warns on stale.
//! Uses fake `.so` bytes (no dlopen) for sync tests, plus one lint-warning
//! test that also uses fake bytes (the warning runs before `dlopen`, so a
//! non-ELF staged file still triggers it — lint then exits 2 on the load
//! error, which the test accepts).

#![cfg(not(target_os = "windows"))]

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn lib_name() -> String {
    murphy_core::plugin_resolver::lib_filename("murphy-rails")
}

fn write(path: &std::path::Path, bytes: &[u8]) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).expect("mkdir parents");
    }
    fs::write(path, bytes).expect("write file");
}

#[test]
fn sync_copies_fresher_build_into_dot_murphy_plugins() {
    let dir = tempdir().expect("tempdir");
    let staged = dir.path().join(".murphy/plugins").join(lib_name());
    write(&staged, b"old-staged-bytes");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let from_dir = dir.path().join("build-out");
    let fresh = from_dir.join(lib_name());
    write(&fresh, b"fresh-build-bytes-different");

    write(
        &dir.path().join(".murphy.yml"),
        b"plugins:\n  - murphy-rails\n",
    );

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env_remove("MURPHY_PLUGIN_PATH")
        .arg("plugins")
        .arg("sync")
        .arg("--from")
        .arg(&from_dir)
        .assert()
        .success();

    assert_eq!(
        fs::read(&staged).expect("read staged"),
        b"fresh-build-bytes-different",
        "sync must copy fresh over staged"
    );
}

#[test]
fn sync_check_exits_2_when_stale_and_0_when_fresh() {
    let dir = tempdir().expect("tempdir");
    let staged = dir.path().join(".murphy/plugins").join(lib_name());
    write(&staged, b"old");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let from_dir = dir.path().join("build-out");
    write(&from_dir.join(lib_name()), b"fresh-different");
    write(
        &dir.path().join(".murphy.yml"),
        b"plugins:\n  - murphy-rails\n",
    );

    // --check: stale → exit 2, no copy.
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env_remove("MURPHY_PLUGIN_PATH")
        .arg("plugins")
        .arg("sync")
        .arg("--from")
        .arg(&from_dir)
        .arg("--check")
        .assert()
        .code(2);
    assert_eq!(fs::read(&staged).unwrap(), b"old", "--check must not copy");

    // Real sync, then --check is clean.
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env_remove("MURPHY_PLUGIN_PATH")
        .arg("plugins")
        .arg("sync")
        .arg("--from")
        .arg(&from_dir)
        .assert()
        .success();
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env_remove("MURPHY_PLUGIN_PATH")
        .arg("plugins")
        .arg("sync")
        .arg("--from")
        .arg(&from_dir)
        .arg("--check")
        .assert()
        .success();
}

#[test]
fn sync_installs_missing_staged_file() {
    // No `.murphy/plugins/` copy yet, but a build exists in --from:
    // sync installs it.
    let dir = tempdir().expect("tempdir");
    let from_dir = dir.path().join("build-out");
    write(&from_dir.join(lib_name()), b"fresh-bytes");
    write(
        &dir.path().join(".murphy.yml"),
        b"plugins:\n  - murphy-rails\n",
    );

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env_remove("MURPHY_PLUGIN_PATH")
        .arg("plugins")
        .arg("sync")
        .arg("--from")
        .arg(&from_dir)
        .assert()
        .success();

    let staged = dir.path().join(".murphy/plugins").join(lib_name());
    assert_eq!(fs::read(&staged).unwrap(), b"fresh-bytes");
}

#[test]
fn lint_warns_on_stale_staged_pack() {
    // Fake (non-ELF) staged + fresher fake build under
    // `<project>/target/release/` (a sync candidate dir, but NOT part of
    // the resolver search path, so the project-local staged copy still
    // loads). The pre-dlopen stale warning must appear on stderr. Lint
    // then fails to dlopen (exit 2), which the test accepts — it asserts
    // the warning, not the lint result.
    let dir = tempdir().expect("tempdir");
    let staged = dir.path().join(".murphy/plugins").join(lib_name());
    write(&staged, b"old-fake-so");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let fresh = dir.path().join("target/release").join(lib_name());
    write(&fresh, b"fresh-fake-so-different");
    write(
        &dir.path().join(".murphy.yml"),
        b"plugins:\n  - murphy-rails\n",
    );
    let rb = dir.path().join("sample.rb");
    write(&rb, b"puts 'hi'\n");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env_remove("MURPHY_PLUGIN_PATH")
        .arg("lint")
        .arg(&rb)
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("is older than") && stderr.contains("plugins sync"),
        "lint must warn on stale staged pack: {stderr}"
    );
}
