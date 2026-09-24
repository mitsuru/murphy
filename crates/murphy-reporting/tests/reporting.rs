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
fn formats_multiple_human_locations_for_one_file_consistently() {
    let path = std::env::temp_dir().join(format!(
        "murphy-reporting-line-column-{}.rb",
        std::process::id()
    ));
    std::fs::write(&path, "あ\nbar\n").expect("write source");
    let file = path.to_string_lossy().into_owned();
    let files = vec![file.clone()];
    let offenses = vec![
        Offense::new(
            &file,
            "Lint/First",
            Range {
                start_offset: 0,
                end_offset: 1,
            },
            Severity::Warning,
            "first offense",
        ),
        Offense::new(
            &file,
            "Lint/Second",
            Range {
                start_offset: 5,
                end_offset: 6,
            },
            Severity::Warning,
            "second offense",
        ),
    ];

    let output = format_lint_output(&offenses, &files, OutputFormat::Human).expect("format human");
    assert!(output.contains(&format!("{file}:1:1: C: Lint/First: first offense")));
    // Columns are byte-based: the Japanese character occupies three bytes.
    assert!(output.contains(&format!("{file}:2:2: C: Lint/Second: second offense")));
    assert_eq!(
        output,
        format_lint_output(&offenses, &files, OutputFormat::Human).expect("format human again")
    );

    std::fs::remove_file(path).expect("remove source");
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
