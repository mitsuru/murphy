//! 再帰ポストオーダー DFS による prism→arena 変換。

use murphy_ast::{Ast, AstBuilder, NodeId, NodeKind, OptNodeId, Range};
use ruby_prism as prism;
use std::path::PathBuf;

/// Ruby ソースを prism で 1 回 parse し、所有権を持つ arena [`Ast`] へ翻訳する。
///
/// prism は total・panic-free なので本関数も常に成功する。prism の借用ツリーは
/// 本関数内で drop され、ライフタイムは外へ漏れない。
pub fn translate(source: &str, path: impl Into<PathBuf>) -> Ast {
    let result = prism::parse(source.as_bytes());
    let mut t = Translator {
        builder: AstBuilder::new(source, path),
    };
    let root = t.translate_program(&result.node());
    t.builder.finish(root)
}

struct Translator {
    builder: AstBuilder,
}

impl Translator {
    /// prism Location → murphy Range。
    fn range(loc: &prism::Location<'_>) -> Range {
        Range {
            start: loc.start_offset() as u32,
            end: loc.end_offset() as u32,
        }
    }

    /// Node の Range。
    fn node_range(node: &prism::Node<'_>) -> Range {
        Self::range(&node.location())
    }

    /// `ConstantId` → interned `Symbol`。非 UTF-8 は lossy 変換。
    fn sym(&mut self, cid: &prism::ConstantId<'_>) -> murphy_ast::Symbol {
        let text = String::from_utf8_lossy(cid.as_slice());
        self.builder.intern_symbol(&text)
    }

    /// ルート ProgramNode → arena ルート NodeId。
    fn translate_program(&mut self, node: &prism::Node<'_>) -> NodeId {
        let prog = match node.as_program_node() {
            Some(p) => p,
            // prism は常に ProgramNode を返すが、防御的に Unknown ルート。
            None => {
                return self.builder.push(NodeKind::Unknown, Self::node_range(node));
            }
        };
        let fallback = Self::node_range(node);
        // parser-gem 準拠: 0 文 → nil、1 文 → その文、複数 → begin。
        // `ProgramNode::statements()` は非 Option の `StatementsNode`（Step 0
        // で bindings 確認済み）。
        match self.translate_stmts_opt(Some(prog.statements())).get() {
            Some(id) => id,
            None => self.builder.push(NodeKind::Nil, fallback),
        }
    }

    /// `Option<StatementsNode>` を「ノード 1 個ぶん」の `OptNodeId` に畳む。
    /// 0 文→None、1 文→その文、複数→`Begin`。これは **foundational helper**:
    /// プログラムルート・条件分岐の本体・ループ本体・`begin`/`rescue` 等すべてが
    /// これを使う（Task 8 以降で再利用、新規定義しない）。
    fn translate_stmts_opt(&mut self, stmts: Option<prism::StatementsNode<'_>>) -> OptNodeId {
        let stmts = match stmts {
            Some(s) => s,
            None => return OptNodeId::NONE,
        };
        let ids: Vec<NodeId> = stmts
            .body()
            .iter()
            .map(|n| self.translate_node(&n))
            .collect();
        match ids.len() {
            0 => OptNodeId::NONE,
            1 => OptNodeId::some(ids[0]),
            _ => {
                let list = self.builder.push_list(&ids);
                OptNodeId::some(
                    self.builder
                        .push(NodeKind::Begin(list), Self::range(&stmts.location())),
                )
            }
        }
    }

    /// def/class/module/block/sclass の `body`（`Option<Node>`）→ `OptNodeId`。
    /// 中身が `StatementsNode` なら `translate_stmts_opt` で parser-gem 準拠に
    /// 畳む。これも **foundational helper**（Task 6/11 で再利用、新規定義しない）。
    #[allow(dead_code)]
    fn translate_body(&mut self, body: Option<prism::Node<'_>>) -> OptNodeId {
        match body {
            None => OptNodeId::NONE,
            Some(n) => match n.as_statements_node() {
                Some(s) => self.translate_stmts_opt(Some(s)),
                None => OptNodeId::some(self.translate_node(&n)),
            },
        }
    }

