//! JUnit XML formatter (`--format junit`) for CI test-report ingestion.
//!
//! Single `<testsuite name="murphy">` with one `<testcase>` per offense.
//! Zero offenses yields a suite with `tests="0"` and no cases.

use murphy_core::Offense;

use super::locations::FileIndexCache;
use super::xml::escape;

pub fn format(offenses: &[Offense], files: &[String]) -> Result<String, String> {
    let cache = FileIndexCache::build(offenses, files);
    let count = offenses.len();
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<testsuites>\n");
    out.push_str(&format!(
        "  <testsuite name=\"murphy\" tests=\"{count}\" failures=\"{count}\" errors=\"0\" skipped=\"0\">\n"
    ));
    for offense in offenses {
        let classname = escape(&offense.file);
        let cop = escape(&offense.cop_name);
        let message_attr = escape(&offense.message);
        let (case_name, body) = match cache.start_line_column(offense) {
            Some((line, column)) => (
                format!(
                    "{} in {}:{}:{}",
                    offense.cop_name, offense.file, line, column
                ),
                format!("{}:{}:{}: {}", offense.file, line, column, offense.message),
            ),
            None => (
                format!("{} in {}", offense.cop_name, offense.file),
                format!("{}: {}", offense.file, offense.message),
            ),
        };
        let case_name_esc = escape(&case_name);
        let body_esc = escape(&body);
        out.push_str(&format!(
            "    <testcase classname=\"{classname}\" name=\"{case_name_esc}\">\n"
        ));
        out.push_str(&format!(
            "      <failure message=\"{message_attr}\" type=\"{cop}\">{body_esc}</failure>\n"
        ));
        out.push_str("    </testcase>\n");
    }
    out.push_str("  </testsuite>\n");
    out.push_str("</testsuites>");
    Ok(out)
}
