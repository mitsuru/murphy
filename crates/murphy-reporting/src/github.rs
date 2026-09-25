//! GitHub Actions annotation formatter (`--format github`).
//!
//! One workflow command per offense:
//!
//! ```text
//! ::warning file=dirty.rb,line=1,col=1,title=Lint/Debugger::message
//! ::error file=dirty.rb,line=1,col=1,title=Cop::message
//! ```
//!
//! `Warning -> ::warning`, `Error -> ::error`. Filepath-only offenses omit
//! `line`/`col`. Property values use `escape_property` (`%`, CR, LF, `:`,
//! `,`), message data uses `escape_data` (`%`, CR, LF) per
//! actions/toolkit escaping rules. Zero offenses yields empty output.

use murphy_core::{Offense, Severity};

use super::locations::FileIndexCache;

fn command(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

fn escape_data(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn escape_property(s: &str) -> String {
    escape_data(s).replace(':', "%3A").replace(',', "%2C")
}

pub fn format(offenses: &[Offense], files: &[String]) -> Result<String, String> {
    let cache = FileIndexCache::build(offenses, files);
    let mut lines = Vec::with_capacity(offenses.len());
    for offense in offenses {
        let level = command(offense.severity);
        let file = escape_property(&offense.file);
        let title = escape_property(&offense.cop_name);
        let message = escape_data(&offense.message);
        match cache.start_line_column(offense) {
            Some((line, column)) => lines.push(format!(
                "::{level} file={file},line={line},col={column},title={title}::{message}"
            )),
            None => lines.push(format!("::{level} file={file},title={title}::{message}")),
        }
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{escape_data, escape_property};

    #[test]
    fn escapes_workflow_fields() {
        assert_eq!(escape_data("a%b\rc\nd"), "a%25b%0Dc%0Ad");
        assert_eq!(escape_property("a:b,c"), "a%3Ab%2Cc");
    }
}
