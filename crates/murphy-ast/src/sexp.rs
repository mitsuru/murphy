//! Textual S-expression printer for an [`Ast`].
//!
//! Counterpart to [`crate::serialize`] (binary): this dumps the arena as
//! human-readable, indented S-expression text suitable for `murphy ast
//! --format sexp` and for golden snapshot tests in `murphy-translate`.
//!
//! The [`write_node`] `match` is exhaustive on [`NodeKind`]; a new variant
//! breaks compilation here, so the printer never silently drops a node.
//!
//! Output convention: the returned string has **no trailing newline**.
//! Callers that want a single terminating newline append one themselves
//! (e.g. `writeln!(stdout, "{}", ast_to_sexp(&ast))`). Snapshot files
//! historically carry a trailing newline; the snapshot driver adds it.

use crate::ast::Ast;
use crate::node::{NodeId, NodeKind, OptNodeId};
use std::fmt::Write as _;

/// Render `ast` as an indented S-expression string. Output has no
/// trailing newline.
pub fn ast_to_sexp(ast: &Ast) -> String {
    let mut out = String::new();
    write_node(ast, ast.root(), 0, &mut out);
    out
}

fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_opt(ast: &Ast, id: OptNodeId, depth: usize, out: &mut String) {
    match id.get() {
        Some(n) => write_node(ast, n, depth, out),
        None => {
            indent(depth, out);
            out.push_str("nil");
        }
    }
}

fn write_slice(ast: &Ast, id: NodeId, skip: usize, take: usize, depth: usize, out: &mut String) {
    for child in ast.children(id).skip(skip).take(take) {
        out.push('\n');
        write_node(ast, child, depth, out);
    }
}

