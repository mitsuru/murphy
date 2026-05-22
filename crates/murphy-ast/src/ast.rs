//! The [`Ast`] arena and its traversal API.

use crate::interner::Interner;
use crate::node::{AstNode, Comment, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer};

#[inline]
fn push_opt(out: &mut Vec<NodeId>, o: OptNodeId) {
    if let Some(id) = o.get() {
        out.push(id);
    }
}

#[inline]
fn push_list(out: &mut Vec<NodeId>, lists: &[NodeId], l: NodeList) {
    let start = l.start as usize;
    out.extend_from_slice(&lists[start..start + l.len as usize]);
}

/// Append every child `NodeId` of `kind`, in source order, to `out`.
///
/// Single source of truth for parent computation
/// ([`AstBuilder::finish`](crate::AstBuilder::finish)) and the
/// [`Ast::children`] iterator. The `match` is exhaustive on purpose: a new
/// `NodeKind` variant will not compile until it is handled here.
pub(crate) fn collect_children(kind: &NodeKind, lists: &[NodeId], out: &mut Vec<NodeId>) {
    match *kind {
        NodeKind::Error
        | NodeKind::Nil
        | NodeKind::True_
        | NodeKind::False_
        | NodeKind::SelfExpr
        | NodeKind::Int(_)
        | NodeKind::Float(_)
        | NodeKind::Str(_)
        | NodeKind::Sym(_)
        | NodeKind::Lvar(_)
        | NodeKind::Ivar(_)
        | NodeKind::Cvar(_)
        | NodeKind::Gvar(_)
        | NodeKind::Arg(_)
        | NodeKind::Unknown
        | NodeKind::Restarg(_)
        | NodeKind::Kwarg(_)
        | NodeKind::Kwrestarg(_)
        | NodeKind::Blockarg(_) => {}

        NodeKind::Const { scope, .. } => push_opt(out, scope),

        NodeKind::Lvasgn { value, .. }
        | NodeKind::Ivasgn { value, .. }
        | NodeKind::Gvasgn { value, .. }
        | NodeKind::Cvasgn { value, .. } => push_opt(out, value),

        NodeKind::Casgn { scope, value, .. } => {
            push_opt(out, scope);
            push_opt(out, value);
        }

        NodeKind::Send { receiver, args, .. } => {
            push_opt(out, receiver);
            push_list(out, lists, args);
        }

        NodeKind::Csend { receiver, args, .. } => {
            out.push(receiver);
            push_list(out, lists, args);
        }

        NodeKind::Block { call, args, body } => {
            out.push(call);
            out.push(args);
            push_opt(out, body);
        }

        NodeKind::BlockPass(o) | NodeKind::Splat(o) | NodeKind::Return(o) => push_opt(out, o),

        NodeKind::Array(l) | NodeKind::Hash(l) | NodeKind::Begin(l) | NodeKind::Args(l) => {
            push_list(out, lists, l)
        }

        NodeKind::Pair { key, value } => {
            out.push(key);
            out.push(value);
        }

        NodeKind::If { cond, then_, else_ } => {
            out.push(cond);
            push_opt(out, then_);
            push_opt(out, else_);
        }

        NodeKind::Case {
            subject,
            whens,
            else_,
        } => {
            push_opt(out, subject);
            push_list(out, lists, whens);
            push_opt(out, else_);
        }

        NodeKind::When { conds, body } => {
            push_list(out, lists, conds);
            push_opt(out, body);
        }

        NodeKind::And { lhs, rhs } | NodeKind::Or { lhs, rhs } => {
            out.push(lhs);
            out.push(rhs);
        }

        NodeKind::Def { args, body, .. } => {
            out.push(args);
            push_opt(out, body);
        }

        NodeKind::Class {
            name,
            superclass,
            body,
        } => {
            out.push(name);
            push_opt(out, superclass);
            push_opt(out, body);
        }

        NodeKind::Module { name, body } => {
            out.push(name);
            push_opt(out, body);
        }

        NodeKind::Optarg { default, .. } | NodeKind::Kwoptarg { default, .. } => out.push(default),
    }
}

/// An owned, flat, parser-shaped, typed AST for one file. See ADR 0037.
#[derive(Debug, Clone, PartialEq)]
pub struct Ast {
    pub(crate) nodes: Vec<AstNode>,
    pub(crate) node_lists: Vec<NodeId>,
    pub(crate) interner: Interner,
    pub(crate) comments: Vec<Comment>,
    pub(crate) source: SourceBuffer,
    pub(crate) root: NodeId,
}

impl Ast {
    /// The root node.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// `true` iff the arena has no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The node at `id`.
    pub fn node(&self, id: NodeId) -> &AstNode {
        &self.nodes[id.0 as usize]
    }