    /// prism `Integer` → `i64`。i64 を超えたら `None`（呼び出し側で Unknown に
    /// 落とす）。
    ///
    /// `Integer` は `Copy` ではなく `TryInto<i32>` は `self` を消費するため、
    /// `to_u32_digits()`（`&self`、`(negative, &[u32])` を LSB 先頭で返す）だけで
    /// 全ケースを再構成する。
    fn integer_to_i64(int: &prism::Integer<'_>) -> Option<i64> {
        let (negative, digits) = int.to_u32_digits();
        let mut acc: u128 = 0;
        for &d in digits.iter().rev() {
            acc = acc.checked_mul(1u128 << 32)?.checked_add(d as u128)?;
        }
        if negative {
            // 最小値 i64::MIN の絶対値 = i64::MAX as u128 + 1 まで許容。
            if acc <= i64::MAX as u128 + 1 {
                Some((acc as i128).wrapping_neg() as i64)
            } else {
                None
            }
        } else {
            i64::try_from(acc).ok()
        }
    }

    /// 任意の prism ノードを翻訳して NodeId を返す。未対応は Unknown。
    fn translate_node(&mut self, node: &prism::Node<'_>) -> NodeId {
        let range = Self::node_range(node);

        // --- atoms / literals ---
        if node.as_nil_node().is_some() {
            return self.builder.push(NodeKind::Nil, range);
        }
        if node.as_true_node().is_some() {
            return self.builder.push(NodeKind::True_, range);
        }
        if node.as_false_node().is_some() {
            return self.builder.push(NodeKind::False_, range);
        }
        if node.as_self_node().is_some() {
            return self.builder.push(NodeKind::SelfExpr, range);
        }
        if let Some(int) = node.as_integer_node() {
            return match Self::integer_to_i64(&int.value()) {
                Some(v) => self.builder.push(NodeKind::Int(v), range),
                None => self.builder.push(NodeKind::Unknown, range),
            };
        }
        if let Some(f) = node.as_float_node() {
            return self.builder.push(NodeKind::Float(f.value()), range);
        }
        if let Some(s) = node.as_string_node() {
            let text = String::from_utf8_lossy(s.unescaped());
            let id = self.builder.intern_string(&text);
            return self.builder.push(NodeKind::Str(id), range);
        }
        if let Some(sym) = node.as_symbol_node() {
            // 補間なしシンボル :foo。`unescaped()` がデコード済みの内容を返す。
            let text = String::from_utf8_lossy(sym.unescaped());
            let id = self.builder.intern_symbol(&text);
            return self.builder.push(NodeKind::Sym(id), range);
        }

        // --- variable reads ---
        if let Some(v) = node.as_local_variable_read_node() {
            let name = self.sym(&v.name());
            return self.builder.push(NodeKind::Lvar(name), range);
        }
        if let Some(v) = node.as_instance_variable_read_node() {
            let name = self.sym(&v.name());
            return self.builder.push(NodeKind::Ivar(name), range);
        }
        if let Some(v) = node.as_class_variable_read_node() {
            let name = self.sym(&v.name());
            return self.builder.push(NodeKind::Cvar(name), range);
        }
        if let Some(v) = node.as_global_variable_read_node() {
            let name = self.sym(&v.name());
            return self.builder.push(NodeKind::Gvar(name), range);
        }
        if let Some(c) = node.as_constant_read_node() {
            let name = self.sym(&c.name());
            return self.builder.push(
                NodeKind::Const {
                    scope: OptNodeId::NONE,
                    name,
                },
                range,
            );
        }
        if let Some(cp) = node.as_constant_path_node() {
            return self.translate_constant_path(&cp, range);
        }

        // --- assignments ---
        if let Some(w) = node.as_local_variable_write_node() {
            let name = self.sym(&w.name());
            let value = OptNodeId::some(self.translate_node(&w.value()));
            return self.builder.push(NodeKind::Lvasgn { name, value }, range);
        }
        if let Some(w) = node.as_instance_variable_write_node() {
            let name = self.sym(&w.name());
            let value = OptNodeId::some(self.translate_node(&w.value()));
            return self.builder.push(NodeKind::Ivasgn { name, value }, range);
        }
        if let Some(w) = node.as_global_variable_write_node() {
            let name = self.sym(&w.name());
            let value = OptNodeId::some(self.translate_node(&w.value()));
            return self.builder.push(NodeKind::Gvasgn { name, value }, range);
        }
        if let Some(w) = node.as_class_variable_write_node() {
            let name = self.sym(&w.name());
            let value = OptNodeId::some(self.translate_node(&w.value()));
            return self.builder.push(NodeKind::Cvasgn { name, value }, range);
        }
        if let Some(w) = node.as_constant_write_node() {
            let name = self.sym(&w.name());
            let value = OptNodeId::some(self.translate_node(&w.value()));
            return self.builder.push(
                NodeKind::Casgn {
                    scope: OptNodeId::NONE,
                    name,
                    value,
                },
                range,
            );
        }
        if let Some(w) = node.as_constant_path_write_node() {
            // `target()` は `ConstantPathNode`。その `parent` を scope、
            // `name` を name に畳む（`A::B = 1` / `::B = 1` を collapse）。
            let target = w.target();
            let scope = match target.parent() {
                Some(p) => OptNodeId::some(self.translate_node(&p)),
                None => OptNodeId::NONE,
            };
            let name = match target.name() {
                Some(cid) => self.sym(&cid),
                None => return self.builder.push(NodeKind::Unknown, range),
            };
            let value = OptNodeId::some(self.translate_node(&w.value()));
            return self
                .builder
                .push(NodeKind::Casgn { scope, name, value }, range);
        }

        // --- calls / splat ---
        if let Some(call) = node.as_call_node() {
            return self.translate_call(&call, range);
        }
        if let Some(s) = node.as_splat_node() {
            // 注: prism `SplatNode` の内容アクセサは `expression()`（appendix の
            // `value()` ではない — bindings.rs で確認済み）。
            let inner = s
                .expression()
                .map(|e| OptNodeId::some(self.translate_node(&e)))
                .unwrap_or(OptNodeId::NONE);
            return self.builder.push(NodeKind::Splat(inner), range);
        }

        // Task 6 以降、ここに各ノード種の arm を足していく。
        self.builder.push(NodeKind::Unknown, range)
    }

