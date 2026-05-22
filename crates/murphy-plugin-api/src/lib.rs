//! Safe, plugin-author-facing surface over the Murphy single-surface
//! plugin ABI (ADR 0038).
//!
//! Every cop — built-in or external `.so` — reads the AST through this
//! one crate. A callback receives a [`Cx`], a direct-read view of an
//! immutable [`murphy-ast`](murphy_ast) arena: traversal and `NodeKind`
//! matching are pure memory reads. [`Cop`] carries compile-time metadata;
//! [`NodeCop`] carries dispatch. The `#[repr(C)]` ABI types
//! ([`CxRaw`], [`FnTable`], [`PluginCopV1`], [`PluginRegistration`], …)
//! cross the `.so` boundary and are re-exported at the crate root.
//!
//! `register_cops!` / `#[derive(CopOptions)]` / `#[on_node]` live in
//! `murphy-plugin-macros` (murphy-9cr.21 / .8) and consume this surface.

#[doc(hidden)]
#[path = "internal.rs"]
pub mod __internal;
mod abi;
mod config_error;
mod cop;
mod cx;
mod node_cop;
mod options;
mod severity;

pub use abi::{
    CxRaw, DispatchFn, FnTable, MURPHY_PLUGIN_ABI_VERSION, MurphyPluginRegister, OptionSpec,
    PluginCopV1, PluginRegistration, RawEdit, RawOffense, RawSlice,
};
pub use config_error::{ConfigError, ConfigErrorKind};
pub use cop::Cop;
pub use cx::Cx;
pub use node_cop::{NodeCop, NodeKindTag};
pub use options::{CopOptions, NoOptions};
pub use severity::{
    SEVERITY_UNSET, Severity, TRISTATE_UNSET, tristate_from_wire, tristate_to_wire,
};
