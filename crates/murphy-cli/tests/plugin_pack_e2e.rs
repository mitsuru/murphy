//! E2E integration test for dynamic plugin pack loading (murphy-9cr.10.1).
//!
//! Loads `murphy-example-pack` via the `[[plugins]]` config + dlopen path
//! and asserts that `Example/NoEval` and `Example/TodoFormat` fire on a
//! fixture .rb file.
//!
//! Windows は plugin pack 非対応 (`registry.rs` Windows guard)。ファイル全体を
//! `cfg(not(target_os = "windows"))` で gating する。

#![cfg(not(target_os = "windows"))]

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

/// Resolve the cdylib artifact path. Cargo's dep graph (murphy-cli's
/// `[dev-dependencies]` → `murphy-example-pack`) guarantees the cdylib is
/// built before this test runs; this helper just locates it.
///
/// 2-tier 解決: `CARGO_TARGET_DIR` env → `${CARGO_MANIFEST_DIR}/../../target`。
/// `.cargo/config.toml` で target-dir を override しない workspace 規約に依存。
/// 加えて (a) `debug` profile 決め打ち (`cargo test --release` は未対応)、
/// (b) host triple 仮定 (`--target=<triple>` のクロスビルドだと artifact は
/// `target/<triple>/debug/` に移動するため未対応)。両条件は CI と通常開発
/// では成立しており、必要になれば env (`MURPHY_TEST_PROFILE` 等) で拡張。
///
/// 探索順:
/// 1. `<target>/debug/lib<name>.{so,dylib}` (Cargo の cdylib 標準配置)
/// 2. `<target>/debug/deps/lib<name>-<hash>.{so,dylib}` (一部 Cargo バージョン /
///    ビルド経路で intermediate が deps/ にだけ生成されるケースの fallback)
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
    // Fallback: search target/debug/deps/lib<name>-<hash>.{so,dylib}
    let deps = target_dir.join("debug").join("deps");
    if let Ok(entries) = std::fs::read_dir(&deps) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(prefix)
                && name.ends_with(&format!(".{ext}"))
                && name.as_bytes().get(prefix.len()) == Some(&b'-')
            {
                return entry.path();
            }
        }
    }
    // どちらも見つからなければ規約パスを返す (assert で具体 path 付き失敗メッセージへ)
    top
}

