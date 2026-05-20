use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
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

#[cfg(not(target_os = "windows"))]
fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate has parent")
        .parent()
        .expect("crates dir has parent")
        .to_path_buf()
}

#[cfg(not(target_os = "windows"))]
fn target_dir(root: &Path) -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from) {
        Some(path) if path.is_absolute() => path,
        Some(path) => root.join(path),
        None => root.join("target"),
    }
}

#[test]
#[cfg(not(target_os = "windows"))]
fn example_native_pack_loads_and_emits_offense() {
    let root = workspace_root();
    let status = std::process::Command::new("cargo")
        .current_dir(&root)
        .args(["build", "-p", "murphy-example-pack"])
        .status()
        .expect("run cargo build for example pack");
    assert!(status.success(), "example pack must build before e2e test");

    let dir = tempdir().expect("create tempdir");
    let target_dir = target_dir(&root);
    let dylib_name = format!(
        "{}murphy_example_pack{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    let dylib = target_dir.join("debug").join(dylib_name);
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

#[test]
#[cfg(not(target_os = "windows"))]
fn rails_native_pack_loads_expected_cops() {
    let root = workspace_root();
    let status = std::process::Command::new("cargo")
        .current_dir(&root)
        .args(["build", "-p", "murphy-rails"])
        .status()
        .expect("run cargo build for rails pack");
    assert!(status.success(), "rails pack must build before e2e test");

    let dir = tempdir().expect("create tempdir");
    let target = target_dir(&root);
    let dylib_name = format!(
        "{}murphy_rails{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    let dylib = target.join("debug").join(dylib_name);
    fs::write(
        dir.path().join("murphy.toml"),
        format!(
            "[[cop_packs]]\nname = \"murphy-rails\"\npath = {}\nversion = \"0.1.0\"\n",
            format_args!("{:?}", dylib.to_string_lossy())
        ),
    )
    .expect("write config");
    fs::write(
        dir.path().join("rails_sample.rb"),
        "has_and_belongs_to_many :groups\nitems = Item.find(:all).to_a\ntext = html_safe('x')\ndefault_scope { order(created_at: :desc) }\nhas_many :projects\nrequest.referer\nDate.today\nTime.now\n",
    )
    .expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("rails_sample.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");

    let mut names: Vec<&str> = parsed
        .iter()
        .map(|offense| offense["cop_name"].as_str().unwrap_or(""))
        .collect();
    names.sort_unstable();

    let required = [
        "Rails/HasAndBelongsToMany",
        "Rails/FindEach",
        "Rails/HtmlSafe",
        "Rails/Date",
        "Rails/DefaultScope",
        "Rails/HasManyOrHasOneDependent",
        "Rails/RequestReferer",
    ];
    for required_name in required {
        assert!(
            names.contains(&required_name),
            "expected rails pack offense for {required_name}, got {names:?}"
        );
    }
}