fn write_node(ast: &Ast, id: NodeId, depth: usize, out: &mut String) {
    indent(depth, out);
    let interner = ast.interner();
    let d = depth + 1;
    match *ast.kind(id) {
        NodeKind::Error => out.push_str("(error)"),

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
            let _ = writeln!(out, "(const :{}", interner.resolve(name.0));
            write_opt(ast, scope, d, out);
            out.push(')');
        }

        NodeKind::Lvasgn { name, value } => {
            let _ = writeln!(out, "(lvasgn {}", interner.resolve(name.0));
            write_opt(ast, value, d, out);
            out.push(')');
        }
        NodeKind::Ivasgn { name, value } => {
            let _ = writeln!(out, "(ivasgn {}", interner.resolve(name.0));
            write_opt(ast, value, d, out);
            out.push(')');
        }
        NodeKind::Gvasgn { name, value } => {
            let _ = writeln!(out, "(gvasgn {}", interner.resolve(name.0));
            write_opt(ast, value, d, out);
            out.push(')');
        }
        NodeKind::Cvasgn { name, value } => {
            let _ = writeln!(out, "(cvasgn {}", interner.resolve(name.0));
            write_opt(ast, value, d, out);
            out.push(')');
        }
        NodeKind::Casgn { scope, name, value } => {
            let _ = writeln!(out, "(casgn :{}", interner.resolve(name.0));
            write_opt(ast, scope, d, out);
            out.push('\n');
            write_opt(ast, value, d, out);
            out.push(')');
        }

        NodeKind::Send {
            receiver, method, ..
        } => {
            let _ = writeln!(out, "(send :{}", interner.resolve(method.0));
            write_opt(ast, receiver, d, out);
            write_slice(
                ast,
                id,
                receiver.get().is_some() as usize,
                usize::MAX,
                d,
                out,
            );
            out.push(')');
        }
        NodeKind::Csend {
            receiver, method, ..
        } => {
            let _ = writeln!(out, "(csend :{}", interner.resolve(method.0));
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

        NodeKind::If { cond, then_, else_ } => {
            out.push_str("(if\n");
            write_node(ast, cond, d, out);
            out.push('\n');
            write_opt(ast, then_, d, out);
            out.push('\n');
            write_opt(ast, else_, d, out);
            out.push(')');
        }
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
            let _ = writeln!(out, "(while post={post}");
            write_node(ast, cond, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::Until { cond, body, post } => {
            let _ = writeln!(out, "(until post={post}");
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
            let _ = writeln!(out, "(range exclusive={exclusive}");
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

        NodeKind::Def {
            receiver,
            name,
            args,
            body,
        } => {
            let _ = writeln!(out, "(def :{}", interner.resolve(name.0));
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

        NodeKind::Args(_) => {
            out.push_str("(args");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Arg(s) => {
            let _ = write!(out, "(arg {})", interner.resolve(s.0));
        }
        NodeKind::Optarg { name, default } => {
            let _ = writeln!(out, "(optarg {}", interner.resolve(name.0));
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
            let _ = writeln!(out, "(kwoptarg {}", interner.resolve(name.0));
            write_node(ast, default, d, out);
            out.push(')');
        }
        NodeKind::Kwrestarg(s) => {
            let _ = write!(out, "(kwrestarg {:?})", interner.resolve(s.0));
        }
        NodeKind::Blockarg(s) => {
            let _ = write!(out, "(blockarg {:?})", interner.resolve(s.0));
        }

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

        NodeKind::OpAsgn { target, op, value } => {
            let _ = writeln!(out, "(op-asgn :{}", interner.resolve(op.0));
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

        // ── murphy-w5ba HIGH-priority extensions ─────────────────────────
        NodeKind::For { var, iter, body } => {
            out.push_str("(for\n");
            write_node(ast, var, d, out);
            out.push('\n');
            write_node(ast, iter, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::Lambda => out.push_str("(lambda)"),
        NodeKind::Defs {
            receiver,
            name,
            args,
            body,
        } => {
            // parser-gem: `(defs receiver :name args body)` — keep that
            // ordering so golden tests and downstream tooling that compares
            // against parser-gem's shape see the expected layout.
            out.push_str("(defs\n");
            write_node(ast, receiver, d, out);
            out.push('\n');
            indent(d, out);
            let _ = writeln!(out, ":{}", interner.resolve(name.0));
            write_node(ast, args, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::Index { receiver, .. } => {
            out.push_str("(index\n");
            write_node(ast, receiver, d, out);
            write_slice(ast, id, 1, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::IndexAsgn {
            receiver, value, ..
        } => {
            out.push_str("(indexasgn\n");
            write_node(ast, receiver, d, out);
            // `collect_children` lays IndexAsgn out as receiver + args... + value
            // (value last). The receiver is already emitted above and the value
            // is emitted explicitly below; the middle slice is the args run.
            // `usize::MAX` for `take` would walk past `value` and emit it
            // twice, so cap the take count to the args length.
            let args_count = ast.children(id).count().saturating_sub(2);
            write_slice(ast, id, 1, args_count, d, out);
            out.push('\n');
            write_node(ast, value, d, out);
            out.push(')');
        }
        NodeKind::Kwbegin(_) => {
            out.push_str("(kwbegin");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Cbase => out.push_str("(cbase)"),
        NodeKind::Regopt(s) => {
            let _ = write!(out, "(regopt :{})", interner.resolve(s.0));
        }
        NodeKind::Rational(s) => {
            let _ = write!(out, "(rational {})", interner.resolve(s.0));
        }
        NodeKind::Complex(s) => {
            let _ = write!(out, "(complex {})", interner.resolve(s.0));
        }
        NodeKind::Not(n) => {
            out.push_str("(not\n");
            write_node(ast, n, d, out);
            out.push(')');
        }
        NodeKind::Retry => out.push_str("(retry)"),
        NodeKind::Redo => out.push_str("(redo)"),
        NodeKind::Numblock { send, max_n, body } => {
            let _ = writeln!(out, "(numblock max_n={max_n}");
            write_node(ast, send, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::Procarg0(_) => {
            out.push_str("(procarg0");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::ForwardArgs => out.push_str("(forward_args)"),
        NodeKind::ForwardedArgs => out.push_str("(forwarded_args)"),

        // ── murphy-o57f MID-priority extensions ─────────────────────────
        NodeKind::CaseMatch {
            subject,
            in_patterns,
            else_body,
        } => {
            out.push_str("(case_match\n");
            write_node(ast, subject, d, out);
            // Use the structural arm count directly. `collect_children`
            // appends `else_body` only when it's `Some`, so deriving the
            // arm count from `children.len()` requires branching on
            // `else_body.is_some()` — using `in_patterns.len` cuts out the
            // branch and stays correct for both with-else and no-else
            // case_match shapes.
            write_slice(ast, id, 1, in_patterns.len as usize, d, out);
            out.push('\n');
            write_opt(ast, else_body, d, out);
            out.push(')');
        }
        NodeKind::InPattern {
            pattern,
            guard,
            body,
        } => {
            out.push_str("(in_pattern\n");
            write_node(ast, pattern, d, out);
            out.push('\n');
            write_opt(ast, guard, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::ArrayPattern(_) => {
            out.push_str("(array_pattern");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::HashPattern(_) => {
            out.push_str("(hash_pattern");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::MatchVar(s) => {
            let _ = write!(out, "(match_var :{})", interner.resolve(s.0));
        }
        NodeKind::FindPattern(_) => {
            out.push_str("(find_pattern");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::MatchAlt { left, right } => {
            out.push_str("(match_alt\n");
            write_node(ast, left, d, out);
            out.push('\n');
            write_node(ast, right, d, out);
            out.push(')');
        }
        NodeKind::MatchRest(inner) => {
            if let Some(inner_id) = inner.get() {
                out.push_str("(match_rest\n");
                write_node(ast, inner_id, d, out);
                out.push(')');
            } else {
                out.push_str("(match_rest)");
            }
        }
        NodeKind::MatchNilPattern => {
            out.push_str("(match_nil_pattern)");
        }
        NodeKind::ArrayPatternWithTail(_) => {
            out.push_str("(array_pattern_with_tail");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Itblock { send, body } => {
            out.push_str("(itblock\n");
            write_node(ast, send, d, out);
            out.push('\n');
            write_opt(ast, body, d, out);
            out.push(')');
        }

        // ── murphy-s4b4 LOW-priority extensions ─────────────────────────
        NodeKind::Alias { new_name, old_name } => {
            out.push_str("(alias\n");
            write_node(ast, new_name, d, out);
            out.push('\n');
            write_node(ast, old_name, d, out);
            out.push(')');
        }
        NodeKind::Undef(_) => {
            out.push_str("(undef");
            write_slice(ast, id, 0, usize::MAX, d, out);
            out.push(')');
        }
        NodeKind::Preexe(body) => {
            out.push_str("(preexe\n");
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::Postexe(body) => {
            out.push_str("(postexe\n");
            write_opt(ast, body, d, out);
            out.push(')');
        }
        NodeKind::BackRef(s) => {
            let _ = write!(out, "(back_ref :{})", interner.resolve(s.0));
        }
        NodeKind::NthRef(n) => {
            let _ = write!(out, "(nth_ref {n})");
        }
        NodeKind::Shadowarg(s) => {
            let _ = write!(out, "(shadowarg :{})", interner.resolve(s.0));
        }
        NodeKind::Kwnilarg => out.push_str("(kwnilarg)"),
        NodeKind::Blocknilarg => out.push_str("(blocknilarg)"),

        // murphy-j1j2 PM-C one-liner pattern matching
        NodeKind::MatchPatternP { value, pattern } => {
            out.push_str("(match_pattern_p\n");
            write_node(ast, value, d, out);
            out.push('\n');
            write_node(ast, pattern, d, out);
            out.push(')');
        }
        NodeKind::MatchPattern { value, pattern } => {
            out.push_str("(match_pattern\n");
            write_node(ast, value, d, out);
            out.push('\n');
            write_node(ast, pattern, d, out);
            out.push(')');
        }

        NodeKind::MatchAs { value, name } => {
            out.push_str("(match_as\n");
            write_node(ast, value, d, out);
            out.push('\n');
            write_node(ast, name, d, out);
            out.push(')');
        }
        NodeKind::ConstPattern { const_, pattern } => {
            out.push_str("(const_pattern\n");
            write_node(ast, const_, d, out);
            out.push('\n');
            write_node(ast, pattern, d, out);
            out.push(')');
        }

        NodeKind::Pin(inner) => {
            out.push_str("(pin
");
            write_node(ast, inner, d, out);
            out.push(')');
        }
        NodeKind::IfGuard(inner) => {
            out.push_str("(if_guard
");
            write_node(ast, inner, d, out);
            out.push(')');
        }
        NodeKind::UnlessGuard(inner) => {
            out.push_str("(unless_guard
");
            write_node(ast, inner, d, out);
            out.push(')');
        }

        NodeKind::Unknown => out.push_str("(unknown)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::AstBuilder;
    use crate::node::{NodeKind, OptNodeId, Range};

    #[test]
    fn leaf_atoms_have_no_trailing_newline() {
        let mut b = AstBuilder::new("nil", "t.rb");
        let n = b.push(NodeKind::Nil, Range { start: 0, end: 3 });
        let ast = b.finish(n);
        assert_eq!(ast_to_sexp(&ast), "(nil)");
    }

    #[test]
    fn int_literal() {
        let mut b = AstBuilder::new("42", "t.rb");
        let n = b.push(NodeKind::Int(42), Range { start: 0, end: 2 });
        let ast = b.finish(n);
        assert_eq!(ast_to_sexp(&ast), "(int 42)");
    }

    #[test]
    fn if_renders_three_children_indented() {
        // if(true, 1, nil) — exercises a present `then_` and a `None` `else_`.
        let mut b = AstBuilder::new("if true then 1 end", "t.rb");
        let r = Range { start: 0, end: 1 };
        let cond = b.push(NodeKind::True_, r);
        let then_ = b.push(NodeKind::Int(1), r);
        let iff = b.push(
            NodeKind::If {
                cond,
                then_: OptNodeId::some(then_),
                else_: OptNodeId::NONE,
            },
            r,
        );
        let ast = b.finish(iff);
        assert_eq!(ast_to_sexp(&ast), "(if\n  (true)\n  (int 1)\n  nil)");
    }

    #[test]
    fn deterministic_across_repeat_calls() {
        // Same arena, two calls — same string. Guards against any
        // accidental order-of-iteration dependency in the printer.
        let mut b = AstBuilder::new("1", "t.rb");
        let n = b.push(NodeKind::Int(7), Range { start: 0, end: 1 });
        let ast = b.finish(n);
        let first = ast_to_sexp(&ast);
        let second = ast_to_sexp(&ast);
        assert_eq!(first, second);
    }
}
