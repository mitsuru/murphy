//! Integration tests for the `murphy` binary (Task 7).
//!
//! These exercise the *compiled binary* via `assert_cmd` — the same surface a
//! user invokes — not a library API (the cli is a bin crate with no lib).
//!
//! Pinned contract (design §5 + plan Task 7):
//! - `murphy lint <clean.rb>`  → stdout is an empty JSON array, exit `0`.
//! - `murphy lint <dirty.rb>`  → stdout is a 1-element JSON array whose
//!   `cop_name == "Murphy/NoReceiverPuts"`, exit `1`.
//! - `murphy lint <missing>`   → exit `2` (file/setup error).
//! - `murphy lint <broken.rb>` → stdout is a 1-element JSON array whose
//!   `cop_name == "Murphy/Syntax"` (cops skipped), exit `1` (design §6).
//!
//! stdout is asserted by *decoding JSON*, never brittle string matching
//! (beyond the canonical empty-array case). Diagnostics go to stderr, so the
//! error-exit cases deliberately do not assert on stdout content.

use assert_cmd::Command;
use murphy_core::SYNTAX_COP_NAME;
use std::fs;
use tempfile::tempdir;

const CLEAN_SOURCE: &str = "# frozen_string_literal: true\n\nx = 1\n";
const CLEAN_SOURCE_2: &str = "# frozen_string_literal: true\n\ny = 2\n";
const DIRTY_PUTS_SOURCE: &str = "# frozen_string_literal: true\n\nputs 'hi'\n";
const DIRTY_PUTS_X_SOURCE: &str = "# frozen_string_literal: true\n\nputs 'x'\n";

/// `lint` a clean file → exit 0, stdout is an empty JSON array.
#[test]
fn lint_clean_file_exits_0_with_empty_json_array() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("clean.rb");
    fs::write(&path, CLEAN_SOURCE).expect("write clean.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(0);

    let stdout = &assert.get_output().stdout;
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout must be a JSON array");
    assert!(
        parsed.is_empty(),
        "clean file must yield zero offenses, got {parsed:?}"
    );
}

/// `lint` a file containing `puts` → exit 1, one NoReceiverPuts offense.
#[test]
fn lint_dirty_file_exits_1_with_one_offense() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("dirty.rb");
    fs::write(&path, DIRTY_PUTS_SOURCE).expect("write dirty.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(1);

    let stdout = &assert.get_output().stdout;
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        1,
        "expected exactly one offense, got {parsed:?}"
    );
    assert_eq!(
        parsed[0]["cop_name"], "Murphy/NoReceiverPuts",
        "offense must be from the NoReceiverPuts cop"
    );
}

#[test]
fn lint_format_progress_omits_offense_details() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("dirty.rb");
    fs::write(&path, DIRTY_PUTS_SOURCE).expect("write dirty.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("progress")
        .arg(&path)
        .assert()
        .code(1);

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("Inspecting 1 file"),
        "progress output should include header, got: {stdout:?}"
    );
    assert!(
        stdout.contains("1 offense detected"),
        "progress output should include summary, got: {stdout:?}"
    );
    assert!(
        !stdout.contains("Murphy/NoReceiverPuts"),
        "progress output should omit offense details, got: {stdout:?}"
    );
}

#[test]
fn lint_default_output_is_human_readable() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("dirty.rb");
    fs::write(&path, DIRTY_PUTS_SOURCE).expect("write dirty.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg(&path)
        .assert()
        .code(1);

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("Inspecting 1 file"),
        "default output should include human progress header, got: {stdout:?}"
    );
    assert!(
        stdout.contains("C"),
        "default output should include progress markers, got: {stdout:?}"
    );
    assert!(
        stdout.contains("Murphy/NoReceiverPuts"),
        "default output should include offense details, got: {stdout:?}"
    );
}

