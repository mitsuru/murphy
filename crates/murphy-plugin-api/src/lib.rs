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
//! The pack-authoring proc macros (`register_cops!`, `#[derive(CopOptions)]`,
//! `#[murphy::cop]`, `#[on_node]`, `node_pattern!`) live in
//! `murphy-plugin-macros` and are re-exported here so a pack's
//! `[dependencies]` stays at one Murphy crate (design §5; enforced for
//! `murphy-std` by `crates/murphy-std/tests/dep_boundary.rs`). The same
//! single-surface rule re-exports the arena AST types a cop's `check`
//! body matches against (`NodeKind`, `NodeId`, `OptNodeId`, …).

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

/// Parser-driven cop test harness. Available only when the
/// `test-support` feature is enabled (typically as a plugin pack's
/// `[dev-dependencies]` entry — production code never pulls in the
/// runtime parser). See the module docs for the usage shape.
#[cfg(feature = "test-support")]
pub mod test_support;

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

// Single-surface re-exports — every type and macro a static / dynamic
// pack needs to author a cop must be reachable through `murphy-plugin-api`
// alone, so the pack's `[dependencies]` stays at one Murphy crate (design
// §5; enforced by `crates/murphy-std/tests/dep_boundary.rs`).
pub use murphy_ast::{
    AstNode, Comment, CommentKind, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer,
    StringId, Symbol,
};
pub use murphy_plugin_macros::{
    CopOptions, cop, node_pattern, on_new_investigation, on_node, register_cops,
};
