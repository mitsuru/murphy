use murphy_core::Offense;

pub fn format(offenses: &[Offense], files: &[String]) -> Result<String, String> {
    let mut out = String::new();
    let file_count = files.len();
    out.push_str(&format!(
        "Inspecting {file_count} file{}\n",
        plural(file_count)
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
            plural(file_count)
        ));
    } else {
        out.push_str(&format!(
            "{file_count} file{} inspected, {} offense{} detected",
            plural(file_count),
            offenses.len(),
            plural(offenses.len())
        ));
    }
    Ok(out)
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
