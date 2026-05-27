//! Source-level checks for `murphy-parity` metadata blocks.
//!
//! The blocks live next to cop implementations so people and agents see
//! parity status while reading the cop. This test keeps the source-near
//! documentation from drifting as new cops are added.

use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn every_cop_has_a_matching_parity_metadata_block() {
    let root = workspace_root();
    let mut failures = Vec::new();

    for file in cop_source_files(&root) {
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("read {}: {err}", file.display()));
        let cop_names = cop_names_in_source(&source);
        if cop_names.is_empty() {
            continue;
        }
        let parity_blocks = parity_blocks_in_source(&source);

        for name in cop_names {
            let Some(block) = parity_blocks
                .iter()
                .find(|block| block_matches_cop(block, &name))
            else {
                failures.push(format!(
                    "{}: missing murphy-parity block for {name}",
                    file.strip_prefix(&root).unwrap_or(&file).display()
                ));
                continue;
            };

            validate_parity_block(
                block,
                &name,
                &file
                    .strip_prefix(&root)
                    .unwrap_or(&file)
                    .display()
                    .to_string(),
                &mut failures,
            );
        }
    }

    assert!(
        failures.is_empty(),
        "missing parity metadata:\n{}",
        failures.join("\n")
    );
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("murphy-cli is under crates/murphy-cli")
        .to_path_buf()
}

fn cop_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for rel in [
        "crates/murphy-std/src",
        "crates/murphy-rspec/src",
        "crates/murphy-rails/src",
        "crates/murphy-example-pack/src",
    ] {
        collect_rs_files(&root.join(rel), &mut files);
    }
    files.sort();
    files
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|err| panic!("read_dir {}: {err}", dir.display()));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

fn cop_names_in_source(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_cop_attr = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#[cop(") {
            in_cop_attr = true;
            continue;
        }
        if !in_cop_attr {
            continue;
        }
        if let Some(name) = parse_name_literal(trimmed) {
            names.push(name.to_string());
        }
        if trimmed == ")]" || trimmed == ")" || trimmed.ends_with(")]") {
            in_cop_attr = false;
        }
    }

    names
}

fn parse_name_literal(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("name = \"")?;
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn parity_blocks_in_source(source: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = source;

    while let Some(start) = rest.find("```murphy-parity") {
        rest = &rest[start + "```murphy-parity".len()..];
        let Some(end) = rest.find("```") else {
            blocks.push(rest);
            break;
        };
        blocks.push(&rest[..end]);
        rest = &rest[end + "```".len()..];
    }

    blocks
}

fn block_matches_cop(block: &str, name: &str) -> bool {
    value_after_key(block, "upstream_cop") == Some(name)
        || value_after_key(block, "cop") == Some(name)
}

fn validate_parity_block(block: &str, name: &str, file: &str, failures: &mut Vec<String>) {
    let Some(status) = value_after_key(block, "status") else {
        failures.push(format!(
            "{file}: murphy-parity block for {name} is missing status"
        ));
        return;
    };

    if !matches!(status, "custom" | "partial" | "stub" | "verified") {
        failures.push(format!(
            "{file}: murphy-parity block for {name} has unknown status {status:?}"
        ));
    }

    if value_after_key(block, "cop") == Some(name) {
        if status != "custom" {
            failures.push(format!(
                "{file}: custom murphy-parity block for {name} must use status: custom"
            ));
        }
        return;
    }

    for key in ["upstream", "upstream_cop", "upstream_version_checked"] {
        if value_after_key(block, key).is_none() {
            failures.push(format!(
                "{file}: murphy-parity block for {name} is missing {key}"
            ));
        }
    }

    if matches!(status, "partial" | "stub") && !block.contains("gap_issues:") {
        failures.push(format!(
            "{file}: {status} murphy-parity block for {name} must list gap_issues"
        ));
    }

    if status == "verified" && !block.contains("gap_issues: []") {
        failures.push(format!(
            "{file}: verified murphy-parity block for {name} must use gap_issues: []"
        ));
    }

    if block.contains("Arena-migration stub registered") && status != "stub" {
        failures.push(format!(
            "{file}: Arena-migration registration for {name} must use status: stub"
        ));
    }
}

fn value_after_key<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}:");
    block.lines().find_map(|line| {
        metadata_line(line)
            .strip_prefix(&prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn metadata_line(line: &str) -> &str {
    line.trim()
        .strip_prefix("//!")
        .or_else(|| line.trim().strip_prefix("///"))
        .unwrap_or_else(|| line.trim())
        .trim()
}
