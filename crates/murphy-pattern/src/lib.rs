//! S-expression pattern grammar, parser, and runtime IR for Murphy.
//!
//! See beads issue murphy-9cr.17 and `docs/plans/2026-05-22-plugin-reboot-design.md` §4.

mod error;

pub use error::{ParseError, PatSpan};
