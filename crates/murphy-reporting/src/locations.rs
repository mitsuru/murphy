//! Shared byte-offset to line/column mapping for formatters.
//!
//! Formatters that render `line`/`column` (checkstyle, SARIF, JUnit, GitHub,
//! GNU, TAP, human) share this helper so they agree on positions. The logic
//! mirrors the previous `human.rs` implementation: newline offsets are
//! collected once per file (O(N)), then each offense resolves in O(log L).
//! When the source file cannot be read (e.g. unit tests with synthetic
//! paths), the historical fallback `(1, offset + 1)` is used so output stays
//! deterministic.

use std::collections::HashMap;

use murphy_core::Offense;

/// Newline offsets for one source file.
#[derive(Debug)]
pub struct LineColumnIndex {
    newline_offsets: Vec<usize>,
}

impl LineColumnIndex {
    /// Build from source text.
    pub fn new(source: &str) -> Self {
        let newline_offsets = source
            .as_bytes()
            .iter()
            .enumerate()
            .filter_map(|(offset, &byte)| (byte == b'\n').then_some(offset))
            .collect();
        Self { newline_offsets }
    }

    /// Build by reading `path`; `None` when the file cannot be read.
    pub fn from_path(path: &str) -> Option<Self> {
        let source = std::fs::read_to_string(path).ok()?;
        Some(Self::new(&source))
    }

    /// 1-based `(line, column)` for a byte `offset`. Columns are byte-based.
    pub fn line_column(&self, offset: u32) -> (usize, usize) {
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

/// Fallback when no index exists (unreadable file): `(1, offset + 1)`.
pub fn line_column_for_offset(index: Option<&LineColumnIndex>, offset: u32) -> (usize, usize) {
    index.map_or((1, offset as usize + 1), |index| index.line_column(offset))
}

/// Cache of per-file indexes for one formatting pass.
#[derive(Debug, Default)]
pub struct FileIndexCache {
    indexes: HashMap<String, Option<LineColumnIndex>>,
}

impl FileIndexCache {
    /// Build for every file referenced by `offenses` plus `files`.
    pub fn build(offenses: &[Offense], files: &[String]) -> Self {
        let mut cache = Self {
            indexes: HashMap::new(),
        };
        for file in files.iter().chain(offenses.iter().map(|o| &o.file)) {
            cache
                .indexes
                .entry(file.clone())
                .or_insert_with(|| LineColumnIndex::from_path(file));
        }
        Self {
            indexes: cache.indexes,
        }
    }

    /// 1-based `(line, column)` for a located offense's start offset.
    /// Returns `None` for filepath-only (no-location) offenses.
    pub fn start_line_column(&self, offense: &Offense) -> Option<(usize, usize)> {
        if !offense.has_location() {
            return None;
        }
        let index = self.indexes.get(&offense.file).and_then(|o| o.as_ref());
        Some(line_column_for_offset(index, offense.range.start_offset))
    }

    /// 1-based `(line, column)` for a located offense's end offset.
    /// Returns `None` for filepath-only (no-location) offenses.
    pub fn end_line_column(&self, offense: &Offense) -> Option<(usize, usize)> {
        if !offense.has_location() {
            return None;
        }
        let index = self.indexes.get(&offense.file).and_then(|o| o.as_ref());
        Some(line_column_for_offset(index, offense.range.end_offset))
    }
}

#[cfg(test)]
mod tests {
    use super::LineColumnIndex;

    #[test]
    fn indexed_line_columns_match_byte_scan() {
        for source in ["", "plain", "one\ntwo", "\u{3042}x\nab\n"] {
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
