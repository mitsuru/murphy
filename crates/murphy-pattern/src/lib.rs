//! S-expression pattern grammar, parser, and runtime IR for Murphy.
//!
//! See beads issue murphy-9cr.17 and `docs/plans/2026-05-22-plugin-reboot-design.md` §4.

mod error;

pub use error::{ParseError, PatSpan};

mod ast;

pub use ast::{CaptureKind, Head, Lit, Pat, PatKind, PatternAst, PredArg};

mod lexer;

mod parser;

pub use parser::parse;

// LALRPOP-generated parser module (murphy-qpf9 Round 1: infra-only stub).
//
// The active parser is still `parser::parse` above; this module is wired in
// so the build script and lalrpop-util runtime link cleanly. Round 2 will
// port the grammar rule-by-rule, and Round 3 will flip the public `parse`
// to this module once error-message parity is achieved.
//
// `lalrpop_mod!` expands to:
//   `mod lalrpop_parser { include!(concat!(env!("OUT_DIR"), "/parser.rs")); }`
// — so the included file is `src/parser.lalrpop` compiled into `$OUT_DIR/parser.rs`.
#[allow(unused, clippy::all, dead_code)]
mod lalrpop_parser_inner {
    #![allow(unused, clippy::all, dead_code)]
    lalrpop_util::lalrpop_mod!(pub lalrpop_parser, "/parser.rs");
}

mod ir;

pub use ir::{CaptureMeta, IrHead, IrNode, IrNodeId, IrPredArg, IrSlice, PatternIr, StrRef, lower};

mod captures;

pub use captures::{CaptureValue, Captures};

mod schema;

pub use schema::{PatChild, pattern_children};

mod matcher;

pub use matcher::{NoPredicates, PredCallArg, PredicateHost, matches};

/// Parse and lower a pattern source string to `PatternIr` in one step.
/// For the C backend (murphy-9cr.19). All errors are parse errors —
/// lowering itself is infallible.
pub fn compile(src: &str) -> Result<PatternIr, ParseError> {
    Ok(lower(&parse(src)?))
}
