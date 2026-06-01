#![cfg(feature = "mruby-user-cops")]

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

#[test]
fn cli_runs_mruby_arena_user_cop_from_cops_directory() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::create_dir(root.join("cops")).expect("mkdir cops");
    fs::write(root.join("target.rb"), "logger.info(1)\n").expect("write target");
    fs::write(
        root.join("cops").join("no_logger.rb"),
        r#"
class NoLogger < Murphy::Cop
  def on_send(node)
    return unless node.field(:method) == :info
    add_offense(node.range, message: "no logger info")
  end
end
"#,
    )
    .expect("write cop");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("target.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout is JSON array");
    assert!(
        parsed.iter().any(|o| o["message"] == "no logger info"),
        "mruby user cop offense must be present, got {parsed:?}"
    );
}

#[test]
fn new_cop_and_test_cop_workflow() {
    let dir = tempdir().expect("create tempdir");

    // 1. murphy new-cop Foo/Bar
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.current_dir(dir.path()).arg("new-cop").arg("Foo/Bar");
    let out = cmd.assert().get_output().clone();
    assert_eq!(out.status.code().expect("exit code"), 0);

    let cop_path = dir.path().join("cops/foo_bar.rb");
    let spec_path = dir.path().join("spec/foo_bar_spec.rb");
    assert!(cop_path.exists(), "cops/foo_bar.rb should exist");
    assert!(spec_path.exists(), "spec/foo_bar_spec.rb should exist");

    let cop_content = fs::read_to_string(&cop_path).unwrap();
    let spec_content = fs::read_to_string(&spec_path).unwrap();
    assert!(cop_content.contains("class Bar < Murphy::Cop"));
    assert!(spec_content.contains("describe_cop \"Foo/Bar\""));

    // 重複時のエラー検証
    let mut cmd_dup = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd_dup
        .current_dir(dir.path())
        .arg("new-cop")
        .arg("Foo/Bar");
    let out_dup = cmd_dup.assert().get_output().clone();
    assert_eq!(out_dup.status.code().expect("exit code"), 2);
    assert!(String::from_utf8_lossy(&out_dup.stderr).contains("already exists"));

    // 2. murphy test-cop spec/foo_bar_spec.rb
    let mut cmd_test = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd_test
        .current_dir(dir.path())
        .arg("test-cop")
        .arg("spec/foo_bar_spec.rb");
    let out_test = cmd_test.assert().get_output().clone();
    if out_test.status.code().expect("exit code") != 0 {
        panic!(
            "test-cop failed. stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out_test.stdout),
            String::from_utf8_lossy(&out_test.stderr)
        );
    }
    assert!(String::from_utf8_lossy(&out_test.stdout).contains("All specs passed"));

    // 3. 意図的にスペックを失敗させて test-cop が exit 1 で失敗することを確認
    // spec/foo_bar_spec.rb の内容を書き換えて失敗させる
    let broken_spec = r#"
describe_cop "Foo/Bar" do
  it "registers an offense when using puts" do
    expect_offense(<<~RUBY)
      puts "hello"
      ^^^^ Incorrect message assertion
    RUBY
  end
end
"#;
    fs::write(&spec_path, broken_spec).unwrap();

    let mut cmd_test_fail = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd_test_fail
        .current_dir(dir.path())
        .arg("test-cop")
        .arg("spec/foo_bar_spec.rb");
    let out_test_fail = cmd_test_fail.assert().get_output().clone();
    assert_eq!(out_test_fail.status.code().expect("exit code"), 1);
    assert!(String::from_utf8_lossy(&out_test_fail.stderr).contains("Some specs failed"));
}
