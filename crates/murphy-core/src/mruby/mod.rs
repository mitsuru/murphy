//! Embedded-mruby user-cop engine (Phase 3).
//!
//! Task 2 shipped the lifecycle/ownership keystone: [`state`] (the
//! `AstContext` carrier + the `MrubyState` RAII wrapper). Task 3 adds
//! [`primitives`] — the read-only live native-primitive IDL operating on an
//! `AstContext` via `ud` (walk-order-index re-walk, ADR 0008). The
//! `Murphy::Cop` SDK (Task 4), the deadline/abandon watchdog (Task 5), and
//! pipeline integration (Task 7) land in their own tasks.

pub mod primitives;
pub mod state;

// No `register` re-export: it is `pub(crate)` and in-crate only — Task 4/5/7
// reach it directly via `crate::mruby::primitives::register` (one extra path
// segment, no redundant alias). Same Task-2 `raw()` discipline.
pub use state::{AstContext, MrubyState};
