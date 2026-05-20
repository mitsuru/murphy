//! End-to-end test: native + mruby cops co-occur under the real CLI pipeline
//! (Phase 3 Task 7 — the integration keystone).
//!
//! This is the FIRST place a native cop (`Murphy/NoReceiverPuts`, Rust,
//! all-core rayon) and a discovered `cops/*.rb` user cop run together over the
//! same parsed source in the compiled `murphy` binary. It exercises exactly
//! the surface a user invokes (`assert_cmd`, `current_dir` = a tempdir project
//! with its own `cops/`), NEVER `sample_project` (whose byte-identical
//! snapshot is the separate proof, in `integration_snapshot.rs` /
//! `parallel_determinism.rs`, that this wiring did not perturb the native-only
//! frozen contract — ADR 0006/0007).
//!
//! ## What is pinned here
//!
//! 1. **Native + mruby co-occur.** A `.rb` source with a receiver-less `puts`
//!    AND a receiver-less `print` + a `cops/no_puts.rb` design-§4-style user
//!    cop → BOTH the native `Murphy/NoReceiverPuts` offense(s) AND the user
//!    cop's offense appear in one aggregated JSON array, ADR 0006 frozen
//!    5-field shape (NO `autocorrect` — soft-(a)), exit `1`.
//! 2. **Determinism (ADR 0007).** Cross-engine offenses are ordered solely by
//!    `aggregate`'s total order. Repeated runs AND shuffled multi-file arg
//!    orders all yield BYTE-IDENTICAL stdout (mirrors `parallel_determinism`'s
//!    discipline for the native+mruby fixture).
//! 3. **Broken-cop isolation + continue (design §6 / ADR 0003).** A second
//!    project additionally has a raising `cops/bad.rb` and a looping
//!    `cops/loop.rb`. Each contributes EXACTLY ONE `Severity::Error` error
//!    offense for that cop; the native + the good user cop's offenses are
//!    still present; the run COMPLETES (not hung) with exit `1`.
//!
//! `cop_name` scheme (pinned by these assertions): a discovered
//! `cops/<stem>.rb` is attributed as `Murphy/<PascalCase(stem)>` —
//! `no_puts.rb` → `Murphy/NoPuts`, `bad.rb` → `Murphy/Bad`,
//! `loop.rb` → `Murphy/Loop`. snake_case `_` segments are dropped and each
//! segment capitalized.

use assert_cmd::Command;
use std::fs;
use std::time::Instant;
use tempfile::{TempDir, tempdir};

/// The design §4 cop, class-based (`Murphy::Cop` SDK, Task 4): flag a
/// receiver-less `puts`. Identical in spirit to `cop_no_puts_mruby.rs`.
const NO_PUTS_COP: &str = r#"
class NoPutsCop < Murphy::Cop
  MSG = "Use a logger instead of puts"

  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    add_offense(node.message_loc, message: MSG)
  end
end
"#;

/// A trivial, fully-VALID `Murphy::Cop` subclass — it would load and run
/// cleanly. Used by the reserved-name collision tests: the point is that the
/// collision guard rejects it on its DERIVED NAME alone, BEFORE it ever runs,
/// so its body being valid (not a syntax/raise error) proves the rejection is
/// the name guard and nothing else.
const TRIVIAL_VALID_COP: &str = r#"
class TrivialCop < Murphy::Cop
  MSG = "trivial"

  def on_call_node(node)
    return unless node.name == :puts && node.receiver_nil?
    add_offense(node.message_loc, message: MSG)
  end
end
"#;

/// A cop that raises at load (design §6: caught → exactly one error offense
/// for this cop×file, the run continues).
const BAD_COP: &str = r#"
raise "boom from bad.rb"
"#;

/// A cop that loops forever at load (ADR 0003 Mechanism A: abandoned on the
/// hardcoded deadline → exactly one error offense, the run is NOT hung).
const LOOP_COP: &str = "while true; end\n";

