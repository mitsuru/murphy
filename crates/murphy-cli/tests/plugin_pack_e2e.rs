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
fn example_pack_path() -> std::path::PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target")
        });
    let lib_name = if cfg!(target_os = "macos") {
        "libmurphy_example_pack.dylib"
    } else {
        "libmurphy_example_pack.so"
    };
    target_dir.join("debug").join(lib_name)
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
    let offenses: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout JSON");
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
