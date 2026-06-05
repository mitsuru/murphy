//! E2E integration test for the `murphy-rails` dynamic plugin pack
//! (murphy-2ob §14a).
//!
//! Loads `murphy-rails` via `plugins:` config + dlopen and asserts:
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
    let deps_no_hash = deps.join(format!("{prefix}.{ext}"));
    if deps_no_hash.exists() {
        return deps_no_hash;
    }
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
    let yml = format!(
        "plugins:\n  - name: murphy-rails\n    path: {:?}\n",
        pack.display().to_string()
    );
    fs::write(dir.path().join(".murphy.yml"), yml).expect("write toml");

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
        "# frozen_string_literal: true\nclass UsersController < ApplicationController\n  \
         before_action :authenticate\n  \
         def index\n    render :index\n  end\nend\n",
    )
    .expect("write rb");

    let yml = format!(
        "plugins:\n  - name: murphy-rails\n    path: {:?}\n",
        pack.display().to_string()
    );
    fs::write(dir.path().join(".murphy.yml"), yml).expect("write toml");

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
    // §14a: a user-authored `Rails/...:` section in
    // .murphy.yml must not raise a setup error (exit 2). With the pack
    // loaded the cop is known; setting `enabled = true` is honoured by
    // config but still produces no offenses because the stub's `check`
    // is a no-op. Exit 0 is the success signal — §12c-compatible.
    let pack = rails_pack_path()
        .canonicalize()
        .expect("murphy-rails artifact should exist");

    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("app.rb");
    // Fixture must not trip any murphy-std built-in cop so the exit code only
    // reflects the Rails pack's behaviour. An empty class body is inert
    // against the current standard pack.
    fs::write(&rb, "# frozen_string_literal: true\nclass Foo\nend\n").expect("write rb");

    let yml = format!(
        "plugins:\n  - name: murphy-rails\n    path: {:?}\nRails/HttpStatus:\n  Enabled: true\n",
        pack.display().to_string()
    );
    fs::write(dir.path().join(".murphy.yml"), yml).expect("write toml");

    Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg(&rb)
        .assert()
        .code(0);
}

/// Resolve the `murphy-example-pack` cdylib artifact (which ships NO
/// bundled `default.yml`), mirroring [`rails_pack_path`].
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
fn loader_reads_rails_pack_bundled_default_config() {
    // The rails pack exports `MURPHY_PLUGIN_DEFAULT_CONFIG` pointing at its
    // embedded `config/default.yml`. The loader must copy those bytes to an
    // owned String, surfaced via `default_config_yaml()`.
    let pack = rails_pack_path()
        .canonicalize()
        .expect("murphy-rails artifact should exist");

    let loaded =
        murphy_core::plugin_loader::load_plugin_pack(&pack).expect("rails pack loads cleanly");

    let yaml = loaded
        .default_config_yaml()
        .expect("rails pack ships a bundled default.yml");
    assert!(
        yaml.contains("ActiveSupportExtensionsEnabled"),
        "rails default.yml should set the ASE default, got: {yaml:?}"
    );
}

#[test]
fn loader_yields_none_for_pack_without_bundled_config() {
    // The example pack exports no `MURPHY_PLUGIN_DEFAULT_CONFIG` symbol;
    // absence is normal and must yield `None` (not an error).
    let pack = example_pack_path()
        .canonicalize()
        .expect("example-pack artifact should exist");

    let loaded =
        murphy_core::plugin_loader::load_plugin_pack(&pack).expect("example pack loads cleanly");

    assert!(
        loaded.default_config_yaml().is_none(),
        "example pack ships no bundled config → None"
    );
}

#[test]
fn registry_pack_default_configs_aggregates_and_skips_none() {
    use murphy_core::{CopRegistry, MurphyConfig};

    let rails = rails_pack_path()
        .canonicalize()
        .expect("murphy-rails artifact should exist");
    let example = example_pack_path()
        .canonicalize()
        .expect("example-pack artifact should exist");

    let dir = tempdir().expect("tempdir");
    // Load BOTH packs: rails ships a default.yml (Some), example does not
    // (None). `pack_default_configs()` must surface exactly the rails layer.
    let yml = format!(
        "plugins:\n  - name: murphy-rails\n    path: {:?}\n  - name: murphy-example-pack\n    path: {:?}\n",
        rails.display().to_string(),
        example.display().to_string(),
    );
    fs::write(dir.path().join(".murphy.yml"), yml).expect("write yml");

    let config = MurphyConfig::load(dir.path()).expect("config loads");
    let registry = CopRegistry::discover_with_config(dir.path(), &config, &[])
        .expect("registry discovers both packs");

    let layers = registry.pack_default_configs();
    assert_eq!(
        layers.len(),
        1,
        "only the rails pack ships a bundled default.yml (example skipped as None)"
    );
    assert!(layers[0].contains("ActiveSupportExtensionsEnabled"));
}

