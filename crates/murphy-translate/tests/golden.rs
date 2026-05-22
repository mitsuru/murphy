//! prism→arena 翻訳の S 式ゴールデンテスト。
//!
//! `BLESS=1 cargo test -p murphy-translate --test golden` で snapshot 再生成。
//!
//! S 式プリンタは [`murphy_ast::Ast`] の公開 API（`root` / `kind` /
//! `children` / `interner`）だけを使う。固定の `NodeId`/`OptNodeId` フィールドは
//! `match` の束縛から直接描画し（`OptNodeId::NONE` は `nil` プレースホルダで
//! 位置を保つ）、可変長の `NodeList` フィールドは `children()`（ソース順）を
//! 既知のレイアウトでスライスして取り出す。`write_node` の `match` は
//! `NodeKind` に対し網羅的（`_` arm なし）であり、将来 variant が追加されると
//! 本ファイルがコンパイルエラーになって出力漏れを強制的に検出する。

use murphy_ast::{Ast, NodeId, NodeKind, OptNodeId};
use std::fmt::Write as _;
use std::path::PathBuf;

/// `Ast` 全体を S 式テキスト 1 本にダンプする。
fn sexp(ast: &Ast) -> String {
    let mut out = String::new();
    write_node(ast, ast.root(), 0, &mut out);
    out.push('\n');
    out
}

/// インデント（深さ × 2 スペース）を書き込む。
fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// `OptNodeId` を再帰描画する。`NONE` は `nil` リテラル。
fn write_opt(ast: &Ast, id: OptNodeId, depth: usize, out: &mut String) {
    match id.get() {
        Some(n) => write_node(ast, n, depth, out),
        None => {
            indent(depth, out);
            out.push_str("nil");
        }
    }
}

/// `children()` を `skip` 個飛ばし `take` 個取った各要素を 1 行ずつ
/// 改行区切りで再帰描画する。
fn write_slice(ast: &Ast, id: NodeId, skip: usize, take: usize, depth: usize, out: &mut String) {
    for child in ast.children(id).skip(skip).take(take) {
        out.push('\n');
        write_node(ast, child, depth, out);
    }
}

