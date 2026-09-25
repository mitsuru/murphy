//! SARIF 2.1.0 formatter (`--format sarif`) for GitHub code scanning.
//!
//! Minimal valid SARIF: `version`, `$schema`, one `run` with
//! `tool.driver` (name `murphy` + deduped `rules`) and `results`. Level maps
//! `Warning -> "warning"`, `Error -> "error"`. Located offenses carry a
//! `region` with 1-based `startLine`/`startColumn`/`endLine`/`endColumn`
//! (byte-based columns); filepath-only offenses carry only `artifactLocation`.

use std::collections::BTreeSet;

use murphy_core::{Offense, Severity};

use super::locations::FileIndexCache;

fn level(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

pub fn format(offenses: &[Offense], files: &[String]) -> Result<String, String> {
    let cache = FileIndexCache::build(offenses, files);

    let mut rule_ids: BTreeSet<&str> = BTreeSet::new();
    for offense in offenses {
        rule_ids.insert(offense.cop_name.as_str());
    }
    let rules: Vec<serde_json::Value> = rule_ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id,
                "name": id.replace('/', "_"),
                "shortDescription": { "text": id },
            })
        })
        .collect();

    let mut results = Vec::with_capacity(offenses.len());
    for offense in offenses {
        let mut location = serde_json::json!({
            "physicalLocation": {
                "artifactLocation": { "uri": offense.file },
            }
        });
        if offense.has_location()
            && let (Some((sl, sc)), Some((el, ec))) = (
                cache.start_line_column(offense),
                cache.end_line_column(offense),
            )
        {
            location["physicalLocation"]["region"] = serde_json::json!({
                "startLine": sl,
                "startColumn": sc,
                "endLine": el,
                "endColumn": ec,
            });
        }
        results.push(serde_json::json!({
            "ruleId": offense.cop_name,
            "level": level(offense.severity),
            "message": { "text": offense.message },
            "locations": [location],
        }));
    }

    let sarif = serde_json::json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "murphy",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/murphy/murphy",
                    "rules": rules,
                }
            },
            "results": results,
        }],
    });
    serde_json::to_string_pretty(&sarif).map_err(|e| format!("failed to serialize SARIF: {e}"))
}