#[test]
fn lint_profile_outputs_profile_json() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("clean.rb");
    fs::write(&path, CLEAN_SOURCE).expect("write clean.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile")
        .arg(&path)
        .assert()
        .code(0);

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim_end()).expect("stdout must be a profile JSON object");

    assert!(parsed.get("cop_wall_micros").is_some());
    assert!(parsed.get("cop_file_micros").is_some());
    assert!(
        parsed
            .get("p95_micros")
            .and_then(serde_json::Value::as_u64)
            .is_some()
    );
    assert!(parsed.get("hot_files").is_some());
    assert!(parsed.get("invocation_count").is_some());
}

#[test]
fn lint_profile_outputs_speedscope_json() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("clean.rb");
    fs::write(&path, CLEAN_SOURCE).expect("write clean.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile")
        .arg("--profile-format")
        .arg("speedscope")
        .arg(&path)
        .assert()
        .code(0);

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim_end()).expect("stdout must be a profile JSON object");

    assert_eq!(
        parsed
            .get("process_name")
            .and_then(serde_json::Value::as_str),
        Some("murphy-lint")
    );
    let events = parsed
        .get("traceEvents")
        .and_then(serde_json::Value::as_array)
        .expect("traceEvents must be an array");
    assert_eq!(
        events.len(),
        parsed
            .get("event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default() as usize
    );

    let mut previous_ts: Option<u64> = None;
    let mut previous_thread: u64 = 0;
    for event in events {
        let start = event.get("ts").and_then(serde_json::Value::as_u64);
        let name = event.get("name").and_then(serde_json::Value::as_str);
        assert!(start.is_some() && name.is_some());
        let start = start.unwrap();
        let thread = event
            .get("tid")
            .and_then(serde_json::Value::as_u64)
            .expect("tid");
        if let Some(last) = previous_ts {
            assert!(last <= start);
        }
        assert!(
            event
                .get("args")
                .and_then(|args| args.get("thread_name"))
                .and_then(serde_json::Value::as_str)
                .is_some()
        );
        previous_ts = Some(start);
        previous_thread = thread;
    }

    assert!(previous_thread >= 1);
}

#[test]
fn lint_profile_speedscope_thread_ids_follow_file_order() {
    let dir = tempdir().expect("create tempdir");
    let first = dir.path().join("a.rb");
    let second = dir.path().join("z.rb");
    fs::write(&first, CLEAN_SOURCE).expect("write a.rb");
    fs::write(&second, DIRTY_PUTS_SOURCE).expect("write z.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile")
        .arg("--profile-format")
        .arg("speedscope")
        .arg(".")
        .current_dir(&dir)
        .assert()
        .code(1);

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim_end()).expect("stdout must be a profile JSON object");
    let events = parsed
        .get("traceEvents")
        .and_then(serde_json::Value::as_array)
        .expect("traceEvents must be an array");

    let mut tid_files: Vec<(u64, String)> = events
        .iter()
        .filter_map(|event| {
            let tid = event.get("tid").and_then(serde_json::Value::as_u64)?;
            let args = event.get("args").unwrap_or(&serde_json::Value::Null);
            let thread_name = args
                .get("thread_name")
                .and_then(serde_json::Value::as_str)?;
            let file = thread_name.strip_prefix("file:").unwrap_or("").to_owned();
            Some((tid, file))
        })
        .collect();

    tid_files.sort_by_key(|(tid, _)| *tid);
    tid_files.dedup_by_key(|(tid, _)| *tid);
    assert!(!tid_files.is_empty());

    let actual_files: Vec<String> = tid_files.iter().map(|(_, file)| file.clone()).collect();
    let mut expected_files = actual_files.clone();
    expected_files.sort_unstable();

    assert_eq!(actual_files, expected_files);
}

#[test]
fn lint_profile_format_requires_profile_flag() {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile-format")
        .arg("speedscope")
        .assert()
        .code(2);

    assert!(
        String::from_utf8_lossy(&assert.get_output().stderr)
            .contains("--profile-format requires --profile")
    );
}

