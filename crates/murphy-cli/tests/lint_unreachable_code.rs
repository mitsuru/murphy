//! End-to-end tests for `Lint/UnreachableCode` against the compiled
//! `murphy` binary (murphy-9cr.23 §12d).
//!
//! Contract anchored: a statement that follows a flow-terminator
//! (`return` / `break` / `next` / receiver-less `raise`) as a *direct
//! sibling* inside the same `Begin` block is unreachable. Statements
//! after a terminator nested inside a conditional are **not**
//! unreachable — the terminator does not always fire.

use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn lint_json(source: &str) -> (i32, Vec<serde_json::Value>) {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("t.rb");
    fs::write(&path, source).expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
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
fn flags_statement_after_return_in_def_body() {
    // The def body is a Begin([return, puts]); `puts` is unreachable.
    let (code, offs) = lint_json("def foo\n  return\n  puts 'x'\nend\n");
    assert_eq!(code, 1, "offenses present → exit 1");
    let dead = offenses_named(&offs, "Lint/UnreachableCode");
    assert_eq!(
        dead.len(),
        1,
        "exactly one unreachable-code offense for `puts` after `return`; got {offs:?}",
    );
}

#[test]
fn does_not_flag_statement_after_return_nested_in_if() {
    // `return` is inside the `if` branch — `puts` after the `if` is
    // still reachable because the conditional does not always fire.
    // This is the load-bearing nested-scope case advisor flagged.
    let (code, offs) = lint_json("def foo\n  if x\n    return\n  end\n  puts 'x'\nend\n");
    let dead = offenses_named(&offs, "Lint/UnreachableCode");
    assert!(
        dead.is_empty(),
        "must NOT flag statements after a conditional return; got: {offs:?}, exit={code}",
    );
}

#[test]
fn flags_dead_code_after_raise_method_call() {
    // `raise` is a Kernel method — appears in the arena as a Send with
    // no receiver. The cop must recognise it as a terminator.
    let (code, offs) = lint_json("def foo\n  raise 'bad'\n  puts 'x'\nend\n");
    assert_eq!(code, 1);
    assert_eq!(
        offenses_named(&offs, "Lint/UnreachableCode").len(),
        1,
        "dead code after `raise` must be flagged; got {offs:?}",
    );
}

#[test]
fn flags_dead_code_after_break_in_loop_body() {
    let (code, offs) = lint_json("while x\n  break\n  puts 'x'\nend\n");
    assert_eq!(code, 1);
    assert_eq!(
        offenses_named(&offs, "Lint/UnreachableCode").len(),
        1,
        "dead code after `break` must be flagged; got {offs:?}",
    );
}

#[test]
fn flags_dead_code_after_next() {
    let (code, offs) = lint_json("while x\n  next\n  puts 'x'\nend\n");
    assert_eq!(code, 1);
    assert_eq!(
        offenses_named(&offs, "Lint/UnreachableCode").len(),
        1,
        "dead code after `next` must be flagged; got {offs:?}",
    );
}

#[test]
fn emits_one_offense_per_dead_sibling() {
    // Two statements follow the terminator; both are unreachable.
    let (code, offs) = lint_json("def foo\n  return\n  puts 'a'\n  puts 'b'\nend\n");
    assert_eq!(code, 1);
    assert_eq!(
        offenses_named(&offs, "Lint/UnreachableCode").len(),
        2,
        "each dead sibling must produce its own offense; got {offs:?}",
    );
}

#[test]
fn does_not_flag_clean_body_where_return_is_last_statement() {
    let (_code, offs) = lint_json("def foo\n  do_work\n  return\nend\n");
    assert!(
        offenses_named(&offs, "Lint/UnreachableCode").is_empty(),
        "no offense when `return` is the last statement; got {offs:?}",
    );
}

#[test]
fn does_not_flag_explicit_receiver_raise() {
    // `obj.raise` is *not* Kernel#raise — it is some user-defined method
    // on `obj`, with no flow-terminator semantics. The terminator gate
    // must check for a receiver-less call.
    let (_code, offs) = lint_json("def foo\n  obj.raise 'msg'\n  puts 'still runs'\nend\n");
    assert!(
        offenses_named(&offs, "Lint/UnreachableCode").is_empty(),
        "explicit-receiver call must not be treated as a terminator; got {offs:?}",
    );
}
