//! Murphy's core AST representation: an owned, flat, parser-shaped, typed
//! arena. See ADR 0037 (`docs/decisions/0037-arena-parser-shaped-typed-ast.md`).
//!
//! One [`Ast`] owns one file. Nodes are fixed-size POD values in a flat
//! `Vec`; variable-length children live in a side table referenced by
//! [`NodeList`]. The crate is prism-independent — `murphy-translate` is the
//! prism→arena bridge.

mod ast;
mod builder;
mod interner;
mod kinds;
mod node;
mod serialize;
mod sexp;

pub use ast::{Ancestors, Ast, AstRawParts, collect_children, slot_layout};
pub use builder::AstBuilder;
pub use interner::Interner;
pub use kinds::{
    GROUP_FOR_TYPE, KIND_PATTERN_NAMES, NodeKindTag, pattern_name, tag_from_pattern_name,
    tags_for_type_name,
};
pub use node::{
    AstNode, CallClosingLoc, CallOperatorLoc, Comment, CommentKind, NodeId, NodeKind, NodeList,
    NodeLoc, OptNodeId, Range, SourceBuffer, SourceToken, SourceTokenKind, StringId, Symbol,
};
pub use serialize::{FORMAT_VERSION, HEADER_LEN, MAGIC, SerError, content_hash};
pub use sexp::ast_to_sexp;
