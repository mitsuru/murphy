//! Murphy core: the native engine for the Murphy Ruby linter/formatter.
//!
//! Post-reboot (ADR 0038): the single integration point between the
//! arena AST and cops is the plugin ABI in `murphy-plugin-api`. The
//! crate exposes:
//!
//! - [`parse`] — Ruby → arena AST (thin wrapper over `murphy-translate`).
//! - [`dispatch::run_cops`] — invoke a slice of `PluginCopV1` cops
//!   against an arena.
//! - [`builtin::BUILTINS`] — the v1 cop set (`Murphy/NoReceiverPuts`
//!   only; more cops migrate over in murphy-9cr.23+).
//! - [`plugin_loader::load_plugin_pack`] — `dlopen` a `.so` and validate
//!   its registration.
//! - [`CopRegistry`] — the host's combined cop list (builtins + loaded
//!   packs), filtered by config.
//! - [`aggregate`] / [`run_to_fixpoint`] — offense aggregator + autocorrect
//!   fixpoint loop (unchanged contract, ADR 0006/0011/0013).
//! - [`MurphyConfig`] — `murphy.toml` schema (ADR 0015).
//! - [`discover`] — file discovery (ADR 0014).

mod aggregator;
pub mod autocorrect;
pub mod builtin;
mod config;
mod discovery;
pub mod dispatch;
#[cfg(feature = "mruby-user-cops")]
mod mruby;
mod offense;
mod parse;
pub mod plugin_loader;
mod registry;

pub use aggregator::{aggregate, aggregate_with_config};
pub use autocorrect::{
    ApplyOutcome, Conflict, ConflictReason, FixpointOutcome, FixpointStatus, apply_edits,
    apply_edits_logged, run_to_fixpoint,
};
pub use config::{CopRule, MurphyConfig, migrate_rubocop_yml_to_murphy_toml};
pub use discovery::{ConfigError, discover, discover_with_config};
#[cfg(feature = "mruby-user-cops")]
pub use mruby::sandbox::{
    PackageCacheKey, PackageFingerprint, ResolvedRequire, ResolvedRequireKind,
    SANDBOX_POLICY_VERSION, STDLIB_ALLOWLIST_VERSION, SandboxBootError, SandboxPackage,
    SandboxViolation, boot_self_check, run_mruby_cop_sandboxed,
    run_mruby_cop_sandboxed_with_package, validate_denied_capabilities_absent,
};
#[cfg(feature = "mruby-user-cops")]
pub use mruby::{
    AstContext, COP_DEADLINE, MrubyState, run_mruby_cop, run_mruby_cop_isolated,
    run_mruby_cop_isolated_with_deadline,
};
pub use offense::{Autocorrect, Edit, Offense, Range, SYNTAX_COP_NAME, Severity};
pub use parse::{ParseError, parse};
pub use registry::CopRegistry;

/// Returns the Murphy core crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(
            !version().is_empty(),
            "version() must return a non-empty string"
        );
    }
}
