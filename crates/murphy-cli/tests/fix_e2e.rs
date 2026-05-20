//! End-to-end tests for `murphy lint --fix / -a` (Task 6 — CLI write-back,
//! exit codes, --debug autocorrect observability).
//!
//! ## What is pinned here
//!
//! 1. **APIN1 (stdout invariant)**: `murphy lint --fix X` stdout == `murphy lint <post-fix-X>` stdout.
//! 2. **APIN2 (data safety / write-back)**: sibling-temp+rename write-back; no
//!    file written when corrected == original; no temp file leftovers.
//! 3. **APIN3 (--fix summary)**: always exactly one stderr line
//!    `murphy: fixed N of M files`; stdout never contains that text.
//! 4. **exit codes under --fix**: 0 when no offenses remain, 1 when some remain.
//! 5. **-a alias**: identical behavior to --fix.
//! 6. **unknown flag**: exit 2.
//! 7. **--debug**: autocorrect-only observability to STDERR; stdout untouched.
//! 8. **mruby e2e money test**: a mruby cop emitting a real fix via
//!    `Murphy::Fix#replace` → --fix actually rewrites the file end-to-end.
//!
//! `sample_project` is NEVER touched; all test state lives in `tempdir()`s.

use assert_cmd::Command;
use std::fs;
use tempfile::{TempDir, tempdir};

// ---------------------------------------------------------------------------
// Shared helpers (mirror mruby_e2e.rs style)
// ---------------------------------------------------------------------------

/// A mruby cop that replaces a bare `puts "..."` call's message with `"logger.info"`.
/// Uses `Murphy::Fix#replace` → real edit → autocorrect attached to offense.
/// After --fix, the file has `logger.info` in place of `puts`.
const PUTS_TO_LOGGER_COP: &str = r#"
class PutsToLoggerCop < Murphy::Cop
  MSG = "Use logger.info instead of bare puts"

  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    msg_loc = node.message_loc
    return unless msg_loc
    add_offense(msg_loc, message: MSG) do |fix|
      fix.replace(msg_loc, "logger.info")
    end
  end
end
"#;

/// Build a tempdir project with `app.rb` (content = `src`) and
/// `cops/<stem>.rb` for each cop pair.  Returns the TempDir guard.
fn project_with_cops(src: &str, cops: &[(&str, &str)]) -> TempDir {
    let dir = tempdir().expect("create tempdir");
    fs::write(dir.path().join("app.rb"), src).expect("write app.rb");
    if !cops.is_empty() {
        let cops_dir = dir.path().join("cops");
        fs::create_dir(&cops_dir).expect("mkdir cops");
        for (stem, cop_src) in cops {
            fs::write(cops_dir.join(format!("{stem}.rb")), cop_src).expect("write cop");
        }
    }
    dir
}

/// Run `murphy lint [extra_args…] <files…>` from `proj.path()`.
/// Returns the full `std::process::Output`.
fn run_murphy(proj: &TempDir, extra: &[&str], files: &[&str]) -> std::process::Output {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.current_dir(proj.path()).arg("lint");
    for e in extra {
        cmd.arg(e);
    }
    for f in files {
        cmd.arg(f);
    }
    cmd.assert().get_output().clone()
}

