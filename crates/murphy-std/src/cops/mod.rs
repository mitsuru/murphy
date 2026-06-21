//! Cop modules grouped by namespace directory.
//!
//! Each subdirectory matches the cop namespace it ships (`Lint/*`,
//! `Style/*`, `Layout/*` per ADR 0018). The file path mirrors the cop
//! id at a glance, matching the convention used by `murphy-rspec` and
//! `murphy-rails`.

pub mod bundler;
pub mod layout;
pub mod lint;
pub mod metrics;
pub mod naming;
pub mod security;
pub mod style;
pub mod util;
