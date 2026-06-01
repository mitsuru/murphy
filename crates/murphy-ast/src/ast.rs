//! The [`Ast`] arena and its traversal API.

use crate::interner::Interner;
use crate::node::{
    AstNode, CallClosingLoc, CallOperatorLoc, Comment, MagicComment, NodeId, NodeKind, NodeList,
    NodeLoc, OptNodeId, Range, SourceBuffer, SourceToken,
};

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
/// ([`AstBuilder::finish`](crate::AstBuilder::finish)), the
/// [`Ast::children`] iterator, and `murphy-plugin-api`'s `Cx::children`.
/// The `match` is exhaustive on purpose: a new `NodeKind` variant will not
/// compile until it is handled here.
pub fn collect_children(kind: &NodeKind, lists: &[NodeId], out: &mut Vec<NodeId>) {
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
        | NodeKind::Blockarg(_)
        | NodeKind::Zsuper => {}

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

        NodeKind::BlockPass(o)
        | NodeKind::Splat(o)
        | NodeKind::Return(o)
        | NodeKind::Kwsplat(o)
        | NodeKind::Break(o)
        | NodeKind::Next(o) => push_opt(out, o),

        NodeKind::Array(l)
        | NodeKind::Hash(l)
        | NodeKind::Begin(l)
        | NodeKind::Args(l)
        | NodeKind::Yield(l)
        | NodeKind::Super(l)
        | NodeKind::Dstr(l)
        | NodeKind::Dsym(l)
        | NodeKind::Xstr(l)
        | NodeKind::Mlhs(l) => push_list(out, lists, l),

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

        NodeKind::Def {
            receiver,
            args,
            body,
            ..
        } => {
            push_opt(out, receiver);
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

        NodeKind::While { cond, body, .. } | NodeKind::Until { cond, body, .. } => {
            out.push(cond);
            push_opt(out, body);
        }

        NodeKind::RangeExpr { begin_, end_, .. } => {
            push_opt(out, begin_);
            push_opt(out, end_);
        }

        NodeKind::Sclass { expr, body } => {
            out.push(expr);
            push_opt(out, body);
        }

        NodeKind::Defined(n) => out.push(n),

        NodeKind::Rescue {
            body,
            resbodies,
            else_,
        } => {
            push_opt(out, body);
            push_list(out, lists, resbodies);
            push_opt(out, else_);
        }

        NodeKind::Resbody {
            exceptions,
            var,
            body,
        } => {
            push_list(out, lists, exceptions);
            push_opt(out, var);
            push_opt(out, body);
        }

        NodeKind::Ensure { body, ensure_ } => {
            push_opt(out, body);
            push_opt(out, ensure_);
        }

        NodeKind::OpAsgn { target, value, .. } => {
            out.push(target);
            out.push(value);
        }

        NodeKind::OrAsgn { target, value } | NodeKind::AndAsgn { target, value } => {
            out.push(target);
            out.push(value);
        }

        NodeKind::Regexp { parts, .. } => push_list(out, lists, parts),

        NodeKind::Masgn { lhs, rhs } => {
            out.push(lhs);
            out.push(rhs);
        }

        // ── murphy-w5ba HIGH-priority extensions ────────────────────────
        NodeKind::For { var, iter, body } => {
            out.push(var);
            out.push(iter);
            push_opt(out, body);
        }

        NodeKind::Lambda | NodeKind::Cbase | NodeKind::Retry | NodeKind::Redo => {}

        NodeKind::Defs {
            receiver,
            args,
            body,
            ..
        } => {
            out.push(receiver);
            out.push(args);
            push_opt(out, body);
        }

        NodeKind::Index { receiver, args } => {
            out.push(receiver);
            push_list(out, lists, args);
        }

        NodeKind::IndexAsgn {
            receiver,
            args,
            value,
        } => {
            out.push(receiver);
            push_list(out, lists, args);
            out.push(value);
        }

        NodeKind::Kwbegin(l) | NodeKind::Procarg0(l) => {
            push_list(out, lists, l);
        }

        NodeKind::Rational(_) | NodeKind::Complex(_) | NodeKind::Regopt(_) => {}

        NodeKind::Not(n) => out.push(n),

        NodeKind::Numblock { send, body, .. } => {
            out.push(send);
            push_opt(out, body);
        }

        NodeKind::ForwardArgs | NodeKind::ForwardedArgs => {}

        // ── murphy-o57f MID-priority extensions ─────────────────────────
        NodeKind::CaseMatch {
            subject,
            in_patterns,
            else_body,
        } => {
            out.push(subject);
            push_list(out, lists, in_patterns);
            push_opt(out, else_body);
        }

        NodeKind::InPattern {
            pattern,
            guard,
            body,
        } => {
            out.push(pattern);
            push_opt(out, guard);
            push_opt(out, body);
        }

        NodeKind::ArrayPattern(l) | NodeKind::HashPattern(l) | NodeKind::FindPattern(l) => {
            push_list(out, lists, l)
        }

        NodeKind::MatchVar(_) => {}

        NodeKind::MatchAlt { left, right } => {
            out.push(left);
            out.push(right);
        }

        NodeKind::MatchRest(inner) => {
            push_opt(out, inner);
        }

        NodeKind::MatchNilPattern => {}

        NodeKind::ArrayPatternWithTail(l) => push_list(out, lists, l),

        NodeKind::MatchPatternP { value, pattern } | NodeKind::MatchPattern { value, pattern } => {
            out.push(value);
            out.push(pattern);
        }

        NodeKind::MatchWithLvasgn { call, targets } => {
            out.push(call);
            push_list(out, lists, targets);
        }

        NodeKind::MatchAs { value, name } => {
            out.push(value);
            out.push(name);
        }

        NodeKind::ConstPattern { const_, pattern } => {
            out.push(const_);
            out.push(pattern);
        }

        // murphy-j1j2 PM-E pin & guard
        NodeKind::Pin(inner) | NodeKind::IfGuard(inner) | NodeKind::UnlessGuard(inner) => {
            out.push(inner);
        }

        NodeKind::Itblock { send, body } => {
            out.push(send);
            push_opt(out, body);
        }

        // ── murphy-s4b4 LOW-priority extensions ─────────────────────────
        NodeKind::Alias { new_name, old_name } => {
            out.push(new_name);
            out.push(old_name);
        }

        NodeKind::Undef(l) => push_list(out, lists, l),

        NodeKind::Preexe(o) | NodeKind::Postexe(o) => push_opt(out, o),

        NodeKind::BackRef(_)
        | NodeKind::NthRef(_)
        | NodeKind::Shadowarg(_)
        | NodeKind::Kwnilarg
        | NodeKind::Blocknilarg => {}
    }
}

