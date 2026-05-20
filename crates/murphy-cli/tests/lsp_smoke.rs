//! Smoke test for `murphy lsp`.
//!
//! Verifies the minimal JSON-RPC stdio route implemented in phase 7.3:
//! - initialize handshake
//! - didOpen diagnostics publication
//! - graceful shutdown
//!
//! The test drives the binary with framed LSP messages and parses LSP frames from
//! stdout to assert publish diagnostics are emitted for a simple offense source.

use assert_cmd::Command;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

const DIRTY_SOURCE: &str = "puts 'x'\n";

fn lsp_frame(json: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(json).expect("frame body must be serializable");
    let mut frame = Vec::new();
    frame.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    frame.extend_from_slice(&body);
    frame
}

fn parse_frames(output: &[u8]) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut cursor = 0;

    while cursor < output.len() {
        let remainder = &output[cursor..];
        let header_end = remainder
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("must receive complete LSP header with body")
            + 4;

        let headers = String::from_utf8_lossy(&remainder[..header_end - 4]);
        let mut content_length: Option<usize> = None;
        for line in headers.lines() {
            if let Some(value) = line.strip_prefix("Content-Length:") {
                content_length = value
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|len| *len <= 100_000_000);
            }
        }
        let body_len = content_length.expect("Content-Length header must be present");
        let body_start = cursor + header_end;
        let body_end = body_start + body_len;

        let message = serde_json::from_slice(&output[body_start..body_end])
            .expect("lsp body must be valid JSON");
        messages.push(message);
        cursor = body_end;
    }

    messages
}

#[test]
fn lsp_initialize_and_open_publishes_diagnostics() {
    let dir = tempdir().expect("create temp dir");
    let path = dir.path().join("app.rb");
    fs::write(&path, DIRTY_SOURCE).expect("write app.rb");

    let uri = format!("file://{}", path.display());

    let initialize = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": null,
            "capabilities": {},
        }
    }));

    let did_open = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": "ruby",
                "version": 1,
                "text": DIRTY_SOURCE,
            }
        }
    }));

    let shutdown = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown"
    }));

    let exit = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "method": "exit"
    }));

    let input: Vec<u8> = initialize
        .into_iter()
        .chain(did_open)
        .chain(shutdown)
        .chain(exit)
        .collect();

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lsp")
        .current_dir(&dir)
        .write_stdin(input)
        .assert()
        .code(0);

    let output = assert.get_output().stdout.clone();
    let frames = parse_frames(&output);

    let has_initialize = frames.iter().any(|m| {
        m.get("id") == Some(&json!(1)) && m.get("result").is_some()
    });
    assert!(has_initialize, "initialize response with id=1 must be present");

    let diagnostics_message = frames
        .into_iter()
        .find(|m| m.get("method") == Some(&Value::from("textDocument/publishDiagnostics")));
    let diagnostics = diagnostics_message.expect("didOpen should emit publishDiagnostics");
    let diagnostics = diagnostics
        .get("params")
        .expect("diagnostics params must exist")
        .get("diagnostics")
        .and_then(Value::as_array)
        .expect("diagnostics must be an array");

    assert!(
        !diagnostics.is_empty(),
        "offense source should produce at least one diagnostic"
    );

    let first = &diagnostics[0];
    let message = first
        .get("message")
        .and_then(Value::as_str)
        .expect("diagnostic message must be present");
    assert!(!message.is_empty(), "diagnostic message must not be empty");

    let severity = first
        .get("severity")
        .and_then(Value::as_u64)
        .expect("diagnostic severity must be present");
    assert_eq!(severity, 2, "warning maps to LSP severity Warning by convention");

    let code = first
        .get("code")
        .and_then(Value::as_str)
        .or_else(|| first.get("code").and_then(|c| c.get("value")).and_then(Value::as_str));
    assert!(
        code.is_some(),
        "diagnostic code should map from offense.cop_name"
    );

    let _ = Path::new("app.rb");
}

