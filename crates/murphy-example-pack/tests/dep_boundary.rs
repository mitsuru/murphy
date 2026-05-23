//! Compile-time enforcement of the single-surface plugin ABI boundary for
//! `murphy-example-pack` (ADR 0038, design §5 in
//! `docs/plans/2026-05-22-plugin-reboot-design.md`).
//!
//! `murphy-example-pack` is the demo external plugin pack distributed
//! with Murphy. To keep its example role honest, the same boundary check
//! `murphy-std/tests/dep_boundary.rs` applies to it: every Murphy-prefixed
//! crate in the **runtime** `[dependencies]` must be exactly
//! `{murphy-plugin-api}`. Anything else (`murphy-core`, `murphy-ast`,
//! `murphy-translate`, …) would let the demo reach past the single-
//! surface API and is rejected here.
//!
//! `[dev-dependencies]` are excluded by design — `serde_json` is needed
//! to parse `cargo metadata` JSON inside this very test.
//! `[build-dependencies]` are excluded for the same reason.

use std::collections::BTreeSet;
use std::process::Command;

/// The closed allow-list. New entries are intentional API-surface
/// expansions and must be reflected in the design doc, not added here
/// silently.
const ALLOWED_MURPHY_RUNTIME_DEPS: &[&str] = &["murphy-plugin-api"];

#[test]
fn murphy_example_pack_runtime_murphy_deps_match_allow_list() {
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
        .find(|p| p["name"].as_str() == Some("murphy-example-pack"))
        .expect("`murphy-example-pack` package present in metadata");

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
        "murphy-example-pack must depend only on `murphy-plugin-api` at runtime \
         (single-surface plugin ABI, ADR 0038). Adjust the design doc, \
         not this test, if you intend to widen the boundary."
    );
}
