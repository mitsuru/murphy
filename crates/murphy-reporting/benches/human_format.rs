//! Human formatter benchmark shaped after the Mastodon profile: 3,178 Ruby files,
//! with 2,212 offenses across 98 files. Run with
//! `cargo bench -p murphy-reporting --bench human_format`.

use std::fmt::Write as _;
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use murphy_core::{Offense, Range, Severity};
use murphy_reporting::{OutputFormat, format_lint_output};

const FILE_COUNT_WITH_OFFENSES: usize = 98;
const PROJECT_FILE_COUNT: usize = 3_178;
const OFFENSE_COUNT: usize = 2_212;
const LINES_PER_FILE: usize = 1_000;

fn human_format(c: &mut Criterion) {
    let fixture_dir =
        std::env::temp_dir().join(format!("murphy-human-format-bench-{}", std::process::id()));
    std::fs::create_dir_all(&fixture_dir).expect("create benchmark fixture directory");

    let mut files = Vec::with_capacity(PROJECT_FILE_COUNT);
    let mut offenses = Vec::with_capacity(OFFENSE_COUNT);
    for file_index in 0..FILE_COUNT_WITH_OFFENSES {
        let path = fixture_dir.join(format!("file-{file_index}.rb"));
        let file = path.to_string_lossy().into_owned();
        let mut source = String::new();
        let mut line_starts = Vec::with_capacity(LINES_PER_FILE);
        for line in 0..LINES_PER_FILE {
            line_starts.push(source.len() as u32);
            writeln!(&mut source, "value_{line} = {}", line % 10)
                .expect("append benchmark source line");
        }
        std::fs::write(path, source).expect("write benchmark source file");

        let count = OFFENSE_COUNT / FILE_COUNT_WITH_OFFENSES
            + usize::from(file_index < OFFENSE_COUNT % FILE_COUNT_WITH_OFFENSES);
        for offense_index in 0..count {
            let line = (offense_index + 1) * LINES_PER_FILE / (count + 1);
            let start = line_starts[line];
            offenses.push(Offense::new(
                &file,
                "Style/StringLiterals",
                Range {
                    start_offset: start,
                    end_offset: start + 1,
                },
                Severity::Warning,
                "Prefer single-quoted strings when interpolation is not needed.",
            ));
        }
        files.push(file);
    }
    for file_index in FILE_COUNT_WITH_OFFENSES..PROJECT_FILE_COUNT {
        files.push(format!(
            "{}/no-offense-{file_index}.rb",
            fixture_dir.display()
        ));
    }

    let cached = format_lint_output(&offenses, &files, OutputFormat::Human)
        .expect("format cached human output");
    assert_eq!(cached, legacy_human_format(&offenses, &files));

    c.bench_function("human_format_cached_2212_offenses_98_offense_files", |b| {
        b.iter(|| {
            black_box(
                format_lint_output(black_box(&offenses), black_box(&files), OutputFormat::Human)
                    .expect("format human output"),
            )
        });
    });
    c.bench_function(
        "human_format_legacy_rescan_2212_offenses_98_offense_files",
        |b| {
            b.iter(|| black_box(legacy_human_format(black_box(&offenses), black_box(&files))));
        },
    );
    c.bench_function("json_format_2212_offenses_98_offense_files", |b| {
        b.iter(|| {
            black_box(
                format_lint_output(black_box(&offenses), black_box(&files), OutputFormat::Json)
                    .expect("format JSON output"),
            )
        });
    });

    for file in files.iter().take(FILE_COUNT_WITH_OFFENSES) {
        std::fs::remove_file(file).expect("remove benchmark source file");
    }
    std::fs::remove_dir(fixture_dir).expect("remove benchmark fixture directory");
}

/// Reference for the pre-index formatter, kept in the benchmark to make the
/// end-to-end comparison use the same offenses and output as the cached path.
fn legacy_human_format(offenses: &[Offense], files: &[String]) -> String {
    let mut out = String::new();
    let file_count = files.len();
    out.push_str(&format!(
        "Inspecting {file_count} file{}\n",
        plural(file_count)
    ));
    for file in files {
        if offenses.iter().any(|offense| offense.file == *file) {
            out.push('C');
        } else {
            out.push('.');
        }
    }
    out.push_str("\n\n");
    if offenses.is_empty() {
        out.push_str(&format!(
            "{file_count} file{} inspected, no offenses detected",
            plural(file_count)
        ));
    } else {
        out.push_str(&format!(
            "{file_count} file{} inspected, {} offense{} detected",
            plural(file_count),
            offenses.len(),
            plural(offenses.len())
        ));
    }

    if !offenses.is_empty() {
        out.push('\n');
        for offense in offenses {
            let (line, column) =
                legacy_line_column_for_offset(&offense.file, offense.range.start_offset);
            let severity = match offense.severity {
                Severity::Warning => "C",
                Severity::Error => "E",
            };
            out.push_str(&format!(
                "{}:{}:{}: {}: {}: {}\n",
                offense.file, line, column, severity, offense.cop_name, offense.message
            ));
        }
    }
    out
}

fn legacy_line_column_for_offset(path: &str, offset: u32) -> (usize, usize) {
    let Ok(source) = std::fs::read_to_string(path) else {
        return (1, offset as usize + 1);
    };
    let mut line = 1usize;
    let mut line_start = 0usize;
    let offset = offset as usize;
    for (index, byte) in source.as_bytes().iter().enumerate() {
        if index >= offset {
            break;
        }
        if *byte == b'\n' {
            line += 1;
            line_start = index + 1;
        }
    }
    (line, offset.saturating_sub(line_start) + 1)
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

criterion_group!(benches, human_format);
criterion_main!(benches);
