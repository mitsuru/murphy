//! TAP version 13 formatter (`--format tap`, Test Anything Protocol).
//!
//! Each offense is a failing test (`not ok`). A clean run emits a `1..0`
//! plan with a `# No offenses detected` comment:
//!
//! ```text
//! TAP version 13
//! 1..2
//! not ok 1 - dirty.rb:1:1: Lint/Debugger: message
//! not ok 2 - other.rb: C: Cop: message
//! ```

use murphy_core::Offense;

use super::locations::FileIndexCache;

pub fn format(offenses: &[Offense], files: &[String]) -> Result<String, String> {
    let cache = FileIndexCache::build(offenses, files);
    let mut out = String::new();
    out.push_str("TAP version 13\n");
    out.push_str(&format!("1..{}\n", offenses.len()));
    if offenses.is_empty() {
        out.push_str("# No offenses detected");
        return Ok(out);
    }
    for (index, offense) in offenses.iter().enumerate() {
        let num = index + 1;
        match cache.start_line_column(offense) {
            Some((line, column)) => {
                out.push_str(&format!(
                    "not ok {num} - {}:{}:{}: {}: {}\n",
                    offense.file, line, column, offense.cop_name, offense.message
                ));
            }
            None => {
                out.push_str(&format!(
                    "not ok {num} - {}: {}: {}\n",
                    offense.file, offense.cop_name, offense.message
                ));
            }
        }
    }
    // Drop the trailing newline so the CLI's `writeln!` supplies exactly one.
    out.pop();
    Ok(out)
}
