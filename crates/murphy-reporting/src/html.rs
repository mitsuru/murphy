//! HTML report formatter (`--format html`, B9).
//!
//! Human/PR-review report as a standalone HTML document with inline CSS
//! (no external assets, no JavaScript), so it renders from `file://` or a
//! CI artifact tab. Structure mirrors [`super::markdown`]: header with
//! inspected/offense counts, a per-file summary table, then one section
//! per offending file with a bullet list of offenses.
//!
//! Files are grouped in sorted order; clean files are omitted from the
//! table but counted in the header. Filepath-only (no-location) offenses
//! render without `line:col`. When the offense carries a B4
//! `documentation_url`, the cop name links to it; otherwise plain text.
//! All user-controlled text (file, cop, message, URL) is HTML-escaped.
//! Zero offenses yields the same skeleton with a total-zero row, no
//! per-file sections, and a "No offenses detected." note. Output is
//! deterministic (no timestamps).

use std::collections::BTreeMap;

use murphy_core::{Offense, Severity};

use super::locations::FileIndexCache;
use super::xml::escape;

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn severity_word(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "warning",
        Severity::Error => "error",
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

    let mut grouped: BTreeMap<&str, Vec<&Offense>> = BTreeMap::new();
    for offense in offenses {
        grouped
            .entry(offense.file.as_str())
            .or_default()
            .push(offense);
    }

    let version = env!("CARGO_PKG_VERSION");
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n");
    out.push_str("<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"UTF-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<title>Murphy report</title>\n");
    out.push_str("<style>\n");
    out.push_str(
        "body{font-family:system-ui,-apple-system,sans-serif;max-width:960px;margin:2rem auto;padding:0 1rem;color:#1f2328;}",
    );
    out.push_str("table{border-collapse:collapse;width:100%;margin:1rem 0;}");
    out.push_str("th,td{border:1px solid #d0d7de;padding:.4rem .6rem;text-align:left;}");
    out.push_str("th{background:#f6f8fa;}");
    out.push_str("td.num,th.num{text-align:right;}");
    out.push_str(".warning{color:#9a6700;}.error{color:#cf222e;}");
    out.push_str("code{background:#f6f8fa;padding:.1rem .3rem;border-radius:4px;}");
    out.push_str("\n</style>\n</head>\n<body>\n");
    out.push_str("<h1>Murphy report</h1>\n");
    if offenses.is_empty() {
        out.push_str(&format!(
            "<p>{file_count} file{} inspected, no offenses detected.</p>\n",
            plural(file_count)
        ));
    } else {
        out.push_str(&format!(
            "<p>{file_count} file{} inspected, {} offense{} detected ({} warning{}, {} error{}).</p>\n",
            plural(file_count),
            offenses.len(),
            plural(offenses.len()),
            warnings,
            plural(warnings),
            errors,
            plural(errors),
        ));
    }

    out.push_str("<h2>Summary</h2>\n");
    out.push_str("<table>\n<thead><tr><th>File</th><th class=\"num\">Warnings</th><th class=\"num\">Errors</th><th class=\"num\">Total</th></tr></thead>\n<tbody>\n");
    for (file, items) in &grouped {
        let w = items
            .iter()
            .filter(|o| o.severity == Severity::Warning)
            .count();
        let e = items.len() - w;
        out.push_str(&format!(
            "<tr><td><code>{}</code></td><td class=\"num\">{w}</td><td class=\"num\">{e}</td><td class=\"num\">{}</td></tr>\n",
            escape(file),
            items.len()
        ));
    }
    out.push_str(&format!(
        "<tr><td><strong>Total</strong></td><td class=\"num\">{warnings}</td><td class=\"num\">{errors}</td><td class=\"num\">{}</td></tr>\n",
        offenses.len()
    ));
    out.push_str("</tbody>\n</table>\n");

    if offenses.is_empty() {
        out.push_str("<p>No offenses detected.</p>\n");
    } else {
        out.push_str("<h2>Offenses</h2>\n");
        for (file, items) in &grouped {
            out.push_str(&format!(
                "<h3><code>{}</code> ({} offense{})</h3>\n<ul>\n",
                escape(file),
                items.len(),
                plural(items.len())
            ));
            for offense in items {
                let loc = escape(&location_text(offense, &cache));
                let cop = match offense.documentation_url.as_deref() {
                    Some(url) => format!(
                        "<a href=\"{}\"><strong>{}</strong></a>",
                        escape(url),
                        escape(&offense.cop_name)
                    ),
                    None => format!("<strong>{}</strong>", escape(&offense.cop_name)),
                };
                let msg = escape(&offense.message);
                let sev = severity_word(offense.severity);
                out.push_str(&format!(
                    "<li><code>{loc}</code> {cop} (<span class=\"{sev}\">{sev}</span>): {msg}</li>\n"
                ));
            }
            out.push_str("</ul>\n");
        }
    }

    out.push_str(&format!(
        "<footer><p>Generated by murphy {version}.</p></footer>\n"
    ));
    out.push_str("</body>\n</html>");
    Ok(out)
}