#[test]
fn lint_profile_unknown_profile_format_exits_2() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("clean.rb");
    fs::write(&path, CLEAN_SOURCE).expect("write clean.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--profile")
        .arg("--profile-format")
        .arg("not-a-format")
        .arg(&path)
        .assert()
        .code(2);

    assert!(
        String::from_utf8_lossy(&assert.get_output().stderr)
            .contains("unknown --profile-format value")
    );
}

#[test]
fn lint_format_json_preserves_machine_readable_stdout() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("dirty.rb");
    fs::write(&path, DIRTY_PUTS_SOURCE).expect("write dirty.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> = serde_json::from_slice(&assert.get_output().stdout)
        .expect("--format json stdout must be a JSON array");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["cop_name"], "Murphy/NoReceiverPuts");
}

#[test]
fn lint_file_with_disable_comment_suppresses_offenses() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("with_disable.rb");
    fs::write(
        &path,
        "# frozen_string_literal: true\n\n# murphy:disable Murphy/NoReceiverPuts\nputs 'x'\n",
    )
    .expect("write with_disable.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(0);

    assert_eq!(assert.get_output().stdout, b"[]\n");
}

#[test]
fn lint_debug_emits_progress_to_stderr_without_touching_stdout_json() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("dirty.rb");
    fs::write(&path, DIRTY_PUTS_SOURCE).expect("write dirty.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("--debug")
        .arg(&path)
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> = serde_json::from_slice(&assert.get_output().stdout)
        .expect("stdout must remain a JSON array in debug mode");
    assert_eq!(parsed.len(), 1, "debug must not change offenses");

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("murphy: debug: lint start files=1"),
        "debug stderr must include lint start progress, got: {stderr:?}"
    );
    assert!(
        stderr.contains("murphy: debug: batch 1/1 read files=1"),
        "debug stderr must include batch read progress, got: {stderr:?}"
    );
    assert!(
        stderr.contains("murphy: debug: batch 1/1 lint unique="),
        "debug stderr must include batch lint progress, got: {stderr:?}"
    );
    assert!(
        stderr.contains("native_ms="),
        "debug stderr must include native lint timing, got: {stderr:?}"
    );
    assert!(
        stderr.contains("parse_ms="),
        "debug stderr must include parse timing, got: {stderr:?}"
    );
    assert!(
        stderr.contains("cops_ms="),
        "debug stderr must include native cop timing, got: {stderr:?}"
    );
    assert!(
        stderr.contains("mruby_ms="),
        "debug stderr must include mruby lint timing, got: {stderr:?}"
    );
    assert!(
        stderr.contains("murphy: debug: aggregate"),
        "debug stderr must include aggregate progress, got: {stderr:?}"
    );
    assert!(
        stderr.contains("murphy: debug: top cops"),
        "debug stderr must include top cop counts, got: {stderr:?}"
    );
    assert!(
        stderr.contains("Murphy/NoReceiverPuts=1"),
        "debug top cop counts must identify noisy cops, got: {stderr:?}"
    );
}

#[test]
fn lint_file_with_disable_then_enable_comment_only_reattaches() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("with_enable.rb");
    fs::write(
        &path,
        "# frozen_string_literal: true\n# murphy:disable Murphy/NoReceiverPuts\nputs 'suppressed'\n# murphy:enable\nputs 'reported'\n",
    )
    .expect("write with_enable.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        1,
        "enable must re-enable the cop for following lines, got {parsed:?}"
    );
    assert_eq!(parsed[0]["cop_name"], "Murphy/NoReceiverPuts");
}

#[test]
fn lint_file_with_todo_comment_skips_current_line_only() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("with_todo.rb");
    fs::write(
        &path,
        "# frozen_string_literal: true\nputs 'suppressed' # murphy:todo Murphy/NoReceiverPuts\nputs 'reported'\n",
    )
    .expect("write with_todo.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        1,
        "todo must suppress only that line, got {parsed:?}"
    );
    assert_eq!(parsed[0]["cop_name"], "Murphy/NoReceiverPuts");
}

