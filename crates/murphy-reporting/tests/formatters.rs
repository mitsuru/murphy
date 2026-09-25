//! B7 output formatter tests: checkstyle / SARIF / JUnit / GitHub / GNU / TAP.
//!
//! Each formatter is exercised for: empty input, a located warning, an error
//! severity mapping, a filepath-only (no-location) offense, and XML/escaping
//! where applicable. Line/column positions use the historical fallback
//! `(1, offset + 1)` for synthetic paths, plus one real-file case proving
//! byte-based file reads.

use murphy_core::{Offense, Range, Severity};
use murphy_reporting::{OutputFormat, format_lint_output};

fn sample_offense() -> Offense {
    Offense::new(
        "dirty.rb",
        "Lint/Debugger",
        Range {
            start_offset: 0,
            end_offset: 8,
        },
        Severity::Warning,
        "Remove debugger entry point `debugger`.",
    )
}

fn error_offense() -> Offense {
    Offense::new(
        "dirty.rb",
        "Murphy/Syntax",
        Range {
            start_offset: 0,
            end_offset: 1,
        },
        Severity::Error,
        "unexpected token",
    )
}

fn file_only_offense() -> Offense {
    Offense::new_without_location(
        "whitelist.rb",
        "Naming/InclusiveLanguage",
        Severity::Warning,
        "Consider replacing 'whitelist' in file path.",
    )
}

// ── checkstyle ─────────────────────────────────────────────────────────────

