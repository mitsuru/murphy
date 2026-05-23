//! Compile-time enforcement of the single-surface plugin ABI boundary for
//! `murphy-std` (ADR 0038, design §5 in
//! `docs/plans/2026-05-22-plugin-reboot-design.md`).
//!
//! `murphy-std` is "the standard cop pack" — the same shape as an external
//! `.so` plugin pack but statically linked into `murphy-cli`. To make the
//! single-surface ABI a compiler-enforced boundary (not merely a
//! convention), this test asserts that every Murphy-prefixed crate in
//! `murphy-std`'s **runtime** `[dependencies]` is exactly
//! `{murphy-plugin-api}`. Anything else (`murphy-core`, `murphy-ast`,
//! `murphy-translate`, …) would let a future contributor reach past the
//! single-surface API and is rejected here.
//!
//! `[dev-dependencies]` are excluded by design (§5): test fixtures and
//! snapshot utilities are not in the production link, so they may pull in
//! anything. `[build-dependencies]` are excluded for the same reason —
//! they run at build time only.

use std::collections::BTreeSet;
use std::process::Command;

/// The closed allow-list. New entries are intentional API-surface
/// expansions and must be reflected in the design doc, not added here
/// silently.
const ALLOWED_MURPHY_RUNTIME_DEPS: &[&str] = &["murphy-plugin-api"];

#[test]
fn murphy_std_runtime_murphy_deps_match_allow_list() {
    let manifest = env!("CARGO_MANIFEST_DIR");

    let output = Command::new(env!("CARGO"))
        .arg("metadata")
        .arg("--format-version=1")
        .arg("--no-deps")
        .arg("--manifest-path")
        .arg(format!("{manifest}/Cargo.toml"))
        .output()
        .expect("cargo metadata should run");

    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let meta: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata emits JSON");

    let packages = meta["packages"].as_array().expect("`packages` array");

    let pkg = packages
        .iter()
        .find(|p| p["name"].as_str() == Some("murphy-std"))
        .expect("`murphy-std` package present in metadata");

    let deps = pkg["dependencies"]
        .as_array()
        .expect("`dependencies` array");

    let runtime_murphy_deps: BTreeSet<String> = deps
        .iter()
        .filter(|d| {
            // `kind` is `null` for normal/runtime deps, `"dev"` for
            // dev-deps, `"build"` for build-deps. Only runtime deps count
            // toward the boundary.
            d.get("kind").map(|k| k.is_null()).unwrap_or(false)
        })
        .filter_map(|d| d["name"].as_str().map(str::to_owned))
        .filter(|name| name.starts_with("murphy-"))
        .collect();

    let allowed: BTreeSet<String> = ALLOWED_MURPHY_RUNTIME_DEPS
        .iter()
        .map(|s| (*s).to_owned())
        .collect();

    assert_eq!(
        runtime_murphy_deps, allowed,
        "murphy-std must depend only on `murphy-plugin-api` at runtime \
         (single-surface plugin ABI, ADR 0038). Adjust the design doc, \
         not this test, if you intend to widen the boundary."
    );
}
