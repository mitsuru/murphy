//! Embedded-mruby user-cop engine (Phase 3).
//!
//! Task 2 ships only the lifecycle/ownership keystone: [`state`] (the
//! `AstContext` carrier + the `MrubyState` RAII wrapper). Native primitives
//! (Task 3), the `Murphy::Cop` SDK (Task 4), the deadline/abandon watchdog
//! (Task 5), and pipeline integration (Task 7) land in their own tasks.

pub mod state;

pub use state::{AstContext, MrubyState};
