use murphy_core::Offense;

pub fn format(offenses: &[Offense]) -> Result<String, String> {
    serde_json::to_string(offenses).map_err(|e| format!("failed to serialize offenses: {e}"))
}
