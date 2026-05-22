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
            let send = self.translate_call(&call, range);
            // `{ }`/`do end` ブロック付き呼び出しは `Block` で包む。
            if let Some(blk) = call.block()
                && let Some(block_node) = blk.as_block_node()
            {
                return self.translate_block(&block_node, send, range);
            }
            return send;
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

        // --- collections ---
        if let Some(a) = node.as_array_node() {
            let ids: Vec<NodeId> = a
                .elements()
                .iter()
                .map(|e| self.translate_node(&e))
                .collect();
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Array(list), range);
        }
        if let Some(h) = node.as_hash_node() {
            let ids: Vec<NodeId> = h
                .elements()
                .iter()
                .map(|e| self.translate_node(&e))
                .collect();
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Hash(list), range);
        }
        if let Some(assoc) = node.as_assoc_node() {
            // `a: 1` / `:a => 1` 等の連想ペア。key/value はともに非 Option。
            let key = self.translate_node(&assoc.key());
            let value = self.translate_node(&assoc.value());
            return self.builder.push(NodeKind::Pair { key, value }, range);
        }
        if let Some(splat) = node.as_assoc_splat_node() {
            // `**h`。`value()` は `Option<Node>`（匿名 `**` は `None`）。
            let inner = splat
                .value()
                .map(|v| OptNodeId::some(self.translate_node(&v)))
                .unwrap_or(OptNodeId::NONE);
            return self.builder.push(NodeKind::Kwsplat(inner), range);
        }

        // --- conditionals ---
        if let Some(iff) = node.as_if_node() {
            let cond = self.translate_node(&iff.predicate());
            let then_ = self.translate_stmts_opt(iff.statements());
            // `subsequent()` は `ElseNode`（`else`）か別の `IfNode`（`elsif`）。
            // `elsif` は入れ子の `If` として else_ に置く（parser-gem 準拠）。
            let else_ = match iff.subsequent() {
                Some(sub) => match sub.as_else_node() {
                    Some(els) => self.translate_stmts_opt(els.statements()),
                    None => OptNodeId::some(self.translate_node(&sub)),
                },
                None => OptNodeId::NONE,
            };
            return self
                .builder
                .push(NodeKind::If { cond, then_, else_ }, range);
        }
        if let Some(unl) = node.as_unless_node() {
            // parser-gem 準拠: `unless` は then/else を入れ替える。prism の
            // `statements()`（unless 本体）が murphy の `else_` に、prism の
            // `else_clause()` が murphy の `then_` に入る。
            let cond = self.translate_node(&unl.predicate());
            let body = self.translate_stmts_opt(unl.statements());
            let else_branch = match unl.else_clause() {
                Some(els) => self.translate_stmts_opt(els.statements()),
                None => OptNodeId::NONE,
            };
            return self.builder.push(
                NodeKind::If {
                    cond,
                    then_: else_branch,
                    else_: body,
                },
                range,
            );
        }
        if let Some(c) = node.as_case_node() {
            // `predicate()` は `Option<Node>`（`case` 式無しの `case/when` は None）。
            let subject = c
                .predicate()
                .map(|p| OptNodeId::some(self.translate_node(&p)))
                .unwrap_or(OptNodeId::NONE);
            let when_ids: Vec<NodeId> = c
                .conditions()
                .iter()
                .map(|w| self.translate_node(&w))
                .collect();
            let whens = self.builder.push_list(&when_ids);
            let else_ = match c.else_clause() {
                Some(els) => self.translate_stmts_opt(els.statements()),
                None => OptNodeId::NONE,
            };
            return self.builder.push(
                NodeKind::Case {
                    subject,
                    whens,
                    else_,
                },
                range,
            );
        }
        if let Some(w) = node.as_when_node() {
            let cond_ids: Vec<NodeId> = w
                .conditions()
                .iter()
                .map(|c| self.translate_node(&c))
                .collect();
            let conds = self.builder.push_list(&cond_ids);
            let body = self.translate_stmts_opt(w.statements());
            return self.builder.push(NodeKind::When { conds, body }, range);
        }

        // Task 9 以降、ここに各ノード種の arm を足していく。
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

    /// `BlockNode`（`{ }`/`do end`）+ 既に翻訳済みの call `NodeId` → `Block` ノード。
    fn translate_block(
        &mut self,
        block: &prism::BlockNode<'_>,
        call: NodeId,
        range: Range,
    ) -> NodeId {
        // `parameters()` は `Option<Node>`。`BlockParametersNode`（`|...|` 構文）
        // か、numbered（`_1`）/`it` パラメータノードのことがある。後者は v1 では
        // params 空 Args として扱う（Unknown を避ける）。
        let params_node = block.parameters().and_then(|p| {
            p.as_block_parameters_node()
                .and_then(|bp| bp.parameters())
                .or_else(|| p.as_parameters_node())
        });
        let block_loc = Self::range(&block.location());
        let args = self.translate_parameters(params_node, block_loc);
        // body は foundational helper `translate_body`（`StatementsNode` を畳む）。
        let body = self.translate_body(block.body());
        self.builder
            .push(NodeKind::Block { call, args, body }, range)
    }

    /// `ParametersNode` → `Args` ノードの `NodeId`。requireds → optionals → rest
    /// → posts → keywords → keyword_rest → block の順（parser-gem 準拠）。
    /// `params` が `None`（パラメータ無し / numbered・it）なら空 `Args`。
    fn translate_parameters(
        &mut self,
        params: Option<prism::ParametersNode<'_>>,
        args_range: Range,
    ) -> NodeId {
        let mut ids: Vec<NodeId> = Vec::new();
        if let Some(p) = &params {
            for n in p.requireds().iter() {
                ids.push(self.translate_param(&n));
            }
            for n in p.optionals().iter() {
                ids.push(self.translate_param(&n));
            }
            if let Some(rest) = p.rest() {
                ids.push(self.translate_param(&rest));
            }
            for n in p.posts().iter() {
                ids.push(self.translate_param(&n));
            }
            for n in p.keywords().iter() {
                ids.push(self.translate_param(&n));
            }
            if let Some(kwrest) = p.keyword_rest() {
                ids.push(self.translate_param(&kwrest));
            }
            if let Some(block) = p.block() {
                ids.push(self.translate_param(&block.as_node()));
            }
        }
        let list = self.builder.push_list(&ids);
        self.builder.push(NodeKind::Args(list), args_range)
    }

    /// 単一パラメータ prism ノード → arg 系 `NodeKind`。未対応は `Unknown`。
    fn translate_param(&mut self, node: &prism::Node<'_>) -> NodeId {
        let range = Self::node_range(node);
        if let Some(p) = node.as_required_parameter_node() {
            let name = self.sym(&p.name());
            return self.builder.push(NodeKind::Arg(name), range);
        }
        if let Some(p) = node.as_optional_parameter_node() {
            let name = self.sym(&p.name());
            let default = self.translate_node(&p.value());
            return self.builder.push(NodeKind::Optarg { name, default }, range);
        }
        if let Some(p) = node.as_rest_parameter_node() {
            let name = self.opt_sym(p.name());
            return self.builder.push(NodeKind::Restarg(name), range);
        }
        if let Some(p) = node.as_required_keyword_parameter_node() {
            let name = self.sym(&p.name());
            return self.builder.push(NodeKind::Kwarg(name), range);
        }
        if let Some(p) = node.as_optional_keyword_parameter_node() {
            let name = self.sym(&p.name());
            let default = self.translate_node(&p.value());
            return self
                .builder
                .push(NodeKind::Kwoptarg { name, default }, range);
        }
        if let Some(p) = node.as_keyword_rest_parameter_node() {
            let name = self.opt_sym(p.name());
            return self.builder.push(NodeKind::Kwrestarg(name), range);
        }
        if let Some(p) = node.as_block_parameter_node() {
            let name = self.opt_sym(p.name());
            return self.builder.push(NodeKind::Blockarg(name), range);
        }
        // `MultiTargetNode`（分割代入パラメータ）等は Task 16 / Unknown。
        self.builder.push(NodeKind::Unknown, range)
    }

    /// `Option<ConstantId>` → `Symbol`。`None`（匿名 `*`/`**`/`&`）は空文字
    /// （Ruby の有効な識別子になり得ない）を interned した `Symbol`（v1 簡略化）。
    fn opt_sym(&mut self, cid: Option<prism::ConstantId<'_>>) -> murphy_ast::Symbol {
        match cid {
            Some(c) => self.sym(&c),
            None => self.builder.intern_symbol(""),
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
    fn translates_block() {
        // `[1].each { |x| x }` → ルートは Block（call を包む）。
        let ast = translate("[1].each { |x| x }", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Block { call, args, body } => {
                // call は Send（[1].each）。
                assert!(matches!(ast.kind(*call), NodeKind::Send { .. }));
                // args は Args ノード（`|x|` → 1 パラメータ）。
                match ast.kind(*args) {
                    NodeKind::Args(l) => assert_eq!(l.len, 1),
                    other => panic!("expected Args, got {other:?}"),
                }
                assert!(body.get().is_some());
            }
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn translates_block_without_params() {
        // パラメータ無しブロック → Args は空。
        let ast = translate("foo { 1 }", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Block { args, .. } => match ast.kind(*args) {
                NodeKind::Args(l) => assert_eq!(l.len, 0),
                other => panic!("expected Args, got {other:?}"),
            },
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn translates_numbered_param_block_to_empty_args() {
        // `_1` 数値パラメータブロック → Args 空（Unknown を避ける）。
        let ast = translate("foo { _1 }", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Block { args, .. } => match ast.kind(*args) {
                NodeKind::Args(l) => assert_eq!(l.len, 0),
                other => panic!("expected Args, got {other:?}"),
            },
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn translates_block_parameters() {
        // `foo { |a, b = 1, *r, &blk| a }` のブロックパラメータが順に並ぶ。
        let ast = translate("foo { |a, b = 1, *r, &blk| a }", "t.rb");
        let args_id = match ast.kind(ast.root()) {
            NodeKind::Block { args, .. } => *args,
            other => panic!("expected Block, got {other:?}"),
        };
        let params: Vec<_> = ast.children(args_id).collect();
        assert_eq!(params.len(), 4);
        assert!(matches!(ast.kind(params[0]), NodeKind::Arg(_)));
        assert!(matches!(ast.kind(params[1]), NodeKind::Optarg { .. }));
        assert!(matches!(ast.kind(params[2]), NodeKind::Restarg(_)));
        assert!(matches!(ast.kind(params[3]), NodeKind::Blockarg(_)));
    }

    #[test]
    fn translates_keyword_block_parameters() {
        // `foo { |k:, m: 2, **o| k }` のキーワードパラメータ。
        let ast = translate("foo { |k:, m: 2, **o| k }", "t.rb");
        let args_id = match ast.kind(ast.root()) {
            NodeKind::Block { args, .. } => *args,
            other => panic!("expected Block, got {other:?}"),
        };
        let params: Vec<_> = ast.children(args_id).collect();
        assert_eq!(params.len(), 3);
        assert!(matches!(ast.kind(params[0]), NodeKind::Kwarg(_)));
        assert!(matches!(ast.kind(params[1]), NodeKind::Kwoptarg { .. }));
        assert!(matches!(ast.kind(params[2]), NodeKind::Kwrestarg(_)));
    }

    #[test]
    fn translates_anonymous_block_param_to_empty_symbol() {
        // 匿名 `*` / `**` / `&` は名前が空文字 interned。
        let ast = translate("foo { |*, **, &| 1 }", "t.rb");
        let args_id = match ast.kind(ast.root()) {
            NodeKind::Block { args, .. } => *args,
            other => panic!("expected Block, got {other:?}"),
        };
        let params: Vec<_> = ast.children(args_id).collect();
        assert_eq!(params.len(), 3);
        match ast.kind(params[0]) {
            NodeKind::Restarg(s) => assert_eq!(ast.interner().resolve(s.0), ""),
            other => panic!("expected Restarg, got {other:?}"),
        }
        match ast.kind(params[1]) {
            NodeKind::Kwrestarg(s) => assert_eq!(ast.interner().resolve(s.0), ""),
            other => panic!("expected Kwrestarg, got {other:?}"),
        }
        match ast.kind(params[2]) {
            NodeKind::Blockarg(s) => assert_eq!(ast.interner().resolve(s.0), ""),
            other => panic!("expected Blockarg, got {other:?}"),
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
    fn translates_array_and_hash() {
        // `[1, 2, 3]` → Array（3 要素）。
        let arr = translate("[1, 2, 3]", "t.rb");
        match arr.kind(arr.root()) {
            NodeKind::Array(l) => assert_eq!(l.len, 3),
            other => panic!("expected Array, got {other:?}"),
        }
        // `{ a: 1, **rest }` → Hash（Pair + Kwsplat の 2 要素）。
        let h = translate("{ a: 1, **rest }", "t.rb");
        match h.kind(h.root()) {
            NodeKind::Hash(l) => assert_eq!(l.len, 2),
            other => panic!("expected Hash, got {other:?}"),
        }
    }

    #[test]
    fn translates_pair_and_kwsplat() {
        // ハッシュ要素は Pair（`a: 1`）と Kwsplat（`**rest`）に翻訳される。
        let h = translate("{ a: 1, **rest }", "t.rb");
        let kids: Vec<_> = h.children(h.root()).collect();
        assert_eq!(kids.len(), 2);
        assert!(matches!(h.kind(kids[0]), NodeKind::Pair { .. }));
        match h.kind(kids[1]) {
            NodeKind::Kwsplat(inner) => assert!(inner.get().is_some()),
            other => panic!("expected Kwsplat, got {other:?}"),
        }
    }

    #[test]
    fn translates_empty_array() {
        // 空配列 → 空 NodeList。
        let ast = translate("[]", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Array(l) => assert_eq!(l.len, 0),
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn translates_if() {
        // `if c ... else ... end` → If { cond, then_: Some, else_: Some }。
        let ast = translate("if c\n  a\nelse\n  b\nend", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::If { then_, else_, .. } => {
                assert!(then_.get().is_some());
                assert!(else_.get().is_some());
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    #[test]
    fn translates_if_without_else() {
        // else 無しの if → else_ は None。
        let ast = translate("if c\n  a\nend", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::If { then_, else_, .. } => {
                assert!(then_.get().is_some());
                assert!(else_.is_none());
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    #[test]
    fn translates_if_elsif_nests() {
        // `elsif` は subsequent が IfNode → else_ に入れ子の If。
        let ast = translate("if a\n  1\nelsif b\n  2\nend", "t.rb");
        let else_id = match ast.kind(ast.root()) {
            NodeKind::If { else_, .. } => else_.get().expect("elsif → else_ に If"),
            other => panic!("expected If, got {other:?}"),
        };
        assert!(matches!(ast.kind(else_id), NodeKind::If { .. }));
    }

    #[test]
    fn translates_unless_swaps_branches() {
        // parser-gem 準拠: `unless c\n  a\nend` → If { cond: c, then_: None,
        // else_: a }（unless 本体は else_ 側へ）。
        let ast = translate("unless c\n  a\nend", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::If { then_, else_, .. } => {
                assert!(then_.is_none(), "unless: then_ は None");
                assert!(else_.get().is_some(), "unless: else_ に本体");
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    #[test]
    fn translates_unless_with_else_swaps_branches() {
        // `unless c\n  a\nelse\n  b\nend` → If { then_: b, else_: a }。
        // else 節（b）が then_ 側、unless 本体（a）が else_ 側。
        let ast = translate("unless c\n  a\nelse\n  b\nend", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::If { then_, else_, .. } => {
                assert!(then_.get().is_some(), "unless+else: then_ に else 節");
                assert!(else_.get().is_some(), "unless+else: else_ に本体");
            }
            other => panic!("expected If, got {other:?}"),
        }
    }

    #[test]
    fn translates_case_when() {
        // `case x\nwhen 1\n  a\nelse\n  b\nend` → Case { subject, whens, else_ }。
        let ast = translate("case x\nwhen 1\n  a\nelse\n  b\nend", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Case {
                subject,
                whens,
                else_,
            } => {
                assert!(subject.get().is_some());
                assert_eq!(whens.len, 1);
                assert!(else_.get().is_some());
            }
            other => panic!("expected Case, got {other:?}"),
        }
    }

    #[test]
    fn translates_when_node() {
        // when 子は When { conds, body }。`when 1, 2` は 2 条件。
        let ast = translate("case x\nwhen 1, 2\n  a\nend", "t.rb");
        let when_id = match ast.kind(ast.root()) {
            NodeKind::Case { whens, .. } => {
                assert_eq!(whens.len, 1);
                ast.children(ast.root())
                    .find(|&c| matches!(ast.kind(c), NodeKind::When { .. }))
                    .expect("Case に When 子")
            }
            other => panic!("expected Case, got {other:?}"),
        };
        match ast.kind(when_id) {
            NodeKind::When { conds, body } => {
                assert_eq!(conds.len, 2);
                assert!(body.get().is_some());
            }
            other => panic!("expected When, got {other:?}"),
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
