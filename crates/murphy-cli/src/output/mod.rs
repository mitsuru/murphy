mod human;
mod json;
mod progress;

use murphy_core::Offense;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
    Progress,
}

pub fn write_lint_output(
    offenses: &[Offense],
    files: &[String],
    format: OutputFormat,
) -> Result<(), String> {
    match format {
        OutputFormat::Human => human::write(offenses, files),
        OutputFormat::Json => json::write(offenses),
        OutputFormat::Progress => progress::write(offenses, files),
    }
}

fn write_stdout_line(line: &str) -> Result<(), String> {
    use std::io::Write;

    let mut stdout = std::io::stdout().lock();
    if let Err(e) = writeln!(stdout, "{line}") {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(format!("failed to write stdout: {e}"));
    }
    Ok(())
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
