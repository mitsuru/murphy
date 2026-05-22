//! Flat, field-by-field little-endian serialization of an [`Ast`].
//!
//! No magic/version header — that is murphy-9cr.26 (the binary cache). This
//! is sound (no `memcpy` of padded enums) and deterministic.

use crate::ast::Ast;
use crate::interner::Interner;
use crate::node::{
    AstNode, Comment, CommentKind, NodeId, NodeKind, NodeList, OptNodeId, Range, SourceBuffer,
    StringId, Symbol,
};

/// Serialization / deserialization failure.
#[derive(Debug)]
pub enum SerError {
    /// The buffer ended before a field could be fully read.
    UnexpectedEof,
    /// A discriminant byte did not name a known variant.
    BadDiscriminant,
    /// A byte section that must be UTF-8 was not.
    InvalidUtf8,
}

fn put_u8(out: &mut Vec<u8>, v: u8) {
    out.push(v);
}
fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_i64(out: &mut Vec<u8>, v: i64) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_f64(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn take<'a>(cur: &mut &'a [u8], n: usize) -> Result<&'a [u8], SerError> {
    if cur.len() < n {
        return Err(SerError::UnexpectedEof);
    }
    let (head, rest) = cur.split_at(n);
    *cur = rest;
    Ok(head)
}
fn get_u8(cur: &mut &[u8]) -> Result<u8, SerError> {
    Ok(take(cur, 1)?[0])
}
fn get_u32(cur: &mut &[u8]) -> Result<u32, SerError> {
    Ok(u32::from_le_bytes(take(cur, 4)?.try_into().unwrap()))
}
fn get_u64(cur: &mut &[u8]) -> Result<u64, SerError> {
    Ok(u64::from_le_bytes(take(cur, 8)?.try_into().unwrap()))
}
fn get_i64(cur: &mut &[u8]) -> Result<i64, SerError> {
    Ok(i64::from_le_bytes(take(cur, 8)?.try_into().unwrap()))
}
fn get_f64(cur: &mut &[u8]) -> Result<f64, SerError> {
    Ok(f64::from_le_bytes(take(cur, 8)?.try_into().unwrap()))
}

// --- NodeKind ---