#[test]
fn lint_file_with_todo_without_cop_suppresses_all_offenses_on_line() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("with_todo_all.rb");
    fs::write(
        &path,
        "# frozen_string_literal: true\nputs 'first' # murphy:todo\nputs 'second'\n",
    )
    .expect("write with_todo_all.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        1,
        "todo without cop should only suppress current-line offenses, got {parsed:?}"
    );
    assert_eq!(parsed[0]["cop_name"], "Murphy/NoReceiverPuts");
}

#[test]
fn lint_file_with_disable_comment_does_not_hide_syntax_offenses() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("syntax_with_disable.rb");
    fs::write(
        &path,
        "# frozen_string_literal: true\n# murphy:disable\ndef broken(\n",
    )
    .expect("write syntax_with_disable.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        1,
        "syntax offenses should still be reported despite inline disable directives"
    );
    assert_eq!(parsed[0]["cop_name"], "Murphy/Syntax");
}

/// `lint` a path that does not exist → exit 2 (file/setup error).
#[test]
fn lint_missing_file_exits_2() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("does_not_exist.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(2);

    // Contract guard: stdout is ONLY ever JSON; error paths emit nothing on it.
    assert!(
        assert.get_output().stdout.is_empty(),
        "error path must write nothing to stdout, got {:?}",
        assert.get_output().stdout
    );
}

/// `lint` an unparseable file → exit 1, exactly one `Murphy/Syntax` offense,
/// cops skipped for that file (design §6: "1 offense, skip cops, continue").
#[test]
fn lint_syntax_error_file_exits_1_with_one_syntax_offense() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("broken.rb");
    // Genuinely unparseable Ruby that ALSO textually contains a receiver-less
    // `puts` (a line `NoReceiverPuts` WOULD flag if cops ran). Verified with
    // the built binary: prism's `parse` returns Err on this source (the
    // trailing `def (` is a hard parse error), so it remains a parse failure,
    // not a parsed file with a NoReceiverPuts offense. This pins design §6's
    // skip-cops contract: a parse failure yields ONLY the synthetic syntax
    // offense and the cop pass is genuinely skipped.
    fs::write(&path, "puts \"x\"\ndef (\n").expect("write broken.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(1);

    let stdout = &assert.get_output().stdout;
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        1,
        "syntax-error file must yield exactly one offense, got {parsed:?}"
    );
    assert_eq!(
        parsed[0]["cop_name"], SYNTAX_COP_NAME,
        "the single offense must be the synthetic syntax-error offense"
    );
    // Skip-cops invariant (design §6): even though the source textually
    // contains a receiver-less `puts`, NO cop offense is emitted because the
    // cop pass is skipped on a parse failure (there is no AST).
    assert!(
        !parsed
            .iter()
            .any(|o| o["cop_name"] == "Murphy/NoReceiverPuts"),
        "cops must be skipped on a parse failure — no NoReceiverPuts offense \
         despite the receiver-less puts in source, got {parsed:?}"
    );
    assert_eq!(
        parsed[0]["severity"], "error",
        "a syntax error is an Error-severity offense"
    );
    let message = parsed[0]["message"]
        .as_str()
        .expect("syntax offense message must be a JSON string");
    assert!(
        !message.is_empty(),
        "syntax offense must carry a non-empty message, got {message:?}"
    );
    // Message-verbatim invariant (design §6): the producer uses prism's
    // first-error text directly. A refactor to the `Display` form would
    // prepend `"parse error at bytes A..B: "` — guard against that drift
    // without hard-equalling the prism string (which can vary across versions).
    assert!(
        !message.starts_with("parse error at bytes"),
        "syntax offense message must be prism's verbatim text, not the \
         ParseError Display form, got {message:?}"
    );
}

// --- Phase 2 Task 3: extra frozen-contract guards (murphy-eu9 #3) ---
//
// Test-only hardening. These pin existing Phase-1 behavior so Phase 2's
// pipeline rework (parallelism / discovery / memoization) cannot silently
// regress the frozen CLI contract. They MUST pass against current code.

