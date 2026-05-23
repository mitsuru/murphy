use murphy_core::{CopRegistry, MurphyConfig, Offense, Severity, aggregate_with_config};

use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::Path;

const EXIT_OK: u8 = 0;
const LSP_ERROR_METHOD_NOT_FOUND: i32 = -32601;
const LSP_ERROR_INVALID_PARAMS: i32 = -32602;

pub fn run(_args: &[String]) -> Result<u8, super::AppError> {
    let config =
        MurphyConfig::load(Path::new(".")).map_err(|e| super::AppError::setup(e.to_string()))?;

    let registry = CopRegistry::discover_with_config(Path::new("."), &config)
        .map_err(|e| super::AppError::setup(e.to_string()))?;

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
                    "codeActionProvider": false,
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
                let response = json!({"jsonrpc": "2.0", "id": value, "result": []});
                write_message(&mut stdout, &response)?;
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
    let offenses = super::lint_source(source, file, &cops_vec);
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
    let start = offset_to_lsp_position(offense.range.start_offset, source);
    let end = offset_to_lsp_position(offense.range.end_offset, source);

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
