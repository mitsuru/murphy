//! S-expression pattern grammar, parser, and runtime IR for Murphy.
//!
//! See beads issue murphy-9cr.17 and `docs/plans/2026-05-22-plugin-reboot-design.md` §4.

mod error;

pub use error::{ParseError, PatSpan};

mod ast;

pub use ast::{CaptureKind, Head, Lit, Pat, PatKind, PatternAst};

mod lexer;

mod parser;

pub use parser::parse;

mod ir;

pub use ir::{CaptureMeta, IrHead, IrNode, IrNodeId, IrSlice, PatternIr, StrRef, lower};

mod captures;

pub use captures::{CaptureValue, Captures};

mod schema;

pub use schema::{PatChild, pattern_children};

mod matcher;

pub use matcher::{NoPredicates, PredicateHost, matches};

/// Parse and lower a pattern source string to `PatternIr` in one step.
/// For the C backend (murphy-9cr.19). All errors are parse errors —
/// lowering itself is infallible.
pub fn compile(src: &str) -> Result<PatternIr, ParseError> {
    Ok(lower(&parse(src)?))
}
