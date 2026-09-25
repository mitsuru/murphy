//! E2E for static-index remote discovery (murphy-uk7.2; ADR 0053).
//!
//! Covers `plugins search`, `plugins fetch`, `plugins publish`, and the
//! `plugins install` remote fallback over `file://` sources (no network).
//! Pack fixtures are fake dirs (manifest + fake cdylib bytes — verify
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

fn write_mirror(dir: &std::path::Path, source_url: &str) -> std::path::PathBuf {
    let path = dir.join("mirror.toml");
    fs::write(
        &path,
        format!(
            "[registry]\nversion = 1\n\n\
             [[packs]]\nname = \"murphy-foo\"\ngem = \"murphy-foo\"\n\
             version = \"0.1.0\"\ndescription = \"Foo cops\"\n\
             homepage = \"https://example.invalid\"\n\
             murphy-api-version = 4\nmin-murphy-version = \"0.1.0\"\n\
             source-url = \"{source_url}\"\n"
        ),
    )
    .expect("write mirror");
    path
}

#[test]
fn search_lists_mirror_pack() {
    let src_parent = tempdir().expect("src");
    make_pack(src_parent.path(), "murphy-foo");
    let reg_dir = tempdir().expect("regdir");
    let url = format!("file://{}", src_parent.path().display());
    let mirror = write_mirror(reg_dir.path(), &url);
    let project = tempdir().expect("project");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .arg("plugins")
        .arg("search")
        .arg("foo")
        .arg("--registry")
        .arg(&mirror)
        .assert()
        .success();
    // Re-run for output inspection (assert_cmd consumes the command).
    let out = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .arg("plugins")
        .arg("search")
        .arg("foo")
        .arg("--registry")
        .arg(&mirror)
        .output()
        .expect("run search");
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("murphy-foo"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn fetch_materializes_file_source() {
    let src_parent = tempdir().expect("src");
    make_pack(src_parent.path(), "murphy-foo");
    let reg_dir = tempdir().expect("regdir");
    let url = format!("file://{}", src_parent.path().display());
    let mirror = write_mirror(reg_dir.path(), &url);
    let project = tempdir().expect("project");
    let dest_parent = tempdir().expect("dest");
    let to = dest_parent.path().join("fetched");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .arg("plugins")
        .arg("fetch")
        .arg("murphy-foo")
        .arg("--registry")
        .arg(&mirror)
        .arg("--to")
        .arg(&to)
        .assert()
        .success();

    assert!(
        to.join(murphy_core::plugin_manifest::MANIFEST_FILENAME)
            .is_file()
    );
}

#[test]
fn install_falls_back_to_remote_cache() {
    let src_parent = tempdir().expect("src");
    make_pack(src_parent.path(), "murphy-foo");
    let reg_dir = tempdir().expect("regdir");
    let url = format!("file://{}", src_parent.path().display());
    let mirror = write_mirror(reg_dir.path(), &url);
    let user = tempdir().expect("user");
    let cache = tempdir().expect("cache");
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
        .arg("--registry")
        .arg(&mirror)
        .arg("--cache-dir")
        .arg(cache.path())
        .assert()
        .success();

    assert!(user.path().join("murphy-foo").is_dir());
    assert!(cache.path().join("murphy-foo").is_dir());
}

#[test]
fn publish_validates_and_prints_snippet() {
    let src_parent = tempdir().expect("src");
    let pack = make_pack(src_parent.path(), "murphy-foo");
    let project = tempdir().expect("project");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .arg("plugins")
        .arg("publish")
        .arg("--from")
        .arg(&pack)
        .assert()
        .success();
    let out = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .arg("plugins")
        .arg("publish")
        .arg("--from")
        .arg(&pack)
        .output()
        .expect("run publish");
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("[[packs]]"),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}
