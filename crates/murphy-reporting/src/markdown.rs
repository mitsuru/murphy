//! Markdown report formatter (`--format markdown`, B9).
//!
//! Human/PR-review report, GitHub-flavoured Markdown. Suitable for pasting
//! into a PR comment or publishing via `$GITHUB_STEP_SUMMARY`:
//!
//! ```text
//! # Murphy report
//!
//! 2 files inspected, 2 offenses detected
//!
//! ## Summary
//!
//! | File | Warnings | Errors | Total |
//! ...
//!
//! ## `dirty.rb` (1 offense)
//!
//! - `dirty.rb:1:1` **Lint/Debugger** (warning): message
//! ```
//!
//! Files are grouped in sorted order (same as `checkstyle`); clean files
//! are omitted from the table but counted in the header (same as
//! `checkstyle` + `progress`). Filepath-only (no-location) offenses render
//! without `line:col`. When the offense carries a B4 `documentation_url`,
//! the cop name renders as a link (`**[Cop](url)**`); otherwise plain bold.
//! Newlines in messages collapse to spaces so each offense stays one bullet.
//! Pipe/backslash characters in file names are escaped for table cells.
//! Zero offenses yields a header + empty summary table (total zeros) with
//! no per-file sections. Output is deterministic (no timestamps).

use std::collections::BTreeMap;

use murphy_core::{Offense, Severity};

use super::locations::FileIndexCache;

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn severity_word(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

/// Escape a table cell: backslashes first, then pipes.
fn escape_table_cell(s: &str) -> String {
    s.replace('\\', "\\\\").replace('|', "\\|")
}

/// One-line message: collapse newlines so each offense is one bullet.
fn one_line_message(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
}

fn cop_label(offense: &Offense) -> String {
    match offense.documentation_url.as_deref() {
        Some(url) => format!("**[{}]({})**", offense.cop_name, url),
        None => format!("**{}**", offense.cop_name),
    }
}

fn location_text(offense: &Offense, cache: &FileIndexCache) -> String {
    match cache.start_line_column(offense) {
        Some((line, column)) => format!("{}:{}:{}", offense.file, line, column),
        None => offense.file.clone(),
    }
}

pub fn format(offenses: &[Offense], files: &[String]) -> Result<String, String> {
    let cache = FileIndexCache::build(offenses, files);
    let file_count = files.len();
    let warnings = offenses
        .iter()
        .filter(|o| o.severity == Severity::Warning)
        .count();
    let errors = offenses.len() - warnings;

    let mut out = String::new();
    out.push_str("# Murphy report\n\n");
    if offenses.is_empty() {
        out.push_str(&format!(
            "{file_count} file{} inspected, no offenses detected\n",
            plural(file_count)
        ));
    } else {
        out.push_str(&format!(
            "{file_count} file{} inspected, {} offense{} detected ({} warning{}, {} error{})\n",
            plural(file_count),
            offenses.len(),
            plural(offenses.len()),
            warnings,
            plural(warnings),
            errors,
            plural(errors),
        ));
    }

    out.push_str("\n## Summary\n\n");
    out.push_str("| File | Warnings | Errors | Total |\n");
    out.push_str("| --- | ---: | ---: | ---: |\n");

    // Group per file in sorted order (BTreeMap over already-sorted input).
    let mut grouped: BTreeMap<&str, Vec<&Offense>> = BTreeMap::new();
    for offense in offenses {
        grouped
            .entry(offense.file.as_str())
            .or_default()
            .push(offense);
    }
    for (file, items) in &grouped {
        let w = items
            .iter()
            .filter(|o| o.severity == Severity::Warning)
            .count();
        let e = items.len() - w;
        out.push_str(&format!(
            "| {} | {w} | {e} | {} |\n",
            escape_table_cell(file),
            items.len()
        ));
    }
    out.push_str(&format!(
        "| Total | {warnings} | {errors} | {} |\n",
        offenses.len()
    ));

    for (file, items) in &grouped {
        out.push_str(&format!(
            "\n## `{}` ({} offense{})\n\n",
            file,
            items.len(),
            plural(items.len())
        ));
        for offense in items {
            let loc = location_text(offense, &cache);
            let cop = cop_label(offense);
            let msg = one_line_message(&offense.message);
            out.push_str(&format!(
                "- `{}` {} ({}): {}\n",
                loc,
                cop,
                severity_word(offense.severity),
                msg
            ));
        }
    }

    // Drop the trailing newline so the CLI's `writeln!` supplies exactly one.
    out.pop();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{escape_table_cell, one_line_message};

    #[test]
    fn table_cells_escape_pipes() {
        assert_eq!(escape_table_cell("a|b\\c"), "a\\|b\\\\c");
    }

    #[test]
    fn messages_collapse_newlines() {
        assert_eq!(one_line_message("a\nb\rc"), "a b c");
    }
}
