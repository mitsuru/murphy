//! E2E for `murphy add <pack>` thin registry install (C1; ADR 0049).
//!
//! The registry is a static catalogue over RubyGems (C2; ADR 0048):
//! `murphy add` resolves the name, checks compat (ABI + min core),
//! appends `plugins: - <name>` to `.murphy.yml`, and prints the Bundler
//! next step. It never shells out to `bundle`/`gem` and never touches
//! the network.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn write_registry(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("registry.toml");
    fs::write(
        &path,
        "[registry]\nversion = 1\n\n\
         [[packs]]\nname = \"murphy-rails\"\ngem = \"murphy-rails\"\n\
         version = \"0.1.0\"\ndescription = \"Rails cops\"\n\
         homepage = \"https://example.invalid\"\n\
         murphy-api-version = 4\nmin-murphy-version = \"0.1.0\"\n",
    )
    .expect("write registry");
    path
}

#[test]
fn add_creates_murphy_yml_and_prints_bundle_hint() {
    let reg_dir = tempdir().expect("regdir");
    let reg = write_registry(reg_dir.path());
    let project = tempdir().expect("project");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .arg("add")
        .arg("murphy-rails")
        .arg("--registry")
        .arg(&reg)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        stdout.contains("murphy-rails"),
        "stdout must mention pack: {stdout:?}"
    );
    assert!(
        stdout.contains("bundle"),
        "stdout must print Bundler next step: {stdout:?}"
    );

    let yml = fs::read_to_string(project.path().join(".murphy.yml")).expect("config written");
    assert!(yml.contains("murphy-rails"), "config:\n{yml}");
    // Parses through the real config loader.
    let cfg = murphy_core::MurphyConfig::from_yaml_str(&yml).expect("generated yml must parse");
    assert!(cfg.plugins.iter().any(|p| match p {
        murphy_core::PluginConfig::Name(n) => n == "murphy-rails",
        murphy_core::PluginConfig::Detailed(d) => d.name == "murphy-rails",
    }));
}

#[test]
fn add_is_idempotent_and_dry_run_writes_nothing() {
    let reg_dir = tempdir().expect("regdir");
    let reg = write_registry(reg_dir.path());
    let project = tempdir().expect("project");
    fs::write(
        project.path().join(".murphy.yml"),
        "plugins:\n  - murphy-rails\n",
    )
    .expect("seed config");

    // Re-adding is a no-op success.
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .arg("add")
        .arg("murphy-rails")
        .arg("--registry")
        .arg(&reg)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(stdout.contains("already"), "re-add is a no-op: {stdout:?}");

    // Dry run on a fresh project writes nothing.
    let fresh = tempdir().expect("fresh");
    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(fresh.path())
        .arg("add")
        .arg("murphy-rails")
        .arg("--registry")
        .arg(&reg)
        .arg("--dry-run")
        .assert()
        .success();
    assert!(
        !fresh.path().join(".murphy.yml").exists(),
        "dry run must not write"
    );
}

#[test]
fn add_unknown_pack_fails_with_available_list() {
    let reg_dir = tempdir().expect("regdir");
    let reg = write_registry(reg_dir.path());
    let project = tempdir().expect("project");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .arg("add")
        .arg("murphy-nope")
        .arg("--registry")
        .arg(&reg)
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("murphy-rails"),
        "error must list available packs: {stderr:?}"
    );
}

#[test]
fn add_rejects_incompatible_abi_before_config_edit() {
    let reg_dir = tempdir().expect("regdir");
    let bad = reg_dir.path().join("bad.toml");
    fs::write(
        &bad,
        "[registry]\nversion = 1\n\n\
         [[packs]]\nname = \"murphy-future\"\ngem = \"murphy-future\"\n\
         version = \"9.9.9\"\ndescription = \"x\"\nhomepage = \"https://example.invalid\"\n\
         murphy-api-version = 999\nmin-murphy-version = \"0.1.0\"\n",
    )
    .expect("write bad registry");
    let project = tempdir().expect("project");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(project.path())
        .arg("add")
        .arg("murphy-future")
        .arg("--registry")
        .arg(&bad)
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("ABI version"),
        "error must explain ABI mismatch: {stderr:?}"
    );
    assert!(
        !project.path().join(".murphy.yml").exists(),
        "incompat pack must not write config"
    );
}