#[inline]
fn slot_phantom(out: &mut Vec<Option<NodeId>>) {
    out.push(None);
}

#[inline]
fn slot_opt(out: &mut Vec<Option<NodeId>>, o: OptNodeId) {
    out.push(o.get());
}

#[inline]
fn slot_node(out: &mut Vec<Option<NodeId>>, id: NodeId) {
    out.push(Some(id));
}

#[inline]
fn slot_list(out: &mut Vec<Option<NodeId>>, lists: &[NodeId], l: NodeList) {
    let start = l.start as usize;
    out.extend(
        lists[start..start + l.len as usize]
            .iter()
            .map(|&id| Some(id)),
    );
}

/// Append every parser-gem **child slot** of `kind`, in source order, to
/// `out` — one entry per slot, `Some(child)` for node children present in the
/// AST and `None` for slots that hold no returnable [`NodeId`].
///
/// Unlike [`collect_children`] (which packs only the node children, skipping
/// `None`/non-node positions), this reconstructs RuboCop/parser-gem's *full*
/// `node.children` array so that `Node#sibling_index` / `#left_sibling` /
/// `#right_sibling` match parser-gem exactly. Two kinds of slot are filled
/// with `None`:
///
/// * **phantom slots** — non-node values parser-gem stores as children:
///   selector symbols (`send`/`csend`), the def/casgn/const name symbol, the
///   `op_asgn` operator symbol, the `numblock` count integer, etc.
/// * **absent nil slots** — optional node children that are absent in this
///   parse (`X = 1` keeps `Casgn`'s `scope` slot even though it is `nil`).
///
/// The load-bearing rule: a nilable *node* slot always occupies its position
/// whether or not it is present, so `X = 1`'s value lands at slot 2, not 0.
///
/// `Def` is two parser-gem nodes collapsed into one variant: with a receiver
/// it is parser-gem `defs` (`[definee, :name, args, body]`); without, it is
/// `def` (`[:name, args, body]`). The arm branches on receiver presence so the
/// indices stay faithful for ported cops, which reason in parser-gem terms.
///
/// The `match` is exhaustive on purpose: a new `NodeKind` variant will not
/// compile until its slot layout is declared here, exactly like
/// [`collect_children`].
///
/// **Known limitation — `Resbody`.** parser-gem wraps a `resbody`'s exception
/// classes in a single `array` child (`(resbody (array …) var body)`, always
/// three slots). Murphy stores them flattened in the `exceptions` `NodeList`,
/// so when exception classes are present the trailing `var`/`body` slots are
/// shifted relative to parser-gem and `sibling_index` diverges there. This is
/// pre-existing (`collect_children` flattens identically); faithful indexing
/// for `resbody` would need an AST/translate change. `when` conds, by
/// contrast, *are* flattened in parser-gem too, so those stay faithful.
pub fn slot_layout(kind: &NodeKind, lists: &[NodeId], out: &mut Vec<Option<NodeId>>) {
    match *kind {
        // Leaves with no children at all (no slots in parser-gem either).
        NodeKind::Error
        | NodeKind::Nil
        | NodeKind::True_
        | NodeKind::False_
        | NodeKind::SelfExpr
        | NodeKind::Unknown
        | NodeKind::Zsuper
        | NodeKind::Lambda
        | NodeKind::Cbase
        | NodeKind::Retry
        | NodeKind::Redo
        | NodeKind::ForwardArgs
        | NodeKind::ForwardedArgs
        | NodeKind::Kwnilarg
        | NodeKind::Blocknilarg
        | NodeKind::MatchNilPattern => {}

        // Leaves whose single parser-gem child is a non-node scalar
        // (literal value / name symbol). They never parent a node, but the
        // phantom slot is declared for faithfulness.
        NodeKind::Int(_)
        | NodeKind::Float(_)
        | NodeKind::Str(_)
        | NodeKind::Sym(_)
        | NodeKind::Lvar(_)
        | NodeKind::Ivar(_)
        | NodeKind::Cvar(_)
        | NodeKind::Gvar(_)
        | NodeKind::Arg(_)
        | NodeKind::Restarg(_)
        | NodeKind::Kwarg(_)
        | NodeKind::Kwrestarg(_)
        | NodeKind::Blockarg(_)
        | NodeKind::Rational(_)
        | NodeKind::Complex(_)
        | NodeKind::Regopt(_)
        | NodeKind::MatchVar(_)
        | NodeKind::BackRef(_)
        | NodeKind::NthRef(_)
        | NodeKind::Shadowarg(_) => slot_phantom(out),

        // `(const scope :name)` — scope slot then name phantom.
        NodeKind::Const { scope, .. } => {
            slot_opt(out, scope);
            slot_phantom(out);
        }

        // `(lvasgn :name value)` — name phantom then value slot.
        NodeKind::Lvasgn { value, .. }
        | NodeKind::Ivasgn { value, .. }
        | NodeKind::Gvasgn { value, .. }
        | NodeKind::Cvasgn { value, .. } => {
            slot_phantom(out);
            slot_opt(out, value);
        }

        // `(casgn scope :name value)`.
        NodeKind::Casgn { scope, value, .. } => {
            slot_opt(out, scope);
            slot_phantom(out);
            slot_opt(out, value);
        }

        // `(send receiver :selector *args)` / `(csend ...)`.
        NodeKind::Send { receiver, args, .. } => {
            slot_opt(out, receiver);
            slot_phantom(out);
            slot_list(out, lists, args);
        }
        NodeKind::Csend { receiver, args, .. } => {
            slot_node(out, receiver);
            slot_phantom(out);
            slot_list(out, lists, args);
        }

        // `(block call args body)` — no phantom; all node slots.
        NodeKind::Block { call, args, body } => {
            slot_node(out, call);
            slot_node(out, args);
            slot_opt(out, body);
        }

        NodeKind::BlockPass(o)
        | NodeKind::Splat(o)
        | NodeKind::Return(o)
        | NodeKind::Kwsplat(o)
        | NodeKind::Break(o)
        | NodeKind::Next(o) => slot_opt(out, o),

        NodeKind::Array(l)
        | NodeKind::Hash(l)
        | NodeKind::Begin(l)
        | NodeKind::Args(l)
        | NodeKind::Yield(l)
        | NodeKind::Super(l)
        | NodeKind::Dstr(l)
        | NodeKind::Dsym(l)
        | NodeKind::Xstr(l)
        | NodeKind::Mlhs(l) => slot_list(out, lists, l),

        NodeKind::Pair { key, value } => {
            slot_node(out, key);
            slot_node(out, value);
        }

        NodeKind::If { cond, then_, else_ } => {
            slot_node(out, cond);
            slot_opt(out, then_);
            slot_opt(out, else_);
        }

        NodeKind::Case {
            subject,
            whens,
            else_,
        } => {
            slot_opt(out, subject);
            slot_list(out, lists, whens);
            slot_opt(out, else_);
        }

        NodeKind::When { conds, body } => {
            slot_list(out, lists, conds);
            slot_opt(out, body);
        }

        NodeKind::And { lhs, rhs } | NodeKind::Or { lhs, rhs } => {
            slot_node(out, lhs);
            slot_node(out, rhs);
        }

        // `Def` with no receiver -> parser-gem `def`: `[:name, args, body]`.
        // `Def` with a receiver -> parser-gem `defs`: `[definee, :name, args, body]`.
        NodeKind::Def {
            receiver,
            args,
            body,
            ..
        } => {
            if let Some(definee) = receiver.get() {
                slot_node(out, definee);
            }
            slot_phantom(out);
            slot_node(out, args);
            slot_opt(out, body);
        }

        // `(defs definee :name args body)`.
        NodeKind::Defs {
            receiver,
            args,
            body,
            ..
        } => {
            slot_node(out, receiver);
            slot_phantom(out);
            slot_node(out, args);
            slot_opt(out, body);
        }

        NodeKind::Class {
            name,
            superclass,
            body,
        } => {
            slot_node(out, name);
            slot_opt(out, superclass);
            slot_opt(out, body);
        }

        NodeKind::Module { name, body } => {
            slot_node(out, name);
            slot_opt(out, body);
        }

        // `(optarg :name default)` / `(kwoptarg :name default)`.
        NodeKind::Optarg { default, .. } | NodeKind::Kwoptarg { default, .. } => {
            slot_phantom(out);
            slot_node(out, default);
        }

        NodeKind::While { cond, body, .. } | NodeKind::Until { cond, body, .. } => {
            slot_node(out, cond);
            slot_opt(out, body);
        }

        NodeKind::RangeExpr { begin_, end_, .. } => {
            slot_opt(out, begin_);
            slot_opt(out, end_);
        }

        NodeKind::Sclass { expr, body } => {
            slot_node(out, expr);
            slot_opt(out, body);
        }

        NodeKind::Defined(n) => slot_node(out, n),

        NodeKind::Rescue {
            body,
            resbodies,
            else_,
        } => {
            slot_opt(out, body);
            slot_list(out, lists, resbodies);
            slot_opt(out, else_);
        }

        NodeKind::Resbody {
            exceptions,
            var,
            body,
        } => {
            slot_list(out, lists, exceptions);
            slot_opt(out, var);
            slot_opt(out, body);
        }

        NodeKind::Ensure { body, ensure_ } => {
            slot_opt(out, body);
            slot_opt(out, ensure_);
        }

        // `(op_asgn target :op value)` — operator symbol phantom at slot 1.
        NodeKind::OpAsgn { target, value, .. } => {
            slot_node(out, target);
            slot_phantom(out);
            slot_node(out, value);
        }

        NodeKind::OrAsgn { target, value } | NodeKind::AndAsgn { target, value } => {
            slot_node(out, target);
            slot_node(out, value);
        }

        NodeKind::Regexp { parts, .. } => slot_list(out, lists, parts),

        NodeKind::Masgn { lhs, rhs } => {
            slot_node(out, lhs);
            slot_node(out, rhs);
        }

        // `(for var iter body)` — no phantom; var at 0, body at 2.
        NodeKind::For { var, iter, body } => {
            slot_node(out, var);
            slot_node(out, iter);
            slot_opt(out, body);
        }

        NodeKind::Index { receiver, args } => {
            slot_node(out, receiver);
            slot_list(out, lists, args);
        }

        NodeKind::IndexAsgn {
            receiver,
            args,
            value,
        } => {
            slot_node(out, receiver);
            slot_list(out, lists, args);
            slot_node(out, value);
        }

        NodeKind::Kwbegin(l) | NodeKind::Procarg0(l) => slot_list(out, lists, l),

        NodeKind::Not(n) => slot_node(out, n),

        // `(numblock call count body)` — the `count` integer is a phantom slot.
        NodeKind::Numblock { send, body, .. } => {
            slot_node(out, send);
            slot_phantom(out);
            slot_opt(out, body);
        }

        NodeKind::CaseMatch {
            subject,
            in_patterns,
            else_body,
        } => {
            slot_node(out, subject);
            slot_list(out, lists, in_patterns);
            slot_opt(out, else_body);
        }

        NodeKind::InPattern {
            pattern,
            guard,
            body,
        } => {
            slot_node(out, pattern);
            slot_opt(out, guard);
            slot_opt(out, body);
        }

        NodeKind::ArrayPattern(l) | NodeKind::HashPattern(l) | NodeKind::FindPattern(l) => {
            slot_list(out, lists, l)
        }

        NodeKind::MatchAlt { left, right } => {
            slot_node(out, left);
            slot_node(out, right);
        }

        NodeKind::MatchRest(inner) => {
            slot_opt(out, inner);
        }

        NodeKind::ArrayPatternWithTail(l) => slot_list(out, lists, l),

        NodeKind::MatchPatternP { value, pattern } | NodeKind::MatchPattern { value, pattern } => {
            slot_node(out, value);
            slot_node(out, pattern);
        }

        NodeKind::MatchWithLvasgn { call, targets } => {
            slot_node(out, call);
            slot_list(out, lists, targets);
        }

        NodeKind::MatchAs { value, name } => {
            slot_node(out, value);
            slot_node(out, name);
        }

        NodeKind::ConstPattern { const_, pattern } => {
            slot_node(out, const_);
            slot_node(out, pattern);
        }

        // murphy-j1j2 PM-E pin & guard
        NodeKind::Pin(inner) | NodeKind::IfGuard(inner) | NodeKind::UnlessGuard(inner) => {
            slot_node(out, inner);
        }

        // `(itblock call :it body)` — `:it` marker is a phantom slot.
        NodeKind::Itblock { send, body } => {
            slot_node(out, send);
            slot_phantom(out);
            slot_opt(out, body);
        }

        NodeKind::Alias { new_name, old_name } => {
            slot_node(out, new_name);
            slot_node(out, old_name);
        }

        NodeKind::Undef(l) => slot_list(out, lists, l),

        NodeKind::Preexe(o) | NodeKind::Postexe(o) => slot_opt(out, o),
    }
}

