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
        NodeKind::Def { name, args, body } => {
            put_u8(out, 32);
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
    use crate::node::{CommentKind, NodeKind, OptNodeId, Range};

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
    fn round_trip_empty_ast() {
        let mut b = AstBuilder::new("", "e.rb");
        let root = b.push(NodeKind::Nil, r(0, 0));
        let ast = b.finish(root);
        let restored = crate::Ast::from_bytes(&ast.to_bytes()).unwrap();
        assert_eq!(ast, restored);
    }
}