/// 1 ノードを `(kind ...)` 形式で描画する。子は改行 + インデントで続ける。
///
/// `match` は `NodeKind` に対し **網羅的**（`_` arm なし）。新 variant の
/// 追加はここをコンパイルエラーにし、S 式出力の更新を強制する。
fn write_node(ast: &Ast, id: NodeId, depth: usize, out: &mut String) {
    indent(depth, out);
    let interner = ast.interner();
    let d = depth + 1;
    match *ast.kind(id) {
        NodeKind::Error => out.push_str("(error)"),

        // --- atoms / literals ---
        NodeKind::Nil => out.push_str("(nil)"),
        NodeKind::True_ => out.push_str("(true)"),
        NodeKind::False_ => out.push_str("(false)"),
        NodeKind::SelfExpr => out.push_str("(self)"),
        NodeKind::Int(v) => {
            let _ = write!(out, "(int {v})");
        }
        NodeKind::Float(v) => {
            let _ = write!(out, "(float {v})");
        }
        NodeKind::Str(s) => {
            let _ = write!(out, "(str {:?})", interner.resolve(s.0));
        }
        NodeKind::Sym(s) => {
            let _ = write!(out, "(sym :{})", interner.resolve(s.0));
        }

        // --- variable reads ---
        NodeKind::Lvar(s) => {
            let _ = write!(out, "(lvar {})", interner.resolve(s.0));
        }
        NodeKind::Ivar(s) => {
            let _ = write!(out, "(ivar {})", interner.resolve(s.0));
        }
        NodeKind::Cvar(s) => {
            let _ = write!(out, "(cvar {})", interner.resolve(s.0));
        }
        NodeKind::Gvar(s) => {
            let _ = write!(out, "(gvar {})", interner.resolve(s.0));
        }
        NodeKind::Const { scope, name } => {
            let _ = write!(out, "(const :{}\n", interner.resolve(name.0));
            write_opt(ast, scope, d, out);
            out.push(')');
        }

        // --- assignments ---
        NodeKind::Lvasgn { name, value } => {
            let _ = write!(out, "(lvasgn {}\n", interner.resolve(name.0));
            write_opt(ast, value, d, out);
            out.push(')');
        }
        NodeKind::Ivasgn { name, value } => {
            let _ = write!(out, "(ivasgn {}\n", interner.resolve(name.0));
            write_opt(ast, value, d, out);
            out.push(')');
        }
        NodeKind::Gvasgn { name, value } => {
            let _ = write!(out, "(gvasgn {}\n", interner.resolve(name.0));
            write_opt(ast, value, d, out);
            out.push(')');
        }
        NodeKind::Cvasgn { name, value } => {
            let _ = write!(out, "(cvasgn {}\n", interner.resolve(name.0));
            write_opt(ast, value, d, out);
            out.push(')');
        }
        NodeKind::Casgn {
            scope,
            name,
            value,
        } => {
            let _ = write!(out, "(casgn :{}\n", interner.resolve(name.0));
            write_opt(ast, scope, d, out);
            out.push('\n');
            write_opt(ast, value, d, out);
            out.push(')');
        }

        // --- calls / blocks ---
        // collect_children: [receiver?] ++ args。receiver の有無で先頭を skip。
        NodeKind::Send {
            receiver, method, ..
        } => {
            let _ = write!(out, "(send :{}\n", interner.resolve(method.0));
            write_opt(ast, receiver, d, out);
            write_slice(ast, id, receiver.get().is_some() as usize, usize::MAX, d, out);
            out.push(')');
        }
        // collect_children: [receiver] ++ args。receiver は常在。
        NodeKind::Csend {
            receiver, method, ..
        } => {
            let _ = write!(out, "(csend :{}\n", interner.resolve(method.0));
            write_node(ast, receiver, d, out);
            write_slice(ast, id, 1, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Block { call, args, body } => {
            out.push_str("(block\n");
            write_node(ast, call, d, out);
            out.push('\n');
            write_node(ast, args, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::BlockPass(inner) => {
            out.push_str("(block-pass\n");
            write_opt(ast, inner, d, out);
            out.push(')');
        }
        NodeKind::Splat(inner) => {
            out.push_str("(splat\n");
            write_opt(ast, inner, d, out);
            out.push(')');
        }

        // --- collections ---
        // 純リスト variant: children() がそのままリスト要素。
        NodeKind::Array(_) => {
            out.push_str("(array");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Hash(_) => {
            out.push_str("(hash");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Pair { key, value } => {
            out.push_str("(pair\n");
            write_node(ast, key, d, out);
            out.push('\n');
            write_node(ast, value, d, out);
            out.push(')');
        }
        NodeKind::Kwsplat(inner) => {
            out.push_str("(kwsplat\n");
            write_opt(ast, inner, d, out);
            out.push(')');
        }

        // --- control flow ---
        NodeKind::If { cond, then_, else_ } => {
            out.push_str("(if\n");
            write_node(ast, cond, d, out);
            out.push('\n');
            write_opt(ast, then_, d, out);
            out.push('\n');
            write_opt(ast, else_, d, out);
            out.push(')');
        }
        // collect_children: [subject?] ++ whens ++ [else?]。
        NodeKind::Case {
            subject,
            whens,
            else_,
        } => {
            out.push_str("(case\n");
            write_opt(ast, subject, d, out);
            write_slice(
                ast,
                id,
                subject.get().is_some() as usize,
                whens.len as usize,
                d,
                out,
            );
            out.push('\n');
            write_opt(ast, else_, d, out);
            out.push(')');
        }
        // collect_children: conds ++ [body?]。
        NodeKind::When { conds, body } => {
            out.push_str("(when");
            write_slice(ast, id, 0, conds.len as usize, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::Begin(_) => {
            out.push_str("(begin");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Return(inner) => {
            out.push_str("(return\n");
            write_opt(ast, inner, d, out);
            out.push(')');
        }
        NodeKind::And { lhs, rhs } => {
            out.push_str("(and\n");
            write_node(ast, lhs, d, out);
            out.push('\n');
            write_node(ast, rhs, d, out);
            out.push(')');
        }
        NodeKind::Or { lhs, rhs } => {
            out.push_str("(or\n");
            write_node(ast, lhs, d, out);
            out.push('\n');
            write_node(ast, rhs, d, out);
            out.push(')');
        }
        NodeKind::While { cond, body, post } => {
            let _ = write!(out, "(while post={post}\n");
            write_node(ast, cond, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::Until { cond, body, post } => {
            let _ = write!(out, "(until post={post}\n");
            write_node(ast, cond, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::RangeExpr {
            begin_,
            end_,
            exclusive,
        } => {
            let _ = write!(out, "(range exclusive={exclusive}\n");
            write_opt(ast, begin_, d, out);
            out.push('\n');
            write_opt(ast, end_, d, out);
            out.push(')');
        }
        NodeKind::Break(inner) => {
            out.push_str("(break\n");
            write_opt(ast, inner, d, out);
            out.push(')');
        }
        NodeKind::Next(inner) => {
            out.push_str("(next\n");
            write_opt(ast, inner, d, out);
            out.push(')');
        }
        NodeKind::Yield(_) => {
            out.push_str("(yield");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Super(_) => {
            out.push_str("(super");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Zsuper => out.push_str("(zsuper)"),
        NodeKind::Defined(inner) => {
            out.push_str("(defined\n");
            write_node(ast, inner, d, out);
            out.push(')');
        }

        // --- definitions ---
        NodeKind::Def {
            receiver,
            name,
            args,
            body,
        } => {
            let _ = write!(out, "(def :{}\n", interner.resolve(name.0));
            write_opt(ast, receiver, d, out);
            out.push('\n');
            write_node(ast, args, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::Class {
            name,
            superclass,
            body,
        } => {
            out.push_str("(class\n");
            write_node(ast, name, d, out);
            out.push('\n');
            write_opt(ast, superclass, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::Module { name, body } => {
            out.push_str("(module\n");
            write_node(ast, name, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::Sclass { expr, body } => {
            out.push_str("(sclass\n");
            write_node(ast, expr, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }

        // --- arguments ---
        NodeKind::Args(_) => {
            out.push_str("(args");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Arg(s) => {
            let _ = write!(out, "(arg {})", interner.resolve(s.0));
        }
        NodeKind::Optarg { name, default } => {
            let _ = write!(out, "(optarg {}\n", interner.resolve(name.0));
            write_node(ast, default, d, out);
            out.push(')');
        }
        NodeKind::Restarg(s) => {
            let _ = write!(out, "(restarg {:?})", interner.resolve(s.0));
        }
        NodeKind::Kwarg(s) => {
            let _ = write!(out, "(kwarg {})", interner.resolve(s.0));
        }
        NodeKind::Kwoptarg { name, default } => {
            let _ = write!(out, "(kwoptarg {}\n", interner.resolve(name.0));
            write_node(ast, default, d, out);
            out.push(')');
        }
        NodeKind::Kwrestarg(s) => {
            let _ = write!(out, "(kwrestarg {:?})", interner.resolve(s.0));
        }
        NodeKind::Blockarg(s) => {
            let _ = write!(out, "(blockarg {:?})", interner.resolve(s.0));
        }

        // --- exceptions ---
        // collect_children: [body?] ++ resbodies ++ [else?]。
        NodeKind::Rescue {
            body,
            resbodies,
            else_,
        } => {
            out.push_str("(rescue\n");
            write_opt(ast, body, d, out);
            write_slice(
                ast,
                id,
                body.get().is_some() as usize,
                resbodies.len as usize,
                d,
                out,
            );
            out.push('\n');
            write_opt(ast, else_, d, out);
            out.push(')');
        }
        // collect_children: exceptions ++ [var?] ++ [body?]。
        NodeKind::Resbody {
            exceptions,
            var,
            body,
        } => {
            out.push_str("(resbody\n");
            indent(d, out);
            out.push_str("(exceptions");
            for child in ast.children(id).take(exceptions.len as usize) {
                out.push('\n');
                write_node(ast, child, d + 1, out);
            }
            out.push_str(")\n");
            write_opt(ast, var, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::Ensure { body, ensure_ } => {
            out.push_str("(ensure\n");
            write_opt(ast, body, d, out);
            out.push('\n');
            write_opt(ast, ensure_, d, out);
            out.push(')');
        }

        // --- op-assign ---
        NodeKind::OpAsgn { target, op, value } => {
            let _ = write!(out, "(op-asgn :{}\n", interner.resolve(op.0));
            write_node(ast, target, d, out);
            out.push('\n');
            write_node(ast, value, d, out);
            out.push(')');
        }
        NodeKind::OrAsgn { target, value } => {
            out.push_str("(or-asgn\n");
            write_node(ast, target, d, out);
            out.push('\n');
            write_node(ast, value, d, out);
            out.push(')');
        }
        NodeKind::AndAsgn { target, value } => {
            out.push_str("(and-asgn\n");
            write_node(ast, target, d, out);
            out.push('\n');
            write_node(ast, value, d, out);
            out.push(')');
        }

        // --- string interpolation / regexp / xstring ---
        NodeKind::Dstr(_) => {
            out.push_str("(dstr");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Dsym(_) => {
            out.push_str("(dsym");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Xstr(_) => {
            out.push_str("(xstr");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Regexp { opts, .. } => {
            let _ = write!(out, "(regexp opts={:?}", interner.resolve(opts.0));
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }

        // --- multiple assignment ---
        NodeKind::Masgn { lhs, rhs } => {
            out.push_str("(masgn\n");
            write_node(ast, lhs, d, out);
            out.push('\n');
            write_node(ast, rhs, d, out);
            out.push(')');
        }
        NodeKind::Mlhs(_) => {
            out.push_str("(mlhs");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }

        // --- fallback ---
        NodeKind::Unknown => out.push_str("(unknown)"),
    }
}

/// `fixtures/<name>.rb` を翻訳し、S 式を `snapshots/<name>.sexp` と照合する。
/// `BLESS` 環境変数があれば snapshot を上書きする。
fn check(name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let src = std::fs::read_to_string(dir.join("fixtures").join(format!("{name}.rb"))).unwrap();
    let ast = murphy_translate::translate(&src, format!("{name}.rb"));
    let got = sexp(&ast);
    let snap_path = dir.join("snapshots").join(format!("{name}.sexp"));
    if std::env::var("BLESS").is_ok() {
        std::fs::write(&snap_path, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&snap_path).unwrap_or_default();
    assert_eq!(got, want, "snapshot mismatch for {name}; BLESS=1 to re-bless");
}

#[test]
fn golden_control_flow() {
    check("control_flow");
}

#[test]
fn golden_method_def() {
    check("method_def");
}

#[test]
fn golden_mixed() {
    check("mixed");
}
