//! B9 human-review report tests: `--format markdown` / `--format html`.
//!
//! Each report is exercised for: empty input, a located warning, error
//! severity counts, a filepath-only (no-location) offense, B4 docs-URL
//! linking, escaping, sorted grouping, and real-file byte positions.
//! Line/column positions use the historical fallback `(1, offset + 1)` for
//! synthetic paths.

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

fn linked_offense() -> Offense {
    let mut o = sample_offense();
    o.documentation_url = Some("https://murphy.dev/docs/cops/Lint/Debugger".to_string());
    o
}

// -- markdown ---------------------------------------------------------------

#[test]
fn markdown_empty_has_header_and_zero_total() {
    let out = format_lint_output(&[], &[], OutputFormat::Markdown).expect("markdown");
    assert!(out.starts_with("# Murphy report\n"), "got: {out}");
    assert!(out.contains("no offenses detected"), "got: {out}");
    assert!(out.contains("| Total | 0 | 0 | 0 |"), "got: {out}");
    assert!(!out.contains("## `"), "no per-file sections, got: {out}");
}

#[test]
fn markdown_single_warning() {
    let out = format_lint_output(
        &[sample_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Markdown,
    )
    .expect("markdown");
    assert!(out.contains("# Murphy report"), "got: {out}");
    assert!(
        out.contains("1 file inspected, 1 offense detected"),
        "got: {out}"
    );
    assert!(out.contains("| dirty.rb | 1 | 0 | 1 |"), "got: {out}");
    assert!(out.contains("## `dirty.rb` (1 offense)"), "got: {out}");
    assert!(
        out.contains("- `dirty.rb:1:1` **Lint/Debugger** (warning): Remove debugger entry point"),
        "got: {out}"
    );
}

#[test]
fn markdown_counts_warnings_and_errors() {
    let out = format_lint_output(
        &[sample_offense(), error_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Markdown,
    )
    .expect("markdown");
    assert!(
        out.contains("2 offenses detected (1 warning, 1 error)"),
        "got: {out}"
    );
    assert!(out.contains("| dirty.rb | 1 | 1 | 2 |"), "got: {out}");
    assert!(out.contains("| Total | 1 | 1 | 2 |"), "got: {out}");
}

#[test]
fn markdown_no_location_omits_line_col() {
    let out = format_lint_output(
        &[file_only_offense()],
        &["whitelist.rb".to_string()],
        OutputFormat::Markdown,
    )
    .expect("markdown");
    assert!(out.contains("## `whitelist.rb` (1 offense)"), "got: {out}");
    assert!(
        out.contains("- `whitelist.rb` **Naming/InclusiveLanguage** (warning):"),
        "got: {out}"
    );
    assert!(!out.contains("whitelist.rb:1:1"), "got: {out}");
}

#[test]
fn markdown_docs_url_renders_cop_link() {
    let out = format_lint_output(
        &[linked_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Markdown,
    )
    .expect("markdown");
    assert!(
        out.contains("**[Lint/Debugger](https://murphy.dev/docs/cops/Lint/Debugger)**"),
        "got: {out}"
    );
}

#[test]
fn markdown_escapes_table_pipes_and_collapses_message_lines() {
    let offense = Offense::new(
        "a|b.rb",
        "Lint/X",
        Range {
            start_offset: 0,
            end_offset: 1,
        },
        Severity::Warning,
        "first\nsecond",
    );
    let out = format_lint_output(&[offense], &["a|b.rb".to_string()], OutputFormat::Markdown)
        .expect("markdown");
    assert!(out.contains("| a\\|b.rb | 1 | 0 | 1 |"), "got: {out}");
    assert!(out.contains("first second"), "got: {out}");
    assert!(!out.contains("first\nsecond"), "got: {out}");
}

#[test]
fn markdown_groups_files_in_sorted_order() {
    let b = Offense::new(
        "b.rb",
        "Lint/B",
        Range {
            start_offset: 0,
            end_offset: 1,
        },
        Severity::Warning,
        "b message",
    );
    let a = Offense::new(
        "a.rb",
        "Lint/A",
        Range {
            start_offset: 0,
            end_offset: 1,
        },
        Severity::Warning,
        "a message",
    );
    // Input in reverse order; output must still be sorted by file.
    let out = format_lint_output(
        &[b, a],
        &["b.rb".to_string(), "a.rb".to_string()],
        OutputFormat::Markdown,
    )
    .expect("markdown");
    let pos_a = out.find("## `a.rb`").expect("a section");
    let pos_b = out.find("## `b.rb`").expect("b section");
    assert!(pos_a < pos_b, "sorted sections, got: {out}");
}

#[test]
fn markdown_clean_files_count_but_omit_from_table() {
    let out = format_lint_output(
        &[sample_offense()],
        &["dirty.rb".to_string(), "clean.rb".to_string()],
        OutputFormat::Markdown,
    )
    .expect("markdown");
    assert!(
        out.contains("2 files inspected, 1 offense detected"),
        "got: {out}"
    );
    assert!(!out.contains("clean.rb"), "clean files omitted, got: {out}");
}

// -- html -------------------------------------------------------------------

#[test]
fn html_empty_is_standalone_document() {
    let out = format_lint_output(&[], &[], OutputFormat::Html).expect("html");
    assert!(out.starts_with("<!DOCTYPE html>"), "got: {out}");
    assert!(out.contains("<title>Murphy report</title>"), "got: {out}");
    assert!(out.contains("no offenses detected"), "got: {out}");
    assert!(out.contains("No offenses detected."), "got: {out}");
    assert!(out.contains("<strong>Total</strong>"), "got: {out}");
    assert!(!out.contains("<h3>"), "no per-file sections, got: {out}");
    assert!(out.ends_with("</html>"), "got: {out}");
}

#[test]
fn html_single_warning() {
    let out = format_lint_output(
        &[sample_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Html,
    )
    .expect("html");
    assert!(out.contains("<h1>Murphy report</h1>"), "got: {out}");
    assert!(
        out.contains("1 file inspected, 1 offense detected"),
        "got: {out}"
    );
    assert!(out.contains("<code>dirty.rb</code>"), "got: {out}");
    assert!(out.contains("<strong>Lint/Debugger</strong>"), "got: {out}");
    assert!(out.contains("<code>dirty.rb:1:1</code>"), "got: {out}");
    assert!(out.contains("Remove debugger entry point"), "got: {out}");
    assert!(out.contains("Generated by murphy"), "got: {out}");
}

#[test]
fn html_error_severity_class() {
    let out = format_lint_output(
        &[error_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Html,
    )
    .expect("html");
    assert!(
        out.contains(r#"<span class="error">error</span>"#),
        "got: {out}"
    );
    assert!(out.contains("Murphy/Syntax"), "got: {out}");
}

#[test]
fn html_no_location_omits_line_col() {
    let out = format_lint_output(
        &[file_only_offense()],
        &["whitelist.rb".to_string()],
        OutputFormat::Html,
    )
    .expect("html");
    assert!(out.contains("<code>whitelist.rb</code>"), "got: {out}");
    assert!(!out.contains("whitelist.rb:1:1"), "got: {out}");
}

#[test]
fn html_docs_url_renders_cop_link() {
    let out = format_lint_output(
        &[linked_offense()],
        &["dirty.rb".to_string()],
        OutputFormat::Html,
    )
    .expect("html");
    assert!(
        out.contains(r#"<a href="https://murphy.dev/docs/cops/Lint/Debugger"><strong>Lint/Debugger</strong></a>"#),
        "got: {out}"
    );
}

#[test]
fn html_escapes_markup() {
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
    let out =
        format_lint_output(&[offense], &["a&b.rb".to_string()], OutputFormat::Html).expect("html");
    assert!(out.contains("a&amp;b.rb"), "got: {out}");
    assert!(
        out.contains("use &lt;a&gt; &amp; &quot;b&quot;"),
        "got: {out}"
    );
}

#[test]
fn html_and_markdown_agree_on_real_file_positions() {
    let path = std::env::temp_dir().join(format!(
        "murphy-human-reports-pos-{}.rb",
        std::process::id()
    ));
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

    let md = format_lint_output(
        std::slice::from_ref(&offense),
        &files,
        OutputFormat::Markdown,
    )
    .expect("markdown");
    assert!(
        md.contains(&format!("`{file}:2:1`")),
        "markdown position, got: {md}"
    );

    let html = format_lint_output(std::slice::from_ref(&offense), &files, OutputFormat::Html)
        .expect("html");
    assert!(
        html.contains(&format!("<code>{file}:2:1</code>")),
        "html position, got: {html}"
    );

    std::fs::remove_file(path).expect("remove source");
}