/// An owned, flat, parser-shaped, typed AST for one file. See ADR 0037.
#[derive(Debug, Clone, PartialEq)]
pub struct Ast {
    pub(crate) nodes: Vec<AstNode>,
    pub(crate) node_lists: Vec<NodeId>,
    pub(crate) interner: Interner,
    pub(crate) comments: Vec<Comment>,
    pub(crate) magic_comments: Vec<MagicComment>,
    pub(crate) source_tokens: Vec<SourceToken>,
    pub(crate) call_closing_locs: Vec<CallClosingLoc>,
    pub(crate) call_operator_locs: Vec<CallOperatorLoc>,
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

    /// The source range of the node at `id` — shorthand for
    /// `self.loc(id).expression`.
    pub fn range(&self, id: NodeId) -> Range {
        self.nodes[id.0 as usize].loc.expression
    }

    /// The `node.loc` bundle for `id` — Murphy's analog of the parser
    /// gem's `node.loc` accessor. `expression` is the AST node's full
    /// source range; `name` is the identifier range (the
    /// `node.loc.name` analog), [`Range::ZERO`] for nodes without
    /// an identifier.
    pub fn loc(&self, id: NodeId) -> NodeLoc {
        self.nodes[id.0 as usize].loc
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

    /// The parser-gem child **slots** of `id`, in source order — see
    /// [`slot_layout`]. Each slot is `Some(child)` for a present node child or
    /// `None` for a phantom/absent slot, so positions match RuboCop's
    /// `node.children` (and thus `Node#sibling_index`).
    pub fn slot_layout(&self, id: NodeId) -> Vec<Option<NodeId>> {
        let mut out = Vec::new();
        slot_layout(self.kind(id), &self.node_lists, &mut out);
        out
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

    /// Overwrite the file path. Useful after [`Ast::from_bytes`] /
    /// [`murphy_cache`](https://docs.rs/murphy-cache) lookup, when the
    /// arena was originally cached under a different filename: the source
    /// text is content-addressed, but the path stays meta-data and the
    /// caller may have a more useful name to attach.
    pub fn set_source_path(&mut self, path: std::path::PathBuf) {
        self.source.path = path;
    }

    /// The source text covered by `range`.
    pub fn raw_source(&self, range: Range) -> &str {
        &self.source.text[range.start as usize..range.end as usize]
    }

    /// The comments, in source order.
    pub fn comments(&self) -> &[Comment] {
        &self.comments
    }

    /// The structured magic comments, in source order.
    pub fn magic_comments(&self) -> &[MagicComment] {
        &self.magic_comments
    }

    /// The source tokens, in source order.
    pub fn sorted_tokens(&self) -> &[SourceToken] {
        &self.source_tokens
    }

    /// Sparse parser-provided closing parens for call nodes.
    pub fn call_closing_locs(&self) -> &[CallClosingLoc] {
        &self.call_closing_locs
    }

    /// Parser-provided call operator ranges, sorted by node id.
    pub fn call_operator_locs(&self) -> &[CallOperatorLoc] {
        &self.call_operator_locs
    }

    /// The string interner.
    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    /// The arena's backing slices as a borrowed, flat view.
    ///
    /// Exposes the otherwise-`pub(crate)` storage (`nodes`, `node_lists`,
    /// the interner blob/offsets, `comments`, `source`) so a consumer can
    /// build a `#[repr(C)]` pointer/length bundle over it — notably
    /// `murphy-plugin-api`'s `CxRaw` (ADR 0038). Strictly a view: the
    /// returned slices borrow `self` and own nothing.
    pub fn raw_parts(&self) -> AstRawParts<'_> {
        AstRawParts {
            nodes: &self.nodes,
            node_lists: &self.node_lists,
            interner_blob: &self.interner.blob,
            interner_offsets: &self.interner.offsets,
            comments: &self.comments,
            magic_comments: &self.magic_comments,
            sorted_tokens: &self.source_tokens,
            call_closing_locs: &self.call_closing_locs,
            call_operator_locs: &self.call_operator_locs,
            source: &self.source.text,
            root: self.root,
        }
    }
}

/// A borrowed, flat view of an [`Ast`]'s backing storage. See
/// [`Ast::raw_parts`]. Owns nothing; every field borrows the source `Ast`.
#[derive(Debug, Clone, Copy)]
pub struct AstRawParts<'a> {
    /// The arena node array.
    pub nodes: &'a [AstNode],
    /// The `node_lists` side table (variable-length children).
    pub node_lists: &'a [NodeId],
    /// The interner's flat byte blob.
    pub interner_blob: &'a [u8],
    /// The interner's per-entry offsets, indexed by `Symbol`/`StringId`.
    pub interner_offsets: &'a [Range],
    /// The source comments, in source order.
    pub comments: &'a [Comment],
    /// The structured magic comments, in source order.
    pub magic_comments: &'a [MagicComment],
    /// The source tokens, in source order.
    pub sorted_tokens: &'a [SourceToken],
    /// Sparse parser-provided closing parens for call nodes.
    pub call_closing_locs: &'a [CallClosingLoc],
    /// Parser-provided call operator ranges, sorted by node id.
    pub call_operator_locs: &'a [CallOperatorLoc],
    /// The full source text (UTF-8).
    pub source: &'a str,
    /// The arena root node.
    pub root: NodeId,
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