/// Serialize one [`NodeKind`]: a `u8` discriminant (declaration order) then
/// the payload fields in declaration order. The `match` is exhaustive on
/// purpose — a new variant will not compile until it is handled here.
fn write_node_kind(k: &NodeKind, out: &mut Vec<u8>) {
    match *k {
        NodeKind::Error => put_u8(out, 0),
        NodeKind::Nil => put_u8(out, 1),
        NodeKind::True_ => put_u8(out, 2),
        NodeKind::False_ => put_u8(out, 3),
        NodeKind::SelfExpr => put_u8(out, 4),
        NodeKind::Int(v) => {
            put_u8(out, 5);
            put_i64(out, v);
        }
        NodeKind::Float(v) => {
            put_u8(out, 6);
            put_f64(out, v);
        }
        NodeKind::Str(s) => {
            put_u8(out, 7);
            put_u32(out, s.0);
        }
        NodeKind::Sym(s) => {
            put_u8(out, 8);
            put_u32(out, s.0);
        }
        NodeKind::Lvar(s) => {
            put_u8(out, 9);
            put_u32(out, s.0);
        }
        NodeKind::Ivar(s) => {
            put_u8(out, 10);
            put_u32(out, s.0);
        }
        NodeKind::Cvar(s) => {
            put_u8(out, 11);
            put_u32(out, s.0);
        }
        NodeKind::Gvar(s) => {
            put_u8(out, 12);
            put_u32(out, s.0);
        }
        NodeKind::Const { scope, name } => {
            put_u8(out, 13);
            put_u32(out, scope.0);
            put_u32(out, name.0);
        }
        NodeKind::Lvasgn { name, value } => {
            put_u8(out, 14);
            put_u32(out, name.0);
            put_u32(out, value.0);
        }
        NodeKind::Ivasgn { name, value } => {
            put_u8(out, 15);
            put_u32(out, name.0);
            put_u32(out, value.0);
        }
        NodeKind::Casgn { scope, name, value } => {
            put_u8(out, 16);
            put_u32(out, scope.0);
            put_u32(out, name.0);
            put_u32(out, value.0);
        }
        NodeKind::Send {
            receiver,
            method,
            args,
        } => {
            put_u8(out, 17);
            put_u32(out, receiver.0);
            put_u32(out, method.0);
            write_node_list(args, out);
        }
        NodeKind::Csend {
            receiver,
            method,
            args,
        } => {
            put_u8(out, 18);
            put_u32(out, receiver.0);
            put_u32(out, method.0);
            write_node_list(args, out);
        }
        NodeKind::Block { call, args, body } => {
            put_u8(out, 19);
            put_u32(out, call.0);
            put_u32(out, args.0);
            put_u32(out, body.0);
        }
        NodeKind::BlockPass(o) => {
            put_u8(out, 20);
            put_u32(out, o.0);
        }
        NodeKind::Splat(o) => {
            put_u8(out, 21);
            put_u32(out, o.0);
        }
        NodeKind::Array(l) => {
            put_u8(out, 22);
            write_node_list(l, out);
        }
        NodeKind::Hash(l) => {
            put_u8(out, 23);
            write_node_list(l, out);
        }
        NodeKind::Pair { key, value } => {
            put_u8(out, 24);
            put_u32(out, key.0);
            put_u32(out, value.0);
        }
        NodeKind::If { cond, then_, else_ } => {
            put_u8(out, 25);
            put_u32(out, cond.0);
            put_u32(out, then_.0);
            put_u32(out, else_.0);
        }
        NodeKind::Case {
            subject,
            whens,
            else_,
        } => {
            put_u8(out, 26);
            put_u32(out, subject.0);
            write_node_list(whens, out);
            put_u32(out, else_.0);
        }
        NodeKind::When { conds, body } => {
            put_u8(out, 27);
            write_node_list(conds, out);
            put_u32(out, body.0);
        }
        NodeKind::Begin(l) => {
            put_u8(out, 28);
            write_node_list(l, out);
        }
        NodeKind::Return(o) => {
            put_u8(out, 29);
            put_u32(out, o.0);
        }
        NodeKind::And { lhs, rhs } => {
            put_u8(out, 30);
            put_u32(out, lhs.0);
            put_u32(out, rhs.0);
        }
        NodeKind::Or { lhs, rhs } => {
            put_u8(out, 31);
            put_u32(out, lhs.0);
            put_u32(out, rhs.0);
        }
        NodeKind::Def {
            receiver,
            name,
            args,
            body,
        } => {
            put_u8(out, 32);
            put_u32(out, receiver.0);
            put_u32(out, name.0);
            put_u32(out, args.0);
            put_u32(out, body.0);
        }
        NodeKind::Class {
            name,
            superclass,
            body,
        } => {
            put_u8(out, 33);
            put_u32(out, name.0);
            put_u32(out, superclass.0);
            put_u32(out, body.0);
        }
        NodeKind::Module { name, body } => {
            put_u8(out, 34);
            put_u32(out, name.0);
            put_u32(out, body.0);
        }
        NodeKind::Args(l) => {
            put_u8(out, 35);
            write_node_list(l, out);
        }
        NodeKind::Arg(s) => {
            put_u8(out, 36);
            put_u32(out, s.0);
        }
        NodeKind::Unknown => put_u8(out, 37),
        NodeKind::Gvasgn { name, value } => {
            put_u8(out, 38);
            put_u32(out, name.0);
            put_u32(out, value.0);
        }
        NodeKind::Cvasgn { name, value } => {
            put_u8(out, 39);
            put_u32(out, name.0);
            put_u32(out, value.0);
        }
        NodeKind::Optarg { name, default } => {
            put_u8(out, 40);
            put_u32(out, name.0);
            put_u32(out, default.0);
        }
        NodeKind::Restarg(s) => {
            put_u8(out, 41);
            put_u32(out, s.0);
        }
        NodeKind::Kwarg(s) => {
            put_u8(out, 42);
            put_u32(out, s.0);
        }
        NodeKind::Kwoptarg { name, default } => {
            put_u8(out, 43);
            put_u32(out, name.0);
            put_u32(out, default.0);
        }
        NodeKind::Kwrestarg(s) => {
            put_u8(out, 44);
            put_u32(out, s.0);
        }
        NodeKind::Blockarg(s) => {
            put_u8(out, 45);
            put_u32(out, s.0);
        }
        NodeKind::Kwsplat(o) => {
            put_u8(out, 46);
            put_u32(out, o.0);
        }
        NodeKind::While { cond, body, post } => {
            put_u8(out, 47);
            put_u32(out, cond.0);
            put_u32(out, body.0);
            put_u8(out, post as u8);
        }
        NodeKind::Until { cond, body, post } => {
            put_u8(out, 48);
            put_u32(out, cond.0);
            put_u32(out, body.0);
            put_u8(out, post as u8);
        }
        NodeKind::RangeExpr {
            begin_,
            end_,
            exclusive,
        } => {
            put_u8(out, 49);
            put_u32(out, begin_.0);
            put_u32(out, end_.0);
            put_u8(out, exclusive as u8);
        }
        NodeKind::Sclass { expr, body } => {
            put_u8(out, 50);
            put_u32(out, expr.0);
            put_u32(out, body.0);
        }
        NodeKind::Break(o) => {
            put_u8(out, 51);
            put_u32(out, o.0);
        }
        NodeKind::Next(o) => {
            put_u8(out, 52);
            put_u32(out, o.0);
        }
        NodeKind::Yield(l) => {
            put_u8(out, 53);
            write_node_list(l, out);
        }
        NodeKind::Super(l) => {
            put_u8(out, 54);
            write_node_list(l, out);
        }
        NodeKind::Zsuper => put_u8(out, 55),
        NodeKind::Defined(n) => {
            put_u8(out, 56);
            put_u32(out, n.0);
        }
        NodeKind::Rescue {
            body,
            resbodies,
            else_,
        } => {
            put_u8(out, 57);
            put_u32(out, body.0);
            write_node_list(resbodies, out);
            put_u32(out, else_.0);
        }
        NodeKind::Resbody {
            exceptions,
            var,
            body,
        } => {
            put_u8(out, 58);
            write_node_list(exceptions, out);
            put_u32(out, var.0);
            put_u32(out, body.0);
        }
        NodeKind::Ensure { body, ensure_ } => {
            put_u8(out, 59);
            put_u32(out, body.0);
            put_u32(out, ensure_.0);
        }
    }
}