/// A `.rb` source with BOTH a receiver-less `puts` (line 1) and a
/// receiver-less `print` (line 2). `Murphy/NoReceiverPuts` (native) flags
/// each receiver-less `puts`/`print`; the `no_puts.rb` user cop flags only
/// the `puts`. So a correctly-wired pipeline yields, for this one file:
///   - native `Murphy/NoReceiverPuts` on `puts`  (line 1)
///   - native `Murphy/NoReceiverPuts` on `print` (line 2)
///   - user   `Murphy/NoPuts`         on `puts`  (line 1)
const DIRTY_SRC: &str = "puts \"hello\"\nprint \"world\"\n";

/// Build a tempdir project: `app.rb` (the dirty source) plus a `cops/` with
/// the given `(stem, source)` cops. Returns the dir guard (kept alive by the
/// caller so the tempdir is not reaped mid-test).
fn project(cops: &[(&str, &str)]) -> TempDir {
    let dir = tempdir().expect("create tempdir");
    fs::write(
        dir.path().join("murphy.toml"),
        r#"[cops.rules."Style/FrozenStringLiteralComment"]
enabled = false

[cops.rules."Style/StringLiterals"]
enabled = false

[cops.rules."Style/SymbolArray"]
enabled = false

[cops.rules."Style/WordArray"]
enabled = false
"#,
    )
    .expect("write murphy.toml");
    fs::write(dir.path().join("app.rb"), DIRTY_SRC).expect("write app.rb");
    let cops_dir = dir.path().join("cops");
    fs::create_dir(&cops_dir).expect("mkdir cops");
    for (stem, src) in cops {
        fs::write(cops_dir.join(format!("{stem}.rb")), src).expect("write cop");
    }
    dir
}

/// Run `murphy lint <args>` with `current_dir` = the project root (so
/// `cops/` is discovered cwd-relative, ADR 0004 mitigation 2, and
/// `Offense.file` is the bare arg). Returns (stdout bytes, exit code).
fn run_lint(proj: &TempDir, args: &[&str]) -> (Vec<u8>, i32) {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.current_dir(proj.path()).arg("lint");
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.assert().get_output().clone();
    (out.stdout.clone(), out.status.code().expect("exit code"))
}

/// Parse stdout as the JSON offense array and assert every element is the
/// exact ADR 0006 frozen 5-field shape (NO `autocorrect`).
fn parse_offenses(stdout: &[u8]) -> Vec<serde_json::Value> {
    let arr: Vec<serde_json::Value> =
        serde_json::from_slice(stdout).expect("stdout must be a JSON array");
    for o in &arr {
        let mut keys: Vec<&str> = o
            .as_object()
            .expect("offense is a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["cop_name", "file", "message", "range", "severity"],
            "ADR 0006 frozen 5-field Offense shape, no autocorrect: {o}"
        );
    }
    arr
}

fn cop_names(offenses: &[serde_json::Value]) -> Vec<String> {
    offenses
        .iter()
        .map(|o| o["cop_name"].as_str().unwrap().to_owned())
        .collect()
}

/// Native + mruby cops co-occur in one aggregated array, ADR 0006 shape,
/// exit 1, and the output is byte-identical across repeated + shuffled runs.
#[test]
fn native_and_mruby_cops_co_occur_and_are_deterministic() {
    let proj = project(&[("no_puts", NO_PUTS_COP)]);

    let (stdout, code) = run_lint(&proj, &["app.rb"]);
    assert_eq!(code, 1, "dirty source → offenses → exit 1");

    let offenses = parse_offenses(&stdout);
    let names = cop_names(&offenses);

    // Native cop fired on BOTH the bare `puts` and the bare `print`.
    let native = names
        .iter()
        .filter(|n| *n == "Murphy/NoReceiverPuts")
        .count();
    assert_eq!(
        native, 2,
        "native Murphy/NoReceiverPuts on both `puts` and `print`, got {offenses:?}"
    );
    // The user cop (cops/no_puts.rb) fired on the bare `puts`.
    let user = names.iter().filter(|n| *n == "Murphy/NoPuts").count();
    assert_eq!(
        user, 1,
        "user cop Murphy/NoPuts on the bare `puts`, got {offenses:?}"
    );
    assert_eq!(
        offenses.len(),
        3,
        "exactly native×2 + user×1, got {offenses:?}"
    );
    // Every offense is attributed to the right file.
    for o in &offenses {
        assert_eq!(o["file"].as_str().unwrap(), "app.rb");
    }

    // Determinism (ADR 0007): repeated runs are byte-identical.
    for _ in 0..4 {
        let (again, c) = run_lint(&proj, &["app.rb"]);
        assert_eq!(c, 1);
        assert_eq!(
            again, stdout,
            "repeated runs must be byte-identical (aggregate total order)"
        );
    }

    // Determinism across SHUFFLED multi-file arg orders: two byte-identical
    // copies of the dirty source; arg order must not perturb output.
    fs::write(proj.path().join("b.rb"), DIRTY_SRC).expect("write b.rb");
    let (ab, _) = run_lint(&proj, &["app.rb", "b.rb"]);
    let (ba, _) = run_lint(&proj, &["b.rb", "app.rb"]);
    assert_eq!(
        ab, ba,
        "multi-file arg order must not change output — aggregate sorts by (file, …)"
    );
    // Each of the 2 files contributes the same 3 cross-engine offenses.
    assert_eq!(parse_offenses(&ab).len(), 6);
}

