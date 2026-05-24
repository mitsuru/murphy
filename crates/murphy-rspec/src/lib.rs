//! murphy-rspec — RSpec cop pack (murphy-4n9).
//!
//! v1 cops (under [`cops::rspec`]):
//! - `RSpec/DescribeClass` — bootstrap (murphy-4n9.4).
//! - `RSpec/ExampleLength` — line cap on example bodies (murphy-6bv).
//! - `RSpec/MultipleExpectations` — `expect(...)` count cap per
//!   example (murphy-6tq).
//!
//! Source layout: each namespace lives under `src/cops/<namespace>/`
//! so the file path tells you the cop's id at a glance.
//!
//! Authored against `murphy-plugin-api` only (single-surface ABI, ADR
//! 0038); the runtime `murphy-` dep set is asserted by
//! `tests/dep_boundary.rs`.

pub mod cops;

use cops::rspec::{DescribeClass, ExampleLength, MultipleExpectations};

// `register_cops!` re-exported from `murphy-plugin-api`. `mode = dynamic`
// emits `#[no_mangle] pub unsafe extern "C" fn murphy_plugin_register`
// for cdylib consumption by the host's plugin loader.
murphy_plugin_api::register_cops!(
    mode = dynamic,
    DescribeClass,
    ExampleLength,
    MultipleExpectations
);

#[cfg(test)]
mod tests {
    /// Dummy smoke test: ensures `cargo test --workspace` materialises
    /// the cdylib build artifact (the e2e test in
    /// `crates/murphy-cli/tests/rspec_pack_e2e.rs` reads it via dlopen).
    /// The Cargo dep graph already guarantees this through `murphy-cli`'s
    /// `[dev-dependencies]`, but the explicit test keeps the invariant
    /// local to this crate.
    #[test]
    fn smoke_compiles() {}
}
