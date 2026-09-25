use murphy_core::{CopRegistry, MurphyConfig, Offense, Severity, aggregate_with_config};

use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::Path;

const EXIT_OK: u8 = 0;
const LSP_ERROR_METHOD_NOT_FOUND: i32 = -32601;
const LSP_ERROR_INVALID_PARAMS: i32 = -32602;

pub fn run(_args: &[String]) -> Result<u8, super::AppError> {
    let mut config =
        MurphyConfig::load_with_defaults(Path::new("."), murphy_std::BUNDLED_DEFAULTS_YAML)
            .map_err(|e| super::AppError::setup(e.to_string()))?;

    let registry =
        CopRegistry::discover_with_config(Path::new("."), &config, super::builtin_pack())
            .map_err(|e| super::AppError::setup(e.to_string()))?;
    // Layer loaded packs' bundled `default.yml` `AllCops` defaults below user
    // config before any `lint_source` call (which reads
    // `config.active_support_extensions_enabled`).
    config.apply_pack_default_layers(&registry.pack_default_configs());

    let mut open_documents: HashMap<String, String> = HashMap::new();

    let mut stdin = io::stdin();
    let mut all = Vec::new();
    stdin
        .read_to_end(&mut all)
        .map_err(|e| super::AppError::setup(format!("failed to read stdin: {e}")))?;

    let mut cursor = 0usize;
    let mut stdout = io::stdout().lock();

    while let Some((message, next)) = parse_message(&all[cursor..]) {
        cursor += next;

        let Some(method) = message
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };

        let id = message.get("id").cloned();
        let params = message.get("params");

        if method == "initialize" {
            let result = json!({
                "capabilities": {
                    "textDocumentSync": 1,
                    "codeActionProvider": true,
                }
            });
            let response = json!({"jsonrpc": "2.0", "id": id, "result": result});
            write_message(&mut stdout, &response)?;
            continue;
        }

        if method == "initialized" {
            continue;
        }

        if method == "textDocument/didOpen" {
            if let Some(uri) = uri_from_message(params) {
                let file = match uri_to_file_path(&uri) {
                    Some(path) => path,
                    None => {
                        if let Some(value) = id {
                            write_message(
                                &mut stdout,
                                &invalid_params_error(value, "Invalid params"),
                            )?;
                        }
                        continue;
                    }
                };

                if let Some(text) = params
                    .and_then(|p| p.get("textDocument"))
                    .and_then(|p| p.get("text"))
                    .and_then(Value::as_str)
                {
                    open_documents.insert(uri.clone(), text.to_string());
                    publish_diagnostics(&mut stdout, &uri, text, file, &config, &registry)?;
                }
            }
            continue;
        }

        if method == "textDocument/didChange" {
            if let Some(uri) = uri_from_message(params) {
                let file = match uri_to_file_path(&uri) {
                    Some(path) => path,
                    None => {
                        if let Some(value) = id {
                            write_message(
                                &mut stdout,
                                &invalid_params_error(value, "Invalid params"),
                            )?;
                        }
                        continue;
                    }
                };

                let next_text = params
                    .and_then(|p| p.get("contentChanges"))
                    .and_then(|changes| changes.as_array())
                    .and_then(|changes| changes.last())
                    .and_then(|change| change.get("text"))
                    .and_then(Value::as_str)
                    .map(str::to_string);

                let text = if let Some(text) = next_text {
                    open_documents.insert(uri.clone(), text.clone());
                    text
                } else {
                    open_documents.get(&uri).cloned().unwrap_or_default()
                };

                if text.is_empty() {
                    let empty = String::new();
                    publish_diagnostics(&mut stdout, &uri, &empty, file, &config, &registry)?;
                    continue;
                }

                publish_diagnostics(&mut stdout, &uri, &text, file, &config, &registry)?;
            }
            continue;
        }

        if method == "textDocument/didClose" {
            if let Some(uri) = uri_from_message(params) {
                if uri_to_file_path(&uri).is_none() {
                    continue;
                }

                open_documents.remove(&uri);
                let response = publish_diagnostics_message(&uri, &[]);
                write_message(&mut stdout, &response)?;
            }
            continue;
        }

        if method == "textDocument/codeAction" {
            if let Some(value) = id {
                match code_actions_for_params(params, &open_documents, &config, &registry) {
                    Ok(actions) => {
                        let response = json!({"jsonrpc": "2.0", "id": value, "result": actions});
                        write_message(&mut stdout, &response)?;
                    }
                    Err(message) => {
                        write_message(&mut stdout, &invalid_params_error(value, &message))?;
                    }
                }
            }
            continue;
        }

        // B2 (murphy-fmw.2.2): range-selected re-lint. A custom request that
        // re-lints the open document and returns only the diagnostics
        // intersecting the requested range, so editors can refresh a
        // selection without a full publish cycle.
        if method == "murphy/rangeDiagnostics" {
            if let Some(value) = id {
                match range_diagnostics_for_params(params, &open_documents, &config, &registry) {
                    Ok(diagnostics) => {
                        let response =
                            json!({"jsonrpc": "2.0", "id": value, "result": diagnostics});
                        write_message(&mut stdout, &response)?;
                    }
                    Err(message) => {
                        write_message(&mut stdout, &invalid_params_error(value, &message))?;
                    }
                }
            }
            continue;
        }

        if method == "shutdown" {
            if let Some(value) = id {
                let response = json!({"jsonrpc": "2.0", "id": value, "result": json!(null)});
                write_message(&mut stdout, &response)?;
            }
            continue;
        }

        if method == "exit" {
            break;
        }

        if let Some(value) = id {
            let response = json!({
                "jsonrpc": "2.0",
                "id": value,
                "error": {
                    "code": LSP_ERROR_METHOD_NOT_FOUND,
                    "message": "method not found"
                }
            });
            write_message(&mut stdout, &response)?;
        }
    }

    Ok(EXIT_OK)
}

