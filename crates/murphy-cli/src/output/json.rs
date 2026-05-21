use murphy_core::Offense;

pub fn write(offenses: &[Offense]) -> Result<(), String> {
    let json = serde_json::to_string(offenses)
        .map_err(|e| format!("failed to serialize offenses: {e}"))?;
    super::write_stdout_line(&json)
}
