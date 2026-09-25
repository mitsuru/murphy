//! Checkstyle XML formatter (`--format checkstyle`).
//!
//! RuboCop-compatible shape:
//!
//! ```xml
//! <?xml version="1.0" encoding="UTF-8"?>
//! <checkstyle>
//!   <file name="dirty.rb">
//!     <error line="1" column="1" severity="warning" message="..." source="Lint/Debugger"/>
//!   </file>
//! </checkstyle>
//! ```
//!
//! Files are grouped in first-seen order (offenses arrive sorted by
//! `(file, start_offset)` from the aggregator). Files without offenses are
//! omitted. `warning` maps from [`Severity::Warning`], `error` from
//! [`Severity::Error`]. Filepath-only offenses omit `line`/`column`.

use std::collections::BTreeMap;

use murphy_core::{Offense, Severity};

use super::locations::FileIndexCache;
use super::xml::escape;

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

pub fn format(offenses: &[Offense], files: &[String]) -> Result<String, String> {
    let cache = FileIndexCache::build(offenses, files);
    // Group by file, preserving sorted order via BTreeMap (offenses are
    // already sorted by file, so BTreeMap order == display order).
    let mut grouped: BTreeMap<&str, Vec<&Offense>> = BTreeMap::new();
    for offense in offenses {
        grouped
            .entry(offense.file.as_str())
            .or_default()
            .push(offense);
    }

    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<checkstyle>");
    if grouped.is_empty() {
        out.push_str("</checkstyle>");
        return Ok(out);
    }
    out.push('\n');
    for (file, items) in &grouped {
        out.push_str(&format!("  <file name=\"{}\">\n", escape(file)));
        for offense in items {
            let message = escape(&offense.message);
            let source = escape(&offense.cop_name);
            let severity = severity_label(offense.severity);
            if let Some((line, column)) = cache.start_line_column(offense) {
                out.push_str(&format!(
                    "    <error line=\"{line}\" column=\"{column}\" severity=\"{severity}\" message=\"{message}\" source=\"{source}\"/>\n"
                ));
            } else {
                out.push_str(&format!(
                    "    <error severity=\"{severity}\" message=\"{message}\" source=\"{source}\"/>\n"
                ));
            }
        }
        out.push_str("  </file>\n");
    }
    out.push_str("</checkstyle>");
    Ok(out)
}
