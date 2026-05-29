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

// LALRPOP-generated parser module (grammar: `src/parser.lalrpop`).
//
// `lalrpop_mod!` expands to:
//   `mod lalrpop_parser { include!(concat!(env!("OUT_DIR"), "/parser.rs")); }`
// — so the included file is `src/parser.lalrpop` compiled into `$OUT_DIR/parser.rs`.
// The inner `#![allow]` suppresses lints in LALRPOP's generated code, which
// contains patterns that trip clippy even in active, correct grammars.
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

mod param;

pub use param::{IntoParam, LitView, Param, match_lit_against_param};

mod matcher;

pub use matcher::{
    NoParams, NoPredicates, ParamHost, PredCallArg, PredicateHost, matches, matches_with_params,
};

/// Parse and lower a pattern source string to `PatternIr` in one step.
/// For the C backend (murphy-9cr.19). All errors are parse errors —
/// lowering itself is infallible.
pub fn compile(src: &str) -> Result<PatternIr, ParseError> {
    Ok(lower(&parse(src)?))
}
