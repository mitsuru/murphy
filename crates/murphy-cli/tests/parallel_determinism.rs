//! Parallel-dispatch determinism guard (Phase 2 Task 5).
//!
//! Task 5 converts the per-file lint loop in `run()` from a sequential
//! `for` loop into a rayon `par_iter` parallel map. Determinism of the final
//! JSON output is *not* provided by thread/arg ordering — it comes from
//! `aggregate`'s **total order** `(file, start, end, cop_name, message,
//! severity)` (Task 2). This test is the permanent guard that the parallel
//! pipeline preserves that guarantee.
//!
//! ## What this asserts
//!
//! 1. **Cross-run byte identity.** `murphy lint <4 sample_project fixtures>`
//!    is run many times (4 distinct CLI arg permutations × 3 repeats = 12
//!    parallel dispatches). Every single stdout is **byte-identical** to the
//!    first run's stdout. If rayon thread interleaving or arg order could
//!    perturb output, this fails — but it cannot, because `aggregate`
//!    re-sorts into a total order before serialization.
//! 2. **Frozen-contract identity.** That stdout is value-equal to the
//!    committed `tests/snapshots/sample_project.json` (ADR 0006 frozen
//!    contract — the same snapshot `integration_snapshot.rs` pins). The
//!    snapshot file is pretty-printed and stdout is compact, so this compares
//!    parsed `serde_json::Value`s (same workaround as `integration_snapshot`),
//!    NOT raw bytes — re-blessing the snapshot to compact form would touch the
//!    frozen contract and is out of scope.
//!
//! No new fixtures, no `tempfile`, no RNG: the fixtures are checked in and the
//! permutations are hardcoded so the test is itself deterministic.

use assert_cmd::Command;
use std::path::PathBuf;

/// Absolute path to `crates/murphy-cli/tests/fixtures/sample_project`
/// (same locator pattern as `integration_snapshot.rs`).
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample_project")
}

/// Absolute path to the committed expected snapshot (the ADR 0006 frozen
/// contract — identical file `integration_snapshot.rs` pins).
fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("sample_project.json")
}

/// Run `murphy lint <files…>` from the fixtures dir (bare filenames, so
/// `Offense.file` is portable) and return raw stdout bytes. Asserts exit `1`
/// (the fixture set is dirty/broken → offenses present).
fn run_lint(files: &[&str]) -> Vec<u8> {
    let mut cmd = Command::cargo_bin("murphy").expect("murphy binary builds");
    cmd.current_dir(fixtures_dir())
        .arg("lint")
        .arg("--format")
        .arg("json");
    for f in files {
        cmd.arg(f);
    }
    let assert = cmd.assert().code(1);
    assert.get_output().stdout.clone()
}

/// The four checked-in fixtures, in several deliberately different CLI arg
/// orders. The final aggregated output MUST be identical for every order —
/// arg order is not output order (`aggregate` sorts by `(file, …)`).
const PERMUTATIONS: &[[&str; 4]] = &[
    ["clean.rb", "dirty.rb", "broken.rb", "multibyte.rb"],
    ["dirty.rb", "multibyte.rb", "clean.rb", "broken.rb"],
    ["multibyte.rb", "broken.rb", "dirty.rb", "clean.rb"],
    ["broken.rb", "clean.rb", "multibyte.rb", "dirty.rb"],
];
/// Repeats per permutation: each dispatch is a fresh rayon thread pool, so
/// repeating drives different real thread interleavings across runs.
const REPEATS: usize = 3;

/// Repeated + shuffled parallel dispatches all yield BYTE-IDENTICAL stdout,
/// and that stdout is value-equal to the committed frozen snapshot.
#[test]
fn parallel_lint_is_byte_identical_across_repeats_and_arg_orders() {
    // Reference = first permutation, first run. Every other dispatch
    // (different arg order, different run) must produce these exact bytes.
    let reference = run_lint(&PERMUTATIONS[0]);
    assert!(
        !reference.is_empty(),
        "reference stdout must be the offense JSON array, not empty"
    );

    let mut dispatches = 0usize;
    for perm in PERMUTATIONS {
        for repeat in 0..REPEATS {
            let got = run_lint(perm);
            assert_eq!(
                got, reference,
                "stdout differs across parallel dispatches — determinism broken \
                 (arg order {perm:?}, repeat {repeat}). The aggregate total order \
                 must make output independent of file/thread ordering."
            );
            dispatches += 1;
        }
    }
    assert_eq!(
        dispatches,
        PERMUTATIONS.len() * REPEATS,
        "expected every permutation×repeat to be dispatched"
    );

    // Frozen-contract check (ADR 0006): the deterministic stdout is value-equal
    // to the committed snapshot. Compared as parsed JSON, not raw bytes,
    // because the snapshot file is pretty-printed and stdout is compact — the
    // same intentional workaround `integration_snapshot.rs` uses. Do NOT
    // re-bless the snapshot to compact form: that would alter the frozen
    // contract and is out of scope for Task 5.
    let got: serde_json::Value =
        serde_json::from_slice(&reference).expect("stdout must be a JSON array");
    let expected_bytes =
        std::fs::read(snapshot_path()).expect("committed sample_project.json snapshot must exist");
    let expected: serde_json::Value =
        serde_json::from_slice(&expected_bytes).expect("committed snapshot must be valid JSON");
    assert_eq!(
        got, expected,
        "parallel pipeline output diverged from the committed frozen snapshot \
         (ADR 0006). If you see this, parallelism broke determinism/contract — \
         STOP, do not re-bless the snapshot."
    );
}
