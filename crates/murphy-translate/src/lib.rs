//! prism AST → murphy-ast arena AST の変換層（murphy-9cr.15）。
//!
//! prism と parser-gem のノード分割差（collapse/split）を変換層の内部だけで
//! 吸収する（Route B）。対応する `NodeKind` が無い prism ノードは
//! [`murphy_ast::NodeKind::Unknown`] へ落とし、`translate` は決して panic
//! しない。

mod translate;

pub use translate::translate;
