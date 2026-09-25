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

    let has_initialize = frames
        .iter()
        .any(|m| m.get("id") == Some(&json!(1)) && m.get("result").is_some());
    assert!(
        has_initialize,
        "initialize response with id=1 must be present"
    );

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
    assert_eq!(
        severity, 2,
        "warning maps to LSP severity Warning by convention"
    );

    let code = first.get("code").and_then(Value::as_str).or_else(|| {
        first
            .get("code")
            .and_then(|c| c.get("value"))
            .and_then(Value::as_str)
    });
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

    assert!(
        diagnostics.len() >= 2,
        "open and close should emit diagnostics twice"
    );

    let open_diagnostics = diagnostics[0]
        .get("params")
        .expect("open diagnostics params must exist")
        .get("diagnostics")
        .and_then(Value::as_array)
        .expect("diagnostics must be an array");
    assert!(
        !open_diagnostics.is_empty(),
        "open should emit at least one diagnostic"
    );

    let close_diagnostics = diagnostics[1]
        .get("params")
        .expect("close diagnostics params must exist")
        .get("diagnostics")
        .and_then(Value::as_array)
        .expect("diagnostics must be an array");
    assert!(
        close_diagnostics.is_empty(),
        "close should clear diagnostics"
    );
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

    let has_invalid = frames.into_iter().any(|frame| {
        frame.get("id") == Some(&json!(2))
            && frame.get("error").is_some()
            && frame
                .get("error")
                .and_then(|error| error.get("code").and_then(Value::as_i64))
                == Some(-32602)
    });

    assert!(
        has_invalid,
        "didOpen with non-file URI should return InvalidParams"
    );
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

    let has_invalid = frames.into_iter().any(|frame| {
        frame.get("id") == Some(&json!(2))
            && frame.get("error").is_some()
            && frame
                .get("error")
                .and_then(|error| error.get("code").and_then(Value::as_i64))
                == Some(-32602)
    });

    assert!(
        has_invalid,
        "didChange with non-file URI should return InvalidParams"
    );
}

// B2 (murphy-fmw.2.2): codeAction returns a quickfix built from the
// offense's autocorrect edits, with a WorkspaceEdit for the open URI.
#[test]
fn lsp_code_action_returns_quickfix_with_text_edit() {
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

    let code_action = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 1, "character": 0},
            },
            "context": {"diagnostics": []},
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
        .chain(did_open)
        .chain(code_action)
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
    let actions = frames
        .iter()
        .find(|m| m.get("id") == Some(&json!(2)))
        .and_then(|m| m.get("result"))
        .and_then(Value::as_array)
        .expect("codeAction response must carry a result array");

    assert!(
        !actions.is_empty(),
        "fixable offense source should produce at least one quickfix"
    );

    let first = &actions[0];
    assert_eq!(first.get("kind"), Some(&json!("quickfix")));
    let title = first
        .get("title")
        .and_then(Value::as_str)
        .expect("action title must be present");
    assert!(
        title.starts_with("Murphy: fix "),
        "action title must name the cop, got {title:?}"
    );

    let edits = first
        .get("edit")
        .and_then(|e| e.get("changes"))
        .and_then(|c| c.get(&uri))
        .and_then(Value::as_array)
        .expect("quickfix must carry a WorkspaceEdit for the open URI");
    assert!(
        !edits.is_empty(),
        "quickfix must carry at least one TextEdit"
    );
    for edit in edits {
        assert!(edit.get("range").is_some(), "TextEdit must carry a range");
        assert!(
            edit.get("newText").and_then(Value::as_str).is_some(),
            "TextEdit must carry newText"
        );
    }
}

// B2: a codeAction range with no intersecting offense yields no actions,
// and an `only` filter excluding quickfix yields no actions.
#[test]
fn lsp_code_action_far_range_and_only_filter_return_empty() {
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

    let far_action = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 50, "character": 0},
                "end": {"line": 51, "character": 0},
            },
            "context": {"diagnostics": []},
        }
    }));

    let filtered_action = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 1, "character": 0},
            },
            "context": {"diagnostics": [], "only": ["refactor"]},
        }
    }));

    let shutdown = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "shutdown"
    }));

    let exit = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "method": "exit"
    }));

    let input: Vec<u8> = initialize
        .into_iter()
        .chain(did_open)
        .chain(far_action)
        .chain(filtered_action)
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
    for id in [2, 3] {
        let actions = frames
            .iter()
            .find(|m| m.get("id") == Some(&json!(id)))
            .and_then(|m| m.get("result"))
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("codeAction id={id} must carry a result array"));
        assert!(
            actions.is_empty(),
            "codeAction id={id} should return no actions"
        );
    }
}

// B2: `murphy/rangeDiagnostics` re-lints the open document and returns only
// diagnostics intersecting the requested range. Two trailing-whitespace
// lines: asking for line 1 must not return line 0 diagnostics.
#[test]
fn lsp_range_diagnostics_filters_to_requested_lines() {
    let dir = tempdir().expect("create temp dir");
    let path = dir.path().join("app.rb");
    let source = "x = 1  \ny = 2  \n";
    fs::write(&path, source).expect("write app.rb");

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
                "text": source,
            }
        }
    }));

    let range_lint = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "murphy/rangeDiagnostics",
        "params": {
            "textDocument": {"uri": uri},
            "range": {
                "start": {"line": 1, "character": 0},
                "end": {"line": 2, "character": 0},
            },
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
        .chain(did_open)
        .chain(range_lint)
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
    let diagnostics = frames
        .iter()
        .find(|m| m.get("id") == Some(&json!(2)))
        .and_then(|m| m.get("result"))
        .and_then(Value::as_array)
        .expect("rangeDiagnostics response must carry a result array");

    assert!(
        !diagnostics.is_empty(),
        "line 1 carries offenses, so the range result must not be empty"
    );
    for diagnostic in diagnostics {
        let line = diagnostic
            .get("range")
            .and_then(|r| r.get("start"))
            .and_then(|s| s.get("line"))
            .and_then(Value::as_u64)
            .expect("diagnostic range must be present");
        assert_eq!(line, 1, "range result must only contain line 1 diagnostics");
    }
}

// B2: `murphy/rangeDiagnostics` with a non-file URI returns InvalidParams,
// matching didOpen/didChange URI policy.
#[test]
fn lsp_range_diagnostics_non_file_uri_returns_invalid_params() {
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

    let invalid_range = lsp_frame(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "murphy/rangeDiagnostics",
        "params": {
            "textDocument": {"uri": "untitled:app.rb"},
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 1, "character": 0},
            },
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
        .chain(invalid_range)
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
    let has_invalid = frames.iter().any(|frame| {
        frame.get("id") == Some(&json!(2))
            && frame.get("error").is_some()
            && frame
                .get("error")
                .and_then(|error| error.get("code").and_then(Value::as_i64))
                == Some(-32602)
    });
    assert!(
        has_invalid,
        "rangeDiagnostics with non-file URI should return InvalidParams"
    );
}