#[test]
fn detailed_form_loads_example_pack_and_emits_offenses() {
    let pack = example_pack_path()
        .canonicalize()
        .expect("example-pack artifact should exist (Cargo dep graph)");

    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("sample.rb");
    fs::write(
        &rb,
        "# frozen_string_literal: true\n# TODO: implement this\neval(\"x\")\n",
    )
    .expect("write rb");

    let toml = format!(
        "[[plugins]]\nname = \"murphy-example-pack\"\npath = {:?}\n",
        pack.display().to_string()
    );
    fs::write(dir.path().join("murphy.toml"), toml).expect("write toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&rb)
        .assert()
        .code(1); // offenses found

    let stdout = &assert.get_output().stdout;
    let offenses: Vec<serde_json::Value> = serde_json::from_slice(stdout).expect("stdout JSON");
    let names: Vec<String> = offenses
        .iter()
        .filter_map(|o| o["cop_name"].as_str().map(str::to_string))
        .collect();
    assert!(
        names.contains(&"Example/NoEval".to_string()),
        "expected Example/NoEval in {names:?}"
    );
    assert!(
        names.contains(&"Example/TodoFormat".to_string()),
        "expected Example/TodoFormat in {names:?}"
    );
}

#[test]
fn detailed_form_missing_path_exits_2_with_diagnostic() {
    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("sample.rb");
    fs::write(&rb, "puts 'hi'\n").expect("write rb");

    let toml = "[[plugins]]\nname = \"nonexistent\"\npath = \"./does-not-exist.so\"\n";
    fs::write(dir.path().join("murphy.toml"), toml).expect("write toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg(&rb)
        .assert()
        .code(2);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("cannot load plugin"),
        "stderr should mention plugin load failure: {stderr}"
    );
}

/// Stage `pack` under `dir/<filename>` as a symlink. Murphy's `lib_filename`
/// gives the exact name the resolver will look for (hyphen-to-underscore +
/// platform extension).
fn stage_pack_symlink(
    dir: &std::path::Path,
    pack: &std::path::Path,
    name: &str,
) -> std::path::PathBuf {
    let staged = dir.join(murphy_core::plugin_resolver::lib_filename(name));
    std::os::unix::fs::symlink(pack, &staged).expect("symlink staging");
    staged
}

#[test]
fn name_form_resolves_via_murphy_plugin_path_env() {
    // `plugins = ["murphy-example-pack"]` + `MURPHY_PLUGIN_PATH=<dir>`
    // proves the resolver follows env-listed search dirs, AND that the
    // Cargo cdylib hyphen→underscore convention is applied (the artifact
    // on disk is `libmurphy_example_pack.so`, not `libmurphy-example-pack.so`).
    let pack = example_pack_path()
        .canonicalize()
        .expect("example-pack artifact should exist");

    let dir = tempdir().expect("tempdir");
    let plugin_dir = dir.path().join("custom_plugins");
    fs::create_dir(&plugin_dir).expect("mkdir plugin_dir");
    stage_pack_symlink(&plugin_dir, &pack, "murphy-example-pack");

    let rb = dir.path().join("sample.rb");
    fs::write(&rb, "# TODO: x\neval(\"x\")\n").expect("write rb");
    fs::write(
        dir.path().join("murphy.toml"),
        "plugins = [\"murphy-example-pack\"]\n",
    )
    .expect("write toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env("MURPHY_PLUGIN_PATH", &plugin_dir)
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
        "expected Example/NoEval in {names:?}"
    );
}

#[test]
fn name_form_resolves_via_project_local_dot_murphy_plugins() {
    // `plugins = ["murphy-example-pack"]` + `<project>/.murphy/plugins/`
    // proves the project-local search dir is wired up. Env explicitly
    // cleared so we only hit the project-local path.
    let pack = example_pack_path()
        .canonicalize()
        .expect("example-pack artifact should exist");

    let dir = tempdir().expect("tempdir");
    let plugin_dir = dir.path().join(".murphy/plugins");
    fs::create_dir_all(&plugin_dir).expect("mkdir .murphy/plugins");
    stage_pack_symlink(&plugin_dir, &pack, "murphy-example-pack");

    let rb = dir.path().join("sample.rb");
    fs::write(&rb, "# TODO: x\n").expect("write rb");
    fs::write(
        dir.path().join("murphy.toml"),
        "plugins = [\"murphy-example-pack\"]\n",
    )
    .expect("write toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env_remove("MURPHY_PLUGIN_PATH")
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
        "expected Example/TodoFormat in {names:?}"
    );
}

#[test]
fn name_form_missing_exits_2_with_search_path_and_detailed_hint() {
    // No staging anywhere; env cleared. The error must echo the
    // searched dirs and point the user at the `[[plugins]]` detailed form.
    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("sample.rb");
    fs::write(&rb, "puts 'hi'\n").expect("write rb");
    fs::write(
        dir.path().join("murphy.toml"),
        "plugins = [\"murphy-not-installed\"]\n",
    )
    .expect("write toml");

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
        stderr.contains("murphy-not-installed"),
        "stderr must echo plugin name: {stderr}"
    );
    assert!(
        stderr.contains("not found") && stderr.contains("Searched"),
        "stderr must surface not-found + Searched: {stderr}"
    );
    assert!(
        stderr.contains("[[plugins]]") && stderr.contains("path"),
        "stderr must point user at the detailed form: {stderr}"
    );
}

#[test]
fn name_and_detailed_same_name_loads_once_via_detailed_path() {
    // Mixing `Name` and `Detailed` for the same plugin must not trigger
    // the registry's name-collision check (which would happen if the
    // resolver loaded the pack twice). The `Detailed` `path` wins.
    let pack = example_pack_path()
        .canonicalize()
        .expect("example-pack artifact should exist");

    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("sample.rb");
    fs::write(&rb, "# TODO: x\n").expect("write rb");
    let toml = format!(
        "plugins = [\n  \"murphy-example-pack\",\n  {{ name = \"murphy-example-pack\", path = {:?} }}\n]\n",
        pack.display().to_string()
    );
    fs::write(dir.path().join("murphy.toml"), toml).expect("write toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .env_remove("MURPHY_PLUGIN_PATH")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&rb)
        .assert()
        .code(1); // offense found, NOT setup-error (would be code 2)
    let stdout = &assert.get_output().stdout;
    let offenses: Vec<serde_json::Value> = serde_json::from_slice(stdout).expect("stdout JSON");
    let names: Vec<String> = offenses
        .iter()
        .filter_map(|o| o["cop_name"].as_str().map(str::to_string))
        .collect();
    assert!(
        names.contains(&"Example/TodoFormat".to_string()),
        "expected Example/TodoFormat in {names:?}"
    );
}
