//! Flat, field-by-field little-endian serialization of an [`Ast`].
//!
//! Buffers begin with an 88-byte fixed header (magic + format version +
//! Murphy version + target triple + content hash) followed by the body
//! (nodes / node_lists / interner / comments / source / path / root). The
//! header lets the cache layer (murphy-9cr.26) detect format and machine
//! mismatches and the content-hash field defends against keying mistakes.
//! `from_bytes` also runs a one-pass bounds check over the deserialized
//! arena so a malformed buffer surfaces as a `Result::Err` rather than a
//! later traversal-time panic.

use crate::ast::Ast;
use crate::interner::Interner;
use crate::node::{
    AstNode, Comment, CommentKind, NodeId, NodeKind, NodeList, NodeLoc, OptNodeId, Range,
    SourceBuffer, SourceToken, SourceTokenKind, StringId, Symbol,
};
use sha2::{Digest, Sha256};

/// Magic bytes at the very start of every serialized arena (`b"MURPHYAS"`).
pub const MAGIC: &[u8; 8] = b"MURPHYAS";

/// Binary format version. Bump on **any** layout change — old caches are
/// then rejected with [`SerError::FormatVersionMismatch`].
pub const FORMAT_VERSION: u32 = 3;

/// Total header size in bytes. The body immediately follows. Downstream
/// (cache, mmap) code can rely on this offset being fixed.
pub const HEADER_LEN: usize = 96;

const MURPHY_VERSION_LEN: usize = 16;
// Target triples like `aarch64-unknown-linux-gnueabihf` (30) and
// `riscv64gc-unknown-linux-gnu` (27) need more than 24 bytes; 32 covers
// every triple in `rustup target list` with a small margin.
const TARGET_TRIPLE_LEN: usize = 32;
const CONTENT_HASH_LEN: usize = 32;

/// This Murphy crate's version, zero-padded into `MURPHY_VERSION_LEN` bytes.
fn current_murphy_version() -> [u8; MURPHY_VERSION_LEN] {
    let mut buf = [0u8; MURPHY_VERSION_LEN];
    let v = env!("CARGO_PKG_VERSION").as_bytes();
    let n = v.len().min(MURPHY_VERSION_LEN);
    buf[..n].copy_from_slice(&v[..n]);
    buf
}

/// This binary's target triple, zero-padded into `TARGET_TRIPLE_LEN` bytes.
fn current_target_triple() -> [u8; TARGET_TRIPLE_LEN] {
    let mut buf = [0u8; TARGET_TRIPLE_LEN];
    let v = env!("MURPHY_TARGET_TRIPLE").as_bytes();
    let n = v.len().min(TARGET_TRIPLE_LEN);
    buf[..n].copy_from_slice(&v[..n]);
    buf
}

/// `sha256(bytes)` as 32 raw bytes.
pub fn content_hash(bytes: &[u8]) -> [u8; CONTENT_HASH_LEN] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

