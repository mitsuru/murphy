//! `murphy explain <cop_id>` — B4 AI向け explain (murphy-fmw.2.4).
//!
//! Human/AI-readable cop documentation: docs URL + fixed-template
//! rationale + fixed-template fix example. All prose is built ONLY from
//! registry-controlled `cop_name` + cop-author `description`
//! (`murphy_core::explain`), never from offense messages or source text,
//! so the output is prompt-injection safe by construction.
//!
//! Exit codes: `0` on success, `2` (setup) on unknown cop — matching the
//! CLI convention that bad usage is exit 2.

use murphy_core::{
    CopRegistry, MurphyConfig, SYNTAX_COP_DESCRIPTION, SYNTAX_COP_NAME, explain_for_cop,
};
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use super::AppError;

/// Output shape for `murphy explain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
}

/// Build the cop-name → description map from a registry.
///
/// Descriptions are cop-author fixed strings (`#[cop(description)]`),
/// safe for rationale construction. Includes user-disabled cops (the
/// catalogue view) so `explain` works even for a disabled cop.
pub fn description_map(registry: &CopRegistry) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (cop, _pack) in registry.all_cops_with_packs() {
        let name = String::from_utf8_lossy(unsafe { cop.name.as_bytes() }).into_owned();
        let desc = String::from_utf8_lossy(unsafe { cop.description.as_bytes() }).into_owned();
        map.insert(name, desc);
    }
    map
}

/// Resolve a cop's registry description, covering the synthetic syntax cop
/// and the arena-migration disabled list (which carry no `PluginCopV1`).
pub fn lookup_description(map: &HashMap<String, String>, cop_id: &str) -> Option<String> {
    if let Some(d) = map.get(cop_id) {
        return Some(d.clone());
    }
    if cop_id == SYNTAX_COP_NAME {
        return Some(SYNTAX_COP_DESCRIPTION.to_owned());
    }
    if murphy_std::DISABLED_COPS.contains(&cop_id) {
        return Some(String::new());
    }
    None
}

/// `murphy explain <cop_id> [--format human|json]`.
pub fn run_explain(cop_id: &str, format: Format) -> Result<u8, AppError> {
    let config =
        MurphyConfig::load_with_defaults(Path::new("."), murphy_std::BUNDLED_DEFAULTS_YAML)
            .map_err(|e| AppError::setup(e.to_string()))?;
    let registry =
        CopRegistry::discover_with_config(Path::new("."), &config, super::builtin_pack())
            .map_err(|e| AppError::setup(e.to_string()))?;

    let map = description_map(&registry);
    let description = lookup_description(&map, cop_id).ok_or_else(|| {
        AppError::setup(format!(
            "unknown cop `{cop_id}` (see `murphy cops list` for known cops)"
        ))
    })?;

    let payload = explain_for_cop(cop_id, &description);

    let mut stdout = std::io::stdout().lock();
    let res = match format {
        Format::Human => write_human(&mut stdout, &payload),
        Format::Json => write_json(&mut stdout, &payload),
    };
    if let Err(e) = res {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(0);
        }
        return Err(AppError::setup(format!("failed to write stdout: {e}")));
    }
    Ok(0)
}

fn write_human(out: &mut impl Write, payload: &murphy_core::CopExplain) -> std::io::Result<()> {
    writeln!(out, "Cop: {}", payload.cop_name)?;
    if payload.description.is_empty() {
        writeln!(out, "Description: (no description)")?;
    } else {
        writeln!(out, "Description: {}", payload.description)?;
    }
    writeln!(out, "Documentation: {}", payload.documentation_url)?;
    writeln!(out, "Rationale: {}", payload.rationale)?;
    writeln!(out, "Example:")?;
    for line in payload.fix_example.lines() {
        writeln!(out, "  {line}")?;
    }
    Ok(())
}

fn write_json(out: &mut impl Write, payload: &murphy_core::CopExplain) -> std::io::Result<()> {
    let body = json!({
        "cop_name": payload.cop_name,
        "description": payload.description,
        "documentation_url": payload.documentation_url,
        "rationale": payload.rationale,
        "fix_example": payload.fix_example,
    });
    writeln!(out, "{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_falls_back_to_syntax_description() {
        let map = HashMap::new();
        let d = lookup_description(&map, SYNTAX_COP_NAME).expect("syntax cop resolves");
        assert!(!d.is_empty());
    }

    #[test]
    fn lookup_returns_none_for_unknown_cop() {
        let map = HashMap::new();
        assert!(lookup_description(&map, "Nope/Nope").is_none());
    }
}