    #[test]
    fn raw_parts_borrows_the_arena_storage() {
        use crate::builder::AstBuilder;

        // `x = 1` interns the symbol `x`; an inline comment exercises the
        // `comments` slice.
        let mut b = AstBuilder::new("x = 1 # c", "t.rb");
        let int = b.push(NodeKind::Int(1), Range { start: 4, end: 5 });
        let x = b.intern_symbol("x");
        let asgn = b.push(
            NodeKind::Lvasgn {
                name: x,
                value: OptNodeId::some(int),
            },
            Range { start: 0, end: 5 },
        );
        b.add_comment(Range { start: 6, end: 9 }, crate::node::CommentKind::Inline);
        let ast = b.finish(asgn);

        let p = ast.raw_parts();
        assert_eq!(p.nodes.len(), ast.len());
        assert_eq!(p.source, ast.source());
        assert_eq!(p.root, ast.root());
        assert_eq!(p.comments, ast.comments());
        // The interner view resolves the same string as `Interner::resolve`.
        assert_eq!(p.interner_offsets.len(), ast.interner().len());
        let r = p.interner_offsets[x.0 as usize];
        assert_eq!(&p.interner_blob[r.start as usize..r.end as usize], b"x");
    }

    #[test]
    fn sorted_tokens_and_raw_parts_borrow_the_arena_storage() {
        use crate::builder::AstBuilder;
        use crate::node::{SourceToken, SourceTokenKind};

        let mut b = AstBuilder::new("foo(1)", "t.rb");
        let one = b.push(NodeKind::Int(1), Range { start: 4, end: 5 });
        b.add_source_token(SourceToken {
            kind: SourceTokenKind::LeftParen,
            range: Range { start: 3, end: 4 },
        });
        b.add_source_token(SourceToken {
            kind: SourceTokenKind::RightParen,
            range: Range { start: 5, end: 6 },
        });
        let ast = b.finish(one);

        assert_eq!(
            ast.sorted_tokens(),
            &[
                SourceToken {
                    kind: SourceTokenKind::LeftParen,
                    range: Range { start: 3, end: 4 },
                },
                SourceToken {
                    kind: SourceTokenKind::RightParen,
                    range: Range { start: 5, end: 6 },
                },
            ]
        );
        assert_eq!(ast.raw_parts().sorted_tokens, ast.sorted_tokens());
    }