/// Multi-file list where ONE path is missing → exit `2`.
///
/// `run` does `files.par_iter().map(lint_one_file).collect::<Result<_, _>>()?`:
/// an unreadable file is a setup-class `AppError` and the fallible `collect`
/// short-circuits on the first `Err`, so the `?` aborts the WHOLE run with
/// exit `2` — it is NOT a per-file skip (design §6: an I/O error aborts the
/// run; a *parse* failure, by contrast, is an offense and would exit `1`).
/// Task 6's discovery wiring leaves this explicit-file path unchanged.
#[test]
fn lint_multi_file_with_one_missing_exits_2() {
    let dir = tempdir().expect("create tempdir");
    let good = dir.path().join("good.rb");
    fs::write(&good, CLEAN_SOURCE).expect("write good.rb");

    // Bare filenames + current_dir = the tempdir, consistent with the other
    // cli tests. `good.rb` is readable & clean; `does_not_exist.rb` is absent
    // — the missing path must abort the run despite the good file linting.
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(dir.path())
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("good.rb")
        .arg("does_not_exist.rb")
        .assert()
        .code(2);

    // Contract guard: stdout is ONLY ever JSON; the aborted run emits nothing
    // on it (the good file's empty array is NOT flushed before the abort).
    assert!(
        assert.get_output().stdout.is_empty(),
        "aborted multi-file run must write nothing to stdout, got {:?}",
        assert.get_output().stdout
    );
}

/// A clean-only invocation → exit `0` AND stdout is EXACTLY `[]\n`.
///
/// Pins the precise wire shape, not merely "decodes to an empty array":
/// `serde_json::to_string(&[])` is `[]` and `writeln!` appends exactly one
/// `\n`. Asserts on the raw bytes so a reformat (pretty-print, extra space,
/// dropped newline) is caught even though it would still JSON-decode empty.
#[test]
fn lint_clean_only_stdout_is_exactly_empty_array() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("clean.rb");
    fs::write(&path, CLEAN_SOURCE).expect("write clean.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(0);

    assert_eq!(
        assert.get_output().stdout,
        b"[]\n",
        "clean-only stdout must be the exact bytes `[]\\n`, got {:?}",
        String::from_utf8_lossy(&assert.get_output().stdout)
    );
}

/// An offense-producing run → exit `1`, stdout is a non-empty JSON array,
/// AND stderr is EMPTY.
///
/// Machine-interface contract: diagnostics go to stderr ONLY and the JSON
/// channel is never polluted — and crucially, an *offense* is not a
/// diagnostic (it does not get an `eprintln!`), so a normal dirty run leaves
/// stderr completely empty.
#[test]
fn lint_offense_run_stderr_is_empty() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("dirty.rb");
    fs::write(&path, DIRTY_PUTS_X_SOURCE).expect("write dirty.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&path)
        .assert()
        .code(1);

    let output = assert.get_output();
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        1,
        "expected exactly one offense, got {parsed:?}"
    );
    assert_eq!(
        parsed[0]["cop_name"], "Murphy/NoReceiverPuts",
        "offense must be from the NoReceiverPuts cop"
    );
    assert!(
        output.stderr.is_empty(),
        "an offense is not a diagnostic — stderr must be empty, got {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Missing subcommand (`murphy` with no args at all) → exit 2 (bad CLI usage
/// is a setup error). Distinct from `murphy lint` with zero PATHS, which is
/// now a cwd discovery (Phase 2 Task 6): this case never has a subcommand, so
/// it stays bad-usage→2 and never reaches discovery.
#[test]
fn bad_usage_exits_2() {
    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .assert()
        .code(2);

    // Contract guard: stdout is ONLY ever JSON; error paths emit nothing on it.
    assert!(
        assert.get_output().stdout.is_empty(),
        "error path must write nothing to stdout, got {:?}",
        assert.get_output().stdout
    );
}

// --- Phase 2 Task 6: directory / zero-arg discovery ---

/// `murphy lint <dir>` discovers `.rb` files under the dir (default
/// `**/*.rb`), honoring a `murphy.toml` `exclude`. The clean+dirty tree →
/// exit 1 with exactly the dirty file's NoReceiverPuts offense; the excluded
/// file is NOT discovered.
#[test]
fn lint_directory_discovers_and_applies_murphy_toml_exclude() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(root.join("clean.rb"), CLEAN_SOURCE).expect("write clean.rb");
    fs::write(root.join("dirty.rb"), DIRTY_PUTS_SOURCE).expect("write dirty.rb");
    fs::create_dir_all(root.join("vendor")).expect("mkdir vendor");
    // A receiver-less puts that WOULD be flagged — proves exclude prunes it.
    fs::write(
        root.join("vendor").join("dep.rb"),
        "# frozen_string_literal: true\n\nputs 'v'\n",
    )
    .expect("write dep.rb");
    fs::write(
        root.join("murphy.toml"),
        "[files]\ninclude = [\"**/*.rb\"]\nexclude = [\"vendor/**\"]\n",
    )
    .expect("write murphy.toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(".")
        .assert()
        .code(1);

    let stdout = &assert.get_output().stdout;
    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        1,
        "only dirty.rb is discovered+dirty (clean.rb clean, vendor excluded), got {parsed:?}"
    );
    assert_eq!(parsed[0]["cop_name"], "Murphy/NoReceiverPuts");
    assert!(
        !parsed
            .iter()
            .any(|o| o["file"].as_str().is_some_and(|f| f.contains("vendor"))),
        "excluded vendor/ must not appear, got {parsed:?}"
    );
}

