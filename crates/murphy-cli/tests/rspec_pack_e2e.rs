//! E2E integration test for the murphy-rspec plugin pack (murphy-4n9.4).
//!
//! Loads `murphy-rspec` via the `plugins:` config + dlopen path and
//! asserts the bootstrap cop `RSpec/DescribeClass` fires on a fixture.
//!
//! Windows は plugin pack 非対応 (`plugin_loader` の Windows guard) なので
//! 全体を `cfg(not(target_os = "windows"))` で除外する。
//!
//! Same shape as `plugin_pack_e2e.rs` (example-pack). The bootstrap
//! scope is one cop; the follow-up cops (`RSpec/ExampleLength`,
//! `RSpec/MultipleExpectations`) are tracked as sub-issues.

#![cfg(not(target_os = "windows"))]

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

/// Resolve the cdylib artifact path for `murphy-rspec`. Cargo's dep graph
/// (murphy-cli's `[dev-dependencies]` → `murphy-rspec`) guarantees the
/// cdylib is built before this test runs; this helper just locates it.
///
/// Mirrors `plugin_pack_e2e.rs::example_pack_path`. Same 2-tier resolution
/// (`CARGO_TARGET_DIR` env → `${CARGO_MANIFEST_DIR}/../../target`) and
/// the same assumptions: `debug` profile, host triple. Both hold for
/// CI and `cargo test` from this workspace.
fn rspec_pack_path() -> std::path::PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target")
        });
    let (prefix, ext) = if cfg!(target_os = "macos") {
        ("libmurphy_rspec", "dylib")
    } else {
        ("libmurphy_rspec", "so")
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

fn run_with_pack(source: &str) -> (i32, Vec<serde_json::Value>) {
    let pack = rspec_pack_path()
        .canonicalize()
        .expect("murphy-rspec cdylib should exist (Cargo dep graph)");

    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join("widget_spec.rb");
    fs::write(&rb, source).expect("write rb");

    let yml = format!(
        "plugins:\n  - name: murphy-rspec\n    path: {:?}\n",
        pack.display().to_string()
    );
    fs::write(dir.path().join(".murphy.yml"), yml).expect("write toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&rb)
        .assert();
    let output = assert.get_output().clone();
    let code = output.status.code().unwrap_or(-1);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).expect("stdout must be JSON");
    (code, parsed)
}

/// Like [`run_with_pack`], but writes the spec at `rel_path` (relative to the
/// project root) and lints that path, so the pack's bundled-default `Exclude`
/// globs (e.g. `RSpec/DescribeClass` for `spec/requests/**`) are exercised
/// end-to-end through the real `.so`'s `MURPHY_PLUGIN_DEFAULT_CONFIG`.
fn run_with_pack_at(rel_path: &str, source: &str) -> Vec<serde_json::Value> {
    let pack = rspec_pack_path()
        .canonicalize()
        .expect("murphy-rspec cdylib should exist (Cargo dep graph)");

    let dir = tempdir().expect("tempdir");
    let rb = dir.path().join(rel_path);
    fs::create_dir_all(rb.parent().expect("rel_path has a parent")).expect("mkdir -p");
    fs::write(&rb, source).expect("write rb");

    let yml = format!(
        "plugins:\n  - name: murphy-rspec\n    path: {:?}\n",
        pack.display().to_string()
    );
    fs::write(dir.path().join(".murphy.yml"), yml).expect("write yml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(rel_path)
        .assert();
    let output = assert.get_output().clone();
    serde_json::from_slice(&output.stdout).expect("stdout must be JSON")
}

fn offenses_named<'a>(offenses: &'a [serde_json::Value], cop: &str) -> Vec<&'a serde_json::Value> {
    offenses.iter().filter(|o| o["cop_name"] == cop).collect()
}

#[test]
fn flags_rspec_describe_with_string_first_arg() {
    // `RSpec.describe "bad describe" do ... end` — the string isn't a
    // constant path, so the cop fires. (Class-name-shaped strings like
    // `"Widget"` are accepted via the `string_constant?` exception that
    // mirrors upstream RuboCop-RSpec.)
    let (code, offs) = run_with_pack("RSpec.describe \"bad describe\" do\nend\n");
    assert_eq!(code, 1, "offense → exit 1; got offenses {offs:?}");
    let hits = offenses_named(&offs, "RSpec/DescribeClass");
    assert_eq!(
        hits.len(),
        1,
        "expected one RSpec/DescribeClass offense; got {offs:?}"
    );
}

#[test]
fn flags_bare_describe_with_string_first_arg() {
    // RSpec's top-level monkey-patch: `describe "bad describe" do ... end`
    // (no `RSpec.` receiver). The non-constant-shaped string fires the
    // cop in both bare and explicit-receiver forms.
    let (_code, offs) = run_with_pack("describe \"bad describe\" do\nend\n");
    let hits = offenses_named(&offs, "RSpec/DescribeClass");
    assert_eq!(
        hits.len(),
        1,
        "bare `describe` must also be flagged; got {offs:?}"
    );
}

