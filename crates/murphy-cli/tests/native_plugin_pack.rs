use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn parse_rails_cop_metadata(root: &Path) -> (Vec<String>, Vec<String>) {
    let dir = root.join("crates").join("murphy-rails/src/cops/rails");
    let mut names = Vec::new();
    let mut patterns = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("rails cops directory exists")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
        .filter(|path| {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            file_name != "mod.rs" && file_name != "util.rs"
        })
        .collect();

    entries.sort_unstable();

    for path in entries {
        let source = fs::read_to_string(&path).expect("read rails cop source");

        if let Some(start) = source.find("pub(crate) const NAME_BYTES: &[u8] = b\"") {
            let rest = &source[start + "pub(crate) const NAME_BYTES: &[u8] = b\"".len()..];
            if let Some(end) = rest.find('"') {
                names.push(rest[..end].to_string());
            }
        }

        if let Some(start) = source.find("let patterns: [&[u8]") {
            let mut cursor = &source[start..];
            if let Some(assign_pos) = cursor.find(" = [") {
                cursor = &cursor[assign_pos + 3..];
                if let Some(right_bracket) = cursor.find("];") {
                    let pattern_block = &cursor[..right_bracket];
                    let mut remaining = pattern_block;
                    while let Some(pos) = remaining.find("b\"") {
                        remaining = &remaining[pos + 2..];
                        if let Some(end) = remaining.find('"') {
                            let p = &remaining[..end];
                            if !p.is_empty() {
                                patterns.push(p.to_string());
                            }
                            remaining = &remaining[end + 1..];
                        } else {
                            break;
                        }
                    }
                }
            }
        }
    }

    patterns.sort_unstable();
    patterns.dedup();
    names.sort_unstable();
    names.dedup();
    (names, patterns)
}

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

    let (required, patterns) = parse_rails_cop_metadata(&root);
    assert_eq!(required.len(), 138, "expected 138 rails cops in repository");
    assert!(
        !patterns.is_empty(),
        "expected non-empty patterns from rails cops"
    );

    let source = patterns
        .into_iter()
        .map(|token| format!("# {token}\n"))
        .collect::<Vec<_>>()
        .join("");
    fs::write(dir.path().join("rails_sample.rb"), source).expect("write source");

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
        .filter(|name| name.starts_with("Rails/"))
        .collect();
    names.sort_unstable();
    names.dedup();

    let required = required.into_iter().collect::<Vec<_>>();
    assert_eq!(
        names.len(),
        required.len(),
        "expected {} rails cops in sample output, got {names:?}",
        required.len()
    );
    for required_name in required {
        assert!(
            names.contains(&required_name.as_str()),
            "expected rails pack offense for {required_name}, got {names:?}"
        );
    }
}
