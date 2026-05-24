//! Compile-time enforcement of the single-surface plugin ABI boundary for
//! `murphy-rspec` (ADR 0038, design §5 in
//! `docs/plans/2026-05-22-plugin-reboot-design.md`).
//!
//! `murphy-rspec` is the bootstrap RSpec cop pack (murphy-4n9.4) and is
//! the same shape as an external `.so` plugin pack: the same boundary
//! check `murphy-std/tests/dep_boundary.rs` and
//! `murphy-example-pack/tests/dep_boundary.rs` apply. Every
//! Murphy-prefixed crate in the **runtime** `[dependencies]` must be
//! exactly `{murphy-plugin-api}`. Anything else (`murphy-core`,
//! `murphy-ast`, `murphy-translate`, …) would let the pack reach past
//! the single-surface API and is rejected here.
//!
//! `[dev-dependencies]` are excluded by design — `serde_json` is needed
//! to parse `cargo metadata` JSON inside this very test.

use std::collections::BTreeSet;
use std::process::Command;

/// The closed allow-list. New entries are intentional API-surface
/// expansions and must be reflected in the design doc, not added here
/// silently.
const ALLOWED_MURPHY_RUNTIME_DEPS: &[&str] = &["murphy-plugin-api"];

#[test]
fn murphy_rspec_runtime_murphy_deps_match_allow_list() {
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
        .find(|p| p["name"].as_str() == Some("murphy-rspec"))
        .expect("`murphy-rspec` package present in metadata");

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
        "murphy-rspec must depend only on `murphy-plugin-api` at runtime \
         (single-surface plugin ABI, ADR 0038). Adjust the design doc, \
         not this test, if you intend to widen the boundary."
    );
}
