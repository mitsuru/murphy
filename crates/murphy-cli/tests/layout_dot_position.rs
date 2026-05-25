//! End-to-end tests for `Layout/DotPosition` against the compiled
//! `murphy` binary (murphy-lpc.3.4).
//!
//! Contract anchored:
//! - default config (no `murphy.toml`) enforces leading dots.
//! - `[cops.rules."Layout/DotPosition"] EnforcedStyle = "trailing"`
//!   flips the enforcement direction, exercising the option-decode
//!   path that the lib-level tests can only stub via an in-module
//!   wrapper.
//! - `enabled = false` silences the cop.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn lint_json_in(dir: &std::path::Path, source: &str) -> (i32, Vec<serde_json::Value>) {
    fs::write(dir.join("t.rb"), source).expect("write source");
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("t.rb")
        .assert();
    let output = assert.get_output().clone();
    let code = output.status.code().unwrap_or(-1);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).expect("stdout must be JSON");
    (code, parsed)
}

fn offenses_named<'a>(offenses: &'a [serde_json::Value], cop: &str) -> Vec<&'a serde_json::Value> {
    offenses.iter().filter(|o| o["cop_name"] == cop).collect()
}

#[test]
fn default_config_flags_trailing_dot() {
    // No `murphy.toml` ⇒ default leading-dot style ⇒ trailing-style
    // input registers an offense.
    let dir = tempdir().expect("create tempdir");
    let (code, offs) = lint_json_in(dir.path(), "something.\n  method_name\n");
    assert_eq!(code, 1, "offense → exit 1");
    let dp = offenses_named(&offs, "Layout/DotPosition");
    assert_eq!(dp.len(), 1, "exactly one offense; got {offs:?}");
    assert!(
        dp[0]["message"]
            .as_str()
            .unwrap()
            .contains("on the next line"),
        "leading-mode message expected; got {:?}",
        dp[0]["message"],
    );
}

#[test]
fn user_config_enforced_style_trailing_flips_enforcement() {
    // `EnforcedStyle = "trailing"` ⇒ leading-style input registers an
    // offense pointing the user toward the trailing form. This is the
    // option-decode end-to-end check that the lib tests cannot make
    // because the test_support harness pins `options_json = "{}"`.
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join("murphy.toml"),
        "[cops.rules.\"Layout/DotPosition\"]\nEnforcedStyle = \"trailing\"\n",
    )
    .expect("write murphy.toml");
    let (code, offs) = lint_json_in(dir.path(), "something\n  .method_name\n");
    assert_eq!(code, 1, "offense → exit 1");
    let dp = offenses_named(&offs, "Layout/DotPosition");
    assert_eq!(
        dp.len(),
        1,
        "trailing mode flags the leading-dot input; got {offs:?}",
    );
    let message = dp[0]["message"].as_str().unwrap();
    assert!(
        message.contains("on the previous line"),
        "trailing-mode message expected; got {message:?}",
    );
}

#[test]
fn user_config_enforced_style_trailing_accepts_trailing_input() {
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join("murphy.toml"),
        "[cops.rules.\"Layout/DotPosition\"]\nEnforcedStyle = \"trailing\"\n",
    )
    .expect("write murphy.toml");
    let (_code, offs) = lint_json_in(dir.path(), "something.\n  method_name\n");
    assert!(
        offenses_named(&offs, "Layout/DotPosition").is_empty(),
        "trailing-input under trailing config must not register an offense; got {offs:?}",
    );
}

#[test]
fn config_disabled_silences_the_cop() {
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join("murphy.toml"),
        "[cops.rules.\"Layout/DotPosition\"]\nenabled = false\n",
    )
    .expect("write murphy.toml");
    let (_code, offs) = lint_json_in(dir.path(), "something.\n  method_name\n");
    assert!(
        offenses_named(&offs, "Layout/DotPosition").is_empty(),
        "user-disabled cop must produce no offenses; got {offs:?}",
    );
}
