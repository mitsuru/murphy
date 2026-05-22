//! The [`Ast`] arena and its traversal API.

use crate::interner::Interner;
use crate::node::{AstNode, Comment, NodeId, NodeKind, NodeList, OptNodeId, SourceBuffer};

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
        | NodeKind::Arg(_) => {}

        NodeKind::Const { scope, .. } => push_opt(out, scope),

        NodeKind::Lvasgn { value, .. } | NodeKind::Ivasgn { value, .. } => push_opt(out, value),

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

    /// The parent of `id`. `OptNodeId::NONE` for the root.
    pub fn parent(&self, id: NodeId) -> OptNodeId {
        self.nodes[id.0 as usize].parent
    }

    /// The full source text.
    pub fn source(&self) -> &str {
        &self.source.text
    }

    /// The file path.
    pub fn path(&self) -> &std::path::Path {
        &self.source.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{NodeId, NodeKind, NodeList, OptNodeId, Symbol};

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
}
