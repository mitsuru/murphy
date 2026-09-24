use std::collections::HashMap;

use murphy_core::{Offense, Severity};

pub fn format(offenses: &[Offense], files: &[String]) -> Result<String, String> {
    let mut out = super::progress::format(offenses, files)?;

    if !offenses.is_empty() {
        out.push('\n');
        let mut line_indexes: HashMap<&str, Option<LineColumnIndex>> = HashMap::new();
        for offense in offenses {
            let line_index = line_indexes
                .entry(offense.file.as_str())
                .or_insert_with(|| LineColumnIndex::from_path(&offense.file));
            let (line, column) =
                line_column_for_offset(line_index.as_ref(), offense.range.start_offset);
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

/// Newline offsets for one source file. A single O(N) build supports O(log L)
/// line/column lookups for each of its offenses, where L is the number of lines.
struct LineColumnIndex {
    newline_offsets: Vec<usize>,
}

impl LineColumnIndex {
    fn from_path(path: &str) -> Option<Self> {
        let source = std::fs::read_to_string(path).ok()?;
        Some(Self::new(&source))
    }

    fn new(source: &str) -> Self {
        let newline_offsets = source
            .as_bytes()
            .iter()
            .enumerate()
            .filter_map(|(offset, &byte)| (byte == b'\n').then_some(offset))
            .collect();
        Self { newline_offsets }
    }

    fn line_column(&self, offset: u32) -> (usize, usize) {
        let offset = offset as usize;
        let preceding = self
            .newline_offsets
            .partition_point(|&newline| newline < offset);
        let line_start = if preceding == 0 {
            0
        } else {
            self.newline_offsets[preceding - 1] + 1
        };
        (preceding + 1, offset.saturating_sub(line_start) + 1)
    }
}

fn line_column_for_offset(index: Option<&LineColumnIndex>, offset: u32) -> (usize, usize) {
    index.map_or((1, offset as usize + 1), |index| index.line_column(offset))
}

#[cfg(test)]
mod tests {
    use super::{LineColumnIndex, line_column_for_offset};

    #[test]
    fn indexed_line_columns_match_the_previous_byte_scan() {
        for source in ["", "plain", "one\ntwo", "あx\nab\n"] {
            let index = LineColumnIndex::new(source);
            for offset in 0..=source.len() + 2 {
                assert_eq!(
                    index.line_column(offset as u32),
                    previous_line_column(source, offset),
                    "source {source:?}, offset {offset}"
                );
            }
        }
    }

    #[test]
    fn missing_sources_keep_the_existing_fallback() {
        assert_eq!(line_column_for_offset(None, 12), (1, 13));
    }

    fn previous_line_column(source: &str, offset: usize) -> (usize, usize) {
        let mut line = 1usize;
        let mut line_start = 0usize;
        for (index, &byte) in source.as_bytes().iter().enumerate() {
            if index >= offset {
                break;
            }
            if byte == b'\n' {
                line += 1;
                line_start = index + 1;
            }
        }
        (line, offset.saturating_sub(line_start) + 1)
    }
}
