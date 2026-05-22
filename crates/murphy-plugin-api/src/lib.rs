//! Safe, plugin-author-facing surface over the Murphy single-surface
//! plugin ABI (ADR 0038). A cop reads the arena AST through [`ConfigError`]
//! and the types added by later tasks of murphy-9cr.20.

mod abi;
mod config_error;
mod cop;
mod cx;
mod node_cop;
mod options;
mod severity;

pub use abi::{CxRaw, FnTable, OptionSpec, RawEdit, RawOffense, RawSlice};
pub use config_error::{ConfigError, ConfigErrorKind};
pub use cop::Cop;
pub use cx::Cx;
pub use node_cop::{NodeCop, NodeKindTag};
pub use options::{CopOptions, NoOptions};
pub use severity::{
    SEVERITY_UNSET, Severity, TRISTATE_UNSET, tristate_from_wire, tristate_to_wire,
};
