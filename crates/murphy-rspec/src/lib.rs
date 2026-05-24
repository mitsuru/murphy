//! murphy-rspec — RSpec cop pack bootstrap (murphy-4n9.4).
//!
//! v1 ships a single cop, [`RSpec/DescribeClass`](describe_class), as
//! the seed of the pack. The remaining v1 set
//! (`RSpec/ExampleLength`, `RSpec/MultipleExpectations`) is tracked as
//! follow-up sub-issues under murphy-4n9.
//!
//! Authored against `murphy-plugin-api` only (single-surface ABI, ADR
//! 0038); the runtime `murphy-` dep set is asserted by
//! `tests/dep_boundary.rs`.

pub mod describe_class;

use crate::describe_class::DescribeClass;

// `register_cops!` re-exported from `murphy-plugin-api`. `mode = dynamic`
// emits `#[no_mangle] pub unsafe extern "C" fn murphy_plugin_register`
// for cdylib consumption by the host's plugin loader.
murphy_plugin_api::register_cops!(mode = dynamic, DescribeClass);

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
