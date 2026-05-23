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

pub mod murphy;

use crate::murphy::no_receiver_puts::NoReceiverPuts;

// `register_cops!` re-exported from `murphy-plugin-api` — the crate is the
// single Murphy-prefixed runtime dependency by design.
murphy_plugin_api::register_cops!(mode = static, NoReceiverPuts);
