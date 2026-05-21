//! Murphy core: the native engine for the Murphy Ruby linter/formatter.

mod aggregator;
mod ast_sexp;
pub mod autocorrect;
mod config;
mod cop;
mod cops;
mod discovery;
mod mruby;
mod offense;
mod parse;
mod plugin;
mod registry;

pub use aggregator::{aggregate, aggregate_with_config};
pub use autocorrect::{
    ApplyOutcome, Conflict, ConflictReason, FixpointOutcome, FixpointStatus, apply_edits,
    apply_edits_logged, run_to_fixpoint,
};
pub use config::{CopRule, MurphyConfig, migrate_rubocop_yml_to_murphy_toml};
pub use cop::{Cop, CopContext, run_cops};
pub use cops::NoReceiverPuts;
pub use discovery::{ConfigError, discover, discover_with_config};
// Phase 3 Task 2 keystone — the mruby lifecycle/ownership wrapper. Task 3's
// read-only native-primitive IDL registration (`register_primitives`) is
// in-crate only (`pub(crate)` in `mruby`), so it is deliberately NOT
// re-exported here — Task 4/5/7 reach it via `crate::mruby::register_primitives`.
// Nothing in the CLI pipeline calls it yet (Task 7 wires it).
pub use ast_sexp::ast_to_sexp;
pub use mruby::sandbox::{
    PackageCacheKey, PackageFingerprint, ResolvedRequire, ResolvedRequireKind,
    SANDBOX_POLICY_VERSION, STDLIB_ALLOWLIST_VERSION, SandboxBootError, SandboxPackage,
    SandboxViolation, boot_self_check, run_mruby_cop_sandboxed,
    run_mruby_cop_sandboxed_with_package, validate_denied_capabilities_absent,
};
pub use mruby::{
    AstContext, COP_DEADLINE, MrubyState, run_mruby_cop, run_mruby_cop_isolated,
    run_mruby_cop_isolated_with_deadline,
};
pub use offense::{Autocorrect, Edit, Offense, Range, SYNTAX_COP_NAME, Severity};
pub use parse::{Ast, ParseError, parse};
#[cfg(not(target_os = "windows"))]
pub use plugin::dynamic::{LoadedPluginPack, load_plugin_pack};
pub use plugin::{
    MURPHY_PLUGIN_ABI_VERSION, MurphyCallContext, MurphyCallDispatchV1, MurphyEmitOffense,
    MurphyFileContext, MurphyPluginAutocorrect, MurphyPluginCopV1, MurphyPluginEdit,
    MurphyPluginOffense, MurphyPluginV1, MurphyRange, MurphyRunCall, MurphyRunFile, MurphySlice,
    PluginFileCop, validate_plugin_cop_ids,
};
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
