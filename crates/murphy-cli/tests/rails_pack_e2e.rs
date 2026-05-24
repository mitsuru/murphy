//! E2E integration test for the `murphy-rails` dynamic plugin pack
//! (murphy-2ob §14a).
//!
//! Loads `murphy-rails` via `[[plugins]]` + dlopen and asserts:
//! 1. `murphy cops list --format=json` enumerates the 138 Rails stubs
//!    with `source_pack = "murphy-rails"`.
//! 2. `murphy lint` on a Rails-shaped fixture exits 0 — the stubs'
//!    no-op `check` bodies must never emit offenses, even when dispatch
//!    runs.
//!
//! The `status` field is intentionally **not** asserted here. It is
//! `"disabled: user config"` today (because `MurphyConfig::cop_enabled`
//! consults the `is_cop_disabled_by_default` hardcode fallback rather
//! than the registry's `DEFAULT_ENABLED`) and will become
//! `"disabled: arena migration"` once `murphy-bnd` lands. Pinning
//! either string here would couple this test to that follow-up.
//!
//! Windows is plugin-pack-unsupported (`registry.rs` Windows guard).

#![cfg(not(target_os = "windows"))]

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

/// Resolve the `murphy-rails` cdylib artifact, mirroring the resolution
/// logic in `plugin_pack_e2e::example_pack_path`. Cargo's dep graph
/// (murphy-cli's `[dev-dependencies]` → `murphy-rails`) guarantees the
/// cdylib is built before this test runs.
fn rails_pack_path() -> std::path::PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target")
        });
    let (prefix, ext) = if cfg!(target_os = "macos") {
        ("libmurphy_rails", "dylib")
    } else {
        ("libmurphy_rails", "so")
    };
    let top = target_dir.join("debug").join(format!("{prefix}.{ext}"));
    if top.exists() {
        return top;
    }
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
    top
}

#[test]
fn cops_list_enumerates_138_rails_stubs_under_murphy_rails_source_pack() {
    let pack = rails_pack_path()
        .canonicalize()
        .expect("murphy-rails artifact should exist (Cargo dep graph)");

    let dir = tempdir().expect("tempdir");
    let toml = format!(
        "[[plugins]]\nname = \"murphy-rails\"\npath = {:?}\n",
        pack.display().to_string()
    );
    fs::write(dir.path().join("murphy.toml"), toml).expect("write toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("cops")
        .arg("list")
        .arg("--format")
        .arg("json")
        .assert()
        .code(0);

    let stdout = &assert.get_output().stdout;
    let listings: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout is a JSON array");

    let rails_from_pack: Vec<&serde_json::Value> = listings
        .iter()
        .filter(|l| l["source_pack"].as_str() == Some("murphy-rails"))
        .collect();

    assert_eq!(
        rails_from_pack.len(),
        138,
        "expected 138 cops under source_pack=murphy-rails, got {}",
        rails_from_pack.len()
    );

    // Every Rails cop from this pack must have a `Rails/` name and the
    // `Rails` namespace — sanity check against accidental rename or
    // namespace drift.
    for l in &rails_from_pack {
        let name = l["name"].as_str().expect("cop has string name");
        assert!(
            name.starts_with("Rails/"),
            "expected Rails-namespaced name, got {name:?}"
        );
        assert_eq!(
            l["namespace"].as_str(),
            Some("Rails"),
            "expected namespace=Rails for {name:?}"
        );
    }
}

#[test]
fn lint_on_rails_fixture_exits_clean_with_pack_loaded() {
    // Even with the 138-cop pack loaded, the stubs' no-op `check`
    // bodies must not emit any offenses on a representative Rails
    // snippet. The fixture is intentionally idiomatic Rails-ish code
    // (controller-ish render, ActiveRecord-ish callback) so that a
    // future stub which accidentally fires would surface here.
    let pack = rails_pack_path()
        .canonicalize()
        .expect("murphy-rails artifact should exist");

    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("app.rb");
    fs::write(
        &rb,
        "class UsersController < ApplicationController\n  \
         before_action :authenticate\n  \
         def index\n    render :index\n  end\nend\n",
    )
    .expect("write rb");

    let toml = format!(
        "[[plugins]]\nname = \"murphy-rails\"\npath = {:?}\n",
        pack.display().to_string()
    );
    fs::write(dir.path().join("murphy.toml"), toml).expect("write toml");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg(&rb)
        .assert()
        .code(0);
}

#[test]
fn rails_rule_section_does_not_error_lint_setup() {
    // §14a: a user-authored `[cops.rules."Rails/..."]` section in
    // murphy.toml must not raise a setup error (exit 2). With the pack
    // loaded the cop is known; setting `enabled = true` is honoured by
    // config but still produces no offenses because the stub's `check`
    // is a no-op. Exit 0 is the success signal — §12c-compatible.
    let pack = rails_pack_path()
        .canonicalize()
        .expect("murphy-rails artifact should exist");

    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("app.rb");
    // Fixture must not trip any murphy-std built-in cop (e.g.
    // `Murphy/NoReceiverPuts` flags bare `puts`) so the exit code only
    // reflects the Rails pack's behaviour. An empty class body is
    // inert against the current standard pack.
    fs::write(&rb, "class Foo\nend\n").expect("write rb");

    let toml = format!(
        "[[plugins]]\nname = \"murphy-rails\"\npath = {:?}\n\n\
         [cops.rules.\"Rails/HttpStatus\"]\nenabled = true\n",
        pack.display().to_string()
    );
    fs::write(dir.path().join("murphy.toml"), toml).expect("write toml");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg(&rb)
        .assert()
        .code(0);
}