/// `murphy lint` with ZERO path args discovers from the cwd (Phase 2 Task 6
/// behavior change — this is NOT bad usage). A clean-only cwd → exit 0, empty
/// array. Distinct from `bad_usage_exits_2` (no subcommand → still exit 2).
#[test]
fn lint_zero_paths_discovers_cwd() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(root.join("a.rb"), CLEAN_SOURCE).expect("write a.rb");
    fs::write(root.join("b.rb"), CLEAN_SOURCE_2).expect("write b.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .assert()
        .code(0);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array");
    assert!(
        parsed.is_empty(),
        "clean cwd discovery yields zero offenses, got {parsed:?}"
    );
}

/// A malformed `murphy.toml` in a discovered dir → exit 2 (ConfigError), NOT
/// a panic (exit 3) and NOT silently ignored. stdout stays empty.
#[test]
fn lint_directory_with_malformed_murphy_toml_exits_2() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(root.join("a.rb"), CLEAN_SOURCE).expect("write a.rb");
    fs::write(root.join("murphy.toml"), "not = valid = toml [[[\n").expect("write murphy.toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(".")
        .assert()
        .code(2);

    assert!(
        assert.get_output().stdout.is_empty(),
        "config-error path must write nothing to stdout, got {:?}",
        assert.get_output().stdout
    );
}

#[test]
fn cops_config_can_disable_native_cop() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(root.join("dirty.rb"), DIRTY_PUTS_SOURCE).expect("write dirty.rb");
    fs::write(
        root.join("murphy.toml"),
        "[cops.rules.\"Murphy/NoReceiverPuts\"]\nenabled = false\n",
    )
    .expect("write murphy.toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .assert()
        .code(0);

    assert_eq!(assert.get_output().stdout, b"[]\n");
}

#[test]
fn cops_config_can_override_native_cop_severity() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(root.join("dirty.rb"), DIRTY_PUTS_SOURCE).expect("write dirty.rb");
    fs::write(
        root.join("murphy.toml"),
        "[cops.rules.\"Murphy/NoReceiverPuts\"]\nseverity = \"error\"\n",
    )
    .expect("write murphy.toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array");
    assert_eq!(parsed.len(), 1, "got {parsed:?}");
    assert_eq!(parsed[0]["severity"], "error");
}