/// Deserialize one [`NodeKind`]. Mirror of [`write_node_kind`].
fn read_node_kind(cur: &mut &[u8]) -> Result<NodeKind, SerError> {
    Ok(match get_u8(cur)? {
        0 => NodeKind::Error,
        1 => NodeKind::Nil,
        2 => NodeKind::True_,
        3 => NodeKind::False_,
        4 => NodeKind::SelfExpr,
        5 => NodeKind::Int(get_i64(cur)?),
        6 => NodeKind::Float(get_f64(cur)?),
        7 => NodeKind::Str(StringId(get_u32(cur)?)),
        8 => NodeKind::Sym(Symbol(get_u32(cur)?)),
        9 => NodeKind::Lvar(Symbol(get_u32(cur)?)),
        10 => NodeKind::Ivar(Symbol(get_u32(cur)?)),
        11 => NodeKind::Cvar(Symbol(get_u32(cur)?)),
        12 => NodeKind::Gvar(Symbol(get_u32(cur)?)),
        13 => NodeKind::Const {
            scope: OptNodeId(get_u32(cur)?),
            name: Symbol(get_u32(cur)?),
        },
        14 => NodeKind::Lvasgn {
            name: Symbol(get_u32(cur)?),
            value: OptNodeId(get_u32(cur)?),
        },
        15 => NodeKind::Ivasgn {
            name: Symbol(get_u32(cur)?),
            value: OptNodeId(get_u32(cur)?),
        },
        16 => NodeKind::Casgn {
            scope: OptNodeId(get_u32(cur)?),
            name: Symbol(get_u32(cur)?),
            value: OptNodeId(get_u32(cur)?),
        },
        17 => NodeKind::Send {
            receiver: OptNodeId(get_u32(cur)?),
            method: Symbol(get_u32(cur)?),
            args: read_node_list(cur)?,
        },
        18 => NodeKind::Csend {
            receiver: NodeId(get_u32(cur)?),
            method: Symbol(get_u32(cur)?),
            args: read_node_list(cur)?,
        },
        19 => NodeKind::Block {
            call: NodeId(get_u32(cur)?),
            args: NodeId(get_u32(cur)?),
            body: OptNodeId(get_u32(cur)?),
        },
        20 => NodeKind::BlockPass(OptNodeId(get_u32(cur)?)),
        21 => NodeKind::Splat(OptNodeId(get_u32(cur)?)),
        22 => NodeKind::Array(read_node_list(cur)?),
        23 => NodeKind::Hash(read_node_list(cur)?),
        24 => NodeKind::Pair {
            key: NodeId(get_u32(cur)?),
            value: NodeId(get_u32(cur)?),
        },
        25 => NodeKind::If {
            cond: NodeId(get_u32(cur)?),
            then_: OptNodeId(get_u32(cur)?),
            else_: OptNodeId(get_u32(cur)?),
        },
        26 => NodeKind::Case {
            subject: OptNodeId(get_u32(cur)?),
            whens: read_node_list(cur)?,
            else_: OptNodeId(get_u32(cur)?),
        },
        27 => NodeKind::When {
            conds: read_node_list(cur)?,
            body: OptNodeId(get_u32(cur)?),
        },
        28 => NodeKind::Begin(read_node_list(cur)?),
        29 => NodeKind::Return(OptNodeId(get_u32(cur)?)),
        30 => NodeKind::And {
            lhs: NodeId(get_u32(cur)?),
            rhs: NodeId(get_u32(cur)?),
        },
        31 => NodeKind::Or {
            lhs: NodeId(get_u32(cur)?),
            rhs: NodeId(get_u32(cur)?),
        },
        32 => NodeKind::Def {
            receiver: OptNodeId(get_u32(cur)?),
            name: Symbol(get_u32(cur)?),
            args: NodeId(get_u32(cur)?),
            body: OptNodeId(get_u32(cur)?),
        },
        33 => NodeKind::Class {
            name: NodeId(get_u32(cur)?),
            superclass: OptNodeId(get_u32(cur)?),
            body: OptNodeId(get_u32(cur)?),
        },
        34 => NodeKind::Module {
            name: NodeId(get_u32(cur)?),
            body: OptNodeId(get_u32(cur)?),
        },
        35 => NodeKind::Args(read_node_list(cur)?),
        36 => NodeKind::Arg(Symbol(get_u32(cur)?)),
        37 => NodeKind::Unknown,
        38 => NodeKind::Gvasgn {
            name: Symbol(get_u32(cur)?),
            value: OptNodeId(get_u32(cur)?),
        },
        39 => NodeKind::Cvasgn {
            name: Symbol(get_u32(cur)?),
            value: OptNodeId(get_u32(cur)?),
        },
        40 => NodeKind::Optarg {
            name: Symbol(get_u32(cur)?),
            default: NodeId(get_u32(cur)?),
        },
        41 => NodeKind::Restarg(Symbol(get_u32(cur)?)),
        42 => NodeKind::Kwarg(Symbol(get_u32(cur)?)),
        43 => NodeKind::Kwoptarg {
            name: Symbol(get_u32(cur)?),
            default: NodeId(get_u32(cur)?),
        },
        44 => NodeKind::Kwrestarg(Symbol(get_u32(cur)?)),
        45 => NodeKind::Blockarg(Symbol(get_u32(cur)?)),
        46 => NodeKind::Kwsplat(OptNodeId(get_u32(cur)?)),
        47 => NodeKind::While {
            cond: NodeId(get_u32(cur)?),
            body: OptNodeId(get_u32(cur)?),
            post: get_u8(cur)? != 0,
        },
        48 => NodeKind::Until {
            cond: NodeId(get_u32(cur)?),
            body: OptNodeId(get_u32(cur)?),
            post: get_u8(cur)? != 0,
        },
        49 => NodeKind::RangeExpr {
            begin_: OptNodeId(get_u32(cur)?),
            end_: OptNodeId(get_u32(cur)?),
            exclusive: get_u8(cur)? != 0,
        },
        50 => NodeKind::Sclass {
            expr: NodeId(get_u32(cur)?),
            body: OptNodeId(get_u32(cur)?),
        },
        51 => NodeKind::Break(OptNodeId(get_u32(cur)?)),
        52 => NodeKind::Next(OptNodeId(get_u32(cur)?)),
        53 => NodeKind::Yield(read_node_list(cur)?),
        54 => NodeKind::Super(read_node_list(cur)?),
        55 => NodeKind::Zsuper,
        56 => NodeKind::Defined(NodeId(get_u32(cur)?)),
        57 => NodeKind::Rescue {
            body: OptNodeId(get_u32(cur)?),
            resbodies: read_node_list(cur)?,
            else_: OptNodeId(get_u32(cur)?),
        },
        58 => NodeKind::Resbody {
            exceptions: read_node_list(cur)?,
            var: OptNodeId(get_u32(cur)?),
            body: OptNodeId(get_u32(cur)?),
        },
        59 => NodeKind::Ensure {
            body: OptNodeId(get_u32(cur)?),
            ensure_: OptNodeId(get_u32(cur)?),
        },
        _ => return Err(SerError::BadDiscriminant),
    })
}