fn parse_message(input: &[u8]) -> Option<(Value, usize)> {
    let header_end = match input.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(pos) => pos + 4,
        None => return None,
    };

    let header = std::str::from_utf8(&input[..header_end - 4]).ok()?;
    let length = header.lines().find_map(|line| {
        line.strip_prefix("Content-Length:")
            .map(str::trim)
            .and_then(|value| value.parse::<usize>().ok())
    })?;

    let body_start = header_end;
    let body_end = body_start + length;
    if body_end > input.len() {
        return None;
    }

    let body = &input[body_start..body_end];
    let value = serde_json::from_slice(body).ok()?;

    Some((value, body_end))
}

fn write_message<W: Write>(out: &mut W, message: &Value) -> Result<(), super::AppError> {
    let body = serde_json::to_vec(message)
        .map_err(|e| super::AppError::setup(format!("failed to serialize LSP response: {e}")))?;

    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    if let Err(err) = out.write_all(header.as_bytes()) {
        if err.kind() == io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(super::AppError::setup(format!(
            "failed to write stdout: {err}"
        )));
    }
    if let Err(err) = out.write_all(&body) {
        if err.kind() == io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(super::AppError::setup(format!(
            "failed to write stdout: {err}"
        )));
    }

    if let Err(err) = out.flush() {
        if err.kind() == io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(super::AppError::setup(format!(
            "failed to write stdout: {err}"
        )));
    }

    Ok(())
}

