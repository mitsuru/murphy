//! Murphy standard cop pack — Murphy / Lint / Style / Layout (ADR 0018).
//!
//! This crate is the "built-in" pack: `murphy-cli` links it statically and
//! registers its cops through the same single-surface plugin ABI (ADR 0038)
//! that external `.so` packs use. The Murphy-internal dependency boundary
//! is **`murphy-plugin-api` only** — every standard cop reaches Murphy
//! through that one surface, with no shortcut through `murphy-core`. The
//! boundary is enforced as an integration test in `tests/dep_boundary.rs`
//! and is the implementation of §5 of
//! `docs/plans/2026-05-22-plugin-reboot-design.md`.
//!
//! `register_cops!(mode = static)` (re-exported from `murphy-plugin-api`)
//! declares the `PACK_COPS` distributed slice and emits a plain Rust
//! `pub fn murphy_plugin_register` entry point — no `#[no_mangle]` symbol
//! that could collide with another statically-linked pack. Each cop file
//! calls `submit_cop!(T)` to register itself; the linker collects all
//! entries into `PACK_COPS` at build time. Adding a new cop requires only
//! editing that cop's own file — no central list in `lib.rs`.

pub mod cops;

/// RuboCop's built-in default configuration, embedded at compile time.
/// Passed to `MurphyConfig::load_with_defaults` so defaults are data-driven
/// rather than hardcoded in `is_cop_disabled_by_default`.
pub const BUNDLED_DEFAULTS_YAML: &str = include_str!("../config/default.yml");

// cop の登録は各 cop ファイルの submit_cop!(T) が担う。
// 新しい cop を追加する場合は cop ファイルに1行追加するだけ — lib.rs は不変。
murphy_plugin_api::register_cops!(mode = static);

/// Standard cops that have **not yet been migrated** to the arena AST.
/// Empty after §12d migrated the last three. The list will repopulate in
/// murphy-au8 §14a when `murphy-rails`'s cops join the disabled registry.
pub static DISABLED_COPS: &[&str] = &[];

/// Friendly pack name reported by `murphy cops list` for cops registered
/// (or held in the disabled list) by this crate.
pub const PACK_NAME: &str = "builtin";
