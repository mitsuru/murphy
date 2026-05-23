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

pub use ast::{Ancestors, Ast, AstRawParts, collect_children};
pub use builder::AstBuilder;
pub use interner::Interner;
pub use kinds::{KIND_PATTERN_NAMES, NodeKindTag, pattern_name, tag_from_pattern_name};
pub use node::{
    AstNode, Comment, CommentKind, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer,
    StringId, Symbol,
};
pub use serialize::{FORMAT_VERSION, HEADER_LEN, MAGIC, SerError, content_hash};
