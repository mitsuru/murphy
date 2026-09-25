mod checkstyle;
mod github;
mod gnu;
mod html;
mod human;
mod json;
mod junit;
mod locations;
mod markdown;
mod progress;
mod sarif;
mod tap;
mod xml;

use murphy_core::Offense;

/// Output formats for `murphy lint --format`.
///
/// `Human` (default), `Json` (ADR 0006 frozen contract), and `Progress` are
/// pre-existing. The six B7 additions are `Checkstyle`, `Sarif`, `Junit`,
/// `Github`, `Gnu`, and `Tap` (machine/CI-readable). The two B9 additions
/// are `Html` and `Markdown` (human/PR-review reports, rubycritic-equivalent).
/// Adding variants never changes the `Json` byte shape (`json::format` is
/// untouched).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
    Progress,
    Checkstyle,
    Sarif,
    Junit,
    Github,
    Gnu,
    Tap,
    Html,
    Markdown,
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
        OutputFormat::Checkstyle => checkstyle::format(offenses, files),
        OutputFormat::Sarif => sarif::format(offenses, files),
        OutputFormat::Junit => junit::format(offenses, files),
        OutputFormat::Github => github::format(offenses, files),
        OutputFormat::Gnu => gnu::format(offenses, files),
        OutputFormat::Tap => tap::format(offenses, files),
        OutputFormat::Html => html::format(offenses, files),
        OutputFormat::Markdown => markdown::format(offenses, files),
    }
}
