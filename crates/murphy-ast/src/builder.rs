//! [`AstBuilder`] — the construction API consumed by `murphy-translate`.

use std::path::PathBuf;

use crate::ast::{Ast, collect_children};
use crate::interner::InternBuilder;
use crate::node::{
    AstNode, Comment, CommentKind, NodeId, NodeKind, NodeList, NodeLoc, OptNodeId, Range,
    SourceBuffer, SourceToken, StringId, Symbol,
};

/// Builds an [`Ast`]. Push nodes and lists; `finish` computes parent links
/// from the node structure in one pass.
pub struct AstBuilder {
    nodes: Vec<AstNode>,
    node_lists: Vec<NodeId>,
    interner: InternBuilder,
    comments: Vec<Comment>,
    source_tokens: Vec<SourceToken>,
    source: SourceBuffer,
}

impl AstBuilder {
    /// Start building an AST for one file.
    pub fn new(source_text: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        AstBuilder {
            nodes: Vec::new(),
            node_lists: Vec::new(),
            interner: InternBuilder::default(),
            comments: Vec::new(),
            source_tokens: Vec::new(),
            source: SourceBuffer {
                text: source_text.into(),
                path: path.into(),
            },
        }
    }

    /// Intern an identifier.
    pub fn intern_symbol(&mut self, s: &str) -> Symbol {
        Symbol(self.interner.intern(s))
    }

    /// Intern string-literal contents.
    pub fn intern_string(&mut self, s: &str) -> StringId {
        StringId(self.interner.intern(s))
    }

    /// Append a node. `parent` is left as `NONE` until [`AstBuilder::finish`].
    /// `loc.name` defaults to [`Range::ZERO`]; use [`AstBuilder::push_named`]
    /// to record an identifier range alongside the expression range.
    pub fn push(&mut self, kind: NodeKind, expression: Range) -> NodeId {
        self.push_with_loc(
            kind,
            NodeLoc {
                expression,
                name: Range::ZERO,
            },
        )
    }

    /// Append a name-bearing node, recording both its full source range
    /// (`expression`) and the identifier range (`name`) — the parser-gem
    /// `node.loc.name` analog.
    pub fn push_named(&mut self, kind: NodeKind, expression: Range, name: Range) -> NodeId {
        self.push_with_loc(kind, NodeLoc { expression, name })
    }

    fn push_with_loc(&mut self, kind: NodeKind, loc: NodeLoc) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        debug_assert!(id.0 != u32::MAX, "arena exceeded u32 node capacity");
        self.nodes.push(AstNode {
            kind,
            parent: OptNodeId::NONE,
            loc,
        });
        id
    }

    /// Append a child list, returning a [`NodeList`] handle.
    pub fn push_list(&mut self, ids: &[NodeId]) -> NodeList {
        let start = self.node_lists.len() as u32;
        self.node_lists.extend_from_slice(ids);
        NodeList {
            start,
            len: ids.len() as u32,
        }
    }

    /// Record a source comment.
    pub fn add_comment(&mut self, range: Range, kind: CommentKind) {
        self.comments.push(Comment { range, kind });
    }

    /// Record a source token.
    pub fn add_source_token(&mut self, token: SourceToken) {
        self.source_tokens.push(token);
    }

    /// Finish building. Computes every node's `parent` from the structure
    /// in one pass, then returns the immutable [`Ast`]. `root` keeps
    /// `parent == NONE`.
    pub fn finish(mut self, root: NodeId) -> Ast {
        let mut buf: Vec<NodeId> = Vec::new();
        for i in 0..self.nodes.len() {
            buf.clear();
            collect_children(&self.nodes[i].kind, &self.node_lists, &mut buf);
            let parent = OptNodeId::some(NodeId(i as u32));
            for &child in &buf {
                self.nodes[child.0 as usize].parent = parent;
            }
        }
        // Prism's lexer emits tokens in *lex* order, not source order: a
        // heredoc's body/closing tokens are streamed right after the
        // `<<~ID` opener, before the rest of the opener's line. The
        // `sorted_tokens` accessor and the partition-based token helpers
        // (`begin`/`end`/`keyword` on `LocRef`; `token_before`/
        // `token_after`/`tokens_in` on `Cx`) all assume a start-sorted
        // stream, so enforce that here. A *stable* sort keeps the relative
        // order of equal-start tokens, which keeps `end` monotonic on the
        // inputs prism produces (the only overlaps are equal-end, e.g. a
        // heredoc-end token that folds the trailing newline shares its end
        // with the standalone newline token).
        self.source_tokens
            .sort_by_key(|t| (t.range.start, t.range.end));
        Ast {
            nodes: self.nodes,
            node_lists: self.node_lists,
            interner: self.interner.finish(),
            comments: self.comments,
            source_tokens: self.source_tokens,
            source: self.source,
            root,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{NodeKind, OptNodeId, Range};

    fn r() -> Range {
        Range { start: 0, end: 1 }
    }

    #[test]
    fn finish_computes_parents_from_structure() {
        // Tree:  Begin [ lvasgn x = int(1) ]
        let mut b = AstBuilder::new("x = 1", "test.rb");
        let int = b.push(NodeKind::Int(1), r());
        let x = b.intern_symbol("x");
        let asgn = b.push(
            NodeKind::Lvasgn {
                name: x,
                value: OptNodeId::some(int),
            },
            r(),
        );
        let list = b.push_list(&[asgn]);
        let root = b.push(NodeKind::Begin(list), r());
        let ast = b.finish(root);

        assert_eq!(ast.parent(root), OptNodeId::NONE, "root has no parent");
        assert_eq!(ast.parent(asgn).get(), Some(root));
        assert_eq!(ast.parent(int).get(), Some(asgn));
        assert_eq!(ast.root(), root);
    }

    #[test]
    fn builder_interns_and_stores_source() {
        let mut b = AstBuilder::new("source", "f.rb");
        let s1 = b.intern_symbol("dup");
        let s2 = b.intern_symbol("dup");
        assert_eq!(s1, s2);
        let root = b.push(NodeKind::Nil, r());
        let ast = b.finish(root);
        assert_eq!(ast.source(), "source");
        assert_eq!(ast.path().to_str(), Some("f.rb"));
    }
}
