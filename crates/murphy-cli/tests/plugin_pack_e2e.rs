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

#[test]
fn name_only_form_exits_2_with_not_yet_implemented_hint() {
    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("sample.rb");
    fs::write(&rb, "puts 'hi'\n").expect("write rb");

    let toml = "plugins = [\"murphy-rails\"]\n";
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
        stderr.contains("name resolution is not yet implemented") && stderr.contains("9cr.10.2"),
        "stderr should mention not-yet-implemented + 10.2 hint: {stderr}"
    );
}
