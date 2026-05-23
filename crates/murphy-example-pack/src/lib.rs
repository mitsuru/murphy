//! murphy-example-pack — demo plugin pack for plugin authors.
//!
//! Reborn under the single-surface ABI (ADR 0038, murphy-9cr.10.1). Ships
//! two cops that illustrate complementary authorship vectors:
//!
//! - [`Example/NoEval`](no_eval) — `Send` (CallNode) dispatch + receiver
//!   matching.
//! - `Example/TodoFormat` — file-visit dispatch (`KINDS = &[]`) +
//!   `#[derive(CopOptions)]` (added by murphy-9cr.10.1 Task 3.3).
//!
//! The pack is the canonical reference distribution for the e2e plugin
//! loading path (`crates/murphy-cli/tests/plugin_pack_e2e.rs`).

pub mod no_eval;

use crate::no_eval::NoEval;

// `register_cops!` re-exported from `murphy-plugin-api`. `mode = dynamic`
// emits `#[no_mangle] pub unsafe extern "C" fn murphy_plugin_register`
// for cdylib consumption by the host's plugin loader.
murphy_plugin_api::register_cops!(mode = dynamic, NoEval);

#[cfg(test)]
mod tests {
    /// Dummy smoke test: ensures `cargo test --workspace` materialises
    /// the cdylib build artifact (the e2e test in
    /// `crates/murphy-cli/tests/plugin_pack_e2e.rs` reads it via dlopen).
    /// The Cargo dep graph already guarantees this through
    /// `murphy-cli`'s `[dev-dependencies]`, but the explicit test keeps
    /// the invariant local to this crate.
    #[test]
    fn smoke_compiles() {}
}
