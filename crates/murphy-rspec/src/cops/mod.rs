//! Cop modules grouped by namespace directory.
//!
//! Each subdirectory matches the cop namespace it ships (e.g.
//! `rspec/*` exposes `RSpec/*` cops). This keeps the source tree
//! readable as the pack grows and makes it trivial to spot which
//! namespace a cop belongs to from its file path alone.

pub mod rspec;