fn uri_from_message(params: Option<&Value>) -> Option<String> {
    params?
        .get("textDocument")
        .and_then(|td| td.get("uri"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// B2 (murphy-fmw.2.2): quick-fix code actions built from the existing
/// autocorrect edits carried by [`Offense`] values.
///
/// The handler re-lints the open document text (same pipeline as
/// `publish_diagnostics`), then:
/// - when `context.diagnostics` is non-empty, only offenses matching a
///   context diagnostic (same cop code + intersecting range) get an action;
/// - otherwise, every fixable offense intersecting the requested `range`
///   gets an action.
///
/// Offenses without autocorrect edits (or without a location) never produce
/// actions. A non-`file://` URI is `InvalidParams`, matching
/// `didOpen`/`didChange`; an unknown (never opened) document yields `[]`.
fn code_actions_for_params(
    params: Option<&Value>,
    open_documents: &HashMap<String, String>,
    config: &MurphyConfig,
    registry: &CopRegistry,
) -> Result<Vec<Value>, String> {
    let uri = uri_from_message(params).ok_or_else(|| "Invalid params".to_string())?;
    let file = uri_to_file_path(&uri)
        .ok_or_else(|| "Invalid params".to_string())?
        .to_string();
    let text = match open_documents.get(&uri) {
        Some(text) => text.clone(),
        None => return Ok(Vec::new()),
    };

    let range = params
        .and_then(|p| p.get("range"))
        .and_then(parse_lsp_range);
    let Some(((start_line, start_char), (end_line, end_char))) = range else {
        return Ok(Vec::new());
    };
    let range_start = lsp_position_to_offset((start_line, start_char), &text);
    let range_end = lsp_position_to_offset((end_line, end_char), &text);

    if only_filters_out_quickfix(params) {
        return Ok(Vec::new());
    }

    let offenses = run_offenses_for_source(&text, &file, config, registry);
    let context_diagnostics = params
        .and_then(|p| p.get("context"))
        .and_then(|c| c.get("diagnostics"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut actions = Vec::new();
    for offense in &offenses {
        if offense
            .autocorrect
            .as_ref()
            .is_none_or(|ac| ac.edits.is_empty())
        {
            continue;
        }
        if !offense.has_location() {
            continue;
        }
        if !context_diagnostics.is_empty() {
            if !context_matches_offense(&context_diagnostics, offense, &text) {
                continue;
            }
        } else if !byte_ranges_intersect(
            offense.range.start_offset as usize,
            offense.range.end_offset as usize,
            range_start,
            range_end,
        ) {
            continue;
        }
        if let Some(action) = build_quickfix_action(&uri, offense, &text) {
            actions.push(action);
        }
    }
    Ok(actions)
}

/// B2 (murphy-fmw.2.2): range-selected re-lint.
///
/// Re-lints the open document and returns only the diagnostics intersecting
/// `params.range`. Errors mirror `codeAction`: non-`file://` URIs and missing
/// ranges are `InvalidParams`; a never-opened document reports
/// `InvalidParams` ("document not open") so clients can distinguish it from
/// an empty (clean) range result.
fn range_diagnostics_for_params(
    params: Option<&Value>,
    open_documents: &HashMap<String, String>,
    config: &MurphyConfig,
    registry: &CopRegistry,
) -> Result<Vec<Value>, String> {
    let uri = uri_from_message(params).ok_or_else(|| "Invalid params".to_string())?;
    let file = uri_to_file_path(&uri)
        .ok_or_else(|| "Invalid params".to_string())?
        .to_string();
    let text = open_documents
        .get(&uri)
        .cloned()
        .ok_or_else(|| "document not open".to_string())?;

    let range = params
        .and_then(|p| p.get("range"))
        .and_then(parse_lsp_range);
    let Some(((start_line, start_char), (end_line, end_char))) = range else {
        return Err("Invalid params".to_string());
    };
    let range_start = lsp_position_to_offset((start_line, start_char), &text);
    let range_end = lsp_position_to_offset((end_line, end_char), &text);

    let offenses = run_offenses_for_source(&text, &file, config, registry);
    Ok(offenses
        .iter()
        .filter(|offense| {
            offense.has_location()
                && byte_ranges_intersect(
                    offense.range.start_offset as usize,
                    offense.range.end_offset as usize,
                    range_start,
                    range_end,
                )
        })
        .map(|offense| to_diagnostic(offense, &text))
        .collect())
}

/// Build one `quickfix` code action from an offense's autocorrect edits.
///
/// Returns `None` when the offense carries no usable fix (no autocorrect,
/// empty edits, or no location). Edit byte offsets map to LSP ranges with
/// [`offset_to_lsp_position`], the same conversion as diagnostics.
fn build_quickfix_action(uri: &str, offense: &Offense, source: &str) -> Option<Value> {
    let edits = offense.autocorrect.as_ref()?.edits.as_slice();
    if edits.is_empty() || !offense.has_location() {
        return None;
    }
    let text_edits = edits
        .iter()
        .map(|edit| {
            let (start, end) = (
                offset_to_lsp_position(edit.range.start_offset, source),
                offset_to_lsp_position(edit.range.end_offset, source),
            );
            json!({
                "range": {
                    "start": {"line": start.0, "character": start.1},
                    "end": {"line": end.0, "character": end.1},
                },
                "newText": edit.replacement,
            })
        })
        .collect::<Vec<_>>();
    Some(json!({
        "title": format!("Murphy: fix {}", offense.cop_name),
        "kind": "quickfix",
        "diagnostics": [to_diagnostic(offense, source)],
        "edit": {"changes": {uri: text_edits}},
    }))
}

/// `true` when `context.only` is present, non-empty, and excludes quickfix
/// actions (we only provide the `quickfix` kind in B2 scope).
fn only_filters_out_quickfix(params: Option<&Value>) -> bool {
    let only = params
        .and_then(|p| p.get("context"))
        .and_then(|c| c.get("only"))
        .and_then(Value::as_array);
    match only {
        None => false,
        Some(kinds) if kinds.is_empty() => false,
        Some(kinds) => !kinds.iter().any(|kind| {
            kind.as_str()
                .is_some_and(|k| k == "quickfix" || k == "source.fixAll")
        }),
    }
}

/// `true` when any context diagnostic refers to this offense: the diagnostic
/// `code` (string or `{value}` form) equals the cop name and its range
/// intersects the offense byte range.
fn context_matches_offense(context: &[Value], offense: &Offense, source: &str) -> bool {
    context.iter().any(|diagnostic| {
        let code_matches = diagnostic
            .get("code")
            .is_some_and(|code| diagnostic_code(code) == offense.cop_name);
        if !code_matches {
            return false;
        }
        let Some(((sl, sc), (el, ec))) = diagnostic.get("range").and_then(parse_lsp_range) else {
            return true;
        };
        byte_ranges_intersect(
            offense.range.start_offset as usize,
            offense.range.end_offset as usize,
            lsp_position_to_offset((sl, sc), source),
            lsp_position_to_offset((el, ec), source),
        )
    })
}

/// Extract a diagnostic code string from either the plain-string form
/// (`"code": "Style/Foo"`) or the object form (`"code": {"value": ...}`).
fn diagnostic_code(code: &Value) -> &str {
    if let Some(name) = code.as_str() {
        return name;
    }
    code.get("value")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// Parse an LSP `{"start": {"line", "character"}, "end": {...}}` range value.
fn parse_lsp_range(value: &Value) -> Option<((u32, u32), (u32, u32))> {
    let start = value.get("start").and_then(parse_lsp_position)?;
    let end = value.get("end").and_then(parse_lsp_position)?;
    Some((start, end))
}

/// Parse an LSP `{"line", "character"}` position value.
fn parse_lsp_position(value: &Value) -> Option<(u32, u32)> {
    let line = value.get("line").and_then(Value::as_u64)?;
    let character = value.get("character").and_then(Value::as_u64)?;
    Some((u32::try_from(line).ok()?, u32::try_from(character).ok()?))
}

/// Convert an LSP line/character position back to a source byte offset.
///
/// This is the inverse of [`offset_to_lsp_position`]: `character` counts bytes
/// within the line (not UTF-16 code units — see the B2 follow-up note in
/// `docs/guides/lsp.md`). Positions past end-of-line/document clamp to the
/// line/document end so out-of-range client ranges degrade to empty results
/// instead of panics.
fn lsp_position_to_offset(position: (u32, u32), source: &str) -> usize {
    let (line, character) = (position.0 as usize, position.1 as usize);
    let bytes = source.as_bytes();
    let mut offset = 0usize;
    let mut current_line = 0usize;
    while current_line < line && offset < bytes.len() {
        if bytes[offset] == b'\n' {
            current_line += 1;
        }
        offset += 1;
    }
    if current_line < line {
        return bytes.len();
    }
    let line_end = bytes[offset..]
        .iter()
        .position(|b| *b == b'\n')
        .map_or(bytes.len(), |pos| offset + pos);
    offset + character.min(line_end.saturating_sub(offset))
}

/// Half-open byte-range intersection with point containment.
///
/// Non-empty ranges intersect when `a.start < b.end && b.start < a.end`
/// (adjacent ranges do not intersect). An empty range (cursor/selection
/// anchor) intersects when its point lies inside — or exactly on the edge
/// of — the other range, so a cursor parked at an offense boundary still
/// offers the fix.
fn byte_ranges_intersect(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    if a_start == a_end && b_start == b_end {
        return a_start == b_start;
    }
    if a_start == a_end {
        return b_start <= a_start && a_start <= b_end;
    }
    if b_start == b_end {
        return a_start <= b_start && b_start <= a_end;
    }
    a_start < b_end && b_start < a_end
}

fn publish_diagnostics(
    out: &mut impl Write,
    uri: &str,
    source: &str,
    file_label: &str,
    config: &MurphyConfig,
    registry: &CopRegistry,
) -> Result<(), super::AppError> {
    let offenses = run_offenses_for_source(source, file_label, config, registry);
    let diagnostics = offenses
        .iter()
        .map(|offense| to_diagnostic(offense, source))
        .collect::<Vec<_>>();
    let message = publish_diagnostics_message(uri, &diagnostics);
    write_message(out, &message)
}

fn run_offenses_for_source(
    source: &str,
    file: &str,
    config: &MurphyConfig,
    registry: &CopRegistry,
) -> Vec<Offense> {
    let cops_vec = registry.cops();
    // LSP: source mutates with every edit so cache hit rate would be ~0;
    // skip cache for now to avoid spurious disk writes on every keystroke.
    let offenses = super::lint_source(source, file, &cops_vec, &[], config, None);
    aggregate_with_config(offenses, config)
}

fn publish_diagnostics_message(uri: &str, diagnostics: &[Value]) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": uri,
            "diagnostics": diagnostics,
        }
    })
}

fn to_diagnostic(offense: &Offense, source: &str) -> Value {
    // LSP has no no-location concept: a filepath-only offense
    // (murphy-e7bz.41.2) degrades to the document start so it stays visible
    // instead of carrying a fabricated 1:1 from `Range::ZERO`.
    let (start, end) = if offense.has_location() {
        (
            offset_to_lsp_position(offense.range.start_offset, source),
            offset_to_lsp_position(offense.range.end_offset, source),
        )
    } else {
        ((0, 0), (0, 0))
    };

    json!({
        "range": {
            "start": {
                "line": start.0,
                "character": start.1,
            },
            "end": {
                "line": end.0,
                "character": end.1,
            },
        },
        "severity": lsp_severity(offense.severity),
        "code": offense.cop_name,
        "source": "murphy",
        "message": offense.message,
    })
}

fn lsp_severity(severity: Severity) -> u8 {
    match severity {
        Severity::Warning => 2,
        Severity::Error => 1,
    }
}

fn offset_to_lsp_position(offset: u32, source: &str) -> (u32, u32) {
    if source.is_empty() {
        return (0, 0);
    }

    let offset = offset as usize;
    let bytes = source.as_bytes();
    let mut line: u32 = 0;
    let mut character: u32 = 0;

    let offset = offset.min(bytes.len());
    for byte in &bytes[..offset] {
        if *byte == b'\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }

    (line, character)
}

fn uri_to_file_path(uri: &str) -> Option<&str> {
    uri.strip_prefix("file://")
}

fn invalid_params_error(id: Value, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": LSP_ERROR_INVALID_PARAMS,
            "message": message,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(message: &Value) -> Vec<u8> {
        let body = serde_json::to_vec(message).expect("frame body must serialize");
        let mut data = Vec::new();
        data.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        data.extend_from_slice(&body);
        data
    }

    #[test]
    fn byte_ranges_intersect_overlaps_but_not_adjacent() {
        assert!(byte_ranges_intersect(0, 8, 0, 8));
        assert!(byte_ranges_intersect(0, 8, 4, 12));
        assert!(!byte_ranges_intersect(0, 8, 8, 12));
        assert!(!byte_ranges_intersect(8, 12, 0, 8));
    }

    #[test]
    fn byte_ranges_intersect_point_containment_includes_edges() {
        // Cursor (empty range) on, inside, or at the edge of an offense
        // still offers the fix; a cursor outside does not.
        assert!(byte_ranges_intersect(0, 8, 0, 0));
        assert!(byte_ranges_intersect(0, 8, 8, 8));
        assert!(byte_ranges_intersect(0, 8, 4, 4));
        assert!(!byte_ranges_intersect(0, 8, 9, 9));
    }

    #[test]
    fn lsp_position_round_trips_through_offset() {
        let source = "x = 1\ny = 2\n";
        for offset in [0u32, 3, 5, 6, 8, 11, 12] {
            let pos = offset_to_lsp_position(offset, source);
            assert_eq!(
                lsp_position_to_offset(pos, source),
                offset as usize,
                "offset {offset} must round-trip"
            );
        }
    }

    #[test]
    fn lsp_position_clamps_past_end() {
        let source = "ab\n";
        assert_eq!(lsp_position_to_offset((10, 0), source), source.len());
        assert_eq!(lsp_position_to_offset((0, 99), source), 2);
        assert_eq!(lsp_position_to_offset((0, 0), ""), 0);
    }

    #[test]
    fn diagnostic_code_reads_string_and_object_forms() {
        assert_eq!(diagnostic_code(&json!("Style/Foo")), "Style/Foo");
        assert_eq!(diagnostic_code(&json!({"value": "Lint/Bar"})), "Lint/Bar");
        assert_eq!(diagnostic_code(&json!(null)), "");
    }

    #[test]
    fn only_filter_keeps_quickfix_and_drops_others() {
        assert!(!only_filters_out_quickfix(None));
        assert!(!only_filters_out_quickfix(Some(
            &json!({"context": {"only": ["quickfix"]}})
        )));
        assert!(!only_filters_out_quickfix(Some(&json!({}))));
        assert!(only_filters_out_quickfix(Some(
            &json!({"context": {"only": ["refactor"]}})
        )));
    }

    #[test]
    fn build_quickfix_action_maps_edits_to_text_edits() {
        use murphy_core::{Autocorrect, Edit, Range};

        let offense = Offense::new(
            "a.rb",
            "Layout/TrailingWhitespace",
            Range {
                start_offset: 5,
                end_offset: 7,
            },
            Severity::Warning,
            "Trailing whitespace detected.",
        )
        .with_autocorrect(Autocorrect {
            edits: vec![Edit {
                range: Range {
                    start_offset: 5,
                    end_offset: 7,
                },
                replacement: String::new(),
            }],
        });

        let action = build_quickfix_action("file:///a.rb", &offense, "x = 1  \n")
            .expect("fixable offense must produce an action");
        assert_eq!(action["kind"], json!("quickfix"));
        assert_eq!(
            action["title"],
            json!("Murphy: fix Layout/TrailingWhitespace")
        );
        let edits = action["edit"]["changes"]["file:///a.rb"]
            .as_array()
            .expect("changes must list text edits");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0]["newText"], json!(""));
        assert_eq!(
            edits[0]["range"]["start"],
            json!({"line": 0, "character": 5})
        );
        assert_eq!(edits[0]["range"]["end"], json!({"line": 0, "character": 7}));
    }

    #[test]
    fn build_quickfix_action_returns_none_without_fix() {
        use murphy_core::Range;

        let offense = Offense::new(
            "a.rb",
            "Lint/Debugger",
            Range {
                start_offset: 0,
                end_offset: 8,
            },
            Severity::Warning,
            "Remove debugger entry point `debugger`.",
        );
        assert_eq!(
            build_quickfix_action("file:///a.rb", &offense, "debugger"),
            None
        );
    }

    #[test]
    fn context_matches_offense_by_code_and_range() {
        use murphy_core::Range;

        let source = "x = 1  \n";
        let offense = Offense::new(
            "a.rb",
            "Layout/TrailingWhitespace",
            Range {
                start_offset: 5,
                end_offset: 7,
            },
            Severity::Warning,
            "Trailing whitespace detected.",
        );
        let matching = json!({
            "range": {
                "start": {"line": 0, "character": 5},
                "end": {"line": 0, "character": 7},
            },
            "code": "Layout/TrailingWhitespace",
        });
        assert!(context_matches_offense(&[matching], &offense, source));

        let wrong_code = json!({
            "range": {
                "start": {"line": 0, "character": 5},
                "end": {"line": 0, "character": 7},
            },
            "code": "Style/Foo",
        });
        assert!(!context_matches_offense(&[wrong_code], &offense, source));

        let far_range = json!({
            "range": {
                "start": {"line": 5, "character": 0},
                "end": {"line": 5, "character": 1},
            },
            "code": "Layout/TrailingWhitespace",
        });
        assert!(!context_matches_offense(&[far_range], &offense, source));
    }

    #[test]
    fn parse_message_consumes_each_frame() {
        let first = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"});
        let second = json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown"});
        let mut stream = Vec::new();
        stream.extend_from_slice(&frame(&first));
        stream.extend_from_slice(&frame(&second));

        let mut cursor = 0;
        let (_, first_len) = parse_message(&stream[cursor..]).expect("first frame must parse");
        cursor += first_len;
        let (second_message, _second_len) =
            parse_message(&stream[cursor..]).expect("second frame must parse");

        assert_eq!(second_message.get("id"), Some(&json!(2)));
    }
}
