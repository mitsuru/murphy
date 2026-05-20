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

/// `lint` a path that does not exist → exit 2 (file/setup error).
#[test]
fn lint_missing_file_exits_2() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("does_not_exist.rb");

    let assert = Command::cargo_bin("murphy")
        .expect("murphy binary builds")
        .arg("lint")
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