#[test]
fn syntax_error_severity_cannot_be_downgraded_by_config() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(root.join("broken.rb"), "def (\n").expect("write broken.rb");
    fs::write(
        root.join("murphy.toml"),
        "[cops.rules.\"Murphy/Syntax\"]\nseverity = \"warning\"\n",
    )
    .expect("write murphy.toml");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array");
    assert_eq!(parsed.len(), 1, "got {parsed:?}");
    assert_eq!(parsed[0]["cop_name"], SYNTAX_COP_NAME);
    assert_eq!(parsed[0]["severity"], "error");
}

#[test]
fn directory_discovery_excludes_configured_cops_path() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::write(root.join("app.rb"), CLEAN_SOURCE).expect("write app.rb");
    fs::create_dir(root.join("cops")).expect("mkdir cops");
    fs::write(root.join("cops").join("broken.rb"), "puts \"x\"\ndef (\n")
        .expect("write broken cop");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        1,
        "only the loaded broken cop should report, got {parsed:?}"
    );
    assert_eq!(parsed[0]["file"], "./app.rb");
    assert_eq!(parsed[0]["cop_name"], "Murphy/Broken");
}

#[test]
fn explicit_cop_file_path_is_still_linted_as_a_target() {
    let dir = tempdir().expect("create tempdir");
    let root = dir.path();
    fs::create_dir(root.join("cops")).expect("mkdir cops");
    fs::write(
        root.join("cops").join("target.rb"),
        "class TargetCop < Murphy::Cop\n  def helper\n    puts \"x\"\n  end\nend\n",
    )
    .expect("write target");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .current_dir(root)
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg("cops/target.rb")
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array");
    assert!(
        parsed.iter().any(|o| o["file"] == "cops/target.rb"),
        "explicit file should not be discovery-excluded, got {parsed:?}"
    );
}

/// In-run content memoization (Phase 2 Task 7): two explicit files with
/// byte-identical content each get the offense in the output — once per
/// path, differing ONLY in `file` (offsets/cop/severity/message identical
/// because the content is). The dedup is a pure speed/correctness no-op on
/// output: linting one dup file twice would (modulo `file`) yield the same
/// JSON. Separate tempdir — NOT the `sample_project` snapshot dir (whose 4
/// fixtures have no dup content, so the snapshot stays a memo no-op).
#[test]
fn lint_two_identical_content_files_emits_offense_per_path() {
    let dir = tempdir().expect("create tempdir");
    let dup_a = dir.path().join("dup_a.rb");
    let dup_b = dir.path().join("dup_b.rb");
    fs::write(&dup_a, DIRTY_PUTS_X_SOURCE).expect("write dup_a.rb");
    fs::write(&dup_b, DIRTY_PUTS_X_SOURCE).expect("write dup_b.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
        .arg("--format")
        .arg("json")
        .arg(&dup_a)
        .arg(&dup_b)
        .assert()
        .code(1);

    let parsed: Vec<serde_json::Value> =
        serde_json::from_slice(&assert.get_output().stdout).expect("stdout must be a JSON array");
    assert_eq!(
        parsed.len(),
        2,
        "two dup-content files → one offense per path (2 total), got {parsed:?}"
    );

    let a = parsed
        .iter()
        .find(|o| o["file"].as_str() == Some(dup_a.to_str().unwrap()))
        .expect("dup_a offense present");
    let b = parsed
        .iter()
        .find(|o| o["file"].as_str() == Some(dup_b.to_str().unwrap()))
        .expect("dup_b offense present");

    // Differ ONLY in `file`: every other field is byte-identical because the
    // source bytes are identical (single shared parse, per-path `file` rewrite).
    assert_ne!(a["file"], b["file"]);
    assert_eq!(a["cop_name"], b["cop_name"]);
    assert_eq!(a["cop_name"], "Murphy/NoReceiverPuts");
    assert_eq!(a["range"], b["range"]);
    assert_eq!(a["severity"], b["severity"]);
    assert_eq!(a["message"], b["message"]);
}
