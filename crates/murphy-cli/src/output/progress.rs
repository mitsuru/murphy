use murphy_core::Offense;

pub fn write(offenses: &[Offense], files: &[String]) -> Result<(), String> {
    super::write_stdout_line(&render(offenses, files))
}

pub fn render(offenses: &[Offense], files: &[String]) -> String {
    let mut out = String::new();
    let file_count = files.len();
    out.push_str(&format!(
        "Inspecting {file_count} file{}\n",
        super::plural(file_count)
    ));
    for file in files {
        if offenses.iter().any(|offense| offense.file == *file) {
            out.push('C');
        } else {
            out.push('.');
        }
    }
    out.push_str("\n\n");
    if offenses.is_empty() {
        out.push_str(&format!(
            "{file_count} file{} inspected, no offenses detected",
            super::plural(file_count)
        ));
    } else {
        out.push_str(&format!(
            "{file_count} file{} inspected, {} offense{} detected",
            super::plural(file_count),
            offenses.len(),
            super::plural(offenses.len())
        ));
    }
    out
}