// --- small POD helpers ---

fn write_range(r: Range, out: &mut Vec<u8>) {
    put_u32(out, r.start);
    put_u32(out, r.end);
}
fn read_range(cur: &mut &[u8]) -> Result<Range, SerError> {
    Ok(Range {
        start: get_u32(cur)?,
        end: get_u32(cur)?,
    })
}

fn write_node_list(l: NodeList, out: &mut Vec<u8>) {
    put_u32(out, l.start);
    put_u32(out, l.len);
}
fn read_node_list(cur: &mut &[u8]) -> Result<NodeList, SerError> {
    Ok(NodeList {
        start: get_u32(cur)?,
        len: get_u32(cur)?,
    })
}

fn write_ast_node(n: &AstNode, out: &mut Vec<u8>) {
    write_node_kind(&n.kind, out);
    put_u32(out, n.parent.0);
    write_range(n.range, out);
}
fn read_ast_node(cur: &mut &[u8]) -> Result<AstNode, SerError> {
    let kind = read_node_kind(cur)?;
    let parent = OptNodeId(get_u32(cur)?);
    let range = read_range(cur)?;
    Ok(AstNode {
        kind,
        parent,
        range,
    })
}

fn write_comment(c: &Comment, out: &mut Vec<u8>) {
    write_range(c.range, out);
    let kind: u8 = match c.kind {
        CommentKind::Inline => 0,
        CommentKind::Block => 1,
    };
    put_u8(out, kind);
}
fn read_comment(cur: &mut &[u8]) -> Result<Comment, SerError> {
    let range = read_range(cur)?;
    let kind = match get_u8(cur)? {
        0 => CommentKind::Inline,
        1 => CommentKind::Block,
        _ => return Err(SerError::BadDiscriminant),
    };
    Ok(Comment { range, kind })
}