#[test]
fn lsp_did_close_clears_diagnostics() {
    let dir = tempdir().expect("create temp dir");
    let path = dir.path().join("app.rb");
    fs::write(&path, DIRTY_SOURCE).expect("write app.rb");

    let uri = format!("file://{}", path.display());

    let initialize = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": null,
            "capabilities": {},
        }
    }));

    let did_open = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": uri,
                "languageId": "ruby",
                "version": 1,
                "text": DIRTY_SOURCE,
            }
        }
    }));

    let did_close = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didClose",
        "params": {
            "textDocument": {
                "uri": uri,
            }
        }
    }));

    let shutdown = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown"
    }));

    let exit = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "method": "exit"
    }));

    let input: Vec<u8> = initialize
        .into_iter()
        .chain(did_open)
        .chain(did_close)
        .chain(shutdown)
        .chain(exit)
        .collect();

    let output = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lsp")
        .current_dir(&dir)
        .write_stdin(input)
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();

    let frames = parse_frames(&output);
    let diagnostics: Vec<_> = frames
        .into_iter()
        .filter(|frame| {
            frame.get("method") == Some(&Value::from("textDocument/publishDiagnostics"))
        })
        .collect();

    assert!(diagnostics.len() >= 2, "open and close should emit diagnostics twice");

    let open_diagnostics = diagnostics[0]
        .get("params")
        .expect("open diagnostics params must exist")
        .get("diagnostics")
        .and_then(Value::as_array)
        .expect("diagnostics must be an array");
    assert!(!open_diagnostics.is_empty(), "open should emit at least one diagnostic");

    let close_diagnostics = diagnostics[1]
        .get("params")
        .expect("close diagnostics params must exist")
        .get("diagnostics")
        .and_then(Value::as_array)
        .expect("diagnostics must be an array");
    assert!(close_diagnostics.is_empty(), "close should clear diagnostics");
}

#[test]
fn lsp_non_file_uri_returns_invalid_params() {
    let dir = tempdir().expect("create temp dir");

    let initialize = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": null,
            "capabilities": {},
        }
    }));

    let invalid_open = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "untitled:app.rb",
                "languageId": "ruby",
                "version": 1,
                "text": DIRTY_SOURCE,
            }
        }
    }));

    let shutdown = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "shutdown"
    }));

    let exit = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "method": "exit"
    }));

    let input: Vec<u8> = initialize
        .into_iter()
        .chain(invalid_open)
        .chain(shutdown)
        .chain(exit)
        .collect();

    let output = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lsp")
        .current_dir(&dir)
        .write_stdin(input)
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();

    let frames = parse_frames(&output);

    let has_invalid = frames
        .into_iter()
        .any(|frame| {
            frame.get("id") == Some(&json!(2))
                && frame.get("error").is_some()
                && frame
                    .get("error")
                    .and_then(|error| error.get("code").and_then(Value::as_i64))
                    == Some(-32602)
        });

    assert!(has_invalid, "didOpen with non-file URI should return InvalidParams");
}

#[test]
fn lsp_non_file_uri_change_returns_invalid_params() {
    let dir = tempdir().expect("create temp dir");

    let initialize = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": null,
            "capabilities": {},
        }
    }));

    let invalid_change = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": "untitled:app.rb",
                "version": 1,
            },
            "contentChanges": [
                {
                    "text": DIRTY_SOURCE,
                }
            ]
        }
    }));

    let shutdown = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "shutdown"
    }));

    let exit = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "method": "exit"
    }));

    let input: Vec<u8> = initialize
        .into_iter()
        .chain(invalid_change)
        .chain(shutdown)
        .chain(exit)
        .collect();

    let output = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lsp")
        .current_dir(&dir)
        .write_stdin(input)
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();

    let frames = parse_frames(&output);

    let has_invalid = frames
        .into_iter()
        .any(|frame| {
            frame.get("id") == Some(&json!(2))
                && frame.get("error").is_some()
                && frame
                    .get("error")
                    .and_then(|error| error.get("code").and_then(Value::as_i64))
                    == Some(-32602)
        });

    assert!(has_invalid, "didChange with non-file URI should return InvalidParams");
}