/// A raising cop AND a looping cop are each isolated to exactly ONE error
/// offense; the native + good user cop offenses still appear; the run
/// COMPLETES (not hung) with exit 1 (design §6 / ADR 0003).
#[test]
fn broken_cop_isolated_one_error_offense_and_run_continues() {
    let proj = project(&[
        ("no_puts", NO_PUTS_COP),
        ("bad", BAD_COP),
        ("loop", LOOP_COP),
    ]);

    let start = Instant::now();
    let (stdout, code) = run_lint(&proj, &["app.rb"]);
    let elapsed = start.elapsed();

    assert_eq!(code, 1, "offenses present (incl. error offenses) → exit 1");

    let offenses = parse_offenses(&stdout);
    let names = cop_names(&offenses);

    // The good native cop is unaffected by the broken user cops.
    assert_eq!(
        names
            .iter()
            .filter(|n| *n == "Murphy/NoReceiverPuts")
            .count(),
        2,
        "native cop still fires despite broken user cops, got {offenses:?}"
    );
    // The good user cop is unaffected.
    assert_eq!(
        names.iter().filter(|n| *n == "Murphy/NoPuts").count(),
        1,
        "good user cop still fires, got {offenses:?}"
    );

    // The raising cop → EXACTLY ONE error offense for Murphy/Bad.
    let bad: Vec<&serde_json::Value> = offenses
        .iter()
        .filter(|o| o["cop_name"] == "Murphy/Bad")
        .collect();
    assert_eq!(
        bad.len(),
        1,
        "raising cop → exactly one error offense, got {offenses:?}"
    );
    assert_eq!(bad[0]["severity"].as_str().unwrap(), "error");

    // The looping cop → EXACTLY ONE error offense for Murphy/Loop.
    let lp: Vec<&serde_json::Value> = offenses
        .iter()
        .filter(|o| o["cop_name"] == "Murphy/Loop")
        .collect();
    assert_eq!(
        lp.len(),
        1,
        "looping cop → exactly one error offense, got {offenses:?}"
    );
    assert_eq!(lp[0]["severity"].as_str().unwrap(), "error");

    // native×2 + good user×1 + bad×1 + loop×1 = 5.
    assert_eq!(offenses.len(), 5, "got {offenses:?}");

    // The run COMPLETED — not hung. The looping cop is abandoned on the
    // hardcoded COP_DEADLINE; this asserts the watchdog bounded it (generous
    // ceiling so a slow CI box does not flake; the point is "terminated", not
    // a tight perf bound).
    assert!(
        elapsed.as_secs() < 60,
        "run must complete (looping cop abandoned, not hung): took {elapsed:?}"
    );
}

/// A run that produced NO stdout JSON: stderr names the collision, exit 2,
/// stdout is empty (a setup-class error prints nothing to stdout — design
/// §"stdout / stderr split"). Returns the captured stderr for assertion.
fn run_lint_capture(proj: &TempDir, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.current_dir(proj.path()).arg("lint");
    for a in args {
        cmd.arg(a);
    }
    cmd.assert().get_output().clone()
}