    // ── slot_layout: parser-gem child-slot reconstruction ──────────────

    fn slots(kind: NodeKind, lists: &[NodeId]) -> Vec<Option<NodeId>> {
        let mut out = Vec::new();
        slot_layout(&kind, lists, &mut out);
        out
    }

    #[test]
    fn slot_layout_send_no_receiver_arg_at_slot_two() {
        // `foo(1)` → [recv:none, :selector phantom, arg]; the arg sits at slot 2.
        let lists = vec![NodeId(7)];
        let s = slots(
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method: Symbol(0),
                args: NodeList { start: 0, len: 1 },
            },
            &lists,
        );
        assert_eq!(s, vec![None, None, Some(NodeId(7))]);
    }

    #[test]
    fn slot_layout_send_with_receiver_keeps_selector_phantom() {
        // `recv.foo(a, b)` → [recv, :foo, a, b].
        let lists = vec![NodeId(8), NodeId(9)];
        let s = slots(
            NodeKind::Send {
                receiver: OptNodeId::some(NodeId(2)),
                method: Symbol(0),
                args: NodeList { start: 0, len: 2 },
            },
            &lists,
        );
        assert_eq!(
            s,
            vec![Some(NodeId(2)), None, Some(NodeId(8)), Some(NodeId(9))]
        );
    }

    #[test]
    fn slot_layout_casgn_value_at_slot_two_even_without_scope() {
        // `X = 1` → [scope:none, :name phantom, value]; value at slot 2.
        let s = slots(
            NodeKind::Casgn {
                scope: OptNodeId::NONE,
                name: Symbol(0),
                value: OptNodeId::some(NodeId(3)),
            },
            &[],
        );
        assert_eq!(s, vec![None, None, Some(NodeId(3))]);
    }

    #[test]
    fn slot_layout_op_asgn_operator_phantom_at_slot_one() {
        // `a += b` → [target, :op phantom, value]; value at slot 2.
        let s = slots(
            NodeKind::OpAsgn {
                target: NodeId(4),
                op: Symbol(0),
                value: NodeId(5),
            },
            &[],
        );
        assert_eq!(s, vec![Some(NodeId(4)), None, Some(NodeId(5))]);
    }

    #[test]
    fn slot_layout_lvasgn_value_at_slot_one() {
        // `x = v` → [:name phantom, value].
        let s = slots(
            NodeKind::Lvasgn {
                name: Symbol(0),
                value: OptNodeId::some(NodeId(6)),
            },
            &[],
        );
        assert_eq!(s, vec![None, Some(NodeId(6))]);
    }

    #[test]
    fn slot_layout_const_scope_then_name_phantom() {
        // `A::B` → [scope(A), :B phantom]; scope at slot 0.
        let s = slots(
            NodeKind::Const {
                scope: OptNodeId::some(NodeId(1)),
                name: Symbol(0),
            },
            &[],
        );
        assert_eq!(s, vec![Some(NodeId(1)), None]);
    }

    #[test]
    fn slot_layout_def_no_receiver_is_parser_def_shape() {
        // `def m; end` → parser `def`: [:name phantom, args, body].
        let s = slots(
            NodeKind::Def {
                receiver: OptNodeId::NONE,
                name: Symbol(0),
                args: NodeId(10),
                body: OptNodeId::NONE,
            },
            &[],
        );
        // args at slot 1, body slot 2 (absent → None).
        assert_eq!(s, vec![None, Some(NodeId(10)), None]);
    }

    #[test]
    fn slot_layout_def_with_receiver_is_parser_defs_shape() {
        // `def self.foo(a); x; end` → parser `defs`:
        // [definee, :name phantom, args, body].
        let s = slots(
            NodeKind::Def {
                receiver: OptNodeId::some(NodeId(11)),
                name: Symbol(0),
                args: NodeId(12),
                body: OptNodeId::some(NodeId(13)),
            },
            &[],
        );
        assert_eq!(
            s,
            vec![Some(NodeId(11)), None, Some(NodeId(12)), Some(NodeId(13))]
        );
    }

    #[test]
    fn slot_layout_numblock_count_phantom_at_slot_one() {
        // numblock → [call, count phantom, body].
        let s = slots(
            NodeKind::Numblock {
                send: NodeId(14),
                max_n: 2,
                body: OptNodeId::some(NodeId(15)),
            },
            &[],
        );
        assert_eq!(s, vec![Some(NodeId(14)), None, Some(NodeId(15))]);
    }

    #[test]
    fn slot_layout_for_has_no_phantom_body_at_slot_two() {
        // `for v in it; b; end` → [var, iter, body]; body at slot 2.
        let s = slots(
            NodeKind::For {
                var: NodeId(16),
                iter: NodeId(17),
                body: OptNodeId::some(NodeId(18)),
            },
            &[],
        );
        assert_eq!(
            s,
            vec![Some(NodeId(16)), Some(NodeId(17)), Some(NodeId(18))]
        );
    }

    #[test]
    fn slot_layout_if_cond_at_slot_zero() {
        // `if c then t else e end` → [cond, then, else]; cond slot 0.
        let s = slots(
            NodeKind::If {
                cond: NodeId(19),
                then_: OptNodeId::some(NodeId(20)),
                else_: OptNodeId::NONE,
            },
            &[],
        );
        assert_eq!(s, vec![Some(NodeId(19)), Some(NodeId(20)), None]);
    }
}
