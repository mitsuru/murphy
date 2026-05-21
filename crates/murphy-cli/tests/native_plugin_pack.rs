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

const RAILS_COPS_DISABLED_BY_DEFAULT: [&str; 12] = [
    "Rails/ActionFilter",
    "Rails/DefaultScope",
    "Rails/Env",
    "Rails/EnvironmentVariableAccess",
    "Rails/OrderById",
    "Rails/PluckId",
    "Rails/RequireDependency",
    "Rails/ReversibleMigrationMethodDefinition",
    "Rails/SaveBang",
    "Rails/SchemaComment",
    "Rails/TableNameAssignment",
    "Rails/UnusedIgnoredColumns",
];

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
        .arg("--format")
        .arg("json")
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

#[cfg(not(target_os = "windows"))]
fn build_example_pack(root: &Path) {
    let status = std::process::Command::new("cargo")
        .current_dir(root)
        .args(["build", "-p", "murphy-example-pack"])
        .status()
        .expect("run cargo build for example pack");
    assert!(status.success(), "example pack must build before e2e test");
}

#[cfg(not(target_os = "windows"))]
fn example_pack_dylib_path(root: &Path) -> PathBuf {
    let target_dir = target_dir(root);
    let dylib_name = format!(
        "{}murphy_example_pack{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    );
    target_dir.join("debug").join(dylib_name)
}

#[test]
#[cfg(not(target_os = "windows"))]
fn example_native_pack_loads_and_emits_offense() {
    let root = workspace_root();
    build_example_pack(&root);

    let dir = tempdir().expect("create tempdir");
    let dylib = example_pack_dylib_path(&root);
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
        .arg("--format")
        .arg("json")
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
fn example_native_pack_receives_cop_config_options() {
    let root = workspace_root();
    build_example_pack(&root);

    let dir = tempdir().expect("create tempdir");
    let dylib = example_pack_dylib_path(&root);
    fs::write(
        dir.path().join("murphy.toml"),
        format!(
            "[[cop_packs]]\nname = \"murphy-example-pack\"\npath = {}\nversion = \"0.1.0\"\n\n[cops.rules.\"Example/FileBanner\"]\nmessage = \"configured native plugin message\"\n",
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
        .arg("--format")
        .arg("json")
        .arg("clean.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    assert!(
        parsed.iter().any(|offense| {
            offense["cop_name"] == "Example/FileBanner"
                && offense["message"] == "configured native plugin message"
        }),
        "expected configured example plugin message, got {parsed:?}"
    );
}

#[test]
#[cfg(not(target_os = "windows"))]
fn example_native_pack_call_dispatch_receives_cop_config_options() {
    let root = workspace_root();
    build_example_pack(&root);

    let dir = tempdir().expect("create tempdir");
    let dylib = example_pack_dylib_path(&root);
    fs::write(
        dir.path().join("murphy.toml"),
        format!(
            "[[cop_packs]]\nname = \"murphy-example-pack\"\npath = {}\nversion = \"0.1.0\"\n\n[cops.rules.\"Example/CallDispatch\"]\nmessage = \"configured call dispatch message\"\n",
            format_args!("{:?}", dylib.to_string_lossy())
        ),
    )
    .expect("write config");
    fs::write(
        dir.path().join("app.rb"),
        "# frozen_string_literal: true\n\nexample_call\n",
    )
    .expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("app.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    assert!(
        parsed.iter().any(|offense| {
            offense["cop_name"] == "Example/CallDispatch"
                && offense["message"] == "configured call dispatch message"
        }),
        "expected configured call dispatch message, got {parsed:?}"
    );
}

#[test]
#[cfg(not(target_os = "windows"))]
fn example_native_pack_dispatches_call_cop_by_static_method_table() {
    let root = workspace_root();
    build_example_pack(&root);

    let dir = tempdir().expect("create tempdir");
    let dylib = example_pack_dylib_path(&root);
    fs::write(
        dir.path().join("murphy.toml"),
        format!(
            "[[cop_packs]]\nname = \"murphy-example-pack\"\npath = {}\nversion = \"0.1.0\"\n",
            format_args!("{:?}", dylib.to_string_lossy())
        ),
    )
    .expect("write config");
    fs::write(
        dir.path().join("app.rb"),
        "# frozen_string_literal: true\n\nignored_call\nexample_call\n",
    )
    .expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("app.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    let call_offenses = parsed
        .iter()
        .filter(|offense| offense["cop_name"] == "Example/CallDispatch")
        .collect::<Vec<_>>();
    let pack_dispatch_offenses = parsed
        .iter()
        .filter(|offense| offense["cop_name"] == "Example/PackDispatch")
        .collect::<Vec<_>>();

    assert_eq!(
        call_offenses.len(),
        1,
        "static call dispatch should invoke the call cop only for example_call, got {parsed:?}"
    );
    assert_eq!(
        pack_dispatch_offenses.len(),
        1,
        "core should call the plugin pack dispatcher once for example_call, got {parsed:?}"
    );
}

#[test]
#[cfg(not(target_os = "windows"))]
fn example_native_pack_file_scope_include_exclude() {
    let root = workspace_root();
    build_example_pack(&root);

    let dir = tempdir().expect("create tempdir");
    let dylib = example_pack_dylib_path(&root);
    fs::write(
        dir.path().join("murphy.toml"),
        format!(
            "[[cop_packs]]\nname = \"murphy-example-pack\"\npath = {}\nversion = \"0.1.0\"\n\n[cops.rules.\"Example/FileBanner\"]\nInclude = [\"app/**/*.rb\"]\nExclude = [\"app/skip.rb\"]\n",
            format_args!("{:?}", dylib.to_string_lossy())
        ),
    )
    .expect("write config");

    fs::create_dir_all(dir.path().join("app")).expect("create app dir");
    fs::create_dir_all(dir.path().join("lib")).expect("create lib dir");
    fs::write(dir.path().join("app/app.rb"), "x = 1\n").expect("write app file");
    fs::write(
        dir.path().join("app/skip.rb"),
        "# frozen_string_literal: true\n\nx = 1\n",
    )
    .expect("write excluded file");
    fs::write(
        dir.path().join("lib/other.rb"),
        "# frozen_string_literal: true\n\nx = 1\n",
    )
    .expect("write excluded file");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("app/app.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    assert!(
        parsed
            .iter()
            .any(|offense| offense["cop_name"] == "Example/FileBanner"),
        "expected include-matched file to be flagged, got {parsed:?}"
    );

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("app/skip.rb")
        .assert()
        .code(0);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    assert!(
        parsed.is_empty(),
        "excluded file should skip FileBanner offense"
    );

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("lib/other.rb")
        .assert()
        .code(0);
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    assert!(
        parsed.is_empty(),
        "non-include file should skip FileBanner offense"
    );
}

#[test]
#[cfg(not(target_os = "windows"))]
fn example_native_pack_file_scope_rejects_parent_segments_in_file_arg() {
    let root = workspace_root();
    build_example_pack(&root);

    let dir = tempdir().expect("create tempdir");
    let dylib = example_pack_dylib_path(&root);
    fs::write(
        dir.path().join("murphy.toml"),
        format!(
            r#"[cops.rules."Style/FrozenStringLiteralComment"]
enabled = false

[[cop_packs]]
name = "murphy-example-pack"
path = {}
version = "0.1.0"

[cops.rules."Example/FileBanner"]
Include = ["**/*.rb"]
"#,
            format_args!("{:?}", dylib.to_string_lossy())
        ),
    )
    .expect("write config");

    fs::create_dir_all(dir.path().join("project").join("app")).expect("create app dir");
    fs::write(dir.path().join("project/app/app.rb"), "x = 1\n").expect("write app file");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("project/app/../app/app.rb")
        .assert()
        .code(0);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    let has_file_banner = parsed
        .iter()
        .any(|offense| offense["cop_name"].as_str() == Some("Example/FileBanner"));
    assert!(
        !has_file_banner,
        "parent-segment path should skip native plugin file-scope matching"
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
    let required_enabled_by_default: Vec<String> = required
        .into_iter()
        .filter(|name| !RAILS_COPS_DISABLED_BY_DEFAULT.contains(&name.as_str()))
        .collect();
    assert!(
        !patterns.is_empty(),
        "expected non-empty patterns from rails cops"
    );

    let mut source = patterns
        .into_iter()
        .map(|token| format!("# {token}\n"))
        .collect::<Vec<_>>()
        .join("");
    source.push_str("\n3.day\n");
    source.push_str("\nassert_not true\n");
    source.push_str("before_action :example\n");
    source.push_str("get '/resource'\n");
    fs::write(dir.path().join("rails_sample.rb"), source).expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
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

    assert_eq!(
        names.len(),
        required_enabled_by_default.len(),
        "expected {} rails cops in sample output, got {names:?}",
        required_enabled_by_default.len()
    );
    for required_name in required_enabled_by_default {
        assert!(
            names.contains(&required_name.as_str()),
            "expected rails pack offense for {required_name}, got {names:?}"
        );
    }

    for disabled_name in RAILS_COPS_DISABLED_BY_DEFAULT {
        assert!(
            !names.contains(&disabled_name),
            "default-disabled rails cop should not emit offense by default: {disabled_name}"
        );
    }
}

#[test]
#[cfg(not(target_os = "windows"))]
fn rails_native_pack_can_enable_default_disabled_cop() {
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
            "[[cop_packs]]\nname = \"murphy-rails\"\npath = {}\nversion = \"0.1.0\"\n\n[cops.rules.\"Rails/ActionFilter\"]\nenabled = true\n",
            format_args!("{:?}", dylib.to_string_lossy())
        ),
    )
    .expect("write config");

    let (_required, patterns) = parse_rails_cop_metadata(&root);
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
        .arg("--format")
        .arg("json")
        .arg("rails_sample.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    let names = parsed
        .iter()
        .filter_map(|offense| offense["cop_name"].as_str())
        .filter(|name| name.starts_with("Rails/"))
        .collect::<Vec<_>>();

    assert!(
        names.contains(&"Rails/ActionFilter"),
        "explicitly enabled ActionFilter should emit offense with a source token match"
    );
}

#[test]
#[cfg(not(target_os = "windows"))]
fn rails_native_pack_does_not_flag_short_tokens_as_plain_text() {
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
        dir.path().join("app.rb"),
        "# frozen_string_literal: true\n\nclass App\n  def perform\n    title = 'plain text with p q s l t ap pp where not get post create save update table name blank access'\n    puts 'real output'\n  end\nend\n",
    )
    .expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("app.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    let rails_names = parsed
        .iter()
        .filter_map(|offense| offense["cop_name"].as_str())
        .filter(|name| name.starts_with("Rails/"))
        .collect::<Vec<_>>();

    assert!(
        rails_names.contains(&"Rails/Output"),
        "real puts call should still be reported, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/I18nLazyLookup"),
        "plain letter t must not trigger I18nLazyLookup, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/SquishedSQLHeredocs"),
        "plain letters s/q/l must not trigger SquishedSQLHeredocs, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/RedundantReceiverInWithOptions"),
        "plain words in/with/options must not trigger RedundantReceiverInWithOptions, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/DangerousColumnNames"),
        "plain type words must not trigger DangerousColumnNames, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/RootPathnameMethods"),
        "plain pathname method words must not trigger RootPathnameMethods, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/WhereEquals"),
        "plain where/not words must not trigger WhereEquals, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/WhereRange"),
        "plain where/not words must not trigger WhereRange, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/WhereNotWithMultipleConditions"),
        "plain not word must not trigger WhereNotWithMultipleConditions, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/MultipleRoutePaths"),
        "plain route verb words must not trigger MultipleRoutePaths, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/SaveBang"),
        "plain persistence method words must not trigger SaveBang, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/TableNameAssignment"),
        "plain table/name words must not trigger TableNameAssignment, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/TopLevelHashWithIndifferentAccess"),
        "plain with/access words must not trigger TopLevelHashWithIndifferentAccess, got {rails_names:?}"
    );
    assert!(
        !rails_names.contains(&"Rails/SafeNavigationWithBlank"),
        "plain with/blank words must not trigger SafeNavigationWithBlank, got {rails_names:?}"
    );
}

#[test]
#[cfg(not(target_os = "windows"))]
fn rails_pluralization_grammar_uses_numeric_receiver_calls_only() {
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
        dir.path().join("app.rb"),
        "# every day should not be inspected as a duration call\n\nclass App\n  SCHEMA = 'all_day year days'\n\n  def perform\n    1.day.ago\n    2.days.ago\n    1.days.ago\n    3.day.ago\n  end\nend\n",
    )
    .expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("app.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON");
    let pluralization = parsed
        .iter()
        .filter(|offense| offense["cop_name"] == "Rails/PluralizationGrammar")
        .collect::<Vec<_>>();

    assert_eq!(
        pluralization.len(),
        2,
        "only singular/plural mismatched numeric receiver calls should be reported, got {parsed:?}"
    );
    let ranges = pluralization
        .iter()
        .map(|offense| {
            (
                offense["range"]["start_offset"].as_u64().unwrap(),
                offense["range"]["end_offset"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let source = fs::read_to_string(dir.path().join("app.rb")).expect("read source");
    let reported = ranges
        .iter()
        .map(|(start, end)| &source[*start as usize..*end as usize])
        .collect::<Vec<_>>();

    assert_eq!(reported, vec!["days", "day"]);
}
