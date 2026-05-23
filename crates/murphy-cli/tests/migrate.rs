use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

#[test]
fn migrate_rubocop_yml_to_murphy_toml_stdout() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(
        root.join(".rubocop.yml"),
        r#"AllCops:
  Include:
    - "lib/**/*.rb"
  Exclude:
    - "vendor/**"
Style/NoPuts:
  Enabled: false
  Severity: error
"#,
    )
    .expect("write rubocop config");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("migrate")
        .arg(".rubocop.yml")
        .assert()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    assert!(stdout.contains("[files]"), "got {stdout}");
    assert!(
        stdout.contains("include = [\"lib/**/*.rb\"]"),
        "got {stdout}"
    );
    assert!(stdout.contains("exclude = [\"vendor/**\"]"), "got {stdout}");
    assert!(
        stdout.contains("[cops.rules.\"Style/NoPuts\"]"),
        "got {stdout}"
    );
    assert!(stdout.contains("enabled = false"), "got {stdout}");
    assert!(stdout.contains("severity = \"error\""), "got {stdout}");
}

#[test]
fn migrated_output_roundtrips_to_lint_behavior() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(
        root.join(".rubocop.yml"),
        r#"Murphy/NoReceiverPuts:
  Enabled: false
"#,
    )
    .expect("write rubocop config");

    let migrate = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("migrate")
        .arg(".rubocop.yml")
        .assert()
        .code(0);
    fs::write(root.join("murphy.toml"), &migrate.get_output().stdout)
        .expect("write migrated murphy.toml");
    fs::write(
        root.join("dirty.rb"),
        "# frozen_string_literal: true\n\nputs 'hi'\n",
    )
    .expect("write dirty.rb");

    let lint = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .assert()
        .code(0);
    assert_eq!(lint.get_output().stdout, b"[]\n");
}

#[test]
fn migrate_missing_file_exits_2() {
    let dir = tempdir().expect("create tempdir");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("migrate")
        .arg(".rubocop.yml")
        .assert()
        .code(2);

    assert!(assert.get_output().stdout.is_empty());
}

#[test]
fn migrate_malformed_rubocop_yml_exits_2() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(root.join(".rubocop.yml"), "AllCops: [\n").expect("write malformed yaml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("migrate")
        .arg(".rubocop.yml")
        .assert()
        .code(2);

    assert!(assert.get_output().stdout.is_empty());
}

#[test]
fn migrate_translates_plugins_directive_preserving_names() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(
        root.join(".rubocop.yml"),
        r#"plugins:
  - rubocop-rails
  - rubocop-rspec
AllCops:
  Include: ['**/*.rb']
"#,
    )
    .expect("write rubocop config");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("migrate")
        .arg(".rubocop.yml")
        .assert()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    // name は preserve (auto-rename しない)
    assert!(
        stdout.contains("plugins = [\"rubocop-rails\", \"rubocop-rspec\"]"),
        "stdout missing plugins line:\n{stdout}"
    );
    // [files] section も出ること (regression)
    assert!(
        stdout.contains("[files]"),
        "stdout missing [files]:\n{stdout}"
    );
}

#[test]
fn migrate_plugins_mapping_form_emits_unsupported_comment() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(
        root.join(".rubocop.yml"),
        r#"plugins:
  - rubocop-rails
  - foo:
      option: x
AllCops:
  Include: ['**/*.rb']
"#,
    )
    .expect("write rubocop config");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("migrate")
        .arg(".rubocop.yml")
        .assert()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    // mapping form は unsupported コメントとして出力される
    assert!(
        stdout.contains("# unsupported plugin entry: foo"),
        "stdout missing unsupported comment:\n{stdout}"
    );
    // mapping form の name は plugins 行には含まれない
    assert!(
        stdout.contains("plugins = [\"rubocop-rails\"]"),
        "stdout should list only string-form plugins:\n{stdout}"
    );
    assert!(
        !stdout.contains("\"foo\""),
        "stdout should not list foo as a plugin name:\n{stdout}"
    );
}

#[test]
fn migrate_inline_yaml_arrays() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(
        root.join(".rubocop.yml"),
        r#"AllCops:
  Include: ["lib/**/*.rb"]
  Exclude: ["vendor/**"]
"#,
    )
    .expect("write rubocop config");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("migrate")
        .arg(".rubocop.yml")
        .assert()
        .code(0);

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    assert!(
        stdout.contains("include = [\"lib/**/*.rb\"]"),
        "got {stdout}"
    );
    assert!(stdout.contains("exclude = [\"vendor/**\"]"), "got {stdout}");
}
