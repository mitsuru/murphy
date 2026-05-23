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
//! `register_cops!(mode = static, …)` (re-exported from `murphy-plugin-api`)
//! emits the per-pack registration entry as a plain Rust `pub fn`
//! `murphy_plugin_register` — no `#[no_mangle]` symbol that could collide
//! with another statically-linked pack. `murphy-cli` calls
//! [`murphy_plugin_register`] directly at startup; the resulting
//! `PluginRegistration` flows into the same code path as `.so`-loaded
//! packs.
//!
//! ## Pack contents
//!
//! v1 (murphy-9cr.23 §12b) ships `Murphy/NoReceiverPuts`, migrated out of
//! `murphy-core/src/builtin/` so murphy-core stops being the home of any
//! standard cop. Subsequent §12d work adds at least one cop per namespace
//! (`Lint`, `Style`, `Layout`) covering the four authorship vectors the
//! issue calls out (call dispatch / flow analysis /
//! literal+option+autocorrect / raw source access).

pub mod lint;
pub mod murphy;
pub mod style;

use crate::lint::unreachable_code::UnreachableCode;
use crate::murphy::no_receiver_puts::NoReceiverPuts;
use crate::style::string_literals::StringLiterals;

// `register_cops!` re-exported from `murphy-plugin-api` — the crate is the
// single Murphy-prefixed runtime dependency by design.
murphy_plugin_api::register_cops!(
    mode = static,
    NoReceiverPuts,
    UnreachableCode,
    StringLiterals,
);

/// Standard cops that have **not yet been migrated** to the arena AST /
/// single-surface ABI and are therefore not dispatched. The host (`murphy-cli`)
/// reads this list when building its `CopRegistry` so that:
///
/// 1. `murphy cops list` surfaces these as `disabled: arena migration`
///    (rather than hiding their existence) — the §12c machine-readable
///    contract documents where the migration backlog is.
/// 2. A `[cops.rules."Name"]` section in `murphy.toml` that references a
///    disabled cop does not error: it is treated as a tolerated
///    no-op until the cop is migrated. If the user explicitly opts in
///    with `enabled = true`, the host emits a warning and still skips
///    the cop (so an upgrade does not silently break previously working
///    configs).
/// 3. The lint runner never attempts to dispatch them — by design they
///    have no `PluginCopV1` entry, only a name.
///
/// §12d adds the first three (`Lint/UnreachableCode`,
/// `Style/StringLiterals`, `Layout/TrailingWhitespace`) as fully
/// migrated cops; once those land they move from this list into the
/// `register_cops!` list above. Until then they sit here so the
/// disabled-registry plumbing has live test data.
pub static DISABLED_COPS: &[&str] = &["Layout/TrailingWhitespace"];

/// Friendly pack name reported by `murphy cops list` for cops registered
/// (or held in the disabled list) by this crate. Matches the "builtin"
/// pack-name convention `CopRegistry` already uses for static built-ins.
pub const PACK_NAME: &str = "builtin";