/// Parse stdout bytes as a JSON offense array.
fn parse_offenses(out: &std::process::Output) -> Vec<serde_json::Value> {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout must be a JSON array, got: {:?} — error: {e}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

/// Return the exit code as `i32`.
fn exit_code(out: &std::process::Output) -> i32 {
    out.status.code().expect("process must have exit code")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Unknown `--flag` (not `--fix`, `-a`, or `--debug`) → exit 2, stdout empty.
#[test]
fn unknown_flag_exits_2() {
    let dir = tempdir().expect("create tempdir");
    let path = dir.path().join("a.rb");
    fs::write(&path, "x = 1\n").expect("write a.rb");

    let out = run_murphy(
        &TempDir::new().expect("create proj dir"),
        &["--unknown-option"],
        &[path.to_str().unwrap()],
    );
    assert_eq!(exit_code(&out), 2, "unknown flag → exit 2");
    assert!(
        out.stdout.is_empty(),
        "error path must write nothing to stdout, got {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// `murphy lint --fix <clean file>` → exit 0, stdout `[]`, file unchanged.
#[test]
fn fix_clean_file_exits_0_no_write() {
    let proj = project_with_cops("# frozen_string_literal: true\n\nx = 1\n", &[]);
    let file = "app.rb";
    let original = fs::read(proj.path().join(file)).expect("read app.rb");

    let out = run_murphy(&proj, &["--fix"], &[file]);
    assert_eq!(exit_code(&out), 0, "--fix clean file → exit 0");

    let after = fs::read(proj.path().join(file)).expect("read app.rb after");
    assert_eq!(original, after, "clean file must not be modified");

    let offenses = parse_offenses(&out);
    assert!(
        offenses.is_empty(),
        "clean file → empty offense array, got {offenses:?}"
    );

    // APIN2: no sibling temp files left
    let temps: Vec<_> = fs::read_dir(proj.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(".murphy-fix-"))
        .collect();
    assert!(
        temps.is_empty(),
        "no sibling temp files left after clean-file fix, got: {temps:?}"
    );
}

/// `murphy lint --fix <dirty native-only file>` → native NoReceiverPuts has
/// NO autocorrect → file unchanged, offense still reported, exit 1.
#[test]
fn fix_file_with_no_autocorrect_cop_exits_1_file_unchanged() {
    let proj = project_with_cops("# frozen_string_literal: true\n\nputs 'hi'\n", &[]);
    let file = "app.rb";
    let original = fs::read(proj.path().join(file)).expect("read app.rb");

    let out = run_murphy(&proj, &["--fix"], &[file]);
    assert_eq!(
        exit_code(&out),
        1,
        "--fix with no-autocorrect offense → exit 1"
    );

    let after = fs::read(proj.path().join(file)).expect("read app.rb after");
    assert_eq!(
        original, after,
        "file without autocorrect must not be modified"
    );

    let offenses = parse_offenses(&out);
    assert_eq!(
        offenses.len(),
        1,
        "offense without autocorrect still reported, got {offenses:?}"
    );
    assert_eq!(offenses[0]["cop_name"], "Murphy/NoReceiverPuts");
}

/// `-a` is an alias for `--fix`.
#[test]
fn short_flag_a_is_alias_for_fix() {
    let proj = project_with_cops("# frozen_string_literal: true\n\nx = 1\n", &[]);
    let file = "app.rb";

    let fix_out = run_murphy(&proj, &["--fix"], &[file]);
    let short_out = run_murphy(&proj, &["-a"], &[file]);

    assert_eq!(
        exit_code(&fix_out),
        exit_code(&short_out),
        "-a and --fix must have identical exit codes"
    );
    assert_eq!(
        fix_out.stdout, short_out.stdout,
        "-a and --fix must have identical stdout"
    );
}

/// APIN3: `--fix` always emits exactly one stderr line `murphy: fixed N of M files`.
/// Stdout must never contain that text.
#[test]
fn fix_always_emits_fixed_summary_on_stderr_not_stdout() {
    // Clean file: fixed 0 of 1.
    let proj = project_with_cops("# frozen_string_literal: true\n\nx = 1\n", &[]);
    let file = "app.rb";

    let out = run_murphy(&proj, &["--fix"], &[file]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    // Exactly one "murphy: fixed … of … files" line on stderr.
    let summary_lines: Vec<&str> = stderr
        .lines()
        .filter(|l| l.starts_with("murphy: fixed "))
        .collect();
    assert_eq!(
        summary_lines.len(),
        1,
        "exactly one summary line on stderr, got stderr: {stderr:?}"
    );
    assert!(
        summary_lines[0].ends_with(" files"),
        "summary line shape: 'murphy: fixed N of M files', got: {:?}",
        summary_lines[0]
    );
    // The numbers for a clean single file: "fixed 0 of 1 files"
    assert!(
        summary_lines[0].contains("0 of 1"),
        "clean file → fixed 0 of 1, got: {:?}",
        summary_lines[0]
    );

    // stdout must NOT contain the summary text.
    assert!(
        !stdout.contains("murphy: fixed"),
        "stdout must not contain fix summary, got: {stdout:?}"
    );
}

/// APIN1: `murphy lint --fix X` stdout == `murphy lint <post-fix-X>` stdout.
/// Locks "uniform: stdout = offenses on source as it now exists".
#[test]
fn fix_stdout_equals_lint_of_post_fix_source() {
    let proj = project_with_cops("puts \"hi\"\n", &[("puts_to_logger", PUTS_TO_LOGGER_COP)]);
    let file = "app.rb";

    // Run --fix (mutates app.rb in place).
    let fix_out = run_murphy(&proj, &["--fix"], &[file]);

    // Now lint the (already-fixed) app.rb without --fix.
    let lint_out = run_murphy(&proj, &[], &[file]);

    assert_eq!(
        fix_out.stdout,
        lint_out.stdout,
        "APIN1: --fix stdout must equal lint stdout of post-fix source\n\
         --fix stdout: {:?}\n\
         lint stdout:  {:?}",
        String::from_utf8_lossy(&fix_out.stdout),
        String::from_utf8_lossy(&lint_out.stdout)
    );
}

/// APIN2(c) data-safety: cp fixture to a.rb + b.rb, --fix both → identical
/// corrected content; no temp files remain.
#[test]
fn fix_two_identical_copies_yields_same_corrected_bytes_no_temps() {
    let proj = project_with_cops("puts \"hi\"\n", &[("puts_to_logger", PUTS_TO_LOGGER_COP)]);
    // Write a second copy.
    fs::write(proj.path().join("b.rb"), "puts \"hi\"\n").expect("write b.rb");

    let out = run_murphy(&proj, &["--fix"], &["app.rb", "b.rb"]);
    // Both files have the fixable puts → corrected; but NoReceiverPuts
    // (native, no autocorrect) remains → exit 1.
    assert!(
        exit_code(&out) <= 1,
        "should exit 0 or 1 after fix, got {}",
        exit_code(&out)
    );

    let a = fs::read(proj.path().join("app.rb")).expect("read app.rb");
    let b = fs::read(proj.path().join("b.rb")).expect("read b.rb");
    assert_eq!(
        a, b,
        "two identical copies must produce identical corrected bytes"
    );

    // No temp files left.
    let temps: Vec<_> = fs::read_dir(proj.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(".murphy-fix-"))
        .collect();
    assert!(
        temps.is_empty(),
        "no sibling temp files left after fix, found: {temps:?}"
    );
}

/// Mruby e2e money test: a mruby cop with `Murphy::Fix#replace` → --fix
/// actually rewrites the file with the corrected source end-to-end.
#[test]
fn fix_mruby_cop_with_autocorrect_rewrites_file() {
    let src = "puts \"hello\"\n";
    let proj = project_with_cops(src, &[("puts_to_logger", PUTS_TO_LOGGER_COP)]);
    let file = "app.rb";

    let out = run_murphy(&proj, &["--fix"], &[file]);

    // File on disk should now contain "logger.info" instead of "puts".
    let after = fs::read_to_string(proj.path().join(file)).expect("read app.rb after fix");
    assert!(
        after.contains("logger.info"),
        "file must be rewritten with corrected source, got: {after:?}"
    );
    assert!(
        !after.contains("puts"),
        "bare puts must have been replaced, got: {after:?}"
    );

    // Remaining stdout: NoReceiverPuts has no autocorrect → still reported.
    // (Native NoReceiverPuts offense still fires on `logger.info` if it's a bare call?
    //  Actually `logger.info` has a receiver, so NoReceiverPuts won't fire.
    //  PutsToLoggerCop offense is gone because puts is replaced.
    //  So remaining offenses = 0 → exit 0.)
    let offenses = parse_offenses(&out);
    // After fix, the source is `logger.info "hello"\n` — NoReceiverPuts
    // won't fire on a receiver call, PutsToLogger won't fire either.
    // Exit must be 0.
    assert_eq!(exit_code(&out), 0, "after fix no offenses remain → exit 0");
    assert!(
        offenses.is_empty(),
        "after fix stdout should be empty offense array, got {offenses:?}"
    );
}

/// `--debug` output goes ONLY to stderr; stdout remains pure JSON.
#[test]
fn debug_output_goes_to_stderr_only_stdout_unchanged() {
    let src = "puts \"hi\"\n";
    let proj = project_with_cops(src, &[("puts_to_logger", PUTS_TO_LOGGER_COP)]);
    let file = "app.rb";

    // Run with --fix --debug
    let out_with_debug = run_murphy(&proj, &["--fix", "--debug"], &[file]);

    // stdout must be valid JSON
    let _offenses: Vec<serde_json::Value> = serde_json::from_slice(&out_with_debug.stdout)
        .expect("stdout must be JSON even with --debug");

    // stderr must have debug content (iterations etc.) for a fixed file
    let stderr = String::from_utf8_lossy(&out_with_debug.stderr);
    // At minimum, the APIN3 summary line must be there.
    assert!(
        stderr.contains("murphy: fixed"),
        "--debug mode must still emit the fix summary on stderr, got: {stderr:?}"
    );
    // stdout must NOT contain any debug text.
    let stdout = String::from_utf8_lossy(&out_with_debug.stdout);
    assert!(
        !stdout.contains("Converged")
            && !stdout.contains("MaxIterations")
            && !stdout.contains("Oscillation")
            && !stdout.contains("iterations"),
        "stdout must not contain --debug text, got: {stdout:?}"
    );
}

/// `--debug` without `--fix`: allowed, no autocorrect runs, nothing
/// autocorrect-specific on stderr, stdout unchanged.
#[test]
fn debug_without_fix_emits_nothing_autocorrect_specific() {
    let proj = project_with_cops("puts \"hi\"\n", &[]);
    let file = "app.rb";

    let out_debug = run_murphy(&proj, &["--debug"], &[file]);
    let out_plain = run_murphy(&proj, &[], &[file]);

    // stdout identical (same offenses, same JSON)
    assert_eq!(
        out_debug.stdout, out_plain.stdout,
        "--debug without --fix must not change stdout"
    );

    // stderr should not contain autocorrect-specific debug text
    let stderr = String::from_utf8_lossy(&out_debug.stderr);
    assert!(
        !stderr.contains("Converged")
            && !stderr.contains("MaxIterations")
            && !stderr.contains("Oscillation"),
        "--debug without --fix must emit nothing autocorrect-specific on stderr, got: {stderr:?}"
    );
}

/// APIN3 with zero path args (discover cwd): --fix still discovers + fixes
/// all discovered files, summary on stderr.
#[test]
fn fix_zero_path_args_discovers_and_fixes() {
    let proj = project_with_cops("puts \"hi\"\n", &[("puts_to_logger", PUTS_TO_LOGGER_COP)]);

    // Run with --fix and NO path args (zero-arg discover)
    let out = run_murphy(&proj, &["--fix"], &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // Summary line must appear.
    let summary_count = stderr
        .lines()
        .filter(|l| l.starts_with("murphy: fixed "))
        .count();
    assert_eq!(
        summary_count, 1,
        "zero-path --fix must still emit exactly one summary line, got stderr: {stderr:?}"
    );

    // The discovered file must have been fixed.
    let after = fs::read_to_string(proj.path().join("app.rb")).expect("read app.rb after");
    assert!(
        after.contains("logger.info"),
        "zero-path --fix must rewrite discovered files, got: {after:?}"
    );
}

/// Idempotency: running --fix twice on a copy produces identical content
/// and identical stdout.
#[test]
fn fix_is_idempotent_on_second_run() {
    let proj = project_with_cops(
        "puts \"hello\"\n",
        &[("puts_to_logger", PUTS_TO_LOGGER_COP)],
    );
    let file = "app.rb";

    // First fix pass.
    run_murphy(&proj, &["--fix"], &[file]);
    let after_first = fs::read(proj.path().join(file)).expect("read after first fix");
    let stdout_first = run_murphy(&proj, &[], &[file]).stdout;

    // Second fix pass (should be a no-op if cop is idempotent).
    run_murphy(&proj, &["--fix"], &[file]);
    let after_second = fs::read(proj.path().join(file)).expect("read after second fix");
    let stdout_second = run_murphy(&proj, &[], &[file]).stdout;

    assert_eq!(
        after_first, after_second,
        "second --fix pass must not change the file content"
    );
    assert_eq!(
        stdout_first, stdout_second,
        "second --fix pass must not change the lint output"
    );
}

/// APIN3: Fixed N count is accurate — a dirty (fixable) file increments N.
#[test]
fn fix_summary_counts_changed_files_accurately() {
    let proj = project_with_cops("puts \"hi\"\n", &[("puts_to_logger", PUTS_TO_LOGGER_COP)]);
    // Add a clean file to verify it doesn't count.
    fs::write(
        proj.path().join("clean.rb"),
        "# frozen_string_literal: true\n\nx = 1\n",
    )
    .expect("write clean.rb");

    let out = run_murphy(&proj, &["--fix"], &["app.rb", "clean.rb"]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    let summary = stderr
        .lines()
        .find(|l| l.starts_with("murphy: fixed "))
        .expect("summary line must exist");
    // app.rb gets fixed (puts→logger.info), clean.rb doesn't change.
    assert!(
        summary.contains("1 of 2"),
        "expected 'fixed 1 of 2 files', got: {summary:?}"
    );
}

// ---------------------------------------------------------------------------
// roborev regression: write-back must preserve file mode and follow symlinks
// (unix-only — exercises permission bits / symlink semantics).
// ---------------------------------------------------------------------------

/// An executable Ruby script must stay executable after `--fix`. The
/// sibling-temp+rename path captures the real file's permissions and applies
/// them to the temp before the rename (roborev medium: a fresh temp would
/// otherwise inherit umask and drop the +x bit).
#[test]
#[cfg(unix)]
fn fix_preserves_executable_mode() {
    use std::os::unix::fs::PermissionsExt;

    let proj = project_with_cops("puts \"hi\"\n", &[("puts_to_logger", PUTS_TO_LOGGER_COP)]);
    let path = proj.path().join("app.rb");
    // Make it -rwxr-xr-x.
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod 755");

    let out = run_murphy(&proj, &["--fix"], &["app.rb"]);
    assert_eq!(exit_code(&out), 0, "fix should succeed");

    let after = fs::read_to_string(&path).expect("read app.rb after");
    assert!(after.contains("logger.info"), "file must be rewritten");

    let mode = fs::metadata(&path)
        .expect("stat app.rb")
        .permissions()
        .mode();
    assert_eq!(
        mode & 0o777,
        0o755,
        "executable mode must be preserved across --fix write-back, got {:o}",
        mode & 0o777
    );
}

/// `--fix` on a symlink must update the link's *destination* and keep the
/// symlink intact (roborev medium: renaming onto the link path would replace
/// the link with a regular file and leave the real target stale).
#[test]
#[cfg(unix)]
fn fix_follows_symlink_and_keeps_link_intact() {
    use std::os::unix::fs::symlink;

    let proj = project_with_cops("x = 1\n", &[("puts_to_logger", PUTS_TO_LOGGER_COP)]);
    // Real file lives elsewhere; `app_link.rb` is a symlink to it.
    let real = proj.path().join("real_src.rb");
    fs::write(&real, "puts \"hi\"\n").expect("write real_src.rb");
    let link = proj.path().join("app_link.rb");
    symlink(&real, &link).expect("create symlink");

    let out = run_murphy(&proj, &["--fix"], &["app_link.rb"]);
    assert_eq!(exit_code(&out), 0, "fix via symlink should succeed");

    // The link must STILL be a symlink (not replaced by a regular file).
    let link_meta = fs::symlink_metadata(&link).expect("lstat link");
    assert!(
        link_meta.file_type().is_symlink(),
        "app_link.rb must remain a symlink after --fix"
    );
    // The real destination's content must have been rewritten.
    let real_after = fs::read_to_string(&real).expect("read real_src.rb after");
    assert!(
        real_after.contains("logger.info") && !real_after.contains("puts"),
        "symlink destination must be rewritten, got: {real_after:?}"
    );
}

// ---------------------------------------------------------------------------
// roborev iter2 regressions: `--` separator, accurate summary denominator on
// write error, and Converged-with-conflicts --debug clarity.
// ---------------------------------------------------------------------------

/// roborev low: a path starting with `-` regressed to "unknown flag". The
/// `--` end-of-flags separator must let such a file be linted again.
#[test]
fn double_dash_allows_dash_prefixed_path() {
    let dir = tempdir().expect("tempdir");
    // A file literally named "-weird.rb".
    fs::write(
        dir.path().join("-weird.rb"),
        "# frozen_string_literal: true\n\nx = 1\n",
    )
    .expect("write -weird.rb");
    let proj = TempDir::new().expect("proj");
    // Without `--`: unknown-flag → exit 2.
    let mut bad = Command::cargo_bin("murphy").expect("bin");
    bad.current_dir(dir.path()).arg("lint").arg("-weird.rb");
    let bad_out = bad.assert().get_output().clone();
    assert_eq!(
        exit_code(&bad_out),
        2,
        "`-weird.rb` w/o `--` is unknown flag"
    );
    // With `--`: linted as a path → clean file → exit 0, empty offenses.
    let mut ok = Command::cargo_bin("murphy").expect("bin");
    ok.current_dir(dir.path())
        .arg("lint")
        .arg("--")
        .arg("-weird.rb");
    let ok_out = ok.assert().get_output().clone();
    assert_eq!(exit_code(&ok_out), 0, "`-- -weird.rb` lints the file");
    assert!(parse_offenses(&ok_out).is_empty());
    let _ = &proj;
}

/// roborev medium: when a write-back error aborts the pass, the summary
/// denominator must be the files ACTUALLY processed, not the full target
/// count (no "fixed 0 of 2" when only 1 file was ever attempted).
#[test]
#[cfg(unix)]
fn write_error_summary_denominator_is_processed_not_total() {
    use std::os::unix::fs::PermissionsExt;

    let proj = project_with_cops("puts \"hi\"\n", &[("puts_to_logger", PUTS_TO_LOGGER_COP)]);
    // Second target in its own subdir we will make read-only so the
    // sibling-temp write fails for it.
    let sub = proj.path().join("ro");
    fs::create_dir(&sub).expect("mkdir ro");
    fs::write(sub.join("b.rb"), "puts \"bye\"\n").expect("write b.rb");
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o555)).expect("chmod ro 555");

    let out = run_murphy(&proj, &["--fix"], &["app.rb", "ro/b.rb"]);
    // Restore perms so TempDir cleanup succeeds.
    let _ = fs::set_permissions(&sub, fs::Permissions::from_mode(0o755));

    assert_eq!(
        exit_code(&out),
        2,
        "write-back failure → setup error exit 2"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let summary = stderr
        .lines()
        .find(|l| l.starts_with("murphy: fixed "))
        .expect("summary line present");
    // app.rb processed+fixed, ro/b.rb processed then write failed → aborted.
    // processed == 2 (both attempted), fixed == 1 (only app.rb written).
    assert!(
        summary.contains("1 of 2"),
        "denominator must be files processed, got: {summary:?}"
    );
}

/// roborev medium: a cop whose only edit is out-of-bounds yields a stable
/// source (Converged) but with an unresolved conflict. `--debug` must NOT
/// present that identically to a clean converge — it emits an explicit
/// "converged with N unresolved conflict(s)" warning.
#[test]
fn debug_flags_converged_with_conflicts() {
    const OOB_FIX_COP: &str = r#"
class OobFixCop < Murphy::Cop
  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    loc = node.message_loc
    return unless loc
    add_offense(loc, message: "oob fix") do |fix|
      fix.replace(Murphy::Range.new(0, 99999), "x")
    end
  end
end
"#;
    let proj = project_with_cops(
        "# frozen_string_literal: true\n\nputs 'hi'\n",
        &[("oob", OOB_FIX_COP)],
    );
    let out = run_murphy(&proj, &["--fix", "--debug"], &["app.rb"]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    // Source unchanged (only edit was OOB → dropped), offense remains → exit 1.
    let after = fs::read_to_string(proj.path().join("app.rb")).expect("read app.rb");
    assert_eq!(
        after, "# frozen_string_literal: true\n\nputs 'hi'\n",
        "OOB edit must not change the file"
    );
    assert_eq!(exit_code(&out), 1, "offense remains → exit 1");
    // The debug status is Converged (source IS a stable fixed point)…
    assert!(
        stderr.contains("status=Converged"),
        "stable source → Converged, got stderr: {stderr}"
    );
    // …but it must be explicitly flagged as converged-with-conflicts, not a
    // silent clean converge.
    assert!(
        stderr.contains("converged with") && stderr.contains("unresolved conflict"),
        "Converged-with-conflicts must be spelled out, got stderr: {stderr}"
    );
}
