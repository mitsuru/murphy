//! E2E for C2 Gem / Bundler pack discovery (murphy-fmw.3.2; ADR 0048).
//!
//! A `murphy-*` cop pack shipped as an installed gem
//! (`<gems>/<name>-<version>/murphy-plugin.toml` + `lib/<arch>/*.so`)
//! resolves from `plugins = ["<name>"]` when the gem env
//! (`MURPHY_GEM_PATH`, or Bundler's `GEM_HOME`/`GEM_PATH`) points at it.
//! The pack artifact reused here is `murphy-example-pack`'s cdylib,
//! staged into a fake gem layout — the lint pipeline (`dlopen` + dispatch)
//! is identical to a real `gem install murphy-example-pack`.

#![cfg(not(target_os = "windows"))]

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn example_pack_path() -> std::path::PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target")
        });
    let (prefix, ext) = if cfg!(target_os = "macos") {
        ("libmurphy_example_pack", "dylib")
    } else {
        ("libmurphy_example_pack", "so")
    };
    let top = target_dir.join("debug").join(format!("{prefix}.{ext}"));
    if top.exists() {
        return top;
    }
    let deps = target_dir.join("debug").join("deps");
    let deps_no_hash = deps.join(format!("{prefix}.{ext}"));
    if deps_no_hash.exists() {
        return deps_no_hash;
    }
    if let Ok(entries) = std::fs::read_dir(&deps) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(prefix)
                && name.ends_with(&format!(".{ext}"))
                && name.as_bytes().get(prefix.len()) == Some(&b'-')
            {
                return entry.path();
            }
        }
    }
    top
}

/// Stage the example-pack cdylib as gem `murphy-example-pack-0.1.0` under
/// `gems_dir`: manifest at the gem root + binary under `lib/<arch>/`.
fn stage_gem_pack(gems_dir: &std::path::Path) -> std::path::PathBuf {
    let pack = example_pack_path()
        .canonicalize()
        .expect("example-pack artifact should exist (Cargo dep graph)");
    let gem_dir = gems_dir.join("murphy-example-pack-0.1.0");
    let arch_dir = gem_dir.join("lib").join("linux-x86_64");
    fs::create_dir_all(&arch_dir).expect("mkdir gem lib/<arch>");
    fs::write(
        gem_dir.join("murphy-plugin.toml"),
        "[plugin]\nname = \"murphy-example-pack\"\nversion = \"0.1.0\"\nmurphy-api-version = 4\n",
    )
    .expect("write manifest");
    let dest = arch_dir.join(if cfg!(target_os = "macos") {
        "libmurphy_example_pack.dylib"
    } else {
        "libmurphy_example_pack.so"
    });
    fs::copy(&pack, &dest).expect("copy cdylib into gem");
    dest
}

#[test]
fn name_form_resolves_pack_from_murphy_gem_path() {
    let gems = tempdir().expect("gems");
    stage_gem_pack(gems.path());

    let dir = tempdir().expect("project");
    let rb = dir.path().join("sample.rb");
    fs::write(&rb, "# TODO: x\neval(\"x\")\n").expect("write rb");
    fs::write(
        dir.path().join(".murphy.yml"),
        "plugins:\n  - murphy-example-pack\n",
    )
    .expect("write yml");
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env("MURPHY_GEM_PATH", gems.path())
        .env_remove("MURPHY_PLUGIN_PATH")
        .env_remove("GEM_HOME")
        .env_remove("GEM_PATH")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&rb)
        .assert()
        .code(1);
    let stdout = &assert.get_output().stdout;
    let offenses: Vec<serde_json::Value> = serde_json::from_slice(stdout).expect("stdout JSON");
    let names: Vec<String> = offenses
        .iter()
        .filter_map(|o| o["cop_name"].as_str().map(str::to_string))
        .collect();
    assert!(
        names.contains(&"Example/NoEval".to_string()),
        "gem-resolved pack must emit Example/NoEval: {names:?}"
    );
}

#[test]
fn name_form_resolves_pack_from_gem_home_gems_subdir() {
    // Bundler shape: `GEM_HOME=<bundle>` holding `gems/`. Covers the
    // `bundle exec murphy lint` path where Bundler — not MURPHY_GEM_PATH —
    // supplies the gem location.
    let home = tempdir().expect("gem home");
    let gems = home.path().join("gems");
    fs::create_dir_all(&gems).expect("mkdir gems");
    stage_gem_pack(&gems);

    let dir = tempdir().expect("project");
    let rb = dir.path().join("sample.rb");
    fs::write(&rb, "# TODO: x\n").expect("write rb");
    fs::write(
        dir.path().join(".murphy.yml"),
        "plugins:\n  - murphy-example-pack\n",
    )
    .expect("write yml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env_remove("MURPHY_PLUGIN_PATH")
        .env_remove("MURPHY_GEM_PATH")
        .env("GEM_HOME", home.path())
        .env_remove("GEM_PATH")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&rb)
        .assert()
        .code(1);
    let stdout = &assert.get_output().stdout;
    let offenses: Vec<serde_json::Value> = serde_json::from_slice(stdout).expect("stdout JSON");
    let names: Vec<String> = offenses
        .iter()
        .filter_map(|o| o["cop_name"].as_str().map(str::to_string))
        .collect();
    assert!(
        names.contains(&"Example/TodoFormat".to_string()),
        "GEM_HOME-resolved pack must emit Example/TodoFormat: {names:?}"
    );
}

#[test]
fn project_local_shadows_gem_pack() {
    // Both a project-local legacy `.so` and a gem pack exist; the staged
    // file must win (local rebuild shadows the released gem).
    let gems = tempdir().expect("gems");
    stage_gem_pack(gems.path());
    let pack = example_pack_path()
        .canonicalize()
        .expect("example-pack artifact should exist");

    let dir = tempdir().expect("project");
    let staged_dir = dir.path().join(".murphy/plugins");
    fs::create_dir_all(&staged_dir).expect("mkdir staged");
    let staged = staged_dir.join(murphy_core::plugin_resolver::lib_filename(
        "murphy-example-pack",
    ));
    #[cfg(unix)]
    std::os::unix::fs::symlink(&pack, &staged).expect("symlink staged .so");
    #[cfg(not(unix))]
    fs::copy(&pack, &staged).expect("copy staged .so");

    let rb = dir.path().join("sample.rb");
    fs::write(&rb, "# TODO: x\n").expect("write rb");
    fs::write(
        dir.path().join(".murphy.yml"),
        "plugins:\n  - murphy-example-pack\n",
    )
    .expect("write yml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env("MURPHY_GEM_PATH", gems.path())
        .env_remove("MURPHY_PLUGIN_PATH")
        .env_remove("GEM_HOME")
        .env_remove("GEM_PATH")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&rb)
        .assert()
        .code(1);
    // Resolves (no exit 2) — priority is asserted at unit level in
    // `plugin_resolver::tests::resolve_prefers_project_local_over_gem_pack`;
    // here we only prove the combined layout still lints.
    let stdout = &assert.get_output().stdout;
    let offenses: Vec<serde_json::Value> = serde_json::from_slice(stdout).expect("stdout JSON");
    assert!(!offenses.is_empty(), "expected offenses, got none");
}