/// Serialization / deserialization failure.
#[derive(Debug)]
pub enum SerError {
    /// The buffer ended before a field could be fully read.
    UnexpectedEof,
    /// A discriminant byte did not name a known variant.
    BadDiscriminant,
    /// A byte section that must be UTF-8 was not.
    InvalidUtf8,
    /// The header did not start with the expected magic bytes.
    BadMagic,
    /// The header's format version did not match [`FORMAT_VERSION`].
    FormatVersionMismatch { found: u32, expected: u32 },
    /// The header's Murphy crate version did not match the running binary's.
    MurphyVersionMismatch,
    /// The header's target triple did not match the running binary's.
    TargetMismatch,
    /// The header's content hash did not match `sha256(source.text)`.
    ContentHashMismatch,
    /// A `NodeId` referenced by the arena was outside `0..nodes.len()`.
    NodeIdOutOfRange { id: u32, count: u32 },
    /// A `Symbol`/`StringId` was outside `0..interner.offsets.len()`.
    SymbolOutOfRange { id: u32, count: u32 },
    /// A `NodeList { start, len }` spanned past `node_lists.len()`.
    BadNodeListRange { start: u32, len: u32 },
    /// The recorded root was outside `0..nodes.len()`.
    BadRoot { id: u32, count: u32 },
    /// `source.path` was not valid UTF-8. The on-disk format encodes the
    /// path as UTF-8, so non-UTF-8 OS paths (e.g. arbitrary-byte Unix paths)
    /// cannot round-trip and are rejected outright instead of being silently
    /// replaced with `U+FFFD`.
    PathNotUtf8,
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
pub(crate) fn write_node_kind(k: &NodeKind, out: &mut Vec<u8>) {
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
        NodeKind::OpAsgn { target, op, value } => {
            put_u8(out, 60);
            put_u32(out, target.0);
            put_u32(out, op.0);
            put_u32(out, value.0);
        }
        NodeKind::OrAsgn { target, value } => {
            put_u8(out, 61);
            put_u32(out, target.0);
            put_u32(out, value.0);
        }
        NodeKind::AndAsgn { target, value } => {
            put_u8(out, 62);
            put_u32(out, target.0);
            put_u32(out, value.0);
        }
        NodeKind::Dstr(l) => {
            put_u8(out, 63);
            write_node_list(l, out);
        }
        NodeKind::Dsym(l) => {
            put_u8(out, 64);
            write_node_list(l, out);
        }
        NodeKind::Xstr(l) => {
            put_u8(out, 65);
            write_node_list(l, out);
        }
        NodeKind::Regexp { parts, opts } => {
            put_u8(out, 66);
            write_node_list(parts, out);
            put_u32(out, opts.0);
        }
        NodeKind::Masgn { lhs, rhs } => {
            put_u8(out, 67);
            put_u32(out, lhs.0);
            put_u32(out, rhs.0);
        }
        NodeKind::Mlhs(l) => {
            put_u8(out, 68);
            write_node_list(l, out);
        }
        // ── murphy-w5ba HIGH-priority extensions ────────────────────────
        NodeKind::For { var, iter, body } => {
            put_u8(out, 69);
            put_u32(out, var.0);
            put_u32(out, iter.0);
            put_u32(out, body.0);
        }
        NodeKind::Lambda => put_u8(out, 70),
        NodeKind::Defs {
            receiver,
            name,
            args,
            body,
        } => {
            put_u8(out, 71);
            put_u32(out, receiver.0);
            put_u32(out, name.0);
            put_u32(out, args.0);
            put_u32(out, body.0);
        }
        NodeKind::Index { receiver, args } => {
            put_u8(out, 72);
            put_u32(out, receiver.0);
            write_node_list(args, out);
        }
        NodeKind::IndexAsgn {
            receiver,
            args,
            value,
        } => {
            put_u8(out, 73);
            put_u32(out, receiver.0);
            write_node_list(args, out);
            put_u32(out, value.0);
        }
        NodeKind::Kwbegin(l) => {
            put_u8(out, 74);
            write_node_list(l, out);
        }
        NodeKind::Cbase => put_u8(out, 75),
        NodeKind::Regopt(l) => {
            put_u8(out, 76);
            write_node_list(l, out);
        }
        NodeKind::Rational(s) => {
            put_u8(out, 77);
            put_u32(out, s.0);
        }
        NodeKind::Complex(s) => {
            put_u8(out, 78);
            put_u32(out, s.0);
        }
        NodeKind::Not(n) => {
            put_u8(out, 79);
            put_u32(out, n.0);
        }
        NodeKind::Retry => put_u8(out, 80),
        NodeKind::Redo => put_u8(out, 81),
        NodeKind::Numblock { send, max_n, body } => {
            put_u8(out, 82);
            put_u32(out, send.0);
            put_u8(out, max_n);
            put_u32(out, body.0);
        }
        NodeKind::Procarg0(l) => {
            put_u8(out, 83);
            write_node_list(l, out);
        }
        NodeKind::ForwardArgs => put_u8(out, 84),
        NodeKind::ForwardedArgs => put_u8(out, 85),
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
        60 => NodeKind::OpAsgn {
            target: NodeId(get_u32(cur)?),
            op: Symbol(get_u32(cur)?),
            value: NodeId(get_u32(cur)?),
        },
        61 => NodeKind::OrAsgn {
            target: NodeId(get_u32(cur)?),
            value: NodeId(get_u32(cur)?),
        },
        62 => NodeKind::AndAsgn {
            target: NodeId(get_u32(cur)?),
            value: NodeId(get_u32(cur)?),
        },
        63 => NodeKind::Dstr(read_node_list(cur)?),
        64 => NodeKind::Dsym(read_node_list(cur)?),
        65 => NodeKind::Xstr(read_node_list(cur)?),
        66 => NodeKind::Regexp {
            parts: read_node_list(cur)?,
            opts: Symbol(get_u32(cur)?),
        },
        67 => NodeKind::Masgn {
            lhs: NodeId(get_u32(cur)?),
            rhs: NodeId(get_u32(cur)?),
        },
        68 => NodeKind::Mlhs(read_node_list(cur)?),
        // ── murphy-w5ba HIGH-priority extensions ────────────────────────
        69 => NodeKind::For {
            var: NodeId(get_u32(cur)?),
            iter: NodeId(get_u32(cur)?),
            body: OptNodeId(get_u32(cur)?),
        },
        70 => NodeKind::Lambda,
        71 => NodeKind::Defs {
            receiver: NodeId(get_u32(cur)?),
            name: Symbol(get_u32(cur)?),
            args: NodeId(get_u32(cur)?),
            body: OptNodeId(get_u32(cur)?),
        },
        72 => NodeKind::Index {
            receiver: NodeId(get_u32(cur)?),
            args: read_node_list(cur)?,
        },
        73 => NodeKind::IndexAsgn {
            receiver: NodeId(get_u32(cur)?),
            args: read_node_list(cur)?,
            value: NodeId(get_u32(cur)?),
        },
        74 => NodeKind::Kwbegin(read_node_list(cur)?),
        75 => NodeKind::Cbase,
        76 => NodeKind::Regopt(read_node_list(cur)?),
        77 => NodeKind::Rational(StringId(get_u32(cur)?)),
        78 => NodeKind::Complex(StringId(get_u32(cur)?)),
        79 => NodeKind::Not(NodeId(get_u32(cur)?)),
        80 => NodeKind::Retry,
        81 => NodeKind::Redo,
        82 => NodeKind::Numblock {
            send: NodeId(get_u32(cur)?),
            max_n: get_u8(cur)?,
            body: OptNodeId(get_u32(cur)?),
        },
        83 => NodeKind::Procarg0(read_node_list(cur)?),
        84 => NodeKind::ForwardArgs,
        85 => NodeKind::ForwardedArgs,
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
    write_range(n.loc.expression, out);
    write_range(n.loc.name, out);
}
fn read_ast_node(cur: &mut &[u8]) -> Result<AstNode, SerError> {
    let kind = read_node_kind(cur)?;
    let parent = OptNodeId(get_u32(cur)?);
    let expression = read_range(cur)?;
    let name = read_range(cur)?;
    Ok(AstNode {
        kind,
        parent,
        loc: NodeLoc { expression, name },
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

fn write_source_token(t: SourceToken, out: &mut Vec<u8>) {
    write_range(t.range, out);
    put_u8(out, t.kind as u8);
}
fn read_source_token(cur: &mut &[u8]) -> Result<SourceToken, SerError> {
    let range = read_range(cur)?;
    let kind = match get_u8(cur)? {
        0 => SourceTokenKind::LeftParen,
        1 => SourceTokenKind::RightParen,
        2 => SourceTokenKind::Comment,
        3 => SourceTokenKind::Newline,
        4 => SourceTokenKind::IgnoredNewline,
        5 => SourceTokenKind::HeredocStart,
        6 => SourceTokenKind::HeredocEnd,
        7 => SourceTokenKind::Other,
        _ => return Err(SerError::BadDiscriminant),
    };
    Ok(SourceToken { range, kind })
}

fn write_header(source_text: &str, out: &mut Vec<u8>) {
    out.reserve(HEADER_LEN);
    out.extend_from_slice(MAGIC);
    put_u32(out, FORMAT_VERSION);
    put_u32(out, 0); // reserved — extensions go through a FORMAT_VERSION bump
    out.extend_from_slice(&current_murphy_version());
    out.extend_from_slice(&current_target_triple());
    out.extend_from_slice(&content_hash(source_text.as_bytes()));
    debug_assert_eq!(out.len(), HEADER_LEN);
}

/// Read and validate the header. Returns the recorded content hash so the
/// caller can verify it against the source text after the body is read.
/// The `reserved` u32 is intentionally not validated — any future extension
/// must come with a [`FORMAT_VERSION`] bump that retires this layout.
fn read_header(cur: &mut &[u8]) -> Result<[u8; CONTENT_HASH_LEN], SerError> {
    let magic = take(cur, MAGIC.len())?;
    if magic != MAGIC {
        return Err(SerError::BadMagic);
    }
    let found_format = get_u32(cur)?;
    if found_format != FORMAT_VERSION {
        return Err(SerError::FormatVersionMismatch {
            found: found_format,
            expected: FORMAT_VERSION,
        });
    }
    let _reserved = get_u32(cur)?;
    let murphy_v = take(cur, MURPHY_VERSION_LEN)?;
    if murphy_v != current_murphy_version() {
        return Err(SerError::MurphyVersionMismatch);
    }
    let target = take(cur, TARGET_TRIPLE_LEN)?;
    if target != current_target_triple() {
        return Err(SerError::TargetMismatch);
    }
    let hash_bytes = take(cur, CONTENT_HASH_LEN)?;
    let mut hash = [0u8; CONTENT_HASH_LEN];
    hash.copy_from_slice(hash_bytes);
    Ok(hash)
}

/// Verify every `NodeId` / `Symbol` / `NodeList` index in an `Ast` lies
/// within its backing array. Run by [`Ast::from_bytes`] after deserialization
/// so a malformed buffer surfaces here rather than as a later panic.
fn validate_indices(ast: &Ast) -> Result<(), SerError> {
    let node_count = ast.nodes.len() as u32;
    let sym_count = ast.interner.offsets.len() as u32;
    let list_count = ast.node_lists.len() as u32;

    let check_node = |id: u32| -> Result<(), SerError> {
        if id >= node_count {
            Err(SerError::NodeIdOutOfRange {
                id,
                count: node_count,
            })
        } else {
            Ok(())
        }
    };
    // OptNodeId::NONE (u32::MAX) is the legitimate sentinel; everything else
    // must be a real index.
    let check_opt_node = |opt: OptNodeId| -> Result<(), SerError> {
        if opt == OptNodeId::NONE {
            Ok(())
        } else {
            check_node(opt.0)
        }
    };
    let check_sym = |id: u32| -> Result<(), SerError> {
        if id >= sym_count {
            Err(SerError::SymbolOutOfRange {
                id,
                count: sym_count,
            })
        } else {
            Ok(())
        }
    };
    let check_list = |l: NodeList| -> Result<(), SerError> {
        let end = (l.start as u64) + (l.len as u64);
        if end > list_count as u64 {
            Err(SerError::BadNodeListRange {
                start: l.start,
                len: l.len,
            })
        } else {
            Ok(())
        }
    };

    for node in &ast.nodes {
        check_opt_node(node.parent)?;
        match node.kind {
            NodeKind::Error
            | NodeKind::Nil
            | NodeKind::True_
            | NodeKind::False_
            | NodeKind::SelfExpr
            | NodeKind::Unknown
            | NodeKind::Zsuper
            | NodeKind::Int(_)
            | NodeKind::Float(_) => {}
            NodeKind::Str(s) => check_sym(s.0)?,
            NodeKind::Sym(s)
            | NodeKind::Lvar(s)
            | NodeKind::Ivar(s)
            | NodeKind::Cvar(s)
            | NodeKind::Gvar(s)
            | NodeKind::Arg(s)
            | NodeKind::Restarg(s)
            | NodeKind::Kwarg(s)
            | NodeKind::Kwrestarg(s)
            | NodeKind::Blockarg(s) => check_sym(s.0)?,
            NodeKind::Const { scope, name } => {
                check_opt_node(scope)?;
                check_sym(name.0)?;
            }
            NodeKind::Lvasgn { name, value }
            | NodeKind::Ivasgn { name, value }
            | NodeKind::Gvasgn { name, value }
            | NodeKind::Cvasgn { name, value } => {
                check_sym(name.0)?;
                check_opt_node(value)?;
            }
            NodeKind::Casgn { scope, name, value } => {
                check_opt_node(scope)?;
                check_sym(name.0)?;
                check_opt_node(value)?;
            }
            NodeKind::Send {
                receiver,
                method,
                args,
            } => {
                check_opt_node(receiver)?;
                check_sym(method.0)?;
                check_list(args)?;
            }
            NodeKind::Csend {
                receiver,
                method,
                args,
            } => {
                check_node(receiver.0)?;
                check_sym(method.0)?;
                check_list(args)?;
            }
            NodeKind::Block { call, args, body } => {
                check_node(call.0)?;
                check_node(args.0)?;
                check_opt_node(body)?;
            }
            NodeKind::BlockPass(o)
            | NodeKind::Splat(o)
            | NodeKind::Return(o)
            | NodeKind::Break(o)
            | NodeKind::Next(o)
            | NodeKind::Kwsplat(o) => check_opt_node(o)?,
            NodeKind::Array(l)
            | NodeKind::Hash(l)
            | NodeKind::Begin(l)
            | NodeKind::Args(l)
            | NodeKind::Yield(l)
            | NodeKind::Super(l)
            | NodeKind::Dstr(l)
            | NodeKind::Dsym(l)
            | NodeKind::Xstr(l)
            | NodeKind::Mlhs(l) => check_list(l)?,
            NodeKind::Pair { key, value } => {
                check_node(key.0)?;
                check_node(value.0)?;
            }
            NodeKind::If { cond, then_, else_ } => {
                check_node(cond.0)?;
                check_opt_node(then_)?;
                check_opt_node(else_)?;
            }
            NodeKind::Case {
                subject,
                whens,
                else_,
            } => {
                check_opt_node(subject)?;
                check_list(whens)?;
                check_opt_node(else_)?;
            }
            NodeKind::When { conds, body } => {
                check_list(conds)?;
                check_opt_node(body)?;
            }
            NodeKind::And { lhs, rhs } | NodeKind::Or { lhs, rhs } => {
                check_node(lhs.0)?;
                check_node(rhs.0)?;
            }
            NodeKind::Def {
                receiver,
                name,
                args,
                body,
            } => {
                check_opt_node(receiver)?;
                check_sym(name.0)?;
                check_node(args.0)?;
                check_opt_node(body)?;
            }
            NodeKind::Class {
                name,
                superclass,
                body,
            } => {
                check_node(name.0)?;
                check_opt_node(superclass)?;
                check_opt_node(body)?;
            }
            NodeKind::Module { name, body } => {
                check_node(name.0)?;
                check_opt_node(body)?;
            }
            NodeKind::Optarg { name, default } | NodeKind::Kwoptarg { name, default } => {
                check_sym(name.0)?;
                check_node(default.0)?;
            }
            NodeKind::While { cond, body, .. } | NodeKind::Until { cond, body, .. } => {
                check_node(cond.0)?;
                check_opt_node(body)?;
            }
            NodeKind::RangeExpr { begin_, end_, .. } => {
                check_opt_node(begin_)?;
                check_opt_node(end_)?;
            }
            NodeKind::Sclass { expr, body } => {
                check_node(expr.0)?;
                check_opt_node(body)?;
            }
            NodeKind::Defined(n) => check_node(n.0)?,
            NodeKind::Rescue {
                body,
                resbodies,
                else_,
            } => {
                check_opt_node(body)?;
                check_list(resbodies)?;
                check_opt_node(else_)?;
            }
            NodeKind::Resbody {
                exceptions,
                var,
                body,
            } => {
                check_list(exceptions)?;
                check_opt_node(var)?;
                check_opt_node(body)?;
            }
            NodeKind::Ensure { body, ensure_ } => {
                check_opt_node(body)?;
                check_opt_node(ensure_)?;
            }
            NodeKind::OpAsgn { target, op, value } => {
                check_node(target.0)?;
                check_sym(op.0)?;
                check_node(value.0)?;
            }
            NodeKind::OrAsgn { target, value } | NodeKind::AndAsgn { target, value } => {
                check_node(target.0)?;
                check_node(value.0)?;
            }
            NodeKind::Regexp { parts, opts } => {
                check_list(parts)?;
                check_sym(opts.0)?;
            }
            NodeKind::Masgn { lhs, rhs } => {
                check_node(lhs.0)?;
                check_node(rhs.0)?;
            }
            // ── murphy-w5ba HIGH-priority extensions ──────────────────
            NodeKind::For { var, iter, body } => {
                check_node(var.0)?;
                check_node(iter.0)?;
                check_opt_node(body)?;
            }
            NodeKind::Lambda
            | NodeKind::Cbase
            | NodeKind::Retry
            | NodeKind::Redo
            | NodeKind::ForwardArgs
            | NodeKind::ForwardedArgs => {}
            NodeKind::Defs {
                receiver,
                name,
                args,
                body,
            } => {
                check_node(receiver.0)?;
                check_sym(name.0)?;
                check_node(args.0)?;
                check_opt_node(body)?;
            }
            NodeKind::Index { receiver, args } => {
                check_node(receiver.0)?;
                check_list(args)?;
            }
            NodeKind::IndexAsgn {
                receiver,
                args,
                value,
            } => {
                check_node(receiver.0)?;
                check_list(args)?;
                check_node(value.0)?;
            }
            NodeKind::Kwbegin(l) | NodeKind::Regopt(l) | NodeKind::Procarg0(l) => {
                check_list(l)?;
            }
            // `Rational(StringId)` / `Complex(StringId)` route through the
            // same `check_sym` as `NodeKind::Str(StringId)` because Murphy's
            // interner is shared between `Symbol` and `StringId` ids — see
            // `interner.rs`. The closure name is historical; both id types
            // share its bounds check.
            NodeKind::Rational(s) | NodeKind::Complex(s) => check_sym(s.0)?,
            NodeKind::Not(n) => check_node(n.0)?,
            NodeKind::Numblock { send, body, .. } => {
                check_node(send.0)?;
                check_opt_node(body)?;
            }
        }
    }
    for id in &ast.node_lists {
        check_node(id.0)?;
    }
    if ast.root.0 >= node_count {
        return Err(SerError::BadRoot {
            id: ast.root.0,
            count: node_count,
        });
    }
    Ok(())
}

impl Ast {
    /// Serialize to a flat byte buffer with an 88-byte header. Round-trips
    /// bit-exactly via [`Ast::from_bytes`]. Returns [`SerError::PathNotUtf8`]
    /// when `source.path` is not valid UTF-8 — the on-disk format encodes
    /// the path as UTF-8 and silently lossy-converting (`U+FFFD`) would
    /// break the bit-equal round-trip contract.
    pub fn to_bytes(&self) -> Result<Vec<u8>, SerError> {
        let mut out = Vec::new();
        write_header(&self.source.text, &mut out);

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

        // 6. source tokens
        put_u64(&mut out, self.source_tokens.len() as u64);
        for t in &self.source_tokens {
            write_source_token(*t, &mut out);
        }

        // 7. source text
        put_u64(&mut out, self.source.text.len() as u64);
        out.extend_from_slice(self.source.text.as_bytes());

        // 8. source path — UTF-8 only. Non-UTF-8 OS paths cannot round-trip
        // through the on-disk format, so reject them here instead of
        // lossy-converting and producing a buffer whose path field will not
        // match the original `PathBuf` on read-back.
        let path = self.source.path.to_str().ok_or(SerError::PathNotUtf8)?;
        let path_bytes = path.as_bytes();
        put_u64(&mut out, path_bytes.len() as u64);
        out.extend_from_slice(path_bytes);

        // 9. root
        put_u32(&mut out, self.root.0);

        Ok(out)
    }

    /// Deserialize a buffer produced by [`Ast::to_bytes`]. Validates the
    /// header (magic / format version / Murphy version / target triple /
    /// content hash) before reading the body, and runs a one-pass index
    /// check over the deserialized arena so a malformed buffer surfaces as
    /// a `Result::Err` rather than as a later traversal-time panic.
    pub fn from_bytes(bytes: &[u8]) -> Result<Ast, SerError> {
        let mut cur = bytes;

        let recorded_hash = read_header(&mut cur)?;

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

        // 6. source tokens
        let token_count =
            usize::try_from(get_u64(&mut cur)?).map_err(|_| SerError::UnexpectedEof)?;
        let mut source_tokens = Vec::with_capacity(token_count);
        for _ in 0..token_count {
            source_tokens.push(read_source_token(&mut cur)?);
        }

        // 7. source text
        let text_len = usize::try_from(get_u64(&mut cur)?).map_err(|_| SerError::UnexpectedEof)?;
        let text_bytes = take(&mut cur, text_len)?.to_vec();
        let text = String::from_utf8(text_bytes).map_err(|_| SerError::InvalidUtf8)?;

        // Self-verify: the recorded content hash must match the source text.
        if content_hash(text.as_bytes()) != recorded_hash {
            return Err(SerError::ContentHashMismatch);
        }

        // 8. source path — UTF-8 only; mirrored with the writer's
        // `PathNotUtf8` rejection.
        let path_len = usize::try_from(get_u64(&mut cur)?).map_err(|_| SerError::UnexpectedEof)?;
        let path_bytes = take(&mut cur, path_len)?.to_vec();
        let path_string = String::from_utf8(path_bytes).map_err(|_| SerError::PathNotUtf8)?;
        let path = std::path::PathBuf::from(path_string);

        // 9. root
        let root = NodeId(get_u32(&mut cur)?);

        let ast = Ast {
            nodes,
            node_lists,
            interner: Interner { blob, offsets },
            comments,
            source_tokens,
            source: SourceBuffer { text, path },
            root,
        };
        validate_indices(&ast)?;
        Ok(ast)
    }
}

#[cfg(test)]
mod tests {
    use crate::SerError;
    use crate::builder::AstBuilder;
    use crate::node::{
        CommentKind, NodeKind, NodeList, OptNodeId, Range, SourceToken, SourceTokenKind,
    };

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

        let bytes = ast.to_bytes().unwrap();
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

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
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

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
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

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
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

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
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

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
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

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
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

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
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

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
        assert_eq!(
            ast, restored,
            "exception variant round-trip must be bit-equal"
        );
    }

    #[test]
    fn round_trip_op_assign_variants() {
        // The three op-assign variants appended after `Ensure`
        // (discriminants 60..=62) must survive the byte round-trip. Covers
        // the struct-with-`op` shape (`OpAsgn`) and the two-`NodeId` shape
        // (`OrAsgn`/`AndAsgn`).
        let mut b = AstBuilder::new("x += 1; @y ||= 2; @@z &&= 3", "t.rb");
        // `x += 1` — target is a value-less `Lvasgn`.
        let x_name = b.intern_symbol("x");
        let x_target = b.push(
            NodeKind::Lvasgn {
                name: x_name,
                value: OptNodeId::NONE,
            },
            r(0, 1),
        );
        let plus = b.intern_symbol("+");
        let one = b.push(NodeKind::Int(1), r(5, 6));
        let op_asgn = b.push(
            NodeKind::OpAsgn {
                target: x_target,
                op: plus,
                value: one,
            },
            r(0, 6),
        );
        // `@y ||= 2` — target is a value-less `Ivasgn`.
        let y_name = b.intern_symbol("@y");
        let y_target = b.push(
            NodeKind::Ivasgn {
                name: y_name,
                value: OptNodeId::NONE,
            },
            r(8, 10),
        );
        let two = b.push(NodeKind::Int(2), r(15, 16));
        let or_asgn = b.push(
            NodeKind::OrAsgn {
                target: y_target,
                value: two,
            },
            r(8, 16),
        );
        // `@@z &&= 3` — target is a value-less `Cvasgn`.
        let z_name = b.intern_symbol("@@z");
        let z_target = b.push(
            NodeKind::Cvasgn {
                name: z_name,
                value: OptNodeId::NONE,
            },
            r(18, 21),
        );
        let three = b.push(NodeKind::Int(3), r(26, 27));
        let and_asgn = b.push(
            NodeKind::AndAsgn {
                target: z_target,
                value: three,
            },
            r(18, 27),
        );
        let list = b.push_list(&[op_asgn, or_asgn, and_asgn]);
        let root = b.push(NodeKind::Begin(list), r(0, 27));
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
        assert_eq!(
            ast, restored,
            "op-assign variant round-trip must be bit-equal"
        );
    }

    #[test]
    fn round_trip_interp_string_variants() {
        // The four interpolation variants appended after `AndAsgn`
        // (discriminants 63..=66) must survive the byte round-trip. Covers
        // the `NodeList` payloads (`Dstr`/`Dsym`/`Xstr`), populated and
        // empty, and the `Regexp` struct with both empty and non-empty
        // `opts`.
        let mut b = AstBuilder::new("\"a#{b}\"; :\"s\"; `ls`; /re/im", "t.rb");
        // `Dstr` — two parts.
        let part_a = b.push(NodeKind::Nil, r(1, 2));
        let part_b = b.push(NodeKind::Nil, r(2, 6));
        let dstr_list = b.push_list(&[part_a, part_b]);
        let dstr = b.push(NodeKind::Dstr(dstr_list), r(0, 7));
        // `Dsym` — single part.
        let sym_part = b.push(NodeKind::Nil, r(10, 11));
        let dsym_list = b.push_list(&[sym_part]);
        let dsym = b.push(NodeKind::Dsym(dsym_list), r(9, 13));
        // `Xstr` — empty parts list.
        let xstr = b.push(NodeKind::Xstr(NodeList::EMPTY), r(15, 19));
        // `Regexp` — non-empty opts.
        let re_part = b.push(NodeKind::Nil, r(22, 24));
        let re_list = b.push_list(&[re_part]);
        let opts_im = b.intern_symbol("im");
        let regexp = b.push(
            NodeKind::Regexp {
                parts: re_list,
                opts: opts_im,
            },
            r(21, 27),
        );
        // `Regexp` — empty opts (no flags).
        let opts_empty = b.intern_symbol("");
        let regexp_no_opts = b.push(
            NodeKind::Regexp {
                parts: NodeList::EMPTY,
                opts: opts_empty,
            },
            r(0, 0),
        );
        let list = b.push_list(&[dstr, dsym, xstr, regexp, regexp_no_opts]);
        let root = b.push(NodeKind::Begin(list), r(0, 27));
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
        assert_eq!(
            ast, restored,
            "interpolation variant round-trip must be bit-equal"
        );
    }

    #[test]
    fn round_trip_masgn_mlhs() {
        // The `Masgn`/`Mlhs` variants appended after `Regexp`
        // (discriminants 67/68) must survive the byte round-trip. Covers a
        // populated `Mlhs` and the `Masgn` lhs/rhs `NodeId` pair.
        let mut b = AstBuilder::new("a, b = 1, 2", "t.rb");
        let a_name = b.intern_symbol("a");
        let target_a = b.push(
            NodeKind::Lvasgn {
                name: a_name,
                value: OptNodeId::NONE,
            },
            r(0, 1),
        );
        let b_name = b.intern_symbol("b");
        let target_b = b.push(
            NodeKind::Lvasgn {
                name: b_name,
                value: OptNodeId::NONE,
            },
            r(3, 4),
        );
        let lhs_list = b.push_list(&[target_a, target_b]);
        let lhs = b.push(NodeKind::Mlhs(lhs_list), r(0, 4));
        let one = b.push(NodeKind::Int(1), r(7, 8));
        let two = b.push(NodeKind::Int(2), r(10, 11));
        let rhs_list = b.push_list(&[one, two]);
        let rhs = b.push(NodeKind::Array(rhs_list), r(7, 11));
        let masgn = b.push(NodeKind::Masgn { lhs, rhs }, r(0, 11));
        let list = b.push_list(&[masgn]);
        let root = b.push(NodeKind::Begin(list), r(0, 11));
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
        assert_eq!(ast, restored, "Masgn/Mlhs round-trip must be bit-equal");
    }

    #[test]
    fn round_trip_empty_ast() {
        let mut b = AstBuilder::new("", "e.rb");
        let root = b.push(NodeKind::Nil, r(0, 0));
        let ast = b.finish(root);
        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).unwrap();
        assert_eq!(ast, restored);
    }

    #[test]
    fn round_trip_source_tokens() {
        let mut b = AstBuilder::new("foo(1)\n", "t.rb");
        let root = b.push(NodeKind::Int(1), r(4, 5));
        b.add_source_token(SourceToken {
            kind: SourceTokenKind::LeftParen,
            range: r(3, 4),
        });
        b.add_source_token(SourceToken {
            kind: SourceTokenKind::Newline,
            range: r(6, 7),
        });
        let ast = b.finish(root);

        let restored = crate::Ast::from_bytes(&ast.to_bytes().unwrap()).expect("round-trip");
        assert_eq!(restored.sorted_tokens(), ast.sorted_tokens());
    }

    #[test]
    fn from_bytes_rejects_bad_source_token_kind() {
        let mut b = AstBuilder::new("foo(1)", "t.rb");
        let root = b.push(NodeKind::Int(1), r(4, 5));
        b.add_source_token(SourceToken {
            kind: SourceTokenKind::LeftParen,
            range: r(3, 4),
        });
        let ast = b.finish(root);
        let mut bytes = ast.to_bytes().unwrap();
        let encoded_token = [3, 0, 0, 0, 4, 0, 0, 0, SourceTokenKind::LeftParen as u8];
        let token_at = bytes
            .windows(encoded_token.len())
            .position(|window| window == encoded_token)
            .expect("encoded source token present");
        bytes[token_at + encoded_token.len() - 1] = 99;

        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::BadDiscriminant)
        ));
    }

    // --- header / validation tests (murphy-9cr.26) ---

    fn simple_ast() -> crate::Ast {
        let mut b = AstBuilder::new("x = 1", "t.rb");
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
        b.finish(root)
    }

    #[test]
    fn to_bytes_starts_with_magic() {
        let ast = simple_ast();
        let bytes = ast.to_bytes().unwrap();
        assert_eq!(&bytes[0..8], super::MAGIC);
    }

    #[test]
    fn from_bytes_rejects_bad_magic() {
        let ast = simple_ast();
        let mut bytes = ast.to_bytes().unwrap();
        bytes[0] = b'X';
        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::BadMagic)
        ));
    }

    #[test]
    fn from_bytes_rejects_format_version_mismatch() {
        let ast = simple_ast();
        let mut bytes = ast.to_bytes().unwrap();
        let bumped = (super::FORMAT_VERSION + 1).to_le_bytes();
        bytes[8..12].copy_from_slice(&bumped);
        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::FormatVersionMismatch { .. })
        ));
    }

    #[test]
    fn from_bytes_rejects_murphy_version_mismatch() {
        let ast = simple_ast();
        let mut bytes = ast.to_bytes().unwrap();
        bytes[16] ^= 0xFF;
        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::MurphyVersionMismatch)
        ));
    }

    #[test]
    fn from_bytes_rejects_target_mismatch() {
        let ast = simple_ast();
        let mut bytes = ast.to_bytes().unwrap();
        bytes[32] ^= 0xFF;
        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::TargetMismatch)
        ));
    }

    #[test]
    fn from_bytes_rejects_content_hash_mismatch() {
        let ast = simple_ast();
        let mut bytes = ast.to_bytes().unwrap();
        // content_hash lives at offset 64 (8 magic + 4 fmt + 4 reserved +
        // 16 murphy_version + 32 target_triple = 64).
        bytes[64] ^= 0xFF;
        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::ContentHashMismatch)
        ));
    }

    #[test]
    fn header_length_is_96_bytes() {
        // Lock the header size so downstream code (cache lookups, mmap)
        // can rely on a fixed offset for the body. Also verify `to_bytes`
        // actually emits that many header bytes before any body content.
        assert_eq!(super::HEADER_LEN, 96);
        let bytes = simple_ast().to_bytes().unwrap();
        assert!(bytes.len() > super::HEADER_LEN);
    }

    // --- bounds validation tests ---

    fn ast_with_corrupt<F: FnOnce(&mut crate::Ast)>(corrupt: F) -> crate::Ast {
        let mut ast = simple_ast();
        corrupt(&mut ast);
        ast
    }

    #[test]
    fn from_bytes_rejects_node_id_out_of_range() {
        let ast = ast_with_corrupt(|ast| {
            let bad_id = ast.nodes.len() as u32 + 5;
            let lvasgn_idx = ast
                .nodes
                .iter()
                .position(|n| matches!(n.kind, NodeKind::Lvasgn { .. }))
                .unwrap();
            if let NodeKind::Lvasgn { ref mut value, .. } = ast.nodes[lvasgn_idx].kind {
                *value = OptNodeId(bad_id);
            }
        });
        let bytes = ast.to_bytes().unwrap();
        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::NodeIdOutOfRange { .. })
        ));
    }

    #[test]
    fn from_bytes_rejects_symbol_out_of_range() {
        let ast = ast_with_corrupt(|ast| {
            let bad_sym = ast.interner.offsets.len() as u32 + 5;
            let lvasgn_idx = ast
                .nodes
                .iter()
                .position(|n| matches!(n.kind, NodeKind::Lvasgn { .. }))
                .unwrap();
            if let NodeKind::Lvasgn { ref mut name, .. } = ast.nodes[lvasgn_idx].kind {
                *name = crate::Symbol(bad_sym);
            }
        });
        let bytes = ast.to_bytes().unwrap();
        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::SymbolOutOfRange { .. })
        ));
    }

    #[test]
    fn from_bytes_rejects_node_list_range_out_of_range() {
        let ast = ast_with_corrupt(|ast| {
            let begin_idx = ast
                .nodes
                .iter()
                .position(|n| matches!(n.kind, NodeKind::Begin(_)))
                .unwrap();
            if let NodeKind::Begin(ref mut list) = ast.nodes[begin_idx].kind {
                list.len = 999;
            }
        });
        let bytes = ast.to_bytes().unwrap();
        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::BadNodeListRange { .. })
        ));
    }

    #[test]
    fn from_bytes_rejects_bad_root() {
        let ast = ast_with_corrupt(|ast| ast.root = crate::NodeId(999));
        let bytes = ast.to_bytes().unwrap();
        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::BadRoot { .. })
        ));
    }

    #[test]
    fn from_bytes_rejects_invalid_node_in_node_lists() {
        // `validate_indices` walks every entry in `node_lists`, not just the
        // entries currently referenced by a `NodeList { start, len }` slice,
        // so a stray bad NodeId at the end is enough to trip the check.
        let ast = ast_with_corrupt(|ast| ast.node_lists.push(crate::NodeId(999)));
        let bytes = ast.to_bytes().unwrap();
        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::NodeIdOutOfRange { .. })
        ));
    }

    // murphy-g2u: the on-disk format encodes `source.path` as UTF-8, so a
    // non-UTF-8 OS path (only constructible on Unix) cannot round-trip.
    // Writer must reject it instead of silently lossy-converting through
    // `to_string_lossy`, which would replace bytes with `U+FFFD` and break
    // the bit-equal round-trip contract.
    #[cfg(unix)]
    #[test]
    fn to_bytes_rejects_non_utf8_path() {
        use std::os::unix::ffi::OsStrExt;
        let mut ast = simple_ast();
        let bad = std::ffi::OsStr::from_bytes(&[0xFF, b'/', b'x', b'.', b'r', b'b']);
        ast.source.path = std::path::PathBuf::from(bad);
        assert!(matches!(ast.to_bytes(), Err(SerError::PathNotUtf8)));
    }

    // Reader side is mirrored: a buffer whose recorded path bytes are not
    // UTF-8 surfaces as `PathNotUtf8` (rather than the generic
    // `InvalidUtf8`), keeping writer/reader errors symmetric.
    #[test]
    fn from_bytes_rejects_non_utf8_path() {
        let ast = simple_ast();
        let mut bytes = ast.to_bytes().unwrap();
        // The path is the last variable-length field before the 4-byte root.
        // Mutate the final byte of the path payload to an invalid UTF-8 lead.
        let root_at = bytes.len() - 4;
        bytes[root_at - 1] = 0xFF;
        assert!(matches!(
            crate::Ast::from_bytes(&bytes),
            Err(SerError::PathNotUtf8)
        ));
    }
}
