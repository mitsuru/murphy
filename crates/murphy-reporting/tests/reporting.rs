use murphy_core::{Offense, Range, Severity};
use murphy_reporting::{OutputFormat, format_lint_output};

#[test]
fn formats_json_offenses_for_machine_consumers() {
    let offense = sample_offense();

    let output = format_lint_output(&[offense], &["dirty.rb".to_string()], OutputFormat::Json)
        .expect("format json");

    let parsed: serde_json::Value = serde_json::from_str(&output).expect("valid json");
    assert_eq!(parsed[0]["cop_name"], "Lint/Debugger");
}

#[test]
fn formats_human_output_with_progress_and_details() {
    let offense = sample_offense();

    let output = format_lint_output(&[offense], &["dirty.rb".to_string()], OutputFormat::Human)
        .expect("format human");

    assert!(output.contains("Inspecting 1 file"));
    assert!(output.contains("C"));
    assert!(output.contains("Lint/Debugger"));
}

#[test]
fn formats_progress_without_offense_details() {
    let offense = sample_offense();

    let output = format_lint_output(
        &[offense],
        &["dirty.rb".to_string()],
        OutputFormat::Progress,
    )
    .expect("format progress");

    assert!(output.contains("Inspecting 1 file"));
    assert!(output.contains("1 offense detected"));
    assert!(!output.contains("Lint/Debugger"));
}

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
