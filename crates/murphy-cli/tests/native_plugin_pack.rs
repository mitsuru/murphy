use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

#[test]
fn configured_missing_native_pack_exits_2_with_empty_stdout() {
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join("murphy.toml"),
        r#"
[[cop_packs]]
name = "missing-pack"
path = "packs/missing/libmissing_pack.so"
version = "0.1.0"
"#,
    )
    .expect("write config");
    fs::write(
        dir.path().join("clean.rb"),
        "# frozen_string_literal: true\n\nx = 1\n",
    )
    .expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("clean.rb")
        .assert()
        .code(2);

    assert!(assert.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("missing-pack"), "stderr was {stderr:?}");
}

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate has parent")
        .parent()
        .expect("crates dir has parent")
        .to_path_buf()
}

#[test]
fn example_native_pack_loads_and_emits_offense() {
    let root = workspace_root();
    let status = std::process::Command::new("cargo")
        .current_dir(&root)
        .args(["build", "-p", "murphy-example-pack"])
        .status()
        .expect("run cargo build for example pack");
    assert!(status.success(), "example pack must build before e2e test");

    let dir = tempdir().expect("create tempdir");
    let dylib = root.join("target/debug/libmurphy_example_pack.so");
    fs::write(
        dir.path().join("murphy.toml"),
        format!(
            "[[cop_packs]]\nname = \"murphy-example-pack\"\npath = {}\nversion = \"0.1.0\"\n",
            format_args!("{:?}", dylib.to_string_lossy())
        ),
    )
    .expect("write config");
    fs::write(
        dir.path().join("clean.rb"),
        "# frozen_string_literal: true\n\nx = 1\n",
    )
    .expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("clean.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    assert!(
        parsed
            .iter()
            .any(|offense| offense["cop_name"] == "Example/FileBanner"),
        "expected example plugin offense, got {parsed:?}"
    );
}
