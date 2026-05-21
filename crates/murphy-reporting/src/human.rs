use murphy_core::{Offense, Severity};

pub fn format(offenses: &[Offense], files: &[String]) -> Result<String, String> {
    let mut out = super::progress::format(offenses, files)?;

    if !offenses.is_empty() {
        out.push('\n');
        for offense in offenses {
            let (line, column) = line_column_for_offset(&offense.file, offense.range.start_offset);
            out.push_str(&format!(
                "{}:{}:{}: {}: {}: {}\n",
                offense.file,
                line,
                column,
                severity_label(offense.severity),
                offense.cop_name,
                offense.message
            ));
        }
    }

    Ok(out)
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "C",
        Severity::Error => "E",
    }
}

fn line_column_for_offset(path: &str, offset: u32) -> (usize, usize) {
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
