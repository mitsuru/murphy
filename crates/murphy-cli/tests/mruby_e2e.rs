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