    /// `CallNode` を `Send`/`Csend` へ。`block` が `BlockArgumentNode`（`&blk`）の
    /// 場合のみ args 末尾に `BlockPass` を付ける。`{ }`/`do end` の `BlockNode` は
    /// Task 6 で呼び出し側が `Block` ラップする（本ヘルパは素の Send/Csend を返す）。
    fn translate_call(&mut self, call: &prism::CallNode<'_>, range: Range) -> NodeId {
        let method = self.sym(&call.name());
        let receiver = call.receiver();

        // 引数リスト。
        let mut arg_ids: Vec<NodeId> = Vec::new();
        if let Some(args) = call.arguments() {
            for a in args.arguments().iter() {
                arg_ids.push(self.translate_node(&a));
            }
        }
        // `&blk` → `BlockPass` を args 末尾へ。`BlockNode` のケースは無視
        // （Task 6 で `translate_call` の呼び出し側が処理する）。
        if let Some(blk) = call.block()
            && let Some(ba) = blk.as_block_argument_node()
        {
            let expr = ba
                .expression()
                .map(|e| OptNodeId::some(self.translate_node(&e)))
                .unwrap_or(OptNodeId::NONE);
            let bp = self
                .builder
                .push(NodeKind::BlockPass(expr), Self::range(&ba.location()));
            arg_ids.push(bp);
        }
        let args = self.builder.push_list(&arg_ids);

        match (receiver, call.is_safe_navigation()) {
            (Some(r), true) => {
                let recv = self.translate_node(&r);
                self.builder.push(
                    NodeKind::Csend {
                        receiver: recv,
                        method,
                        args,
                    },
                    range,
                )
            }
            (recv_opt, _) => {
                let receiver = recv_opt
                    .map(|r| OptNodeId::some(self.translate_node(&r)))
                    .unwrap_or(OptNodeId::NONE);
                self.builder.push(
                    NodeKind::Send {
                        receiver,
                        method,
                        args,
                    },
                    range,
                )
            }
        }
    }

