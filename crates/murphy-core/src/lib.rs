//! Murphy core: the native engine for the Murphy Ruby linter/formatter.
//!
//! Post-reboot (ADR 0038): the single integration point between the
//! arena AST and cops is the plugin ABI in `murphy-plugin-api`. The
//! crate exposes:
//!
//! - [`parse`] — Ruby → arena AST (thin wrapper over `murphy-translate`).
//! - [`dispatch::run_cops`] — invoke a slice of `PluginCopV1` cops
//!   against an arena.
//! - [`plugin_loader::load_plugin_pack`] — `dlopen` a `.so` and validate
//!   its registration.
//! - [`CopRegistry`] — the host's combined cop list (caller-supplied
//!   static built-in pack + loaded dynamic packs), filtered by config.
//!   The standard cop pack itself lives in `murphy-std` (single-surface
//!   ABI, ADR 0038; murphy-9cr.23 §12b); murphy-core no longer hardcodes
//!   it.
//! - [`aggregate`] / [`run_to_fixpoint`] — offense aggregator + autocorrect
//!   fixpoint loop (unchanged contract, ADR 0006/0011/0013).
//! - [`MurphyConfig`] — `murphy.toml` schema (ADR 0015).
//! - [`discover`] — file discovery (ADR 0014).

mod aggregator;
pub mod autocorrect;
mod config;
mod discovery;
pub mod dispatch;
#[cfg(feature = "mruby-user-cops")]
mod mruby;
mod offense;
mod parse;
pub mod plugin_loader;
pub mod plugin_resolver;
mod registry;

pub use aggregator::{aggregate, aggregate_with_config};
pub use autocorrect::{
    ApplyOutcome, Conflict, ConflictReason, FixpointOutcome, FixpointStatus, apply_edits,
    apply_edits_logged, run_to_fixpoint,
};
pub use config::{
    CopRule, MurphyConfig, PluginConfig, PluginDetailed, migrate_rubocop_yml_to_murphy_toml,
};
pub use discovery::{ConfigError, discover, discover_with_config};
#[cfg(feature = "mruby-user-cops")]
pub use mruby::proxy::{
    MrubyCopProxy, build_mruby_cop, current_mruby_proxies_drain, current_mruby_proxies_populate,
    with_current_mruby_proxies,
};
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
pub use murphy_ast::ast_to_sexp;
pub use offense::{Autocorrect, Edit, Offense, Range, SYNTAX_COP_NAME, Severity};
pub use parse::{ParseError, parse, parse_with_cache};
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