#[test]
fn does_not_flag_describe_with_constant_first_arg() {
    // `RSpec.describe Widget do ... end` — describing a class is the
    // exact form the cop wants. Must not fire.
    let (_code, offs) = run_with_pack("RSpec.describe Widget do\nend\n");
    assert!(
        offenses_named(&offs, "RSpec/DescribeClass").is_empty(),
        "constant first-arg must not be flagged; got {offs:?}"
    );
}

#[test]
fn does_not_flag_describe_with_scoped_constant_first_arg() {
    // `describe Foo::Bar do ... end` — scoped constants are still
    // describing a class.
    let (_code, offs) = run_with_pack("describe Foo::Bar do\nend\n");
    assert!(
        offenses_named(&offs, "RSpec/DescribeClass").is_empty(),
        "scoped constant first-arg must not be flagged; got {offs:?}"
    );
}

#[test]
fn does_not_flag_unrelated_describe_receiver() {
    // `Other.describe "x"` — explicit non-RSpec receiver is some other
    // DSL's `describe`; the cop must not fire.
    let (_code, offs) = run_with_pack("Other.describe \"x\"\n");
    assert!(
        offenses_named(&offs, "RSpec/DescribeClass").is_empty(),
        "non-RSpec receiver must not be flagged; got {offs:?}"
    );
}

#[test]
fn flags_describe_with_symbol_first_arg() {
    // `describe :widget do ... end` — symbols name a non-class subject
    // just like strings, so the cop must flag.
    let (_code, offs) = run_with_pack("describe :widget do\nend\n");
    let hits = offenses_named(&offs, "RSpec/DescribeClass");
    assert_eq!(
        hits.len(),
        1,
        "symbol first-arg must be flagged; got {offs:?}"
    );
}

#[test]
fn flags_describe_with_interpolated_string_first_arg() {
    // `RSpec.describe "Widget #{n}" do ... end` — Dstr (interpolated)
    // is still a string-like literal and must be flagged.
    let (_code, offs) = run_with_pack("n = 1\nRSpec.describe \"Widget #{n}\" do\nend\n");
    let hits = offenses_named(&offs, "RSpec/DescribeClass");
    assert_eq!(
        hits.len(),
        1,
        "interpolated-string first-arg must be flagged; got {offs:?}"
    );
}

#[test]
fn does_not_flag_describe_with_variable_first_arg() {
    // `describe subject_under_test do ... end` — variable first-arg is
    // unknowable statically; the cop must skip rather than guess.
    let (_code, offs) =
        run_with_pack("subject_under_test = Widget\ndescribe subject_under_test do\nend\n");
    assert!(
        offenses_named(&offs, "RSpec/DescribeClass").is_empty(),
        "variable first-arg must not be flagged; got {offs:?}"
    );
}

#[test]
fn bundled_default_excludes_describe_class_in_request_specs() {
    // rubocop-rspec's bundled default Excludes `RSpec/DescribeClass` from
    // `spec/requests/**` (and features/routing/system/views). Murphy mirrors
    // that via the pack's `config/default.yml`, merged below user config by
    // `apply_pack_default_layers`. A non-constant `describe` string here must
    // NOT be flagged. (murphy-duih — 275 mastodon FPs.)
    let offs = run_with_pack_at(
        "spec/requests/widget_spec.rb",
        "RSpec.describe \"bad describe\" do\nend\n",
    );
    assert!(
        offenses_named(&offs, "RSpec/DescribeClass").is_empty(),
        "spec/requests must be excluded by the pack default; got {offs:?}"
    );
}

#[test]
fn bundled_default_still_flags_describe_class_in_model_specs() {
    // Over-exclusion guard: `spec/models/**` is NOT in the default Exclude,
    // so the same non-constant `describe` string IS flagged. Proves the pack
    // default scopes the cop rather than disabling it wholesale.
    let offs = run_with_pack_at(
        "spec/models/widget_spec.rb",
        "RSpec.describe \"bad describe\" do\nend\n",
    );
    assert_eq!(
        offenses_named(&offs, "RSpec/DescribeClass").len(),
        1,
        "spec/models must still be flagged; got {offs:?}"
    );
}

// `RSpec/ExampleLength` (murphy-6bv) and `RSpec/MultipleExpectations`
// (murphy-6tq) are exercised by parser-driven unit tests in
// `crates/murphy-rspec/src/{example_length,multiple_expectations}.rs`
// (`#[cfg(test)] mod tests`). Plugin-load + dispatch is already proven
// for the pack by the DescribeClass cases above, so the follow-up cops
// don't add e2e smoke here.