#[test]
fn applying_rails_pack_layer_flips_active_support_extensions_enabled() {
    // The single behaviour this wiring exists to produce: loading the rails
    // pack (whose default.yml sets ASE true) flips the run-level config's
    // `active_support_extensions_enabled` from its std-default false to true.
    //
    // Built against the PRODUCTION constructor `load_with_defaults` with
    // `murphy_std::BUNDLED_DEFAULTS_YAML` (which carries
    // `AllCops.ActiveSupportExtensionsEnabled: false` in its base layer).
    // That base default lives in `base_defaults`, NOT in the user-set channel,
    // so `apply_pack_default_layers` must NOT treat it as a user override and
    // must still let the pack layer flip ASE on. Using `load` (no defaults)
    // would mask a regression where the std default makes `user_set` true and
    // early-returns.
    use murphy_core::{CopRegistry, MurphyConfig};

    let rails = rails_pack_path()
        .canonicalize()
        .expect("murphy-rails artifact should exist");

    let dir = tempdir().expect("tempdir");
    let yml = format!(
        "plugins:\n  - name: murphy-rails\n    path: {:?}\n",
        rails.display().to_string(),
    );
    fs::write(dir.path().join(".murphy.yml"), yml).expect("write yml");

    let mut config =
        MurphyConfig::load_with_defaults(dir.path(), murphy_std::BUNDLED_DEFAULTS_YAML)
            .expect("config loads with std defaults");
    assert!(
        !config.active_support_extensions_enabled,
        "std default is false before any pack layer"
    );

    let registry = CopRegistry::discover_with_config(dir.path(), &config, &[])
        .expect("registry discovers the rails pack");
    config.apply_pack_default_layers(&registry.pack_default_configs());

    assert!(
        config.active_support_extensions_enabled,
        "loading the rails pack must flip ASE true even with std defaults folded in"
    );
}

#[test]
fn rails_pack_exempts_lambda_symbol_proc_through_full_lint_pipeline() {
    // End-to-end payoff: a `->(x) { x.method }` lambda is a `Style/SymbolProc`
    // candidate, but RuboCop (with rubocop-rails) exempts lambda/proc blocks
    // under `AllCops.ActiveSupportExtensionsEnabled`. The rails pack's bundled
    // `default.yml` sets that flag true. This test runs the REAL `murphy lint`
    // binary through its full pipeline (embedded default.yml → loader symbol
    // read → `apply_pack_default_layers` → ASE flag → dispatch → Cx accessor →
    // SymbolProc exemption) and asserts the lambda offense is suppressed.
    //
    // The CONTROL arm (same fixture, NO pack) asserts the SAME lambda STILL
    // flags `Style/SymbolProc`, proving the pack is what causes the exemption
    // — not that the cop simply never fires on this shape.
    let pack = rails_pack_path()
        .canonicalize()
        .expect("murphy-rails artifact should exist");

    // A lambda whose sole parameter is the receiver of a single method call —
    // the canonical SymbolProc shape, and exactly Mastodon's
    // `normalizes :x, with: ->(v) { v.strip }` form.
    let fixture = "# frozen_string_literal: true\nFOO = ->(v) { v.strip }\n";

    // ── pack-loaded arm: lambda exemption active, NO SymbolProc offense ──
    let with_pack = tempdir().expect("tempdir");
    let rb = with_pack.path().join("lam.rb");
    fs::write(&rb, fixture).expect("write rb");
    let yml = format!(
        "plugins:\n  - name: murphy-rails\n    path: {:?}\n",
        pack.display().to_string()
    );
    fs::write(with_pack.path().join(".murphy.yml"), yml).expect("write yml");

    // Pin exit 0: the lambda is the only offense candidate and it must be
    // exempted, so a clean run is exit 0. Pinning the code guards the
    // absence-assertion below from passing vacuously on a setup/internal
    // error (exit 2/3) that would also produce no `Style/SymbolProc` line.
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(with_pack.path())
        .arg("lint")
        .arg(&rb)
        .assert()
        .code(0);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        !stdout.contains("Style/SymbolProc"),
        "rails pack must exempt the lambda from Style/SymbolProc; got:\n{stdout}"
    );

    // ── control arm: SAME fixture, NO pack → SymbolProc fires ──
    let no_pack = tempdir().expect("tempdir");
    let rb2 = no_pack.path().join("lam.rb");
    fs::write(&rb2, fixture).expect("write rb");
    // No `.murphy.yml` at all: ASE defaults false, so the lambda is NOT exempt.

    // Pin exit 1: the lambda is NOT exempt here, so the lint must report the
    // offense and exit 1 (offenses found). This rules out a vacuous pass from
    // a setup error and confirms the offense line below comes from a real run.
    let assert2 = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(no_pack.path())
        .arg("lint")
        .arg(&rb2)
        .assert()
        .code(1);
    let stdout2 = String::from_utf8_lossy(&assert2.get_output().stdout);
    assert!(
        stdout2.contains("Style/SymbolProc"),
        "without the rails pack the lambda MUST flag Style/SymbolProc (proves \
         the pack is the cause of the exemption); got:\n{stdout2}"
    );
}
