//! GNU-style formatter (`--format gnu`): `file:line:col: severity: message [Cop]`.
//!
//! One line per offense, no header (unlike `human`). `warning`/`error` words
//! match GCC conventions. Filepath-only offenses render as
//! `file: warning: message [Cop]`. Zero offenses yields empty output.

use murphy_core::{Offense, Severity};

use super::locations::FileIndexCache;

fn severity_word(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

pub fn format(offenses: &[Offense], files: &[String]) -> Result<String, String> {
    let cache = FileIndexCache::build(offenses, files);
    let mut lines = Vec::with_capacity(offenses.len());
    for offense in offenses {
        let severity = severity_word(offense.severity);
        match cache.start_line_column(offense) {
            Some((line, column)) => lines.push(format!(
                "{}:{}:{}: {}: {} [{}]",
                offense.file, line, column, severity, offense.message, offense.cop_name
            )),
            None => lines.push(format!(
                "{}: {}: {} [{}]",
                offense.file, severity, offense.message, offense.cop_name
            )),
        }
    }
    Ok(lines.join("\n"))
}