#[test]
fn checkstyle_empty_has_no_files() {
    let out = format_lint_output(&[], &[], OutputFormat::Checkstyle).expect("checkstyle");
    assert!(out.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(out.contains("<checkstyle>"));
    assert!(out.contains("</checkstyle>"));
    assert!(!out.contains("<file"));
}

#[test]
fn checkstyle_single_warning() {
    let out = format_lint_output(
        &[sample_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Checkstyle,
    )
    .expect("checkstyle");
    assert!(out.contains("<file name=\"dirty.rb\">"));
    assert!(out.contains("line=\"1\""));
    assert!(out.contains("column=\"1\""));
    assert!(out.contains("severity=\"warning\""));
    assert!(out.contains("source=\"Lint/Debugger\""));
    assert!(out.contains("Remove debugger entry point"));
}

#[test]
fn checkstyle_error_severity() {
    let out = format_lint_output(
        &[error_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Checkstyle,
    )
    .expect("checkstyle");
    assert!(out.contains("severity=\"error\""));
    assert!(out.contains("source=\"Murphy/Syntax\""));
}

#[test]
fn checkstyle_no_location_omits_line_column() {
    let out = format_lint_output(
        &[file_only_offense()],
        &["whitelist.rb".to_string()],
        OutputFormat::Checkstyle,
    )
    .expect("checkstyle");
    assert!(out.contains("<file name=\"whitelist.rb\">"));
    assert!(out.contains("source=\"Naming/InclusiveLanguage\""));
    // The error element for a no-location offense must not fabricate 1:1.
    let error_line = out
        .lines()
        .find(|l| l.contains("<error"))
        .expect("error line");
    assert!(!error_line.contains("line="), "got: {error_line}");
    assert!(!error_line.contains("column="), "got: {error_line}");
}

#[test]
fn checkstyle_escapes_xml() {
    let offense = Offense::new(
        "a&b.rb",
        "Lint/X",
        Range {
            start_offset: 0,
            end_offset: 1,
        },
        Severity::Warning,
        "use <a> & \"b\"",
    );
    let out = format_lint_output(
        &[offense],
        &["a&b.rb".to_string()],
        OutputFormat::Checkstyle,
    )
    .expect("checkstyle");
    assert!(out.contains("a&amp;b.rb"));
    assert!(out.contains("use &lt;a&gt; &amp; &quot;b&quot;"));
}

// ── SARIF ──────────────────────────────────────────────────────────────────

#[test]
fn sarif_empty_is_valid_skeleton() {
    let out = format_lint_output(&[], &[], OutputFormat::Sarif).expect("sarif");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid sarif json");
    assert_eq!(v["version"], "2.1.0");
    assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "murphy");
    assert_eq!(v["runs"][0]["results"].as_array().unwrap().len(), 0);
}

#[test]
fn sarif_single_warning_with_region() {
    let out = format_lint_output(
        &[sample_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Sarif,
    )
    .expect("sarif");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid sarif json");
    let result = &v["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "Lint/Debugger");
    assert_eq!(result["level"], "warning");
    assert_eq!(
        result["message"]["text"],
        "Remove debugger entry point `debugger`."
    );
    assert_eq!(
        result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "dirty.rb"
    );
    let region = &result["locations"][0]["physicalLocation"]["region"];
    assert_eq!(region["startLine"], 1);
    assert_eq!(region["startColumn"], 1);
}

#[test]
fn sarif_error_level() {
    let out = format_lint_output(
        &[error_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Sarif,
    )
    .expect("sarif");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid sarif json");
    assert_eq!(v["runs"][0]["results"][0]["level"], "error");
}

#[test]
fn sarif_no_location_omits_region() {
    let out = format_lint_output(
        &[file_only_offense()],
        &["whitelist.rb".to_string()],
        OutputFormat::Sarif,
    )
    .expect("sarif");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid sarif json");
    let loc = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"];
    assert_eq!(loc["artifactLocation"]["uri"], "whitelist.rb");
    assert!(
        loc.get("region").is_none(),
        "no-location must omit region, got {loc:?}"
    );
}

#[test]
fn sarif_rules_deduplicated() {
    let out = format_lint_output(
        &[sample_offense(), sample_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Sarif,
    )
    .expect("sarif");
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid sarif json");
    let rules = v["runs"][0]["tool"]["driver"]["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["id"], "Lint/Debugger");
    assert_eq!(v["runs"][0]["results"].as_array().unwrap().len(), 2);
}

// ── JUnit ──────────────────────────────────────────────────────────────────

#[test]
fn junit_empty_suite() {
    let out = format_lint_output(&[], &[], OutputFormat::Junit).expect("junit");
    assert!(out.contains("tests=\"0\""));
    assert!(out.contains("failures=\"0\""));
    assert!(!out.contains("<testcase"));
}

#[test]
fn junit_single_warning() {
    let out = format_lint_output(
        &[sample_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Junit,
    )
    .expect("junit");
    assert!(out.contains("tests=\"1\""));
    assert!(out.contains("failures=\"1\""));
    assert!(out.contains("classname=\"dirty.rb\""));
    assert!(out.contains("Lint/Debugger"));
    assert!(out.contains("<failure"));
}

#[test]
fn junit_no_location_body_has_no_line_col() {
    let out = format_lint_output(
        &[file_only_offense()],
        &["whitelist.rb".to_string()],
        OutputFormat::Junit,
    )
    .expect("junit");
    assert!(out.contains("classname=\"whitelist.rb\""));
    assert!(out.contains("whitelist.rb: Consider replacing"));
    assert!(!out.contains("whitelist.rb:1:1"));
}

#[test]
fn junit_escapes_xml() {
    let offense = Offense::new(
        "dirty.rb",
        "Lint/X",
        Range {
            start_offset: 0,
            end_offset: 1,
        },
        Severity::Warning,
        "bad <tag> & stuff",
    );
    let out = format_lint_output(&[offense], &["dirty.rb".to_string()], OutputFormat::Junit)
        .expect("junit");
    assert!(out.contains("bad &lt;tag&gt; &amp; stuff"));
}

// ── GitHub ─────────────────────────────────────────────────────────────────

#[test]
fn github_empty_is_empty() {
    let out = format_lint_output(&[], &[], OutputFormat::Github).expect("github");
    assert_eq!(out, "");
}

#[test]
fn github_single_warning() {
    let out = format_lint_output(
        &[sample_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Github,
    )
    .expect("github");
    assert_eq!(
        out,
        "::warning file=dirty.rb,line=1,col=1,title=Lint/Debugger::Remove debugger entry point `debugger`."
    );
}

#[test]
fn github_error_command() {
    let out = format_lint_output(
        &[error_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Github,
    )
    .expect("github");
    assert!(out.starts_with("::error file=dirty.rb,line=1,col=1,title=Murphy/Syntax::"));
}

#[test]
fn github_no_location_omits_line_col() {
    let out = format_lint_output(
        &[file_only_offense()],
        &["whitelist.rb".to_string()],
        OutputFormat::Github,
    )
    .expect("github");
    assert_eq!(
        out,
        "::warning file=whitelist.rb,title=Naming/InclusiveLanguage::Consider replacing 'whitelist' in file path."
    );
}

#[test]
fn github_escapes_percent_and_newlines() {
    let offense = Offense::new(
        "dirty.rb",
        "Lint/X",
        Range {
            start_offset: 0,
            end_offset: 1,
        },
        Severity::Warning,
        "100% bad\nsecond line",
    );
    let out = format_lint_output(&[offense], &["dirty.rb".to_string()], OutputFormat::Github)
        .expect("github");
    assert!(out.contains("100%25 bad%0Asecond line"), "got: {out}");
}

// ── GNU ────────────────────────────────────────────────────────────────────

#[test]
fn gnu_empty_is_empty() {
    let out = format_lint_output(&[], &[], OutputFormat::Gnu).expect("gnu");
    assert_eq!(out, "");
}

#[test]
fn gnu_single_warning() {
    let out = format_lint_output(
        &[sample_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Gnu,
    )
    .expect("gnu");
    assert_eq!(
        out,
        "dirty.rb:1:1: warning: Remove debugger entry point `debugger`. [Lint/Debugger]"
    );
}

#[test]
fn gnu_error_word() {
    let out = format_lint_output(
        &[error_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Gnu,
    )
    .expect("gnu");
    assert!(out.contains(": error: "), "got: {out}");
}

#[test]
fn gnu_no_location() {
    let out = format_lint_output(
        &[file_only_offense()],
        &["whitelist.rb".to_string()],
        OutputFormat::Gnu,
    )
    .expect("gnu");
    assert_eq!(
        out,
        "whitelist.rb: warning: Consider replacing 'whitelist' in file path. [Naming/InclusiveLanguage]"
    );
}

// ── TAP ────────────────────────────────────────────────────────────────────

#[test]
fn tap_empty_plan() {
    let out = format_lint_output(&[], &[], OutputFormat::Tap).expect("tap");
    assert_eq!(out, "TAP version 13\n1..0\n# No offenses detected");
}

#[test]
fn tap_single_offense() {
    let out = format_lint_output(
        &[sample_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Tap,
    )
    .expect("tap");
    assert_eq!(
        out,
        "TAP version 13\n1..1\nnot ok 1 - dirty.rb:1:1: Lint/Debugger: Remove debugger entry point `debugger`."
    );
}

#[test]
fn tap_multiple_numbered() {
    let out = format_lint_output(
        &[sample_offense(), error_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Tap,
    )
    .expect("tap");
    assert!(out.starts_with("TAP version 13\n1..2\n"));
    assert!(out.contains("not ok 1 - "));
    assert!(out.contains("not ok 2 - "));
}

#[test]
fn tap_no_location() {
    let out = format_lint_output(
        &[file_only_offense()],
        &["whitelist.rb".to_string()],
        OutputFormat::Tap,
    )
    .expect("tap");
    assert!(out.contains("not ok 1 - whitelist.rb: Naming/InclusiveLanguage: Consider replacing"));
}

// ── real-file line/col ─────────────────────────────────────────────────────

#[test]
fn formatters_agree_on_real_file_positions() {
    let path =
        std::env::temp_dir().join(format!("murphy-formatters-pos-{}.rb", std::process::id()));
    std::fs::write(&path, "ab\ncde\n").expect("write source");
    let file = path.to_string_lossy().into_owned();
    let offense = Offense::new(
        &file,
        "Lint/X",
        Range {
            start_offset: 3,
            end_offset: 4,
        },
        Severity::Warning,
        "second line",
    );
    let files = vec![file.clone()];

    let gnu =
        format_lint_output(std::slice::from_ref(&offense), &files, OutputFormat::Gnu).expect("gnu");
    assert!(
        gnu.contains(&format!("{file}:2:1: warning: second line [Lint/X]")),
        "got: {gnu}"
    );

    let github = format_lint_output(std::slice::from_ref(&offense), &files, OutputFormat::Github)
        .expect("github");
    assert!(github.contains("line=2,col=1"), "got: {github}");

    let checkstyle = format_lint_output(
        std::slice::from_ref(&offense),
        &files,
        OutputFormat::Checkstyle,
    )
    .expect("checkstyle");
    assert!(
        checkstyle.contains("line=\"2\" column=\"1\""),
        "got: {checkstyle}"
    );

    std::fs::remove_file(path).expect("remove source");
}

// ── JSON frozen ────────────────────────────────────────────────────────────

#[test]
fn json_contract_unchanged_by_formatter_expansion() {
    let out = format_lint_output(
        &[sample_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Json,
    )
    .expect("json");
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid json");
    assert_eq!(parsed[0]["file"], "dirty.rb");
    assert_eq!(parsed[0]["cop_name"], "Lint/Debugger");
    assert_eq!(parsed[0]["range"]["start_offset"], 0);
    assert_eq!(parsed[0]["range"]["end_offset"], 8);
    assert_eq!(parsed[0]["severity"], "warning");
    assert!(parsed[0].get("autocorrect").is_none());
}