    /// `ConstantPathNode`（`A::B` / `::B`）→ `Const { scope, name }`。
    /// `parent` が `Some` なら `A::B`、`None` なら `::B`（トップレベル）。
    fn translate_constant_path(
        &mut self,
        cp: &prism::ConstantPathNode<'_>,
        range: Range,
    ) -> NodeId {
        let scope = match cp.parent() {
            Some(p) => OptNodeId::some(self.translate_node(&p)),
            None => OptNodeId::NONE,
        };
        // `name` は `Option<ConstantId>`。`None`（壊れた path）なら Unknown。
        let name = match cp.name() {
            Some(cid) => self.sym(&cid),
            None => return self.builder.push(NodeKind::Unknown, range),
        };
        self.builder.push(NodeKind::Const { scope, name }, range)
    }
}

#[cfg(test)]
mod tests {
    use super::translate;
    use murphy_ast::NodeKind;

    #[test]
    fn empty_program_root_is_nil() {
        let ast = translate("", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Nil));
    }

    #[test]
    fn single_statement_root_is_that_statement() {
        // Single statement → root IS that statement (no Begin wrapping).
        // `nil` translates to `NodeKind::Nil`; the `children().count() == 0`
        // clause discriminates this from the multi-statement (Begin) case.
        let ast = translate("nil", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Nil));
        assert_eq!(ast.children(ast.root()).count(), 0);
    }

    #[test]
    fn multi_statement_root_is_begin() {
        // Task 1 時点では各文は未対応 → Unknown だが、ルートは Begin。
        let ast = translate("1\n2\n", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Begin(_)));
        assert_eq!(ast.children(ast.root()).count(), 2);
    }

    #[test]
    fn untranslated_node_falls_to_unknown() {
        // Begin はまだ未対応のためルートでは確認できないが、`__FILE__`
        // のような未対応ノードは Unknown に落ちる。
        let ast = translate("__FILE__", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Unknown));
    }

    #[test]
    fn translates_atoms() {
        let nil = translate("nil", "t.rb");
        assert!(matches!(nil.kind(nil.root()), NodeKind::Nil));
        let t = translate("true", "t.rb");
        assert!(matches!(t.kind(t.root()), NodeKind::True_));
        let f = translate("false", "t.rb");
        assert!(matches!(f.kind(f.root()), NodeKind::False_));
        let s = translate("self", "t.rb");
        assert!(matches!(s.kind(s.root()), NodeKind::SelfExpr));
    }

    #[test]
    fn translates_integer() {
        let ast = translate("42", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Int(42)));
    }

    #[test]
    fn translates_negative_and_large_integer() {
        let neg = translate("-7", "t.rb");
        assert!(matches!(neg.kind(neg.root()), NodeKind::Int(-7)));
        // i64::MIN もちょうど収まる。
        let min = translate("-9223372036854775808", "t.rb");
        assert!(matches!(min.kind(min.root()), NodeKind::Int(i64::MIN)));
        // i64 を超える巨大整数は Unknown に落ちること（panic しない）。
        let huge = translate("999999999999999999999999999999", "t.rb");
        assert!(matches!(huge.kind(huge.root()), NodeKind::Unknown));
    }

    #[test]
    fn translates_float_string_symbol() {
        let f = translate("3.5", "t.rb");
        match f.kind(f.root()) {
            NodeKind::Float(v) => assert_eq!(*v, 3.5),
            other => panic!("expected Float, got {other:?}"),
        }
        let ast = translate("\"hi\"", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Str(s) => assert_eq!(ast.interner().resolve(s.0), "hi"),
            other => panic!("expected Str, got {other:?}"),
        }
        let sym = translate(":sym", "t.rb");
        match sym.kind(sym.root()) {
            NodeKind::Sym(s) => assert_eq!(sym.interner().resolve(s.0), "sym"),
            other => panic!("expected Sym, got {other:?}"),
        }
    }

    #[test]
    fn translates_variable_reads() {
        // `@x` → Ivar、`$g` → Gvar、`@@c` → Cvar。`x` だけだと
        // LocalVariableRead ではなく CallNode（variable_call）になるため、
        // lvar は代入後にのみ出る — Task 4 の代入テストで間接的に通る。
        let ivar = translate("@x", "t.rb");
        match ivar.kind(ivar.root()) {
            NodeKind::Ivar(s) => assert_eq!(ivar.interner().resolve(s.0), "@x"),
            other => panic!("expected Ivar, got {other:?}"),
        }
        let gvar = translate("$g", "t.rb");
        match gvar.kind(gvar.root()) {
            NodeKind::Gvar(s) => assert_eq!(gvar.interner().resolve(s.0), "$g"),
            other => panic!("expected Gvar, got {other:?}"),
        }
        let cvar = translate("@@c", "t.rb");
        match cvar.kind(cvar.root()) {
            NodeKind::Cvar(s) => assert_eq!(cvar.interner().resolve(s.0), "@@c"),
            other => panic!("expected Cvar, got {other:?}"),
        }
    }

    #[test]
    fn translates_lvar_read_after_assignment() {
        // 代入後に参照すると LocalVariableReadNode が出る。
        let ast = translate("x = 1\nx\n", "t.rb");
        // ルートは Begin [ Lvasgn(unknown until Task 4), Lvar(x) ]。
        let kids: Vec<_> = ast.children(ast.root()).collect();
        let last = *kids.last().unwrap();
        match ast.kind(last) {
            NodeKind::Lvar(s) => assert_eq!(ast.interner().resolve(s.0), "x"),
            other => panic!("expected Lvar, got {other:?}"),
        }
    }

    #[test]
    fn translates_plain_constant() {
        let ast = translate("FOO", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Const { scope, name } => {
                assert!(scope.is_none());
                assert_eq!(ast.interner().resolve(name.0), "FOO");
            }
            other => panic!("expected Const, got {other:?}"),
        }
    }

    #[test]
    fn translates_constant_path() {
        // `A::B` → Const { scope: Some(Const A), name: B }。
        let ast = translate("A::B", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Const { scope, name } => {
                assert!(scope.get().is_some());
                assert_eq!(ast.interner().resolve(name.0), "B");
            }
            other => panic!("expected Const, got {other:?}"),
        }
    }

    #[test]
    fn translates_toplevel_constant_path() {
        // `::B` → Const { scope: None, name: B }（トップレベル参照）。
        let ast = translate("::B", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Const { scope, name } => {
                assert!(scope.is_none());
                assert_eq!(ast.interner().resolve(name.0), "B");
            }
            other => panic!("expected Const, got {other:?}"),
        }
    }

    #[test]
    fn translates_local_assignment() {
        let ast = translate("x = 1", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Lvasgn { name, value } => {
                assert_eq!(ast.interner().resolve(name.0), "x");
                assert!(value.get().is_some());
            }
            other => panic!("expected Lvasgn, got {other:?}"),
        }
    }

    #[test]
    fn translates_variable_assignments() {
        // `@x = 1` → Ivasgn、`$g = 1` → Gvasgn、`@@c = 1` → Cvasgn。
        let iv = translate("@x = 1", "t.rb");
        match iv.kind(iv.root()) {
            NodeKind::Ivasgn { name, value } => {
                assert_eq!(iv.interner().resolve(name.0), "@x");
                assert!(value.get().is_some());
            }
            other => panic!("expected Ivasgn, got {other:?}"),
        }
        let gv = translate("$g = 1", "t.rb");
        match gv.kind(gv.root()) {
            NodeKind::Gvasgn { name, value } => {
                assert_eq!(gv.interner().resolve(name.0), "$g");
                assert!(value.get().is_some());
            }
            other => panic!("expected Gvasgn, got {other:?}"),
        }
        let cv = translate("@@c = 1", "t.rb");
        match cv.kind(cv.root()) {
            NodeKind::Cvasgn { name, value } => {
                assert_eq!(cv.interner().resolve(name.0), "@@c");
                assert!(value.get().is_some());
            }
            other => panic!("expected Cvasgn, got {other:?}"),
        }
    }

    #[test]
    fn translates_call_no_receiver() {
        // `puts 1` → Send { receiver: None, method: puts, args: [Int 1] }。
        let ast = translate("puts 1", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Send {
                receiver,
                method,
                args,
            } => {
                assert!(receiver.is_none());
                assert_eq!(ast.interner().resolve(method.0), "puts");
                assert_eq!(args.len, 1);
            }
            other => panic!("expected Send, got {other:?}"),
        }
    }

    #[test]
    fn translates_call_with_receiver() {
        // `a.foo(b)` → Send { receiver: Some, method: foo, args: [..] }。
        let ast = translate("a.foo(b)", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Send {
                receiver, method, ..
            } => {
                assert!(receiver.get().is_some());
                assert_eq!(ast.interner().resolve(method.0), "foo");
            }
            other => panic!("expected Send, got {other:?}"),
        }
    }

    #[test]
    fn translates_safe_navigation_to_csend() {
        let ast = translate("a&.foo", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Csend { method, .. } => {
                assert_eq!(ast.interner().resolve(method.0), "foo");
            }
            other => panic!("expected Csend, got {other:?}"),
        }
    }

    #[test]
    fn translates_block_pass_arg() {
        // `foo(&blk)` → Send の args 末尾が BlockPass。
        let ast = translate("foo(&blk)", "t.rb");
        if let NodeKind::Send { args, .. } = *ast.kind(ast.root()) {
            assert!(args.len >= 1);
            let last = ast.children(ast.root()).last().unwrap();
            assert!(matches!(ast.kind(last), NodeKind::BlockPass(_)));
        } else {
            panic!("expected Send");
        }
    }

    #[test]
    fn translates_splat_arg() {
        // `foo(*arr)` → Send の args に Splat。
        let ast = translate("foo(*arr)", "t.rb");
        if let NodeKind::Send { args, .. } = *ast.kind(ast.root()) {
            assert_eq!(args.len, 1);
            let first = ast.children(ast.root()).next().unwrap();
            assert!(matches!(ast.kind(first), NodeKind::Splat(_)));
        } else {
            panic!("expected Send");
        }
    }

    #[test]
    fn translates_constant_assignment_plain_and_path() {
        let plain = translate("FOO = 1", "t.rb");
        match plain.kind(plain.root()) {
            NodeKind::Casgn { scope, name, value } => {
                assert!(scope.is_none());
                assert_eq!(plain.interner().resolve(name.0), "FOO");
                assert!(value.get().is_some());
            }
            other => panic!("expected Casgn, got {other:?}"),
        }
        // `A::B = 1` も Casgn（scope = Some）。
        let p = translate("A::B = 1", "t.rb");
        match p.kind(p.root()) {
            NodeKind::Casgn { scope, name, value } => {
                assert!(scope.get().is_some());
                assert_eq!(p.interner().resolve(name.0), "B");
                assert!(value.get().is_some());
            }
            other => panic!("expected Casgn, got {other:?}"),
        }
        // `::B = 1` はトップレベル代入（scope = None）。
        let top = translate("::B = 1", "t.rb");
        match top.kind(top.root()) {
            NodeKind::Casgn { scope, name, .. } => {
                assert!(scope.is_none());
                assert_eq!(top.interner().resolve(name.0), "B");
            }
            other => panic!("expected Casgn, got {other:?}"),
        }
    }
}
