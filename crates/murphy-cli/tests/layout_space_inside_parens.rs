//! End-to-end tests for `Layout/SpaceInsideParens` against the compiled
//! `murphy` binary.

use assert_cmd::Command;
use murphy_core::{Edit, Range, apply_edits};
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

fn lint_json_with_config(source: &str, config: &str) -> (i32, Vec<serde_json::Value>) {
    let dir = tempdir().expect("create tempdir");
    fs::write(dir.path().join("murphy.toml"), config).expect("write murphy.toml");
    fs::write(dir.path().join("t.rb"), source).expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
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

fn lint_json_twice_same_path(source: &str) -> (Vec<u8>, Vec<u8>) {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("t.rb");
    fs::write(&path, source).expect("write source");

    let run = || {
        Command::cargo_bin("murphy")
            .expect("murphy binary builds")
            .arg("lint")
            .arg("--format")
            .arg("json")
            .arg(&path)
            .assert()
            .get_output()
            .stdout
            .clone()
    };

    (run(), run())
}

fn offenses_named<'a>(offenses: &'a [serde_json::Value], cop: &str) -> Vec<&'a serde_json::Value> {
    offenses.iter().filter(|o| o["cop_name"] == cop).collect()
}

fn autocorrect_edits(offenses: &[&serde_json::Value]) -> Vec<Edit> {
    offenses
        .iter()
        .flat_map(|offense| {
            offense["autocorrect"]["edits"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .map(|edit| Edit {
            range: Range {
                start_offset: edit["range"]["start_offset"].as_u64().unwrap() as u32,
                end_offset: edit["range"]["end_offset"].as_u64().unwrap() as u32,
            },
            replacement: edit["replacement"].as_str().unwrap().to_string(),
        })
        .collect()
}

#[test]
fn json_contract_is_deterministic() {
    let (first, second) = lint_json_twice_same_path("foo( 1, 2 )\n");

    assert_eq!(
        first, second,
        "same input should produce byte-identical JSON output",
    );
}

#[test]
fn autocorrect_is_idempotent() {
    let source = "foo( 1, 2 )\n";
    let (_code, first_offenses) = lint_json(source);
    let first_edits =
        autocorrect_edits(&offenses_named(&first_offenses, "Layout/SpaceInsideParens"));
    let corrected = apply_edits(source, &first_edits);

    let (_code, second_offenses) = lint_json(&corrected);
    let second_edits = autocorrect_edits(&offenses_named(
        &second_offenses,
        "Layout/SpaceInsideParens",
    ));
    let corrected_again = apply_edits(&corrected, &second_edits);

    assert_eq!(corrected, corrected_again);
}

#[test]
fn flags_spaces_inside_send_parentheses() {
    let (code, offs) = lint_json("foo( 1, 2 )\nbar()\n");
    assert_eq!(code, 1, "offense -> exit 1");
    let parens = offenses_named(&offs, "Layout/SpaceInsideParens");
    assert_eq!(
        parens.len(),
        2,
        "one offense after `(` and one before `)`; got {offs:?}",
    );
    assert!(
        parens
            .iter()
            .all(|offense| offense["message"] == "Space inside parentheses detected."),
        "message should match RuboCop; got {parens:?}",
    );

    let edits: Vec<_> = parens
        .iter()
        .flat_map(|offense| {
            offense["autocorrect"]["edits"]
                .as_array()
                .expect("autocorrect must emit edits")
        })
        .collect();
    assert_eq!(edits.len(), 2);
    assert!(
        edits.iter().all(|edit| edit["replacement"] == ""),
        "edits delete only the extra spaces; got {edits:?}",
    );
}

#[test]
fn space_style_requires_spaces_inside_non_empty_parentheses() {
    let (code, offs) = lint_json_with_config(
        "foo(1)\nbar()\n",
        "[cops.rules.\"Layout/SpaceInsideParens\"]\nEnforcedStyle = \"space\"\n",
    );
    assert_eq!(code, 1);
    let parens = offenses_named(&offs, "Layout/SpaceInsideParens");
    assert_eq!(
        parens.len(),
        2,
        "space style should add one space after `(` and one before `)`; got {offs:?}",
    );
    assert!(
        parens
            .iter()
            .all(|offense| offense["message"] == "No space inside parentheses detected."),
        "space-style message should match RuboCop; got {parens:?}",
    );

    let replacements: Vec<_> = parens
        .iter()
        .flat_map(|offense| {
            offense["autocorrect"]["edits"]
                .as_array()
                .expect("missing-space offenses must autocorrect")
        })
        .map(|edit| edit["replacement"].as_str().unwrap())
        .collect();
    assert_eq!(replacements, vec![" ", " "]);
}

#[test]
fn space_style_removes_space_inside_empty_parentheses() {
    let (_code, offs) = lint_json_with_config(
        "foo( )\n",
        "[cops.rules.\"Layout/SpaceInsideParens\"]\nEnforcedStyle = \"space\"\n",
    );
    let parens = offenses_named(&offs, "Layout/SpaceInsideParens");
    assert_eq!(
        parens.len(),
        1,
        "space style still keeps empty parentheses compact; got {offs:?}",
    );
    let edits = parens[0]["autocorrect"]["edits"]
        .as_array()
        .expect("empty paren offense must autocorrect");
    assert_eq!(edits[0]["replacement"], "");
}

#[test]
fn space_style_removes_tab_inside_empty_parentheses_without_reinserting_space() {
    let (_code, offs) = lint_json_with_config(
        "foo(\t)\n",
        "[cops.rules.\"Layout/SpaceInsideParens\"]\nEnforcedStyle = \"space\"\n",
    );
    let parens = offenses_named(&offs, "Layout/SpaceInsideParens");
    assert_eq!(
        parens.len(),
        1,
        "space style should only remove whitespace inside empty parentheses; got {offs:?}",
    );
    let edits = parens[0]["autocorrect"]["edits"]
        .as_array()
        .expect("empty paren offense must autocorrect");
    assert_eq!(edits[0]["replacement"], "");
}

#[test]
fn compact_style_allows_consecutive_closing_parens_without_space() {
    let (code, offs) = lint_json_with_config(
        "outer(inner(1))\n",
        "[cops.rules.\"Layout/SpaceInsideParens\"]\nEnforcedStyle = \"compact\"\n",
    );
    assert_eq!(code, 1);
    let parens = offenses_named(&offs, "Layout/SpaceInsideParens");
    assert_eq!(
        parens.len(),
        3,
        "compact style should require spaces except between consecutive parens; got {offs:?}",
    );
    assert!(
        parens.iter().all(|offense| {
            offense["range"]["start_offset"] != 14 || offense["range"]["end_offset"] != 14
        }),
        "compact style must not require a space between consecutive `))`; got {parens:?}",
    );
}

#[test]
fn compact_style_removes_multiple_spaces_between_consecutive_closing_parens() {
    let (_code, offs) = lint_json_with_config(
        "outer(inner(1)  )\n",
        "[cops.rules.\"Layout/SpaceInsideParens\"]\nEnforcedStyle = \"compact\"\n",
    );
    let parens = offenses_named(&offs, "Layout/SpaceInsideParens");
    assert!(
        parens.iter().any(|offense| {
            offense["range"]["start_offset"] == 14
                && offense["range"]["end_offset"] == 16
                && offense["autocorrect"]["edits"][0]["replacement"] == ""
        }),
        "compact style should remove the whole whitespace gap between consecutive parens; got {parens:?}",
    );
}

#[test]
fn flags_spaces_inside_grouping_parentheses() {
    let (code, offs) = lint_json("x = ( 1 + 2 )\n");
    assert_eq!(code, 1);
    assert_eq!(
        offenses_named(&offs, "Layout/SpaceInsideParens").len(),
        2,
        "grouping parens should be checked; got {offs:?}",
    );
}

#[test]
fn flags_spaces_inside_def_argument_parentheses() {
    let (code, offs) = lint_json("def foo( a, b )\nend\n");
    assert_eq!(code, 1);
    assert_eq!(
        offenses_named(&offs, "Layout/SpaceInsideParens").len(),
        2,
        "method definition parens should be checked; got {offs:?}",
    );
}

#[test]
fn does_not_flag_clean_parentheses_or_block_params() {
    let (_code, offs) = lint_json("foo(1, 2)\nitems.each { | x | x }\n");
    assert!(
        offenses_named(&offs, "Layout/SpaceInsideParens").is_empty(),
        "clean parens and block params must not be flagged; got {offs:?}",
    );
}

#[test]
fn does_not_flag_space_before_comment_after_open_paren() {
    let (_code, offs) = lint_json("foo( # inline comment\n  1\n)\n");
    assert!(
        offenses_named(&offs, "Layout/SpaceInsideParens").is_empty(),
        "space before an opening-paren comment is not inside-paren spacing; got {offs:?}",
    );
}

#[test]
fn does_not_flag_multiline_parentheses() {
    let (_code, offs) = lint_json("foo(\n  1\n)\nx = (\n  1 + 2\n)\n");
    assert!(
        offenses_named(&offs, "Layout/SpaceInsideParens").is_empty(),
        "newlines inside parentheses must not be reported as inline spacing; got {offs:?}",
    );
}

#[test]
fn does_not_flag_heredoc_argument_parentheses() {
    let (_code, offs) = lint_json("foo(<<~TEXT)\n  body\nTEXT\n");
    assert!(
        offenses_named(&offs, "Layout/SpaceInsideParens").is_empty(),
        "heredoc argument parentheses without inline spaces must stay clean; got {offs:?}",
    );
}

#[test]
fn does_not_flag_string_interpolation_braces() {
    let (_code, offs) = lint_json("name = \"#{ value }\"\n");
    assert!(
        offenses_named(&offs, "Layout/SpaceInsideParens").is_empty(),
        "interpolation braces are not parentheses; got {offs:?}",
    );
}

#[test]
fn config_disabled_silences_the_cop() {
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join("murphy.toml"),
        "[cops.rules.\"Layout/SpaceInsideParens\"]\nenabled = false\n",
    )
    .expect("write murphy.toml");
    fs::write(dir.path().join("t.rb"), "foo( 1 )\n").expect("write source");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("t.rb")
        .assert();
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be JSON");
    assert!(
        offenses_named(&parsed, "Layout/SpaceInsideParens").is_empty(),
        "user-disabled cop must produce no offenses; got {parsed:?}",
    );
}
