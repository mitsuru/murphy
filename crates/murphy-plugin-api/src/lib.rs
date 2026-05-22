//! Safe, plugin-author-facing surface over the Murphy single-surface
//! plugin ABI (ADR 0038). A cop reads the arena AST through [`ConfigError`]
//! and the types added by later tasks of murphy-9cr.20.

mod config_error;

pub use config_error::{ConfigError, ConfigErrorKind};
