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

pub fn format_lint_output(
    offenses: &[Offense],
    files: &[String],
    format: OutputFormat,
) -> Result<String, String> {
    match format {
        OutputFormat::Human => human::format(offenses, files),
        OutputFormat::Json => json::format(offenses),
        OutputFormat::Progress => progress::format(offenses, files),
    }
}