    /// The kind of the node at `id`.
    pub fn kind(&self, id: NodeId) -> &NodeKind {
        &self.nodes[id.0 as usize].kind
    }

    /// The source range of the node at `id`.
    pub fn range(&self, id: NodeId) -> Range {
        self.nodes[id.0 as usize].range
    }

    /// The parent of `id`. `OptNodeId::NONE` for the root.
    pub fn parent(&self, id: NodeId) -> OptNodeId {
        self.nodes[id.0 as usize].parent
    }

    /// The direct children of `id`, in source order.
    pub fn children(&self, id: NodeId) -> std::vec::IntoIter<NodeId> {
        let mut out = Vec::new();
        collect_children(self.kind(id), &self.node_lists, &mut out);
        out.into_iter()
    }

    /// The ancestors of `id`, nearest first, up to (and including) the root.
    pub fn ancestors(&self, id: NodeId) -> Ancestors<'_> {
        Ancestors {
            ast: self,
            current: self.parent(id),
        }
    }

    /// All descendants of `id` in DFS pre-order, excluding `id` itself.
    pub fn descendants(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut stack: Vec<NodeId> = self.children(id).collect();
        stack.reverse();
        std::iter::from_fn(move || {
            let next = stack.pop()?;
            let mut kids: Vec<NodeId> = self.children(next).collect();
            kids.reverse();
            stack.extend(kids);
            Some(next)
        })
    }

    /// The full source text.
    pub fn source(&self) -> &str {
        &self.source.text
    }

    /// The file path.
    pub fn path(&self) -> &std::path::Path {
        &self.source.path
    }

    /// The source text covered by `range`.
    pub fn raw_source(&self, range: Range) -> &str {
        &self.source.text[range.start as usize..range.end as usize]
    }

    /// The comments, in source order.
    pub fn comments(&self) -> &[Comment] {
        &self.comments
    }

    /// The string interner.
    pub fn interner(&self) -> &Interner {
        &self.interner
    }
}

/// Iterator over a node's ancestors, nearest first. See [`Ast::ancestors`].
pub struct Ancestors<'a> {
    ast: &'a Ast,
    current: OptNodeId,
}

impl Iterator for Ancestors<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let id = self.current.get()?;
        self.current = self.ast.parent(id);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{NodeId, NodeKind, NodeList, OptNodeId, Range, Symbol};

    #[test]
    fn collect_children_handles_opt_list_and_direct() {
        // Send { receiver: Some(1), args: [2, 3] } → [1, 2, 3]
        let lists = vec![NodeId(2), NodeId(3)];
        let kind = NodeKind::Send {
            receiver: OptNodeId::some(NodeId(1)),
            method: Symbol(0),
            args: NodeList { start: 0, len: 2 },
        };
        let mut out = Vec::new();
        collect_children(&kind, &lists, &mut out);
        assert_eq!(out, vec![NodeId(1), NodeId(2), NodeId(3)]);
    }

    #[test]
    fn collect_children_skips_none() {
        // Send { receiver: None, args: [] } → []
        let kind = NodeKind::Send {
            receiver: OptNodeId::NONE,
            method: Symbol(0),
            args: NodeList::EMPTY,
        };
        let mut out = Vec::new();
        collect_children(&kind, &[], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn collect_children_leaf_has_no_children() {
        let mut out = Vec::new();
        collect_children(&NodeKind::Int(5), &[], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn traversal_children_ancestors_descendants() {
        use crate::builder::AstBuilder;

        // Begin [ if(cond=int, then=int) ]
        let mut b = AstBuilder::new("src", "t.rb");
        let r = Range { start: 0, end: 1 };
        let cond = b.push(NodeKind::Int(1), r);
        let then_ = b.push(NodeKind::Int(2), r);
        let iff = b.push(
            NodeKind::If {
                cond,
                then_: OptNodeId::some(then_),
                else_: OptNodeId::NONE,
            },
            r,
        );
        let list = b.push_list(&[iff]);
        let root = b.push(NodeKind::Begin(list), r);
        let ast = b.finish(root);

        // children
        assert_eq!(ast.children(root).collect::<Vec<_>>(), vec![iff]);
        assert_eq!(ast.children(iff).collect::<Vec<_>>(), vec![cond, then_]);

        // ancestors (nearest first)
        assert_eq!(ast.ancestors(cond).collect::<Vec<_>>(), vec![iff, root]);
        assert_eq!(
            ast.ancestors(root).collect::<Vec<_>>(),
            Vec::<NodeId>::new()
        );

        // descendants (DFS pre-order, excludes self)
        assert_eq!(
            ast.descendants(root).collect::<Vec<_>>(),
            vec![iff, cond, then_]
        );
    }
}