/// Reserved-name collision guard (P3 Task 7 review I-1, ADR-0006 cop_name).
///
/// A user cop file whose derived `Murphy/<PascalCase(stem)>` equals an
/// engine-owned name must abort the run as a setup/config error (exit 2) with
/// a stderr message naming the offending file path AND the reserved name —
/// NOT silently run and have its offenses deduped against the engine's by
/// `aggregate`'s 4-tuple key.
///
/// `cops/no_receiver_puts.rb` → `Murphy/NoReceiverPuts` collides with the
/// native cop's exact `name()`. `cops/syntax.rb` → `Murphy/Syntax` collides
/// with `murphy_core::SYNTAX_COP_NAME`. Both variants: exit 2, NO stdout JSON,
/// stderr names the file + the reserved name.
///
/// Pre-fix verification (perturb/observe, NOT committed): with the guard
/// removed, `cops/no_receiver_puts.rb` loads and RUNS — its `Murphy/...`
/// offense at the same (file, range, message) as the native cop's is silently
/// merged by `aggregate`, the run exits 1 with a stdout JSON array (NOT 2),
/// and stderr names no collision. This test FAILS in that pre-fix state
/// (asserts exit 2 + empty stdout + the collision message) and PASSES with
/// the guard. Confirmed empirically before committing the guard.
#[test]
fn reserved_name_collision_native_cop_aborts_exit_2() {
    let proj = project(&[("no_receiver_puts", TRIVIAL_VALID_COP)]);

    let out = run_lint_capture(&proj, &["app.rb"]);
    assert_eq!(
        out.status.code().expect("exit code"),
        2,
        "user cop deriving the native cop's reserved name → setup error exit 2"
    );
    assert!(
        out.stdout.is_empty(),
        "a setup-class error prints NO stdout JSON, got {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no_receiver_puts.rb"),
        "stderr must name the offending cop file path, got: {stderr}"
    );
    assert!(
        stderr.contains("Murphy/NoReceiverPuts"),
        "stderr must name the reserved name it collides with, got: {stderr}"
    );
}

/// Sibling of the above for the synthetic `Murphy/Syntax` name
/// (`murphy_core::SYNTAX_COP_NAME`): `cops/syntax.rb` → `Murphy/Syntax`.
#[test]
fn reserved_name_collision_syntax_cop_aborts_exit_2() {
    let proj = project(&[("syntax", TRIVIAL_VALID_COP)]);

    let out = run_lint_capture(&proj, &["app.rb"]);
    assert_eq!(
        out.status.code().expect("exit code"),
        2,
        "user cop deriving Murphy/Syntax (SYNTAX_COP_NAME) → setup error exit 2"
    );
    assert!(
        out.stdout.is_empty(),
        "a setup-class error prints NO stdout JSON, got {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("syntax.rb"),
        "stderr must name the offending cop file path, got: {stderr}"
    );
    assert!(
        stderr.contains("Murphy/Syntax"),
        "stderr must name the reserved name it collides with, got: {stderr}"
    );
}

/// Guard rail: a NON-colliding user cop name (`no_puts.rb` → `Murphy/NoPuts`)
/// still works EXACTLY as before — loads, runs, contributes its offense, the
/// run exits 1 with the cross-engine JSON array. This pins that the guard is
/// scoped to the RESERVED set only and did not regress the normal path
/// (the existing `native_and_mruby_cops_co_occur_and_are_deterministic`
/// asserts the full shape; this is a focused non-collision smoke).
#[test]
fn non_colliding_user_cop_name_still_runs() {
    let proj = project(&[("no_puts", NO_PUTS_COP)]);

    let (stdout, code) = run_lint(&proj, &["app.rb"]);
    assert_eq!(code, 1, "non-colliding user cop runs normally → exit 1");
    let names = cop_names(&parse_offenses(&stdout));
    assert!(
        names.iter().any(|n| n == "Murphy/NoPuts"),
        "non-colliding Murphy/NoPuts still fires, got {names:?}"
    );
}