impl Ast {
    /// Serialize to a flat byte buffer. Round-trips bit-exactly via
    /// [`Ast::from_bytes`]. No header — see murphy-9cr.26 for the cache.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();

        // 1. nodes
        put_u64(&mut out, self.nodes.len() as u64);
        for node in &self.nodes {
            write_ast_node(node, &mut out);
        }

        // 2. node_lists
        put_u64(&mut out, self.node_lists.len() as u64);
        for id in &self.node_lists {
            put_u32(&mut out, id.0);
        }

        // 3. interner blob (raw bytes)
        put_u64(&mut out, self.interner.blob.len() as u64);
        out.extend_from_slice(&self.interner.blob);

        // 4. interner offsets
        put_u64(&mut out, self.interner.offsets.len() as u64);
        for r in &self.interner.offsets {
            write_range(*r, &mut out);
        }

        // 5. comments
        put_u64(&mut out, self.comments.len() as u64);
        for c in &self.comments {
            write_comment(c, &mut out);
        }

        // 6. source text
        put_u64(&mut out, self.source.text.len() as u64);
        out.extend_from_slice(self.source.text.as_bytes());

        // 7. source path
        let path = self.source.path.to_string_lossy();
        let path_bytes = path.as_bytes();
        put_u64(&mut out, path_bytes.len() as u64);
        out.extend_from_slice(path_bytes);

        // 8. root
        put_u32(&mut out, self.root.0);

