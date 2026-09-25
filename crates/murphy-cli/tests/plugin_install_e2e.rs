//! E2E for `murphy plugins install <name>` (murphy-uk7.1).
//!
//! Resolves via the thin registry + gem discovery (or `--from`), copies
//! the pack into the user-local dir, and verifies the installed copy.
//! Uses fake pack dirs (manifest + fake cdylib bytes — no dlopen; verify
//! checks manifest + ABI + cdylib presence, not `dlopen`).

#![cfg(not(target_os = "windows"))]

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn manifest(name: &str) -> String {
    format!("[plugin]\nname = \"{name}\"\nversion = \"0.1.0\"\nmurphy-api-version = 4\n")
}

fn make_pack(parent: &std::path::Path, name: &str) -> std::path::PathBuf {
    let pack = parent.join(name);
    fs::create_dir_all(&pack).expect("mkdir pack");
    fs::write(
        pack.join(murphy_core::plugin_manifest::MANIFEST_FILENAME),
        manifest(name),
    )
    .expect("write manifest");
    let arch = pack.join("lib").join("linux-x86_64");
    fs::create_dir_all(&arch).expect("mkdir lib/<arch>");
    let ext = if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    fs::write(arch.join(format!("libfake.{ext}")), b"fake-cdylib").expect("cdylib");
    pack
}

#[test]
fn install_from_dir_to_user_dir_and_verify() {
    let src_parent = tempdir().expect("src");
    make_pack(src_parent.path(), "murphy-foo");
    let user = tempdir().expect("user");
    let project = tempdir().expect("project");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .env(
            murphy_core::plugin_install::USER_PLUGINS_DIR_ENV,
            user.path(),
        )
        .arg("plugins")
        .arg("install")
        .arg("murphy-foo")
        .arg("--from")
        .arg(src_parent.path())
        .assert()
        .success();

    let dest = user.path().join("murphy-foo");
    assert!(dest.is_dir(), "pack dir must be installed");
    assert!(
        dest.join(murphy_core::plugin_manifest::MANIFEST_FILENAME)
            .is_file()
    );
    // Resolver visibility: same probe the loader uses.
    let cdylib =
        murphy_core::plugin_install::verify_installed("murphy-foo", user.path()).expect("verify");
    assert!(cdylib.is_file());
}

#[test]
fn plugin_singular_alias_installs() {
    let src_parent = tempdir().expect("src");
    make_pack(src_parent.path(), "murphy-foo");
    let user = tempdir().expect("user");
    let project = tempdir().expect("project");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .env(
            murphy_core::plugin_install::USER_PLUGINS_DIR_ENV,
            user.path(),
        )
        .arg("plugin")
        .arg("install")
        .arg("murphy-foo")
        .arg("--from")
        .arg(src_parent.path())
        .assert()
        .success();

    assert!(user.path().join("murphy-foo").is_dir());
}

#[test]
fn dry_run_does_not_write_and_second_needs_force() {
    let src_parent = tempdir().expect("src");
    make_pack(src_parent.path(), "murphy-foo");
    let user = tempdir().expect("user");
    let project = tempdir().expect("project");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .env(
            murphy_core::plugin_install::USER_PLUGINS_DIR_ENV,
            user.path(),
        )
        .arg("plugins")
        .arg("install")
        .arg("murphy-foo")
        .arg("--from")
        .arg(src_parent.path())
        .arg("--dry-run")
        .assert()
        .success();
    assert!(
        !user.path().join("murphy-foo").exists(),
        "dry run must not write"
    );

    // Real install, then reinstall without --force fails.
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .env(
            murphy_core::plugin_install::USER_PLUGINS_DIR_ENV,
            user.path(),
        )
        .arg("plugins")
        .arg("install")
        .arg("murphy-foo")
        .arg("--from")
        .arg(src_parent.path())
        .assert()
        .success();
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .env(
            murphy_core::plugin_install::USER_PLUGINS_DIR_ENV,
            user.path(),
        )
        .arg("plugins")
        .arg("install")
        .arg("murphy-foo")
        .arg("--from")
        .arg(src_parent.path())
        .assert()
        .code(2);
}