        out
    }

    /// Deserialize a buffer produced by [`Ast::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Ast, SerError> {
        let mut cur = bytes;

        // 1. nodes
        let node_count =
            usize::try_from(get_u64(&mut cur)?).map_err(|_| SerError::UnexpectedEof)?;
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            nodes.push(read_ast_node(&mut cur)?);
        }

        // 2. node_lists
        let list_count =
            usize::try_from(get_u64(&mut cur)?).map_err(|_| SerError::UnexpectedEof)?;
        let mut node_lists = Vec::with_capacity(list_count);
        for _ in 0..list_count {
            node_lists.push(NodeId(get_u32(&mut cur)?));
        }

        // 3. interner blob (raw bytes, validated UTF-8)
        let blob_len = usize::try_from(get_u64(&mut cur)?).map_err(|_| SerError::UnexpectedEof)?;
        let blob = take(&mut cur, blob_len)?.to_vec();
        std::str::from_utf8(&blob).map_err(|_| SerError::InvalidUtf8)?;

        // 4. interner offsets
        let offset_count =
            usize::try_from(get_u64(&mut cur)?).map_err(|_| SerError::UnexpectedEof)?;
        let mut offsets = Vec::with_capacity(offset_count);
        for _ in 0..offset_count {
            offsets.push(read_range(&mut cur)?);
        }

        // 5. comments
        let comment_count =
            usize::try_from(get_u64(&mut cur)?).map_err(|_| SerError::UnexpectedEof)?;
        let mut comments = Vec::with_capacity(comment_count);
        for _ in 0..comment_count {
            comments.push(read_comment(&mut cur)?);
        }

        // 6. source text
        let text_len = usize::try_from(get_u64(&mut cur)?).map_err(|_| SerError::UnexpectedEof)?;
        let text_bytes = take(&mut cur, text_len)?.to_vec();
        let text = String::from_utf8(text_bytes).map_err(|_| SerError::InvalidUtf8)?;

        // 7. source path
        let path_len = usize::try_from(get_u64(&mut cur)?).map_err(|_| SerError::UnexpectedEof)?;
        let path_bytes = take(&mut cur, path_len)?.to_vec();
        let path_string = String::from_utf8(path_bytes).map_err(|_| SerError::InvalidUtf8)?;
        let path = std::path::PathBuf::from(path_string);

        // 8. root
        let root = NodeId(get_u32(&mut cur)?);

        Ok(Ast {
            nodes,
            node_lists,
            interner: Interner { blob, offsets },
            comments,
            source: SourceBuffer { text, path },
            root,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::builder::AstBuilder;
    use crate::node::{CommentKind, NodeKind, NodeList, OptNodeId, Range};

    fn r(a: u32, b: u32) -> Range {
        Range { start: a, end: b }
    }

    #[test]
    fn round_trip_is_bit_equal() {
        let mut b = AstBuilder::new("x = 1 # c", "t.rb");
        let int = b.push(NodeKind::Int(1), r(4, 5));
        let name = b.intern_symbol("x");
        let asgn = b.push(
            NodeKind::Lvasgn {
                name,
                value: OptNodeId::some(int),
            },
            r(0, 5),
        );
        let list = b.push_list(&[asgn]);
        let root = b.push(NodeKind::Begin(list), r(0, 5));
        b.add_comment(r(6, 9), CommentKind::Inline);
        let ast = b.finish(root);

        let bytes = ast.to_bytes();
        let restored = crate::Ast::from_bytes(&bytes).expect("round-trip");
        assert_eq!(ast, restored, "round-trip must be bit-equal");
    }

    #[test]
    fn round_trip_gvasgn_cvasgn() {
        // The two variants appended after `Unknown` (discriminants 38/39)
        // must survive the byte round-trip.
        let mut b = AstBuilder::new("$g = 1; @@c = 2", "t.rb");
        let one = b.push(NodeKind::Int(1), r(5, 6));
        let g_name = b.intern_symbol("$g");
        let gv = b.push(
            NodeKind::Gvasgn {
                name: g_name,
                value: OptNodeId::some(one),
            },
            r(0, 6),
        );
        let two = b.push(NodeKind::Int(2), r(13, 14));
        let c_name = b.intern_symbol("@@c");
        let cv = b.push(
            NodeKind::Cvasgn {
                name: c_name,
                value: OptNodeId::some(two),
            },
            r(8, 14),
        );
        let list = b.push_list(&[gv, cv]);
        let root = b.push(NodeKind::Begin(list), r(0, 14));
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes()).expect("round-trip");
        assert_eq!(ast, restored, "Gvasgn/Cvasgn round-trip must be bit-equal");
    }

    #[test]
    fn round_trip_param_variants() {
        // The six parameter variants appended after `Cvasgn`
        // (discriminants 40..=45) must survive the byte round-trip.
        // Covers both shapes: struct-with-`default` (Optarg/Kwoptarg) and
        // tuple-leaf (Restarg/Kwarg/Kwrestarg/Blockarg).
        let mut b = AstBuilder::new("def f(a = 1, *r, k:, m: 2, **o, &blk); end", "t.rb");
        let one = b.push(NodeKind::Int(1), r(11, 12));
        let a_name = b.intern_symbol("a");
        let optarg = b.push(
            NodeKind::Optarg {
                name: a_name,
                default: one,
            },
            r(6, 12),
        );
        let r_name = b.intern_symbol("r");
        let restarg = b.push(NodeKind::Restarg(r_name), r(14, 16));
        let k_name = b.intern_symbol("k");
        let kwarg = b.push(NodeKind::Kwarg(k_name), r(18, 20));
        let two = b.push(NodeKind::Int(2), r(25, 26));
        let m_name = b.intern_symbol("m");
        let kwoptarg = b.push(
            NodeKind::Kwoptarg {
                name: m_name,
                default: two,
            },
            r(22, 26),
        );
        let o_name = b.intern_symbol("o");
        let kwrestarg = b.push(NodeKind::Kwrestarg(o_name), r(28, 31));
        let blk_name = b.intern_symbol("blk");
        let blockarg = b.push(NodeKind::Blockarg(blk_name), r(33, 37));
        let list = b.push_list(&[optarg, restarg, kwarg, kwoptarg, kwrestarg, blockarg]);
        let root = b.push(NodeKind::Args(list), r(5, 38));
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes()).expect("round-trip");
        assert_eq!(
            ast, restored,
            "parameter variant round-trip must be bit-equal"
        );
    }

    #[test]
    fn round_trip_kwsplat() {
        // The `Kwsplat` variant appended after `Blockarg` (discriminant 46)
        // must survive the byte round-trip. Covers both a present inner and
        // the `None` (anonymous `**`) inner.
        let mut b = AstBuilder::new("{ **rest }", "t.rb");
        let rest = b.push(NodeKind::Nil, r(4, 8));
        let kwsplat = b.push(NodeKind::Kwsplat(OptNodeId::some(rest)), r(2, 8));
        let anon = b.push(NodeKind::Kwsplat(OptNodeId::NONE), r(2, 4));
        let list = b.push_list(&[kwsplat, anon]);
        let root = b.push(NodeKind::Hash(list), r(0, 10));
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes()).expect("round-trip");
        assert_eq!(ast, restored, "Kwsplat round-trip must be bit-equal");
    }

    #[test]
    fn round_trip_while_until() {
        // The `While`/`Until` variants appended after `Kwsplat`
        // (discriminants 47/48) must survive the byte round-trip. Covers
        // both `post` values and present/`None` `body`.
        let mut b = AstBuilder::new("while c\n  x\nend until d", "t.rb");
        let cond_w = b.push(NodeKind::Nil, r(6, 7));
        let body_w = b.push(NodeKind::Nil, r(10, 11));
        let while_node = b.push(
            NodeKind::While {
                cond: cond_w,
                body: OptNodeId::some(body_w),
                post: false,
            },
            r(0, 15),
        );
        let cond_u = b.push(NodeKind::Nil, r(20, 21));
        // body `None`, `post = true` (do-while shape).
        let until_node = b.push(
            NodeKind::Until {
                cond: cond_u,
                body: OptNodeId::NONE,
                post: true,
            },
            r(16, 23),
        );
        let list = b.push_list(&[while_node, until_node]);
        let root = b.push(NodeKind::Begin(list), r(0, 23));
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes()).expect("round-trip");
        assert_eq!(ast, restored, "While/Until round-trip must be bit-equal");
    }

    #[test]
    fn round_trip_range_expr() {
        // The `RangeExpr` variant appended after `Until` (discriminant 49)
        // must survive the byte round-trip. Covers `exclusive` both ways and
        // a beginless (`begin_` None) plus endless (`end_` None) end.
        let mut b = AstBuilder::new("1...5; 1..; ..5", "t.rb");
        let one = b.push(NodeKind::Int(1), r(0, 1));
        let five = b.push(NodeKind::Int(5), r(4, 5));
        // `1...5` — both ends present, exclusive.
        let inclusive_excl = b.push(
            NodeKind::RangeExpr {
                begin_: OptNodeId::some(one),
                end_: OptNodeId::some(five),
                exclusive: true,
            },
            r(0, 5),
        );
        // `1..` — endless, inclusive.
        let one2 = b.push(NodeKind::Int(1), r(7, 8));
        let endless = b.push(
            NodeKind::RangeExpr {
                begin_: OptNodeId::some(one2),
                end_: OptNodeId::NONE,
                exclusive: false,
            },
            r(7, 11),
        );
        // `..5` — beginless, inclusive.
        let five2 = b.push(NodeKind::Int(5), r(15, 16));
        let beginless = b.push(
            NodeKind::RangeExpr {
                begin_: OptNodeId::NONE,
                end_: OptNodeId::some(five2),
                exclusive: false,
            },
            r(13, 16),
        );
        let list = b.push_list(&[inclusive_excl, endless, beginless]);
        let root = b.push(NodeKind::Begin(list), r(0, 16));
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes()).expect("round-trip");
        assert_eq!(ast, restored, "RangeExpr round-trip must be bit-equal");
    }

    #[test]
    fn round_trip_def_with_receiver_and_sclass() {
        // `Def` 改修（discriminant 32、`receiver` フィールド追加）と `Sclass`
        // （discriminant 50、`RangeExpr` の次）が byte round-trip で保存される。
        // singleton `def self.foo` の `receiver` Some と素の `def` の None
        // 両方を、`Sclass` の present/`None` body とともに確認する。
        let mut b = AstBuilder::new("class << self\n  def self.f; end\nend", "t.rb");
        let empty_args1 = b.push(NodeKind::Args(NodeList::EMPTY), r(20, 20));
        let self_recv = b.push(NodeKind::SelfExpr, r(18, 22));
        let f_name = b.intern_symbol("f");
        // singleton def: receiver Some, body None.
        let singleton_def = b.push(
            NodeKind::Def {
                receiver: OptNodeId::some(self_recv),
                name: f_name,
                args: empty_args1,
                body: OptNodeId::NONE,
            },
            r(16, 30),
        );
        // plain def: receiver None, body Some.
        let empty_args2 = b.push(NodeKind::Args(NodeList::EMPTY), r(0, 0));
        let g_name = b.intern_symbol("g");
        let body = b.push(NodeKind::Nil, r(0, 3));
        let plain_def = b.push(
            NodeKind::Def {
                receiver: OptNodeId::NONE,
                name: g_name,
                args: empty_args2,
                body: OptNodeId::some(body),
            },
            r(0, 10),
        );
        let def_list = b.push_list(&[singleton_def, plain_def]);
        let sclass_body = b.push(NodeKind::Begin(def_list), r(16, 30));
        let expr = b.push(NodeKind::SelfExpr, r(8, 12));
        let root = b.push(
            NodeKind::Sclass {
                expr,
                body: OptNodeId::some(sclass_body),
            },
            r(0, 34),
        );
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes()).expect("round-trip");
        assert_eq!(
            ast, restored,
            "Def-with-receiver / Sclass round-trip must be bit-equal"
        );
    }

    #[test]
    fn round_trip_jump_variants() {
        // The six jump variants appended after `Sclass`
        // (discriminants 51..=56) must survive the byte round-trip. Covers
        // the `OptNodeId` payloads (`Break`/`Next`) both ways, the
        // `NodeList` payloads (`Yield`/`Super`), the fieldless `Zsuper`, and
        // the `NodeId` payload (`Defined`).
        let mut b = AstBuilder::new(
            "break 1; next; yield 2; super(3); super; defined?(x)",
            "t.rb",
        );
        let one = b.push(NodeKind::Int(1), r(6, 7));
        let break_some = b.push(NodeKind::Break(OptNodeId::some(one)), r(0, 7));
        let next_none = b.push(NodeKind::Next(OptNodeId::NONE), r(9, 13));
        let two = b.push(NodeKind::Int(2), r(21, 22));
        let yield_list = b.push_list(&[two]);
        let yield_node = b.push(NodeKind::Yield(yield_list), r(15, 22));
        let three = b.push(NodeKind::Int(3), r(30, 31));
        let super_list = b.push_list(&[three]);
        let super_node = b.push(NodeKind::Super(super_list), r(24, 32));
        // empty-list `Yield` to exercise the empty `NodeList` path too.
        let yield_empty = b.push(NodeKind::Yield(NodeList::EMPTY), r(34, 39));
        let zsuper = b.push(NodeKind::Zsuper, r(34, 39));
        let x = b.push(NodeKind::Nil, r(50, 51));
        let defined = b.push(NodeKind::Defined(x), r(41, 52));
        let list = b.push_list(&[
            break_some,
            next_none,
            yield_node,
            super_node,
            yield_empty,
            zsuper,
            defined,
        ]);
        let root = b.push(NodeKind::Begin(list), r(0, 52));
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes()).expect("round-trip");
        assert_eq!(ast, restored, "jump variant round-trip must be bit-equal");
    }

    #[test]
    fn round_trip_exception_variants() {
        // The three exception variants appended after `Defined`
        // (discriminants 57..=59) must survive the byte round-trip. Covers
        // the `NodeList` payloads (`Rescue.resbodies`, `Resbody.exceptions`)
        // both populated and empty, and the `OptNodeId` fields both ways.
        let mut b = AstBuilder::new("begin\n  x\nrescue E => e\n  y\nensure\n  z\nend", "t.rb");
        let exc = b.push(NodeKind::Nil, r(18, 19));
        let exc_list = b.push_list(&[exc]);
        let var = b.push(NodeKind::Nil, r(23, 24));
        let resbody_body = b.push(NodeKind::Nil, r(28, 29));
        // `Resbody` with exceptions present and a bound var.
        let resbody = b.push(
            NodeKind::Resbody {
                exceptions: exc_list,
                var: OptNodeId::some(var),
                body: OptNodeId::some(resbody_body),
            },
            r(10, 29),
        );
        // bare `Resbody` — empty exceptions list, no var, no body.
        let bare_resbody = b.push(
            NodeKind::Resbody {
                exceptions: NodeList::EMPTY,
                var: OptNodeId::NONE,
                body: OptNodeId::NONE,
            },
            r(10, 12),
        );
        let resbodies = b.push_list(&[resbody, bare_resbody]);
        let protected = b.push(NodeKind::Nil, r(8, 9));
        let rescue = b.push(
            NodeKind::Rescue {
                body: OptNodeId::some(protected),
                resbodies,
                else_: OptNodeId::NONE,
            },
            r(0, 40),
        );
        let ensure_body = b.push(NodeKind::Nil, r(37, 38));
        let ensure = b.push(
            NodeKind::Ensure {
                body: OptNodeId::some(rescue),
                ensure_: OptNodeId::some(ensure_body),
            },
            r(0, 40),
        );
        let list = b.push_list(&[ensure]);
        let root = b.push(NodeKind::Begin(list), r(0, 40));
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes()).expect("round-trip");
        assert_eq!(
            ast, restored,
            "exception variant round-trip must be bit-equal"
        );
    }

    #[test]
    fn round_trip_empty_ast() {
        let mut b = AstBuilder::new("", "e.rb");
        let root = b.push(NodeKind::Nil, r(0, 0));
        let ast = b.finish(root);
        let restored = crate::Ast::from_bytes(&ast.to_bytes()).unwrap();
        assert_eq!(ast, restored);
    }
}
