//! 再帰ポストオーダー DFS による prism→arena 変換。

use murphy_ast::{
    Ast, AstBuilder, MagicComment, MagicCommentKind, NodeId, NodeKind, OptNodeId, Range,
    SourceToken, SourceTokenKind,
};
use murphy_prism as prism;
use std::path::PathBuf;

/// Ruby ソースを prism で 1 回 parse し、所有権を持つ arena [`Ast`] へ翻訳する。
///
/// prism は total・panic-free なので本関数も常に成功する。prism の借用ツリーは
/// 本関数内で drop され、ライフタイムは外へ漏れない。
pub fn translate(source: &str, path: impl Into<PathBuf>) -> Ast {
    let result = prism::parse_with_tokens(source.as_bytes());
    let mut t = Translator {
        builder: AstBuilder::new(source, path),
    };
    let root = t.translate_program(&result.parse().node());
    for token in result.tokens() {
        t.builder.add_source_token(SourceToken {
            kind: Translator::source_token_kind(token.type_()),
            range: Range {
                start: token.start_offset() as u32,
                end: token.end_offset() as u32,
            },
        });
    }
    // prism のコメントを arena の comment list へ移送する。
    for c in result.parse().comments() {
        let loc = c.location();
        let range = Translator::range(&loc);
        // `CommentType` は `InlineComment`（`#`）/ `EmbDocComment`
        // （`=begin`/`=end`）の 2 variant のみ（ruby-prism 1.9.0 src/lib.rs
        // 430〜435 行で確認済み）。ワイルドカード arm は prism に 3 つ目の
        // variant が増えてもコンパイルを壊さず Block 扱いにフォールバックする。
        let kind = match c.type_() {
            prism::CommentType::InlineComment => murphy_ast::CommentKind::Inline,
            _ => murphy_ast::CommentKind::Block,
        };
        t.builder.add_comment(range, kind);
    }
    t.add_magic_comments(result.parse());
    t.builder.finish(root)
}

struct Translator {
    builder: AstBuilder,
}

impl Translator {
    fn add_magic_comments(&mut self, parse: &prism::ParseResult<'_>) {
        let source = parse.source();
        if source.starts_with(b"#!") {
            self.builder.add_magic_comment(MagicComment {
                range: Self::line_range(source, 0),
                key_range: Range::ZERO,
                value_range: Range::ZERO,
                kind: MagicCommentKind::Shebang,
                value_bool: 0,
            });
        }

        for comment in parse.magic_comments() {
            let Some(key_range) = Self::slice_range(source, comment.key()) else {
                continue;
            };
            let Some(value_range) = Self::slice_range(source, comment.value()) else {
                continue;
            };
            let Some(kind) = Self::magic_comment_kind(comment.key()) else {
                continue;
            };
            self.builder.add_magic_comment(MagicComment {
                range: Self::line_range(source, key_range.start as usize),
                key_range,
                value_range,
                kind,
                value_bool: u8::from(
                    kind == MagicCommentKind::FrozenStringLiteral
                        && comment.value().eq_ignore_ascii_case(b"true"),
                ),
            });
        }
    }

    fn magic_comment_kind(key: &[u8]) -> Option<MagicCommentKind> {
        fn eq_normalized(actual: &[u8], expected: &[u8]) -> bool {
            actual.len() == expected.len()
                && actual.iter().zip(expected).all(|(&actual, &expected)| {
                    let actual = if actual == b'-' {
                        b'_'
                    } else {
                        actual.to_ascii_lowercase()
                    };
                    actual == expected
                })
        }

        if eq_normalized(key, b"frozen_string_literal") {
            Some(MagicCommentKind::FrozenStringLiteral)
        } else if eq_normalized(key, b"encoding") || eq_normalized(key, b"coding") {
            Some(MagicCommentKind::Encoding)
        } else {
            None
        }
    }

    fn slice_range(source: &[u8], slice: &[u8]) -> Option<Range> {
        let source_start = source.as_ptr() as usize;
        let slice_start = slice.as_ptr() as usize;
        let start = slice_start.checked_sub(source_start)?;
        let end = start.checked_add(slice.len())?;
        if end > source.len() {
            return None;
        }
        Some(Range {
            start: start as u32,
            end: end as u32,
        })
    }

    fn line_range(source: &[u8], offset: usize) -> Range {
        let start = source[..offset]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |pos| pos + 1);
        let mut end = source[offset..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(source.len(), |pos| offset + pos);
        if end > start && source[end - 1] == b'\r' {
            end -= 1;
        }
        Range {
            start: start as u32,
            end: end as u32,
        }
    }

    fn source_token_kind(kind: prism::pm_token_type_t) -> SourceTokenKind {
        match kind {
            prism::PM_TOKEN_PARENTHESIS_LEFT | prism::PM_TOKEN_PARENTHESIS_LEFT_PARENTHESES => {
                SourceTokenKind::LeftParen
            }
            prism::PM_TOKEN_PARENTHESIS_RIGHT => SourceTokenKind::RightParen,
            prism::PM_TOKEN_COMMENT => SourceTokenKind::Comment,
            prism::PM_TOKEN_NEWLINE => SourceTokenKind::Newline,
            prism::PM_TOKEN_IGNORED_NEWLINE => SourceTokenKind::IgnoredNewline,
            prism::PM_TOKEN_HEREDOC_START => SourceTokenKind::HeredocStart,
            prism::PM_TOKEN_HEREDOC_END => SourceTokenKind::HeredocEnd,
            prism::PM_TOKEN_COMMA => SourceTokenKind::Comma,
            // `PM_TOKEN_BRACE_LEFT`/`PM_TOKEN_BRACE_RIGHT` cover hash-literal
            // and brace-block braces only. String interpolation (`#{`/`}`)
            // uses `PM_TOKEN_EMBEXPR_BEGIN`/`PM_TOKEN_EMBEXPR_END` and lambda
            // openers (`-> {`) use `PM_TOKEN_LAMBDA_BEGIN`, so those fall
            // through to `Other` and never masquerade as braces.
            prism::PM_TOKEN_BRACE_LEFT => SourceTokenKind::LeftBrace,
            prism::PM_TOKEN_BRACE_RIGHT => SourceTokenKind::RightBrace,
            _ => SourceTokenKind::Other,
        }
    }

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

    /// `Option<Location>` → `Range`。`None`（anonymous `*`/`**`/`&` 等）は
    /// [`Range::ZERO`] にフォールバック。
    fn opt_loc_range(loc: Option<prism::Location<'_>>) -> Range {
        loc.as_ref().map(Self::range).unwrap_or(Range::ZERO)
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
        if node.as_rational_node().is_some() {
            // Payload is the raw literal text (`1r`, `2/3r`).
            let text = self.builder.raw_source(range).to_string();
            let id = self.builder.intern_string(&text);
            return self.builder.push(NodeKind::Rational(id), range);
        }
        if node.as_imaginary_node().is_some() {
            // Imaginary/complex literal — payload is the raw text (`1i`).
            let text = self.builder.raw_source(range).to_string();
            let id = self.builder.intern_string(&text);
            return self.builder.push(NodeKind::Complex(id), range);
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
        if node.as_it_local_variable_read_node().is_some() {
            // `it` inside a parameterless block (Ruby 3.4) reads as the
            // implicit `it` local — parser-gem's `(lvar :it)`.
            let name = self.builder.intern_symbol("it");
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
        if let Some(n) = node.as_numbered_reference_read_node() {
            // `$1`, `$2`, … — regexp capture references.
            return self.builder.push(NodeKind::NthRef(n.number()), range);
        }
        if let Some(b) = node.as_back_reference_read_node() {
            // `$&`, `$~`, `$'`, … — regexp back references.
            let name = self.sym(&b.name());
            return self.builder.push(NodeKind::BackRef(name), range);
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

        // --- op-assign (`+=` / `||=` / `&&=`) ---
        // lvar/ivar/cvar/gvar/const の 5 ファミリ × operator/or/and の 3 種を
        // 翻訳する。`target` は値なし write ノード（`*asgn` の `value` が `None`）。
        // constant-path ターゲット系は arm を持たず `Unknown` に落ちる（設計どおり）。
        if let Some(w) = node.as_local_variable_operator_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Lvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let op = self.sym(&w.binary_operator());
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::OpAsgn { target, op, value }, range);
        }
        if let Some(w) = node.as_local_variable_or_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Lvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let value = self.translate_node(&w.value());
            return self.builder.push(NodeKind::OrAsgn { target, value }, range);
        }
        if let Some(w) = node.as_local_variable_and_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Lvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::AndAsgn { target, value }, range);
        }
        if let Some(w) = node.as_instance_variable_operator_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Ivasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let op = self.sym(&w.binary_operator());
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::OpAsgn { target, op, value }, range);
        }
        if let Some(w) = node.as_instance_variable_or_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Ivasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let value = self.translate_node(&w.value());
            return self.builder.push(NodeKind::OrAsgn { target, value }, range);
        }
        if let Some(w) = node.as_instance_variable_and_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Ivasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::AndAsgn { target, value }, range);
        }
        if let Some(w) = node.as_class_variable_operator_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Cvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let op = self.sym(&w.binary_operator());
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::OpAsgn { target, op, value }, range);
        }
        if let Some(w) = node.as_class_variable_or_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Cvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let value = self.translate_node(&w.value());
            return self.builder.push(NodeKind::OrAsgn { target, value }, range);
        }
        if let Some(w) = node.as_class_variable_and_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Cvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::AndAsgn { target, value }, range);
        }
        if let Some(w) = node.as_global_variable_operator_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Gvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let op = self.sym(&w.binary_operator());
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::OpAsgn { target, op, value }, range);
        }
        if let Some(w) = node.as_global_variable_or_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Gvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let value = self.translate_node(&w.value());
            return self.builder.push(NodeKind::OrAsgn { target, value }, range);
        }
        if let Some(w) = node.as_global_variable_and_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Gvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::AndAsgn { target, value }, range);
        }
        if let Some(w) = node.as_constant_operator_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Casgn {
                    scope: OptNodeId::NONE,
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let op = self.sym(&w.binary_operator());
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::OpAsgn { target, op, value }, range);
        }
        if let Some(w) = node.as_constant_or_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Casgn {
                    scope: OptNodeId::NONE,
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let value = self.translate_node(&w.value());
            return self.builder.push(NodeKind::OrAsgn { target, value }, range);
        }
        if let Some(w) = node.as_constant_and_write_node() {
            let name = self.sym(&w.name());
            let target = self.builder.push(
                NodeKind::Casgn {
                    scope: OptNodeId::NONE,
                    name,
                    value: OptNodeId::NONE,
                },
                Self::range(&w.name_loc()),
            );
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::AndAsgn { target, value }, range);
        }
        if let Some(w) = node.as_call_operator_write_node() {
            let method = self.sym(&w.read_name());
            let receiver = w.receiver();
            let selector_range = Self::opt_loc_range(w.message_loc());
            let target_range = receiver
                .as_ref()
                .zip(w.message_loc())
                .map(|(recv, message)| Range {
                    start: Self::node_range(recv).start,
                    end: Self::range(&message).end,
                })
                .unwrap_or(selector_range);
            let args = self.builder.push_list(&[]);
            let target = match (receiver, w.is_safe_navigation()) {
                (Some(r), true) => {
                    let recv = self.translate_node(&r);
                    self.builder.push_named(
                        NodeKind::Csend {
                            receiver: recv,
                            method,
                            args,
                        },
                        target_range,
                        selector_range,
                    )
                }
                (recv_opt, _) => {
                    let receiver = recv_opt
                        .map(|r| OptNodeId::some(self.translate_node(&r)))
                        .unwrap_or(OptNodeId::NONE);
                    self.builder.push_named(
                        NodeKind::Send {
                            receiver,
                            method,
                            args,
                        },
                        target_range,
                        selector_range,
                    )
                }
            };
            if let Some(operator) = w.call_operator_loc() {
                self.builder
                    .add_call_operator_loc(target, Self::range(&operator));
            }
            let op = self.sym(&w.binary_operator());
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::OpAsgn { target, op, value }, range);
        }
        if let Some(w) = node.as_index_operator_write_node() {
            let Some(receiver_node) = w.receiver() else {
                return self.builder.push(NodeKind::Unknown, range);
            };
            let receiver_range = Self::node_range(&receiver_node);
            let receiver = self.translate_node(&receiver_node);
            let mut arg_ids = self.translate_arg_list(w.arguments());
            if let Some(ba) = w.block() {
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
            let target_range = Range {
                start: receiver_range.start,
                end: Self::range(&w.closing_loc()).end,
            };
            let target = self
                .builder
                .push(NodeKind::Index { receiver, args }, target_range);
            if let Some(operator) = w.call_operator_loc() {
                self.builder
                    .add_call_operator_loc(target, Self::range(&operator));
            }
            self.builder
                .add_call_closing_loc(target, Self::range(&w.closing_loc()));
            let op = self.sym(&w.binary_operator());
            let value = self.translate_node(&w.value());
            return self
                .builder
                .push(NodeKind::OpAsgn { target, op, value }, range);
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
        if let Some(mw) = node.as_match_write_node() {
            // Regexp named captures through `=~` are implicit local writes.
            // Reuse existing nodes: first the match call, then value-less
            // Lvasgn targets whose ranges point at the capture names.
            let call = mw.call();
            let mut ids = vec![self.translate_call(&call, Self::range(&call.location()))];
            ids.extend(
                mw.targets()
                    .iter()
                    .map(|target| self.translate_target(&target)),
            );
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Begin(list), range);
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
        if let Some(kh) = node.as_keyword_hash_node() {
            // 呼び出し側キーワード引数 `foo(key: value)`。prism は末尾の
            // `key: value` 群を 1 個の `KeywordHashNode` に包む（`ArgumentsNode`
            // の最後の要素）。parser-gem は同じものを trailing hash 引数として
            // 表すため `Hash` に翻訳する（`HashNode` arm と同形：要素は
            // 既存の `AssocNode → Pair` / `AssocSplatNode → Kwsplat` arm が処理）。
            let ids: Vec<NodeId> = kh
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

        // --- loops ---
        if let Some(w) = node.as_while_node() {
            // `is_begin_modifier` を `post` に畳む（`while`/`while_post`）。
            let cond = self.translate_node(&w.predicate());
            let body = self.translate_stmts_opt(w.statements());
            return self.builder.push(
                NodeKind::While {
                    cond,
                    body,
                    post: w.is_begin_modifier(),
                },
                range,
            );
        }
        if let Some(u) = node.as_until_node() {
            let cond = self.translate_node(&u.predicate());
            let body = self.translate_stmts_opt(u.statements());
            return self.builder.push(
                NodeKind::Until {
                    cond,
                    body,
                    post: u.is_begin_modifier(),
                },
                range,
            );
        }
        if let Some(f) = node.as_for_node() {
            // `for <var> in <iter>; <body>; end` → `For { var, iter, body }`.
            // `index()` is an assignment target (`LocalVariableTargetNode` /
            // `MultiTargetNode`), translated value-less like RuboCop's
            // `(for (lvasgn :x) <iter> <body>)`.
            let var = self.translate_target(&f.index());
            let iter = self.translate_node(&f.collection());
            let body = self.translate_stmts_opt(f.statements());
            return self.builder.push(NodeKind::For { var, iter, body }, range);
        }
        if let Some(lam) = node.as_lambda_node() {
            // Stabby lambda `-> (params) { body }` → `(block (lambda) (args)
            // body)`, mirroring the parser gem. The `Lambda` marker's range
            // is the `->` operator so it stays distinct from a `lambda {}`
            // method call (which is a `Block` over a `send nil :lambda`).
            let call = self
                .builder
                .push(NodeKind::Lambda, Self::range(&lam.operator_loc()));
            let params_node = lam.parameters().and_then(|p| {
                p.as_block_parameters_node()
                    .and_then(|bp| bp.parameters())
                    .or_else(|| p.as_parameters_node())
            });
            let args = self.translate_parameters(params_node, range);
            let body = self.translate_body(lam.body());
            return self
                .builder
                .push(NodeKind::Block { call, args, body }, range);
        }

        // --- logical operators ---
        if let Some(a) = node.as_and_node() {
            // `AndNode::left`/`right` は非 Option の `Node`。
            let lhs = self.translate_node(&a.left());
            let rhs = self.translate_node(&a.right());
            return self.builder.push(NodeKind::And { lhs, rhs }, range);
        }
        if let Some(o) = node.as_or_node() {
            let lhs = self.translate_node(&o.left());
            let rhs = self.translate_node(&o.right());
            return self.builder.push(NodeKind::Or { lhs, rhs }, range);
        }

        // --- range ---
        if let Some(r) = node.as_range_node() {
            // `left`/`right` は `Option<Node>`（beginless/endless は `None`）。
            // `is_exclude_end` で `..`/`...` を `exclusive` に畳む。
            let begin_ = r
                .left()
                .map(|n| OptNodeId::some(self.translate_node(&n)))
                .unwrap_or(OptNodeId::NONE);
            let end_ = r
                .right()
                .map(|n| OptNodeId::some(self.translate_node(&n)))
                .unwrap_or(OptNodeId::NONE);
            return self.builder.push(
                NodeKind::RangeExpr {
                    begin_,
                    end_,
                    exclusive: r.is_exclude_end(),
                },
                range,
            );
        }

        // --- definitions ---
        if let Some(d) = node.as_def_node() {
            // singleton `def self.foo` は `receiver` Some に畳む。
            let receiver = d
                .receiver()
                .map(|r| OptNodeId::some(self.translate_node(&r)))
                .unwrap_or(OptNodeId::NONE);
            let name = self.sym(&d.name());
            let args = self.translate_parameters(d.parameters(), range);
            // body は foundational helper `translate_body`（`StatementsNode` を畳む）。
            let body = self.translate_body(d.body());
            return self.builder.push(
                NodeKind::Def {
                    receiver,
                    name,
                    args,
                    body,
                },
                range,
            );
        }
        if let Some(c) = node.as_class_node() {
            // `constant_path()` は名前ノード（`Const` / `ConstantPath`）。
            let name = self.translate_node(&c.constant_path());
            let superclass = c
                .superclass()
                .map(|s| OptNodeId::some(self.translate_node(&s)))
                .unwrap_or(OptNodeId::NONE);
            let body = self.translate_body(c.body());
            return self.builder.push(
                NodeKind::Class {
                    name,
                    superclass,
                    body,
                },
                range,
            );
        }
        if let Some(m) = node.as_module_node() {
            let name = self.translate_node(&m.constant_path());
            let body = self.translate_body(m.body());
            return self.builder.push(NodeKind::Module { name, body }, range);
        }
        if let Some(sc) = node.as_singleton_class_node() {
            // `expression()` は `class << EXPR` の `EXPR`（非 Option の `Node`）。
            let expr = self.translate_node(&sc.expression());
            let body = self.translate_body(sc.body());
            return self.builder.push(NodeKind::Sclass { expr, body }, range);
        }

        // --- jumps ---
        if let Some(r) = node.as_return_node() {
            let v = self.translate_jump_arg(r.arguments(), range);
            return self.builder.push(NodeKind::Return(v), range);
        }
        if let Some(b) = node.as_break_node() {
            let v = self.translate_jump_arg(b.arguments(), range);
            return self.builder.push(NodeKind::Break(v), range);
        }
        if let Some(n) = node.as_next_node() {
            let v = self.translate_jump_arg(n.arguments(), range);
            return self.builder.push(NodeKind::Next(v), range);
        }
        if let Some(y) = node.as_yield_node() {
            let ids = self.translate_arg_list(y.arguments());
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Yield(list), range);
        }
        if let Some(s) = node.as_super_node() {
            // `super(args)`（明示引数あり）。`.block()` は `Option<Node>` で
            // `BlockArgumentNode`（`&blk`）か `BlockNode`（`{ }`/`do end`）の
            // 2 通り。前者は Send と同様に args 末尾へ `BlockPass` を付け、
            // 後者は素の `Super` を `Block` で包む（parser-gem 準拠）。
            let mut arg_ids = self.translate_arg_list(s.arguments());
            let mut block_to_wrap: Option<prism::BlockNode<'_>> = None;
            if let Some(blk) = s.block() {
                if let Some(ba) = blk.as_block_argument_node() {
                    let expr = ba
                        .expression()
                        .map(|e| OptNodeId::some(self.translate_node(&e)))
                        .unwrap_or(OptNodeId::NONE);
                    let bp = self
                        .builder
                        .push(NodeKind::BlockPass(expr), Self::range(&ba.location()));
                    arg_ids.push(bp);
                } else if let Some(bn) = blk.as_block_node() {
                    block_to_wrap = Some(bn);
                }
            }
            let list = self.builder.push_list(&arg_ids);
            let super_id = self.builder.push(NodeKind::Super(list), range);
            return match block_to_wrap {
                Some(bn) => self.translate_block(&bn, super_id, range),
                None => super_id,
            };
        }
        if let Some(fs) = node.as_forwarding_super_node() {
            // 括弧も引数も無い `super` — `ForwardingSuperNode`。
            // `.block()` は `Option<BlockNode>`（`&blk` は構文上不可）。
            let zsuper = self.builder.push(NodeKind::Zsuper, range);
            return match fs.block() {
                Some(bn) => self.translate_block(&bn, zsuper, range),
                None => zsuper,
            };
        }
        if let Some(d) = node.as_defined_node() {
            // `value()` は非 Option の `Node`。
            let inner = self.translate_node(&d.value());
            return self.builder.push(NodeKind::Defined(inner), range);
        }

        // --- exceptions ---
        if let Some(b) = node.as_begin_node() {
            return self.translate_begin(&b, range);
        }

        // --- string interpolation / regexp / xstring ---
        if let Some(s) = node.as_interpolated_string_node() {
            let ids = self.translate_interp_parts(s.parts());
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Dstr(list), range);
        }
        if let Some(s) = node.as_interpolated_symbol_node() {
            let ids = self.translate_interp_parts(s.parts());
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Dsym(list), range);
        }
        if let Some(x) = node.as_x_string_node() {
            // 補間なし xstring `` `cmd` ``。content を `Str` 1 部品に畳む。
            let text = String::from_utf8_lossy(x.unescaped());
            let sid = self.builder.intern_string(&text);
            let str_id = self.builder.push(NodeKind::Str(sid), range);
            let list = self.builder.push_list(&[str_id]);
            return self.builder.push(NodeKind::Xstr(list), range);
        }
        if let Some(x) = node.as_interpolated_x_string_node() {
            let ids = self.translate_interp_parts(x.parts());
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::Xstr(list), range);
        }
        if let Some(re) = node.as_regular_expression_node() {
            // 補間なし正規表現。content を `Str` 1 部品に畳む。
            let text = String::from_utf8_lossy(re.unescaped());
            let sid = self.builder.intern_string(&text);
            let str_id = self.builder.push(NodeKind::Str(sid), range);
            let parts = self.builder.push_list(&[str_id]);
            let opts = self.regexp_opts(re.is_ignore_case(), re.is_extended(), re.is_multi_line());
            return self.builder.push(NodeKind::Regexp { parts, opts }, range);
        }
        if let Some(re) = node.as_interpolated_regular_expression_node() {
            let ids = self.translate_interp_parts(re.parts());
            let parts = self.builder.push_list(&ids);
            let opts = self.regexp_opts(re.is_ignore_case(), re.is_extended(), re.is_multi_line());
            return self.builder.push(NodeKind::Regexp { parts, opts }, range);
        }

        // --- multiple assignment ---
        if let Some(mw) = node.as_multi_write_node() {
            let lhs =
                self.translate_mlhs(mw.lefts(), mw.rest(), mw.rights(), Self::node_range(node));
            let rhs = self.translate_node(&mw.value());
            return self.builder.push(NodeKind::Masgn { lhs, rhs }, range);
        }

        // --- pattern matching (case/in) ---
        if let Some(cm) = node.as_case_match_node() {
            // `predicate()` is `Option` defensively, but a `case ... in`
            // always has a subject; fall back to Unknown if ever absent so
            // the required `subject: NodeId` slot stays well-formed.
            let subject = match cm.predicate() {
                Some(p) => self.translate_node(&p),
                None => self.builder.push(NodeKind::Unknown, range),
            };
            let in_ids: Vec<NodeId> = cm
                .conditions()
                .iter()
                .map(|c| self.translate_node(&c))
                .collect();
            let in_patterns = self.builder.push_list(&in_ids);
            let else_body = match cm.else_clause() {
                Some(els) => self.translate_stmts_opt(els.statements()),
                None => OptNodeId::NONE,
            };
            return self.builder.push(
                NodeKind::CaseMatch {
                    subject,
                    in_patterns,
                    else_body,
                },
                range,
            );
        }
        if let Some(in_node) = node.as_in_node() {
            // Guard interception: prism has no dedicated guard node (1.9.0).
            // `in <pat> if <g>` parses as `InNode { pattern: IfNode {
            // predicate: g, statements: [pat] } }`; `unless` uses an
            // `UnlessNode`. We hoist the guard expression into the dedicated
            // `guard` slot and translate the wrapped pattern directly.
            //
            // NodeKind::InPattern drops the if/unless distinction by design
            // (see node.rs) — both lower to a bare guard expression.
            let raw_pattern = in_node.pattern();
            let (pattern_node, guard) = if let Some(iff) = raw_pattern.as_if_node() {
                (
                    iff.statements(),
                    Some(self.translate_node(&iff.predicate())),
                )
            } else if let Some(unl) = raw_pattern.as_unless_node() {
                (
                    unl.statements(),
                    Some(self.translate_node(&unl.predicate())),
                )
            } else {
                (None, None)
            };
            let pattern = match (pattern_node, guard.is_some()) {
                // Guard form: the wrapper's `statements` holds exactly the
                // pattern. Pull the single inner node out (no Begin wrap).
                (Some(stmts), true) => match stmts.body().iter().next() {
                    Some(inner) => self.translate_pattern(&inner),
                    None => self.builder.push(NodeKind::Unknown, range),
                },
                // No guard: the InNode pattern is the pattern itself.
                _ => self.translate_pattern(&raw_pattern),
            };
            let guard = guard.map(OptNodeId::some).unwrap_or(OptNodeId::NONE);
            let body = self.translate_stmts_opt(in_node.statements());
            return self.builder.push(
                NodeKind::InPattern {
                    pattern,
                    guard,
                    body,
                },
                range,
            );
        }

        // ---- one-liner pattern matching (Ruby 3.0+) ----
        if let Some(mp) = node.as_match_predicate_node() {
            // `expr in pat` — `MatchPredicateNode` → `match_pattern_p`
            let value = self.translate_node(&mp.value());
            let pattern = self.translate_pattern(&mp.pattern());
            return self.builder.push(NodeKind::MatchPatternP { value, pattern }, range);
        }
        if let Some(mr) = node.as_match_required_node() {
            // `expr => pat` — `MatchRequiredNode` → `match_pattern`
            let value = self.translate_node(&mr.value());
            let pattern = self.translate_pattern(&mr.pattern());
            return self.builder.push(NodeKind::MatchPattern { value, pattern }, range);
        }

        // Task 17 以降、ここに各ノード種の arm を足していく。
        self.builder.push(NodeKind::Unknown, range)
    }

    /// Translate a pattern-matching pattern node (the thing after `in`).
    fn translate_pattern(&mut self, node: &prism::Node<'_>) -> NodeId {
        let range = Self::node_range(node);
        if let Some(ap) = node.as_array_pattern_node() {
            // parser-gem: `(array_pattern <elem>...)` or
            // `(array_pattern_with_tail <elem>...)` (trailing comma).
            // `requireds` + `posts` are the bracketed elements.
            // `rest()` is either:
            //   - SplatNode         → `(match_rest)` / `(match_rest (match_var :x))`
            //   - ImplicitRestNode  → trailing comma — emit ArrayPatternWithTail
            let mut ids: Vec<NodeId> = ap
                .requireds()
                .iter()
                .map(|e| self.translate_pattern_element(&e))
                .collect();
            // Detect trailing comma (ImplicitRestNode) vs named/bare splat.
            let is_with_tail = ap.rest().and_then(|r| r.as_implicit_rest_node()).is_some();
            if let Some(rest) = ap.rest() {
                // Real splat: SplatNode → match_rest.
                // ImplicitRestNode (trailing comma) adds no child.
                if rest.as_implicit_rest_node().is_none() {
                    ids.push(self.translate_match_rest(&rest));
                }
            }
            ids.extend(
                ap.posts()
                    .iter()
                    .map(|e| self.translate_pattern_element(&e)),
            );
            let list = self.builder.push_list(&ids);
            let kind = if is_with_tail {
                NodeKind::ArrayPatternWithTail(list)
            } else {
                NodeKind::ArrayPattern(list)
            };
            return self.builder.push(kind, range);
        }
        if let Some(hp) = node.as_hash_pattern_node() {
            // parser-gem: `(hash_pattern <pair>...)`. Route elements through
            // translate_pattern_element so that hash values are translated as
            // patterns (not via translate_node which would yield Unknown for
            // pattern-only constructs like `{a: Integer}` or `{a:}`).
            // `rest()` is either:
            //   - AssocSplatNode          → `**val` (translate_node handles it as Kwsplat)
            //   - NoKeywordsParameterNode → `**nil` → match_nil_pattern
            let mut ids: Vec<NodeId> = hp
                .elements()
                .iter()
                .map(|e| self.translate_pattern_element(&e))
                .collect();
            if let Some(rest) = hp.rest() {
                let rest_id = if rest.as_no_keywords_parameter_node().is_some() {
                    self.builder
                        .push(NodeKind::MatchNilPattern, Self::node_range(&rest))
                } else if let Some(assoc_splat) = rest.as_assoc_splat_node() {
                    // `**rest` in a hash pattern → match_rest with optional match_var.
                    let inner = assoc_splat.value().and_then(|v| {
                        v.as_local_variable_target_node().map(|t| {
                            let name = self.sym(&t.name());
                            self.builder
                                .push(NodeKind::MatchVar(name), Self::node_range(&v))
                        })
                    });
                    self.builder.push(
                        NodeKind::MatchRest(murphy_ast::OptNodeId::from(inner)),
                        Self::node_range(&rest),
                    )
                } else {
                    self.translate_node(&rest)
                };
                ids.push(rest_id);
            }
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::HashPattern(list), range);
        }
        if let Some(fp) = node.as_find_pattern_node() {
            // parser-gem: `(find_pattern <left_rest> <elem>... <right_rest>)`.
            // `left()` and `right()` are SplatNodes (the `*` anchors).
            // `requireds()` is the inner elements list.
            let left = self.translate_node(&fp.left().as_node());
            let mut ids = vec![left];
            ids.extend(
                fp.requireds()
                    .iter()
                    .map(|e| self.translate_pattern_element(&e)),
            );
            let right = self.translate_node(&fp.right());
            ids.push(right);
            let list = self.builder.push_list(&ids);
            return self.builder.push(NodeKind::FindPattern(list), range);
        }
        if let Some(alt) = node.as_alternation_pattern_node() {
            // parser-gem: `(match_alt <left> <right>)`.
            let left = self.translate_pattern(&alt.left());
            let right = self.translate_pattern(&alt.right());
            return self.builder.push(NodeKind::MatchAlt { left, right }, range);
        }
        // Bare top-level pattern (e.g. `in x` with no brackets is wrapped in
        // an ArrayPattern by prism, so this path is for atoms / unsupported
        // kinds like MatchAs/Pin): route through the element translator.
        self.translate_pattern_element(node)
    }

    /// Translate a single element inside a pattern (an array/hash pattern
    /// member). A `LocalVariableTargetNode` is a binding → `MatchVar`
    /// (parser-gem `(match_var :name)`). An `AssocNode` inside a hash pattern
    /// gets its value translated as a pattern so that `{a: Integer}` and `{a:}`
    /// produce the right shapes. Everything else routes through general
    /// translators.
    fn translate_pattern_element(&mut self, node: &prism::Node<'_>) -> NodeId {
        if let Some(t) = node.as_local_variable_target_node() {
            let name = self.sym(&t.name());
            return self
                .builder
                .push(NodeKind::MatchVar(name), Self::node_range(node));
        }
        if let Some(assoc) = node.as_assoc_node() {
            // Hash pattern element: `{a: Integer}` or shorthand `{a:}`.
            // In prism, shorthand `{a:}` yields an AssocNode whose value is a
            // `LocalVariableTargetNode` (the implicit binding). Detect that and
            // emit `(match_var :a)` per parser-gem. For non-shorthand, route the
            // value through translate_pattern so nested patterns like `Integer`
            // are correctly rendered instead of falling to Unknown.
            let range = Self::node_range(node);
            let key = self.translate_node(&assoc.key());
            let value_node = assoc.value();
            // Shorthand `{a:}` in prism: AssocNode whose value is an
            // `ImplicitNode` wrapping a `LocalVariableTargetNode`.
            let inner_target = value_node
                .as_implicit_node()
                .and_then(|imp| imp.value().as_local_variable_target_node());
            let value = if let Some(t) = inner_target {
                // Shorthand `{a:}` — the binding side is a match_var.
                let name = self.sym(&t.name());
                self.builder
                    .push(NodeKind::MatchVar(name), Self::node_range(&value_node))
            } else {
                self.translate_pattern(&value_node)
            };
            return self.builder.push(NodeKind::Pair { key, value }, range);
        }
        if node.as_array_pattern_node().is_some()
            || node.as_hash_pattern_node().is_some()
            || node.as_find_pattern_node().is_some()
            || node.as_alternation_pattern_node().is_some()
        {
            return self.translate_pattern(node);
        }
        // SplatNode inside a pattern element (e.g. array pattern element that
        // is itself a splat-style rest capture).
        if node.as_splat_node().is_some() {
            return self.translate_match_rest(node);
        }
        // Literals / nested constants / unsupported kinds (MatchAs, Pin etc).
        self.translate_node(node)
    }

    /// Translate a SplatNode appearing as a pattern rest element
    /// (`*rest` or bare `*`) → `(match_rest (match_var :name))` or
    /// `(match_rest)`.
    fn translate_match_rest(&mut self, node: &prism::Node<'_>) -> NodeId {
        let range = Self::node_range(node);
        // SplatNode.expression() is the optional named target.
        let inner = if let Some(s) = node.as_splat_node() {
            s.expression().and_then(|e| {
                // Named rest: expression is a LocalVariableTargetNode → match_var.
                e.as_local_variable_target_node().map(|t| {
                    let name = self.sym(&t.name());
                    self.builder
                        .push(NodeKind::MatchVar(name), Self::node_range(&e))
                })
            })
        } else {
            None
        };
        let inner_id = murphy_ast::OptNodeId::from(inner);
        self.builder.push(NodeKind::MatchRest(inner_id), range)
    }

    /// 多重代入左辺（lefts + rest + rights）→ `Mlhs` ノード。
    /// `rest` は prism の `MultiWriteNode`/`MultiTargetNode` では `SplatNode`
    /// として渡る（`*rest`）。その `expression()` を `translate_target` で
    /// 翻訳し、`Splat` で 1 重だけ包む。
    fn translate_mlhs(
        &mut self,
        lefts: prism::NodeList<'_>,
        rest: Option<prism::Node<'_>>,
        rights: prism::NodeList<'_>,
        range: Range,
    ) -> NodeId {
        let mut ids: Vec<NodeId> = Vec::new();
        for n in lefts.iter() {
            ids.push(self.translate_target(&n));
        }
        if let Some(r) = rest {
            let sr = Self::node_range(&r);
            // rest 位置に来うる prism ノード:
            // - `SplatNode`(`*rest`): `expression()` をターゲット翻訳し
            //   `Splat` 1 重で包む(二重 `Splat` 回避)。`*`(匿名)は内側 None。
            // - `ImplicitRestNode`(`a, b, = ...` の末尾カンマ):
            //   parser-gem 準拠で `Splat(None)`(中身なし)。
            // - その他は防御的にそのまま target 翻訳。
            let inner = if r.as_implicit_rest_node().is_some() {
                OptNodeId::NONE
            } else if let Some(s) = r.as_splat_node() {
                s.expression()
                    .map(|e| OptNodeId::some(self.translate_target(&e)))
                    .unwrap_or(OptNodeId::NONE)
            } else {
                OptNodeId::some(self.translate_target(&r))
            };
            ids.push(self.builder.push(NodeKind::Splat(inner), sr));
        }
        for n in rights.iter() {
            ids.push(self.translate_target(&n));
        }
        let list = self.builder.push_list(&ids);
        self.builder.push(NodeKind::Mlhs(list), range)
    }

    /// 代入ターゲット（`LocalVariableTargetNode` 等、または入れ子の
    /// `MultiTargetNode`）を翻訳する。target 系は値なし write ノードへ。
    /// 対応 arm を持たないターゲット（constant/call/index target 等）は
    /// `translate_node` に委譲する（多くは `Unknown` に落ちる、v1 許容）。
    fn translate_target(&mut self, node: &prism::Node<'_>) -> NodeId {
        let range = Self::node_range(node);
        if let Some(t) = node.as_local_variable_target_node() {
            let name = self.sym(&t.name());
            return self.builder.push(
                NodeKind::Lvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                range,
            );
        }
        if let Some(t) = node.as_instance_variable_target_node() {
            let name = self.sym(&t.name());
            return self.builder.push(
                NodeKind::Ivasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                range,
            );
        }
        if let Some(t) = node.as_class_variable_target_node() {
            let name = self.sym(&t.name());
            return self.builder.push(
                NodeKind::Cvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                range,
            );
        }
        if let Some(t) = node.as_global_variable_target_node() {
            let name = self.sym(&t.name());
            return self.builder.push(
                NodeKind::Gvasgn {
                    name,
                    value: OptNodeId::NONE,
                },
                range,
            );
        }
        if let Some(mt) = node.as_multi_target_node() {
            return self.translate_mlhs(mt.lefts(), mt.rest(), mt.rights(), range);
        }
        // constant/call/index target 等は v1 では `translate_node` に委譲。
        self.translate_node(node)
    }

    /// 補間部品の並びを翻訳して `NodeId` の `Vec` を返す。部品は
    /// `StringNode` / `EmbeddedStatementsNode` / `EmbeddedVariableNode` の
    /// いずれか。`EmbeddedStatementsNode`（`#{...}`）は内側 statements を
    /// `Begin` に畳む。`EmbeddedVariableNode`（`#@x` 等）は中の変数ノードを
    /// 翻訳する。
    fn translate_interp_parts(&mut self, parts: prism::NodeList<'_>) -> Vec<NodeId> {
        let mut ids = Vec::new();
        for p in parts.iter() {
            if let Some(emb) = p.as_embedded_statements_node() {
                let emb_range = Self::range(&emb.location());
                let inner: Vec<NodeId> = match emb.statements() {
                    Some(s) => s.body().iter().map(|n| self.translate_node(&n)).collect(),
                    None => Vec::new(),
                };
                let list = self.builder.push_list(&inner);
                ids.push(self.builder.push(NodeKind::Begin(list), emb_range));
            } else if let Some(ev) = p.as_embedded_variable_node() {
                ids.push(self.translate_node(&ev.variable()));
            } else {
                // `StringNode` 等はそのまま翻訳。
                ids.push(self.translate_node(&p));
            }
        }
        ids
    }

    /// 正規表現フラグから `"imx"` 形式のフラグ文字列を組み立てて intern する。
    /// フラグ無しなら空文字列を interned した `Symbol`。
    fn regexp_opts(&mut self, ignore: bool, ext: bool, multi: bool) -> murphy_ast::Symbol {
        let mut s = String::new();
        if ignore {
            s.push('i');
        }
        if multi {
            s.push('m');
        }
        if ext {
            s.push('x');
        }
        self.builder.intern_symbol(&s)
    }

    /// prism `BeginNode` → arena ノード。
    /// 構造: `Begin([ Ensure?( Rescue?( body, resbodies, else ) ) ])`。
    /// rescue も ensure も無ければ素の `Begin([statements])`（`kwbegin` 準拠）。
    fn translate_begin(&mut self, b: &prism::BeginNode<'_>, range: Range) -> NodeId {
        let body = self.translate_stmts_opt(b.statements());

        // rescue 節（`subsequent()` でリンクした `RescueNode` 列）。
        let inner = if let Some(first) = b.rescue_clause() {
            let mut resbody_ids: Vec<NodeId> = Vec::new();
            let mut cur = Some(first);
            while let Some(rn) = cur {
                resbody_ids.push(self.translate_resbody(&rn));
                cur = rn.subsequent();
            }
            let resbodies = self.builder.push_list(&resbody_ids);
            let else_ = match b.else_clause() {
                Some(els) => self.translate_stmts_opt(els.statements()),
                None => OptNodeId::NONE,
            };
            OptNodeId::some(self.builder.push(
                NodeKind::Rescue {
                    body,
                    resbodies,
                    else_,
                },
                range,
            ))
        } else {
            body
        };

        // ensure 節。
        let protected = if let Some(ens) = b.ensure_clause() {
            let ensure_ = self.translate_stmts_opt(ens.statements());
            OptNodeId::some(self.builder.push(
                NodeKind::Ensure {
                    body: inner,
                    ensure_,
                },
                range,
            ))
        } else {
            inner
        };

        // `begin..end`（`kwbegin`）は `Begin` で包む。
        let child: Vec<NodeId> = match protected.get() {
            Some(id) => vec![id],
            None => Vec::new(),
        };
        let list = self.builder.push_list(&child);
        self.builder.push(NodeKind::Begin(list), range)
    }

    /// prism `RescueNode` 1 個 → `Resbody`。
    fn translate_resbody(&mut self, rn: &prism::RescueNode<'_>) -> NodeId {
        let range = Self::range(&rn.location());
        let exc_ids: Vec<NodeId> = rn
            .exceptions()
            .iter()
            .map(|e| self.translate_node(&e))
            .collect();
        let exceptions = self.builder.push_list(&exc_ids);
        // `reference()` は `=> e` の束縛先（`*TargetNode`）。`translate_target`
        // 経由で値なし write ノード（`Lvasgn` 等）へ翻訳する。
        let var = rn
            .reference()
            .map(|r| OptNodeId::some(self.translate_target(&r)))
            .unwrap_or(OptNodeId::NONE);
        let body = self.translate_stmts_opt(rn.statements());
        self.builder.push(
            NodeKind::Resbody {
                exceptions,
                var,
                body,
            },
            range,
        )
    }

    /// `break`/`next`/`return` の引数を単一 `OptNodeId` に畳む。
    /// 0→`None`、1→その式、複数→`Array`。
    fn translate_jump_arg(
        &mut self,
        args: Option<prism::ArgumentsNode<'_>>,
        range: Range,
    ) -> OptNodeId {
        let ids = self.translate_arg_list(args);
        match ids.len() {
            0 => OptNodeId::NONE,
            1 => OptNodeId::some(ids[0]),
            _ => {
                let list = self.builder.push_list(&ids);
                OptNodeId::some(self.builder.push(NodeKind::Array(list), range))
            }
        }
    }

    /// `Option<ArgumentsNode>` の各引数を翻訳して `Vec<NodeId>` にする。
    /// `None`（引数無し）は空ベクタ。
    fn translate_arg_list(&mut self, args: Option<prism::ArgumentsNode<'_>>) -> Vec<NodeId> {
        match args {
            Some(a) => a
                .arguments()
                .iter()
                .map(|n| self.translate_node(&n))
                .collect(),
            None => Vec::new(),
        }
    }

    /// `CallNode` を `Send`/`Csend` へ。`block` が `BlockArgumentNode`（`&blk`）の
    /// 場合のみ args 末尾に `BlockPass` を付ける。`{ }`/`do end` の `BlockNode` は
    /// Task 6 で呼び出し側が `Block` ラップする（本ヘルパは素の Send/Csend を返す）。
    fn translate_call(&mut self, call: &prism::CallNode<'_>, range: Range) -> NodeId {
        let method = self.sym(&call.name());
        let receiver = call.receiver();
        // `loc.name` には selector (e.g. `File.exists?` の `exists?` 部分)
        // を入れる。implicit call (`foo.()`) 等は selector がないので
        // `Range::ZERO` フォールバック。
        let selector_range = Self::opt_loc_range(call.message_loc());

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

        let id = match (receiver, call.is_safe_navigation()) {
            (Some(r), true) => {
                let recv = self.translate_node(&r);
                self.builder.push_named(
                    NodeKind::Csend {
                        receiver: recv,
                        method,
                        args,
                    },
                    range,
                    selector_range,
                )
            }
            (recv_opt, _) => {
                let receiver = recv_opt
                    .map(|r| OptNodeId::some(self.translate_node(&r)))
                    .unwrap_or(OptNodeId::NONE);
                self.builder.push_named(
                    NodeKind::Send {
                        receiver,
                        method,
                        args,
                    },
                    range,
                    selector_range,
                )
            }
        };
        if let Some(closing) = call.closing_loc() {
            self.builder.add_call_closing_loc(id, Self::range(&closing));
        }
        if let Some(operator) = call.call_operator_loc() {
            self.builder
                .add_call_operator_loc(id, Self::range(&operator));
        }
        id
    }

    /// `BlockNode`（`{ }`/`do end`）+ 既に翻訳済みの call `NodeId` → `Block` ノード。
    fn translate_block(
        &mut self,
        block: &prism::BlockNode<'_>,
        call: NodeId,
        range: Range,
    ) -> NodeId {
        // body は foundational helper `translate_body`（`StatementsNode` を畳む）。
        let body = self.translate_body(block.body());
        // `parameters()` は `Option<Node>`。`BlockParametersNode`（`|...|` 構文）
        // のほか、numbered（`_1`）/`it` パラメータノードがある。後者は専用の
        // `Numblock`/`Itblock` へ（`_1` 本体は通常の lvar、`it` は下の専用 arm）。
        if let Some(params) = block.parameters() {
            if let Some(np) = params.as_numbered_parameters_node() {
                return self.builder.push(
                    NodeKind::Numblock {
                        send: call,
                        max_n: np.maximum(),
                        body,
                    },
                    range,
                );
            }
            if params.as_it_parameters_node().is_some() {
                return self
                    .builder
                    .push(NodeKind::Itblock { send: call, body }, range);
            }
        }
        let params_node = block.parameters().and_then(|p| {
            p.as_block_parameters_node()
                .and_then(|bp| bp.parameters())
                .or_else(|| p.as_parameters_node())
        });
        let block_loc = Self::range(&block.location());
        let args = self.translate_parameters(params_node, block_loc);
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
    ///
    /// 各 arg/rest/block ノードでは prism の `name_loc()` を `loc.name`
    /// として記録し、`cx.node(arg).loc.name` で sigil 後の識別子範囲を
    /// 直接取れるようにする (`UnusedMethodArgument` の autocorrect が
    /// `*  args` のような sigil + 空白パターンを正しく扱うため)。
    /// `RequiredParameterNode` には `name_loc()` がない (sigil なしで
    /// expression 全体が name そのもの)。
    fn translate_param(&mut self, node: &prism::Node<'_>) -> NodeId {
        let range = Self::node_range(node);
        let (kind, name_range) = if let Some(p) = node.as_required_parameter_node() {
            (NodeKind::Arg(self.sym(&p.name())), range)
        } else if let Some(p) = node.as_optional_parameter_node() {
            let name = self.sym(&p.name());
            let nr = Self::range(&p.name_loc());
            let default = self.translate_node(&p.value());
            (NodeKind::Optarg { name, default }, nr)
        } else if let Some(p) = node.as_rest_parameter_node() {
            (
                NodeKind::Restarg(self.opt_sym(p.name())),
                Self::opt_loc_range(p.name_loc()),
            )
        } else if let Some(p) = node.as_required_keyword_parameter_node() {
            (
                NodeKind::Kwarg(self.sym(&p.name())),
                Self::range(&p.name_loc()),
            )
        } else if let Some(p) = node.as_optional_keyword_parameter_node() {
            let name = self.sym(&p.name());
            let nr = Self::range(&p.name_loc());
            let default = self.translate_node(&p.value());
            (NodeKind::Kwoptarg { name, default }, nr)
        } else if let Some(p) = node.as_keyword_rest_parameter_node() {
            (
                NodeKind::Kwrestarg(self.opt_sym(p.name())),
                Self::opt_loc_range(p.name_loc()),
            )
        } else if let Some(p) = node.as_block_parameter_node() {
            (
                NodeKind::Blockarg(self.opt_sym(p.name())),
                Self::opt_loc_range(p.name_loc()),
            )
        } else if node.as_forwarding_parameter_node().is_some() {
            // `def f(...)` — the forward-all `...` parameter.
            (NodeKind::ForwardArgs, range)
        } else {
            // `MultiTargetNode`（分割代入パラメータ）等は Task 16 / Unknown。
            return self.builder.push(NodeKind::Unknown, range);
        };
        self.builder.push_named(kind, range, name_range)
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
    use murphy_ast::{MagicCommentKind, NodeKind};

    #[test]
    fn translates_structured_magic_comments() {
        let src = "#!/usr/bin/env ruby\n# frozen_string_literal: true\n# encoding: utf-8\nnil\n";
        let ast = translate(src, "t.rb");
        let comments = ast.magic_comments();

        assert_eq!(comments.len(), 3);
        assert_eq!(comments[0].kind, MagicCommentKind::Shebang);
        assert_eq!(ast.raw_source(comments[0].range), "#!/usr/bin/env ruby");
        assert_eq!(comments[1].kind, MagicCommentKind::FrozenStringLiteral);
        assert_eq!(
            ast.raw_source(comments[1].key_range),
            "frozen_string_literal"
        );
        assert_eq!(ast.raw_source(comments[1].value_range), "true");
        assert_eq!(comments[1].value_bool, 1);
        assert_eq!(comments[2].kind, MagicCommentKind::Encoding);
        assert_eq!(ast.raw_source(comments[2].key_range), "encoding");
        assert_eq!(ast.raw_source(comments[2].value_range), "utf-8");
    }

    #[test]
    fn translates_false_frozen_string_literal_magic_comment() {
        let ast = translate("# frozen_string_literal: false\nnil\n", "t.rb");
        let comment = ast
            .magic_comments()
            .iter()
            .find(|comment| comment.kind == MagicCommentKind::FrozenStringLiteral)
            .expect("frozen_string_literal comment");

        assert_eq!(ast.raw_source(comment.value_range), "false");
        assert_eq!(comment.value_bool, 0);
    }

    #[test]
    fn translates_coding_alias_as_encoding_magic_comment() {
        let ast = translate("# coding: utf-8\nnil\n", "t.rb");
        let comment = ast
            .magic_comments()
            .iter()
            .find(|comment| comment.kind == MagicCommentKind::Encoding)
            .expect("encoding comment");

        assert_eq!(ast.raw_source(comment.key_range), "coding");
        assert_eq!(ast.raw_source(comment.value_range), "utf-8");
    }

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
    fn translates_call_operator_locs() {
        let dot = translate("foo.bar", "t.rb");
        assert_eq!(dot.call_operator_locs().len(), 1);
        assert_eq!(dot.raw_source(dot.call_operator_locs()[0].operator), ".");

        let safe_nav = translate("foo&.bar", "t.rb");
        assert_eq!(safe_nav.call_operator_locs().len(), 1);
        assert_eq!(
            safe_nav.raw_source(safe_nav.call_operator_locs()[0].operator),
            "&."
        );

        let operator_method = translate("foo + bar", "t.rb");
        assert!(operator_method.call_operator_locs().is_empty());

        let bare_call = translate("bar", "t.rb");
        assert!(bare_call.call_operator_locs().is_empty());
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
    fn translates_call_site_keyword_args() {
        // `foo(a: 1, b: 2)` → Send。末尾引数は prism の `KeywordHashNode`
        // ＝ parser-gem の trailing hash 相当 → `Hash` に翻訳され、
        // 中身は `Pair`（NOT `Unknown`）。
        let ast = translate("foo(a: 1, b: 2)", "t.rb");
        let last = ast
            .children(ast.root())
            .last()
            .expect("Send should have at least one argument child");
        match ast.kind(last) {
            NodeKind::Hash(l) => assert_eq!(l.len, 2),
            other => panic!("expected Hash for trailing kwargs, got {other:?}"),
        }
        let pairs: Vec<_> = ast.children(last).collect();
        assert_eq!(pairs.len(), 2);
        assert!(
            pairs
                .iter()
                .all(|&p| matches!(ast.kind(p), NodeKind::Pair { .. }))
        );
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
    fn translates_numbered_and_it_param_blocks() {
        // `_1` 数値パラメータブロック → Numblock { max_n }, 本体の `_1` は lvar。
        let ast = translate("foo.map { _1 + _2 }", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Numblock { max_n, body, .. } => {
                assert_eq!(*max_n, 2);
                assert!(body.get().is_some());
            }
            other => panic!("expected Numblock, got {other:?}"),
        }
        // `it` パラメータブロック → Itblock, 本体の `it` は lvar。
        let it = translate("foo { it.bar }", "t.rb");
        let NodeKind::Itblock { body, .. } = it.kind(it.root()) else {
            panic!("expected Itblock");
        };
        assert!(body.get().is_some());
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
    fn translates_while_and_until() {
        // `while c ... end` → While { post: false }。
        let w = translate("while c\n  x\nend", "t.rb");
        match w.kind(w.root()) {
            NodeKind::While { post, .. } => assert!(!post),
            other => panic!("expected While, got {other:?}"),
        }
        // `until c ... end` → Until。
        let u = translate("until c\n  x\nend", "t.rb");
        assert!(matches!(u.kind(u.root()), NodeKind::Until { .. }));
    }

    #[test]
    fn translates_for_to_for_node() {
        // `for x in [1, 2]; x; end` → For { var: Lvasgn, iter: Array, body }
        // (previously fell through to Unknown).
        let f = translate("for x in [1, 2]\n  x\nend", "t.rb");
        match f.kind(f.root()) {
            NodeKind::For { var, iter, body } => {
                assert!(matches!(f.kind(*var), NodeKind::Lvasgn { .. }));
                assert!(matches!(f.kind(*iter), NodeKind::Array(..)));
                assert!(body.get().is_some());
            }
            other => panic!("expected For, got {other:?}"),
        }
        // Multi-target `for a, b in h` → var is an Mlhs of value-less writes.
        let m = translate("for a, b in h\n  a\nend", "t.rb");
        let NodeKind::For { var, .. } = m.kind(m.root()) else {
            panic!("expected For");
        };
        assert!(matches!(m.kind(*var), NodeKind::Mlhs(..)));
    }

    #[test]
    fn translates_do_while_post_flag() {
        // `begin ... end while c` は do-while → post=true。
        let w = translate("begin\n  x\nend while c", "t.rb");
        match w.kind(w.root()) {
            NodeKind::While { post, .. } => assert!(post, "do-while は post=true"),
            other => panic!("expected While, got {other:?}"),
        }
    }

    #[test]
    fn translates_and_or() {
        let and = translate("a && b", "t.rb");
        assert!(matches!(and.kind(and.root()), NodeKind::And { .. }));
        let or = translate("a || b", "t.rb");
        assert!(matches!(or.kind(or.root()), NodeKind::Or { .. }));
    }

    #[test]
    fn translates_range() {
        // `1..5` → RangeExpr { exclusive: false, 両端 Some }。
        let inc = translate("1..5", "t.rb");
        match inc.kind(inc.root()) {
            NodeKind::RangeExpr {
                exclusive,
                begin_,
                end_,
            } => {
                assert!(!exclusive);
                assert!(begin_.get().is_some() && end_.get().is_some());
            }
            other => panic!("expected RangeExpr, got {other:?}"),
        }
        // `1...5` → exclusive: true。
        let excl = translate("1...5", "t.rb");
        match excl.kind(excl.root()) {
            NodeKind::RangeExpr { exclusive, .. } => assert!(exclusive),
            other => panic!("expected RangeExpr, got {other:?}"),
        }
        // endless range `1..` → end_ は None。
        let endless = translate("1..", "t.rb");
        match endless.kind(endless.root()) {
            NodeKind::RangeExpr { begin_, end_, .. } => {
                assert!(begin_.get().is_some());
                assert!(end_.is_none());
            }
            other => panic!("expected RangeExpr, got {other:?}"),
        }
        // beginless range `..5` → begin_ は None。
        let beginless = translate("..5", "t.rb");
        match beginless.kind(beginless.root()) {
            NodeKind::RangeExpr { begin_, end_, .. } => {
                assert!(begin_.is_none());
                assert!(end_.get().is_some());
            }
            other => panic!("expected RangeExpr, got {other:?}"),
        }
    }

    #[test]
    fn translates_def() {
        // `def foo(a); a; end` → Def { receiver: None, name: foo, body: Some }。
        let ast = translate("def foo(a); a; end", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Def {
                receiver,
                name,
                body,
                ..
            } => {
                assert!(receiver.is_none());
                assert_eq!(ast.interner().resolve(name.0), "foo");
                assert!(body.get().is_some());
            }
            other => panic!("expected Def, got {other:?}"),
        }
    }

    #[test]
    fn translates_def_args_is_args_node() {
        // Def の `args` 子は常に `Args` ノード（パラメータ無しでも空 Args）。
        let ast = translate("def foo(a, b = 1); end", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Def { args, body, .. } => {
                match ast.kind(*args) {
                    NodeKind::Args(l) => assert_eq!(l.len, 2),
                    other => panic!("expected Args, got {other:?}"),
                }
                assert!(body.is_none(), "空ボディは None");
            }
            other => panic!("expected Def, got {other:?}"),
        }
    }

    #[test]
    fn translates_singleton_def() {
        // `def self.foo; end` → Def { receiver: Some(self) }。
        let ast = translate("def self.foo; end", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Def { receiver, .. } => {
                let recv = receiver.get().expect("singleton def → receiver Some");
                assert!(matches!(ast.kind(recv), NodeKind::SelfExpr));
            }
            other => panic!("expected Def, got {other:?}"),
        }
    }

    #[test]
    fn translates_class_module_sclass() {
        let c = translate("class C; end", "t.rb");
        assert!(matches!(c.kind(c.root()), NodeKind::Class { .. }));
        let m = translate("module M; end", "t.rb");
        assert!(matches!(m.kind(m.root()), NodeKind::Module { .. }));
        let sc = translate("class << self; end", "t.rb");
        match sc.kind(sc.root()) {
            NodeKind::Sclass { expr, .. } => {
                assert!(matches!(sc.kind(*expr), NodeKind::SelfExpr));
            }
            other => panic!("expected Sclass, got {other:?}"),
        }
    }

    #[test]
    fn translates_class_with_superclass_and_body() {
        // `class C < D; x; end` → Class { name: Const C, superclass: Some, body: Some }。
        let ast = translate("class C < D\n  x\nend", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Class {
                name,
                superclass,
                body,
            } => {
                assert!(matches!(ast.kind(*name), NodeKind::Const { .. }));
                assert!(superclass.get().is_some());
                assert!(body.get().is_some());
            }
            other => panic!("expected Class, got {other:?}"),
        }
    }

    #[test]
    fn translates_module_with_body() {
        // `module M; x; y; end` → Module、body は複数文なので Begin に畳まれる。
        let ast = translate("module M\n  x\n  y\nend", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Module { name, body } => {
                assert!(matches!(ast.kind(*name), NodeKind::Const { .. }));
                let b = body.get().expect("module body");
                assert!(matches!(ast.kind(b), NodeKind::Begin(_)));
            }
            other => panic!("expected Module, got {other:?}"),
        }
    }

    #[test]
    fn translates_sclass_with_body() {
        // `class << self; def f; end; end` → Sclass、body に Def。
        let ast = translate("class << self\n  def f; end\nend", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Sclass { body, .. } => {
                let b = body.get().expect("sclass body");
                assert!(matches!(ast.kind(b), NodeKind::Def { .. }));
            }
            other => panic!("expected Sclass, got {other:?}"),
        }
    }

    #[test]
    fn translates_return() {
        // `def f; return 1; end` の本体に Return（引数 1 個）。
        let ast = translate("def f; return 1; end", "t.rb");
        let ret = ast
            .descendants(ast.root())
            .find(|&n| matches!(ast.kind(n), NodeKind::Return(_)))
            .expect("expected a Return node");
        match ast.kind(ret) {
            NodeKind::Return(v) => assert!(v.get().is_some(), "return 1 → 引数 Some"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn translates_bare_return() {
        // 引数なし `return` → Return(None)。
        let ast = translate("def f; return; end", "t.rb");
        let ret = ast
            .descendants(ast.root())
            .find(|&n| matches!(ast.kind(n), NodeKind::Return(_)))
            .expect("expected a Return node");
        match ast.kind(ret) {
            NodeKind::Return(v) => assert!(v.is_none(), "bare return → 引数 None"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn translates_break_and_next() {
        // ループ本体内の break / next。
        let b = translate("while c; break 1; end", "t.rb");
        assert!(
            b.descendants(b.root())
                .any(|n| matches!(b.kind(n), NodeKind::Break(_)))
        );
        let n = translate("while c; next; end", "t.rb");
        let next = n
            .descendants(n.root())
            .find(|&id| matches!(n.kind(id), NodeKind::Next(_)))
            .expect("expected a Next node");
        match n.kind(next) {
            NodeKind::Next(v) => assert!(v.is_none(), "bare next → 引数 None"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn translates_break_multi_arg_to_array() {
        // `break 1, 2` → Break(Some(Array))。
        let ast = translate("while c; break 1, 2; end", "t.rb");
        let brk = ast
            .descendants(ast.root())
            .find(|&n| matches!(ast.kind(n), NodeKind::Break(_)))
            .expect("expected a Break node");
        match ast.kind(brk) {
            NodeKind::Break(v) => {
                let inner = v.get().expect("break 1, 2 → 引数 Some");
                assert!(matches!(ast.kind(inner), NodeKind::Array(_)));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn translates_yield() {
        // `def f; yield 1; end` → Yield（引数 1）。
        let ast = translate("def f; yield 1; end", "t.rb");
        let y = ast
            .descendants(ast.root())
            .find(|&n| matches!(ast.kind(n), NodeKind::Yield(_)))
            .expect("expected a Yield node");
        match ast.kind(y) {
            NodeKind::Yield(l) => assert_eq!(l.len, 1),
            _ => unreachable!(),
        }
    }

    #[test]
    fn translates_super_and_zsuper() {
        // 明示引数つき super → Super（NodeList）。
        let s = translate("def f; super(1); end", "t.rb");
        let sup = s
            .descendants(s.root())
            .find(|&n| matches!(s.kind(n), NodeKind::Super(_)))
            .expect("expected a Super node");
        match s.kind(sup) {
            NodeKind::Super(l) => assert_eq!(l.len, 1),
            _ => unreachable!(),
        }
        // 括弧も引数も無い super → Zsuper。
        let z = translate("def f; super; end", "t.rb");
        assert!(
            z.descendants(z.root())
                .any(|n| matches!(z.kind(n), NodeKind::Zsuper))
        );
    }

    #[test]
    fn translates_defined() {
        // `defined?(x)` → Defined(inner)。
        let ast = translate("defined?(x)", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Defined(inner) => {
                // `x` は variable_call なので Send になる。
                assert!(matches!(ast.kind(*inner), NodeKind::Send { .. }));
            }
            other => panic!("expected Defined, got {other:?}"),
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

    #[test]
    fn translates_begin_rescue() {
        // `begin..rescue..end` → ルートは Begin、子孫に Rescue と Resbody。
        let ast = translate("begin\n  x\nrescue => e\n  y\nend", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Begin(_)));
        assert!(
            ast.descendants(ast.root())
                .chain([ast.root()])
                .any(|n| matches!(ast.kind(n), NodeKind::Rescue { .. }))
        );
        assert!(
            ast.descendants(ast.root())
                .any(|n| matches!(ast.kind(n), NodeKind::Resbody { .. }))
        );
    }

    #[test]
    fn translates_begin_ensure() {
        // `begin..ensure..end` → 子孫に Ensure。
        let ast = translate("begin\n  x\nensure\n  z\nend", "t.rb");
        let ensure = ast
            .descendants(ast.root())
            .chain([ast.root()])
            .find(|&n| matches!(ast.kind(n), NodeKind::Ensure { .. }))
            .expect("expected an Ensure node");
        match ast.kind(ensure) {
            NodeKind::Ensure { body, ensure_ } => {
                assert!(body.get().is_some(), "ensure: 保護本体 Some");
                assert!(ensure_.get().is_some(), "ensure: ensure 節 Some");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn translates_rescue_with_exception_class() {
        // `rescue StandardError => e` → Resbody { exceptions: [..], var: Some }。
        let ast = translate("begin\nx\nrescue StandardError => e\ny\nend", "t.rb");
        let resbody = ast
            .descendants(ast.root())
            .find(|&n| matches!(ast.kind(n), NodeKind::Resbody { .. }))
            .expect("expected a Resbody node");
        match ast.kind(resbody) {
            NodeKind::Resbody {
                exceptions, var, ..
            } => {
                assert_eq!(exceptions.len, 1);
                assert!(var.get().is_some());
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn translates_begin_rescue_else_ensure_nesting() {
        // 全部入り `begin..rescue..else..ensure..end` →
        // Begin([ Ensure( Rescue( body, [Resbody], else ) ) ])。
        let ast = translate(
            "begin\n  a\nrescue\n  b\nelse\n  c\nensure\n  d\nend",
            "t.rb",
        );
        let ensure = match ast.kind(ast.root()) {
            NodeKind::Begin(_) => ast.children(ast.root()).next().expect("Begin に子が 1 つ"),
            other => panic!("expected Begin, got {other:?}"),
        };
        let rescue = match ast.kind(ensure) {
            NodeKind::Ensure { body, ensure_ } => {
                assert!(ensure_.get().is_some(), "ensure 節");
                body.get().expect("Ensure.body は Rescue")
            }
            other => panic!("expected Ensure, got {other:?}"),
        };
        match ast.kind(rescue) {
            NodeKind::Rescue {
                body,
                resbodies,
                else_,
            } => {
                assert!(body.get().is_some(), "Rescue.body");
                assert_eq!(resbodies.len, 1, "1 個の Resbody");
                assert!(else_.get().is_some(), "else 節");
            }
            other => panic!("expected Rescue, got {other:?}"),
        }
    }

    #[test]
    fn translates_op_assign() {
        // `x += 1` → OpAsgn { target: Lvasgn(x, None), op: "+", value }。
        let ast = translate("x = 0; x += 1", "t.rb");
        let op = ast
            .descendants(ast.root())
            .find(|&n| matches!(ast.kind(n), NodeKind::OpAsgn { .. }))
            .expect("expected an OpAsgn node");
        match ast.kind(op) {
            NodeKind::OpAsgn { target, op, .. } => {
                assert_eq!(ast.interner().resolve(op.0), "+");
                // target は値なし Lvasgn。
                match ast.kind(*target) {
                    NodeKind::Lvasgn { name, value } => {
                        assert_eq!(ast.interner().resolve(name.0), "x");
                        assert!(value.is_none(), "op-assign の target は値なし");
                    }
                    other => panic!("expected Lvasgn target, got {other:?}"),
                }
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn translates_or_and_assign() {
        // `@x ||= 1` → OrAsgn、`@x &&= 1` → AndAsgn。target は値なし Ivasgn。
        let or = translate("@x ||= 1", "t.rb");
        match or.kind(or.root()) {
            NodeKind::OrAsgn { target, .. } => {
                assert!(matches!(or.kind(*target), NodeKind::Ivasgn { .. }));
            }
            other => panic!("expected OrAsgn, got {other:?}"),
        }
        let and = translate("@x &&= 1", "t.rb");
        match and.kind(and.root()) {
            NodeKind::AndAsgn { target, .. } => {
                assert!(matches!(and.kind(*target), NodeKind::Ivasgn { .. }));
            }
            other => panic!("expected AndAsgn, got {other:?}"),
        }
    }

    #[test]
    fn translates_op_assign_all_name_families() {
        // lvar/ivar/cvar/gvar/const の 5 ファミリすべてが OpAsgn になり、
        // target は対応する値なし write ノード。期待 target は variant 名で照合。
        fn target_variant_name(k: &NodeKind) -> &'static str {
            match k {
                NodeKind::Lvasgn { .. } => "Lvasgn",
                NodeKind::Ivasgn { .. } => "Ivasgn",
                NodeKind::Cvasgn { .. } => "Cvasgn",
                NodeKind::Gvasgn { .. } => "Gvasgn",
                NodeKind::Casgn { .. } => "Casgn",
                _ => "other",
            }
        }
        let cases = [
            ("x = 0; x += 1", "Lvasgn"),
            ("@i += 1", "Ivasgn"),
            ("@@c += 1", "Cvasgn"),
            ("$g += 1", "Gvasgn"),
            ("K += 1", "Casgn"),
        ];
        for (src, expected) in cases {
            let ast = translate(src, "t.rb");
            let op = ast
                .descendants(ast.root())
                .chain([ast.root()])
                .find(|&n| matches!(ast.kind(n), NodeKind::OpAsgn { .. }))
                .unwrap_or_else(|| panic!("expected OpAsgn for `{src}`"));
            match ast.kind(op) {
                NodeKind::OpAsgn { target, .. } => {
                    assert_eq!(
                        target_variant_name(ast.kind(*target)),
                        expected,
                        "wrong target for `{src}`"
                    );
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn translates_const_op_assign_target_scope_is_none() {
        // 定数 op-assign の target は `Casgn { scope: None, value: None }`。
        let ast = translate("K += 1", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::OpAsgn { target, .. } => match ast.kind(*target) {
                NodeKind::Casgn { scope, name, value } => {
                    assert!(scope.is_none(), "const op-assign target は scope None");
                    assert_eq!(ast.interner().resolve(name.0), "K");
                    assert!(value.is_none(), "const op-assign target は値なし");
                }
                other => panic!("expected Casgn target, got {other:?}"),
            },
            other => panic!("expected OpAsgn, got {other:?}"),
        }
    }

    #[test]
    fn translates_or_and_assign_const_and_global_families() {
        // const / gvar ファミリの ||= / &&= も対応。
        let or = translate("K ||= 1", "t.rb");
        match or.kind(or.root()) {
            NodeKind::OrAsgn { target, .. } => {
                assert!(matches!(or.kind(*target), NodeKind::Casgn { .. }));
            }
            other => panic!("expected OrAsgn, got {other:?}"),
        }
        let and = translate("$g &&= 1", "t.rb");
        match and.kind(and.root()) {
            NodeKind::AndAsgn { target, .. } => {
                assert!(matches!(and.kind(*target), NodeKind::Gvasgn { .. }));
            }
            other => panic!("expected AndAsgn, got {other:?}"),
        }
    }

    #[test]
    fn translates_call_target_op_assign() {
        // `a.b += 1`（CallOperatorWriteNode）は getter send を target にした OpAsgn。
        let ast = translate("a.b += 1", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::OpAsgn { target, op, .. } => {
                assert_eq!(ast.interner().resolve(op.0), "+");
                match ast.kind(*target) {
                    NodeKind::Send {
                        receiver, method, ..
                    } => {
                        assert!(
                            receiver.get().is_some(),
                            "call op-assign target has receiver"
                        );
                        assert_eq!(ast.interner().resolve(method.0), "b");
                    }
                    other => panic!("expected Send target, got {other:?}"),
                }
            }
            other => panic!("expected OpAsgn, got {other:?}"),
        }
    }

    #[test]
    fn translates_index_target_op_assign() {
        // `a[0] += 1`（IndexOperatorWriteNode）は index read を target にした OpAsgn。
        let ast = translate("a[0] += 1", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::OpAsgn { target, op, .. } => {
                assert_eq!(ast.interner().resolve(op.0), "+");
                assert!(matches!(ast.kind(*target), NodeKind::Index { .. }));
            }
            other => panic!("expected OpAsgn, got {other:?}"),
        }
    }

    #[test]
    fn constant_path_target_op_assign_is_unknown_not_panic() {
        // `A::B += 1`（ConstantPathOperatorWriteNode）も v1 では Unknown 許容。
        // `A::B ||= 1` / `A::B &&= 1`（ConstantPath{Or,And}WriteNode）も同様。
        for src in ["A::B += 1", "A::B ||= 1", "A::B &&= 1"] {
            let ast = translate(src, "t.rb");
            assert!(
                matches!(ast.kind(ast.root()), NodeKind::Unknown),
                "expected Unknown for `{src}`"
            );
        }
    }

    // --- Task 15: string interpolation / regexp / xstring ---

    #[test]
    fn translates_interpolated_string() {
        let ast = translate("\"a#{b}c\"", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Dstr(parts) => assert!(parts.len >= 2),
            other => panic!("expected Dstr, got {other:?}"),
        }
    }

    #[test]
    fn translates_interpolated_string_embedded_stmt_is_begin() {
        // `#{...}` 部品は内側 statements を `Begin` に畳む。
        let ast = translate("\"a#{b}c\"", "t.rb");
        let parts: Vec<_> = ast.children(ast.root()).collect();
        // 部品のどれかが Begin（補間部）であること。
        assert!(
            parts
                .iter()
                .any(|&p| matches!(ast.kind(p), NodeKind::Begin(_))),
            "expected an embedded `#{{...}}` part folded into Begin"
        );
    }

    #[test]
    fn translates_interpolated_symbol() {
        let ast = translate(":\"a#{b}\"", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Dsym(_)));
    }

    #[test]
    fn translates_regexp_with_opts() {
        let ast = translate("/ab/im", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Regexp { opts, .. } => {
                let s = ast.interner().resolve(opts.0);
                assert!(s.contains('i') && s.contains('m'));
            }
            other => panic!("expected Regexp, got {other:?}"),
        }
    }

    #[test]
    fn translates_regexp_without_opts() {
        // フラグ無し `/ab/` は `opts` が空文字列に interned される。
        let ast = translate("/ab/", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Regexp { opts, .. } => {
                assert_eq!(ast.interner().resolve(opts.0), "");
            }
            other => panic!("expected Regexp, got {other:?}"),
        }
    }

    #[test]
    fn translates_interpolated_regexp() {
        let ast = translate("/a#{b}/x", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Regexp { opts, .. } => {
                assert!(ast.interner().resolve(opts.0).contains('x'));
            }
            other => panic!("expected Regexp, got {other:?}"),
        }
    }

    #[test]
    fn translates_xstring() {
        let ast = translate("`ls`", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Xstr(_)));
    }

    #[test]
    fn translates_interpolated_xstring() {
        let ast = translate("`ls #{dir}`", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Xstr(_)));
    }

    // --- Task 16: multiple assignment ---

    #[test]
    fn translates_multiple_assignment() {
        let ast = translate("a, b = 1, 2", "t.rb");
        match ast.kind(ast.root()) {
            NodeKind::Masgn { lhs, .. } => {
                assert!(matches!(ast.kind(*lhs), NodeKind::Mlhs(_)));
            }
            other => panic!("expected Masgn, got {other:?}"),
        }
    }

    #[test]
    fn translates_multiple_assignment_targets_are_value_less_lvasgn() {
        // `Mlhs` の各ターゲットは値なし write ノード（`translate_target`）。
        let ast = translate("a, b = 1, 2", "t.rb");
        let lhs = match ast.kind(ast.root()) {
            NodeKind::Masgn { lhs, .. } => *lhs,
            other => panic!("expected Masgn, got {other:?}"),
        };
        let targets: Vec<_> = ast.children(lhs).collect();
        assert_eq!(targets.len(), 2);
        for t in targets {
            match ast.kind(t) {
                NodeKind::Lvasgn { value, .. } => assert!(value.is_none()),
                other => panic!("expected value-less Lvasgn target, got {other:?}"),
            }
        }
    }

    #[test]
    fn translates_multiple_assignment_with_splat_rest_not_double_wrapped() {
        // `a, *b = 1, 2` — rest ターゲットは `Splat` 1 重で包まれる
        // （prism の `MultiWriteNode.rest()` は `SplatNode` を返すため、
        // 二重 `Splat` にならないこと）。
        let ast = translate("a, *b = 1, 2", "t.rb");
        let lhs = match ast.kind(ast.root()) {
            NodeKind::Masgn { lhs, .. } => *lhs,
            other => panic!("expected Masgn, got {other:?}"),
        };
        let targets: Vec<_> = ast.children(lhs).collect();
        assert_eq!(targets.len(), 2, "a + *b");
        // 2 番目は `Splat`、その子は値なし `Lvasgn`（二重 Splat でないこと）。
        match ast.kind(targets[1]) {
            NodeKind::Splat(inner) => {
                let inner = inner.get().expect("splat has inner");
                match ast.kind(inner) {
                    NodeKind::Lvasgn { value, .. } => assert!(value.is_none()),
                    other => {
                        panic!("expected value-less Lvasgn inside Splat, got {other:?}")
                    }
                }
            }
            other => panic!("expected Splat for `*b`, got {other:?}"),
        }
    }

    #[test]
    fn translates_nested_multiple_assignment() {
        // `a, (b, c) = 1, [2, 3]` — 入れ子 `MultiTargetNode` は `Mlhs` になる。
        let ast = translate("a, (b, c) = 1, [2, 3]", "t.rb");
        let lhs = match ast.kind(ast.root()) {
            NodeKind::Masgn { lhs, .. } => *lhs,
            other => panic!("expected Masgn, got {other:?}"),
        };
        let targets: Vec<_> = ast.children(lhs).collect();
        assert_eq!(targets.len(), 2);
        assert!(
            matches!(ast.kind(targets[1]), NodeKind::Mlhs(_)),
            "nested `(b, c)` must be an Mlhs"
        );
    }

    // --- Task 16 follow-up: rescue binding goes through translate_target ---

    #[test]
    fn rescue_binding_var_is_value_less_lvasgn_not_unknown() {
        // `begin; rescue => e; end` — `Resbody.var` は値なし `Lvasgn`
        // （`translate_target` 経由）。以前は `Unknown` に落ちていた。
        let ast = translate("begin\nrescue => e\nend", "t.rb");
        // Begin -> Rescue -> 最初の Resbody。
        let begin_kids: Vec<_> = ast.children(ast.root()).collect();
        let rescue = begin_kids[0];
        let resbodies: Vec<_> = match ast.kind(rescue) {
            NodeKind::Rescue { .. } => ast
                .children(rescue)
                .filter(|&c| matches!(ast.kind(c), NodeKind::Resbody { .. }))
                .collect(),
            other => panic!("expected Rescue, got {other:?}"),
        };
        let var = match ast.kind(resbodies[0]) {
            NodeKind::Resbody { var, .. } => var.get().expect("rescue binding present"),
            other => panic!("expected Resbody, got {other:?}"),
        };
        match ast.kind(var) {
            NodeKind::Lvasgn { name, value } => {
                assert!(value.is_none(), "rescue binding is value-less");
                assert_eq!(ast.interner().resolve(name.0), "e");
            }
            other => panic!("expected value-less Lvasgn, got {other:?}"),
        }
    }

    // --- Task 17: comments ---

    #[test]
    fn translates_comments() {
        let ast = translate("# a line comment\nx = 1\n", "t.rb");
        assert_eq!(ast.comments().len(), 1);
        assert_eq!(ast.comments()[0].kind, murphy_ast::CommentKind::Inline);
    }

    #[test]
    fn translates_block_comment() {
        let ast = translate("=begin\nblock\n=end\nx = 1\n", "t.rb");
        assert_eq!(ast.comments().len(), 1);
        assert_eq!(ast.comments()[0].kind, murphy_ast::CommentKind::Block);
    }

    #[test]
    fn translates_sorted_tokens_for_layout_punctuation_and_comments() {
        let ast = translate("foo(1) # c\nbar(\n  2\n)\n", "t.rb");
        let tokens: Vec<_> = ast
            .sorted_tokens()
            .iter()
            .filter(|t| {
                matches!(
                    t.kind,
                    murphy_ast::SourceTokenKind::LeftParen
                        | murphy_ast::SourceTokenKind::RightParen
                        | murphy_ast::SourceTokenKind::Comment
                        | murphy_ast::SourceTokenKind::Newline
                )
            })
            .map(|t| (t.kind, ast.raw_source(t.range).to_string()))
            .collect();

        assert!(tokens.contains(&(murphy_ast::SourceTokenKind::LeftParen, "(".to_string())));
        assert!(tokens.contains(&(murphy_ast::SourceTokenKind::RightParen, ")".to_string())));
        assert!(
            tokens
                .iter()
                .any(|(kind, text)| *kind == murphy_ast::SourceTokenKind::Comment
                    && text.starts_with("# c"))
        );
        assert!(tokens.contains(&(murphy_ast::SourceTokenKind::Newline, "\n".to_string())));
        assert!(
            ast.sorted_tokens()
                .windows(2)
                .all(|pair| pair[0].range.start <= pair[1].range.start)
        );
    }

    #[test]
    fn translates_ignored_newline_and_heredoc_tokens() {
        let ignored = translate("foo(\n  1\n)\n", "t.rb");
        let kinds: Vec<_> = ignored.sorted_tokens().iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&murphy_ast::SourceTokenKind::IgnoredNewline));

        let heredoc = translate("foo( <<~HEREDOC )\nbody\nHEREDOC\n", "t.rb");
        let kinds: Vec<_> = heredoc.sorted_tokens().iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&murphy_ast::SourceTokenKind::HeredocStart));
        assert!(kinds.contains(&murphy_ast::SourceTokenKind::HeredocEnd));
    }

    #[test]
    fn translates_comma_token() {
        let ast = translate("foo(a, b)", "t.rb");
        let commas: Vec<_> = ast
            .sorted_tokens()
            .iter()
            .filter(|t| t.kind == murphy_ast::SourceTokenKind::Comma)
            .map(|t| ast.raw_source(t.range).to_string())
            .collect();
        assert_eq!(commas, vec![",".to_string()]);
    }

    #[test]
    fn translates_hash_and_block_braces() {
        // Hash literal braces.
        let hash = translate("{a: 1}", "t.rb");
        let kinds: Vec<_> = hash
            .sorted_tokens()
            .iter()
            .map(|t| (t.kind, hash.raw_source(t.range).to_string()))
            .collect();
        assert!(kinds.contains(&(murphy_ast::SourceTokenKind::LeftBrace, "{".to_string())));
        assert!(kinds.contains(&(murphy_ast::SourceTokenKind::RightBrace, "}".to_string())));

        // Brace block braces.
        let block = translate("foo { }", "t.rb");
        let kinds: Vec<_> = block.sorted_tokens().iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&murphy_ast::SourceTokenKind::LeftBrace));
        assert!(kinds.contains(&murphy_ast::SourceTokenKind::RightBrace));
    }

    #[test]
    fn do_end_block_is_not_classified_as_braces() {
        // `do`/`end` keywords are not brace tokens — they must stay `Other`.
        let ast = translate("foo do end", "t.rb");
        let kinds: Vec<_> = ast.sorted_tokens().iter().map(|t| t.kind).collect();
        assert!(!kinds.contains(&murphy_ast::SourceTokenKind::LeftBrace));
        assert!(!kinds.contains(&murphy_ast::SourceTokenKind::RightBrace));
    }

    #[test]
    fn string_interpolation_braces_are_not_classified_as_braces() {
        // `#{` / `}` in interpolation are EMBEXPR tokens, not brace tokens.
        let ast = translate("\"#{x}\"", "t.rb");
        let kinds: Vec<_> = ast.sorted_tokens().iter().map(|t| t.kind).collect();
        assert!(!kinds.contains(&murphy_ast::SourceTokenKind::LeftBrace));
        // The interpolation-closing `}` is EMBEXPR_END, so it stays Other —
        // there is no BRACE_RIGHT token in a bare interpolation.
        assert!(!kinds.contains(&murphy_ast::SourceTokenKind::RightBrace));
    }

    // --- murphy-ocv: super-with-block ---

    #[test]
    fn translates_super_with_block_wraps_in_block() {
        // `super(1) { |x| x }` — SuperNode.block() は BlockNode。
        // parser-gem 準拠: `(block (super (int 1)) (args (arg :x)) (lvar :x))`。
        let ast = translate("def f; super(1) { |x| x }; end", "t.rb");
        let block = ast
            .descendants(ast.root())
            .find(|&n| matches!(ast.kind(n), NodeKind::Block { .. }))
            .expect("expected a Block node wrapping super");
        match ast.kind(block) {
            NodeKind::Block { call, .. } => match ast.kind(*call) {
                NodeKind::Super(l) => assert_eq!(l.len, 1, "super has 1 arg"),
                other => panic!("expected Block.call = Super, got {other:?}"),
            },
            _ => unreachable!(),
        }
    }

    #[test]
    fn translates_forwarding_super_with_block_wraps_zsuper() {
        // `super { |x| x }` — ForwardingSuperNode.block() は BlockNode。
        // 括弧なし bare `super` でブロックだけ付いた形は ForwardingSuperNode。
        let ast = translate("def f; super { |x| x }; end", "t.rb");
        let block = ast
            .descendants(ast.root())
            .find(|&n| matches!(ast.kind(n), NodeKind::Block { .. }))
            .expect("expected a Block node wrapping zsuper");
        match ast.kind(block) {
            NodeKind::Block { call, .. } => {
                assert!(
                    matches!(ast.kind(*call), NodeKind::Zsuper),
                    "expected Block.call = Zsuper, got {:?}",
                    ast.kind(*call)
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn translates_super_with_block_pass_arg_appends_block_pass() {
        // `super(&blk)` — SuperNode.block() は BlockArgumentNode。
        // Send と同様に args 末尾へ BlockPass を付ける（Block では包まない）。
        let ast = translate("def f; super(&blk); end", "t.rb");
        let sup = ast
            .descendants(ast.root())
            .find(|&n| matches!(ast.kind(n), NodeKind::Super(_)))
            .expect("expected a Super node");
        // Block で包まれていないこと。
        assert!(
            !ast.descendants(ast.root())
                .any(|n| matches!(ast.kind(n), NodeKind::Block { .. })),
            "Super(&blk) は Block で包まれない"
        );
        // Super の最後の子（args の最後）が BlockPass。
        let last = ast.children(sup).last().expect("super has args");
        assert!(
            matches!(ast.kind(last), NodeKind::BlockPass(_)),
            "expected BlockPass at end of Super args, got {:?}",
            ast.kind(last)
        );
    }

    // --- murphy-ocv: ImplicitRestNode in multi-assign LHS ---

    #[test]
    fn translates_multi_write_implicit_rest_to_bare_splat() {
        // `a, b, = 1, 2, 3` — 末尾カンマで rest が ImplicitRestNode。
        // parser-gem 準拠: `(splat)`（中身なし）→ `Splat(OptNodeId::NONE)`。
        // 以前は `Splat(Some(Unknown))` に落ちていた。
        let ast = translate("a, b, = 1, 2, 3", "t.rb");
        let lhs = match ast.kind(ast.root()) {
            NodeKind::Masgn { lhs, .. } => *lhs,
            other => panic!("expected Masgn, got {other:?}"),
        };
        let targets: Vec<_> = ast.children(lhs).collect();
        assert_eq!(targets.len(), 3, "a + b + implicit *");
        match ast.kind(targets[2]) {
            NodeKind::Splat(inner) => {
                assert!(
                    inner.is_none(),
                    "implicit rest must be bare Splat (no inner)"
                );
            }
            other => panic!("expected bare Splat for implicit rest, got {other:?}"),
        }
    }

    // ── murphy-es99.13: case/in pattern matching ────────────────────

    #[test]
    fn translates_case_in_array_pattern() {
        // `case x; in [a]; a; end` — previously the whole CaseMatch fell to
        // Unknown. Now: (case_match (send) (in_pattern (array_pattern
        // (match_var :a)) nil (send)) nil).
        let ast = translate("case x\nin [a]\n  a\nend\n", "t.rb");
        assert_eq!(
            murphy_ast::ast_to_sexp(&ast),
            "(case_match\n  (send :x\n    nil)\n  (in_pattern\n    (array_pattern\n      (match_var :a))\n    nil\n    (lvar a))\n  nil)"
        );
    }

    #[test]
    fn translates_case_in_with_guard() {
        // `in [a] if a` — prism wraps the pattern in an IfNode whose
        // predicate is the guard. The translator intercepts that wrapper so
        // the guard slot carries the condition and the pattern slot carries
        // the array pattern (not the IfNode).
        let ast = translate("case x\nin [a] if a\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(in_pattern\n    (array_pattern\n      (match_var :a))\n    (lvar a)\n"),
            "guard must be hoisted into the guard slot: {sexp}"
        );
    }

    #[test]
    fn translates_case_in_with_unless_guard() {
        // `unless` guard — prism wraps in an UnlessNode. The translator
        // intercepts it the same way; the if/unless distinction is dropped
        // (documented v1 limitation, see NodeKind::InPattern).
        let ast = translate("case x\nin [a] unless a\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(in_pattern\n    (array_pattern\n      (match_var :a))\n    (lvar a)\n"),
            "unless guard must be hoisted into the guard slot: {sexp}"
        );
    }

    #[test]
    fn translates_case_in_with_else() {
        // `else` branch lands in the case_match else slot.
        let ast = translate("case x\nin [a]\n  a\nelse\n  0\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(sexp.starts_with("(case_match\n"), "{sexp}");
        assert!(
            sexp.trim_end().ends_with("(int 0))"),
            "else body present: {sexp}"
        );
    }

    #[test]
    fn translates_case_in_hash_pattern() {
        // `in {a:}` — HashPattern with an Assoc child (the binding side is
        // an implicit match-var; we lean on the existing Assoc translation).
        let ast = translate("case x\nin {a:}\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(hash_pattern"),
            "hash_pattern present: {sexp}"
        );
    }

    #[test]
    fn translates_case_in_hash_pattern_explicit_capture_value() {
        let ast = translate("case x\nin {name: name}\n  name\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(pair\n        (sym :name)\n        (match_var :name))"),
            "explicit hash-pattern capture must be a match_var: {sexp}"
        );
    }

    #[test]
    fn translates_case_in_hash_pattern_shorthand_capture() {
        let ast = translate("case x\nin {name:}\n  name\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(pair\n        (sym :name)\n        (match_var :name))"),
            "hash-pattern shorthand capture must be a match_var: {sexp}"
        );
    }

    #[test]
    fn translates_regexp_named_capture_match_write() {
        let ast = translate("/(?<name>foo)/ =~ value\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.starts_with("(begin\n"),
            "match-write root must be exposed: {sexp}"
        );
        assert!(
            sexp.contains("(send :=~"),
            "match call must be present: {sexp}"
        );
        assert!(
            sexp.contains("(lvasgn name\n    nil)"),
            "implicit named capture target must be present: {sexp}"
        );
    }

    #[test]
    fn translates_case_in_array_pattern_multiple_bindings() {
        // `in a, b` — bare (no brackets) array pattern with two match-vars.
        let ast = translate("case x\nin a, b\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(array_pattern\n      (match_var :a)\n      (match_var :b))"),
            "two match_vars: {sexp}"
        );
    }

    #[test]
    fn unsupported_pattern_kinds_fall_to_unknown() {
        // MatchAs(`=>`) / Pin(`^x`) have no prism binding in v1, so they
        // remain Unknown. The surrounding case_match/in_pattern still translate.
        // MatchRest (`*rest`) is now lowered to match_rest (murphy-j1j2 PM-B).
        for src in [
            "case x\nin Integer => n\n  n\nend\n", // match-as / capture (no prism binding)
            "case x\nin ^foo\n  a\nend\n",         // pin (no prism binding)
        ] {
            let ast = translate(src, "t.rb");
            let sexp = murphy_ast::ast_to_sexp(&ast);
            assert!(sexp.starts_with("(case_match\n"), "{src}: {sexp}");
            assert!(sexp.contains("(in_pattern"), "{src}: {sexp}");
        }
    }

    #[test]
    fn translates_match_rest_named() {
        // `[*rest]` — named match_rest inside array pattern.
        let ast = translate("case x\nin [*rest]\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(match_rest\n"),
            "match_rest expected: {sexp}"
        );
        assert!(
            sexp.contains("(match_var :rest)"),
            "match_var :rest expected: {sexp}"
        );
        assert!(
            !sexp.contains("(unknown)"),
            "no Unknown after match_rest lowering: {sexp}"
        );
    }

    #[test]
    fn translates_match_rest_bare() {
        // `[*]` — bare match_rest (no name).
        let ast = translate("case x\nin [*]\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(match_rest)"),
            "bare match_rest expected: {sexp}"
        );
    }

    #[test]
    fn translates_match_nil_pattern() {
        // `{a:, **nil}` — no-other-keys hash pattern.
        let ast = translate("case x\nin {a:, **nil}\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(match_nil_pattern)"),
            "match_nil_pattern expected: {sexp}"
        );
        assert!(
            !sexp.contains("(unknown)"),
            "no Unknown after match_nil_pattern lowering: {sexp}"
        );
    }

    #[test]
    fn translates_array_pattern_with_tail() {
        // `[a, b,]` — array pattern with trailing comma.
        let ast = translate("case x\nin [a, b,]\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(array_pattern_with_tail"),
            "array_pattern_with_tail expected: {sexp}"
        );
        assert!(
            sexp.contains("(match_var :a)") && sexp.contains("(match_var :b)"),
            "match_var elements expected: {sexp}"
        );
    }

    #[test]
    fn translates_find_pattern() {
        // `[*, a, *]` — FindPatternNode: pre rests + requireds + post rest.
        // The `*` anchors (left/right SplatNodes) go through translate_node —
        // v1 has no MatchRest NodeKind, so they become `(splat (unknown))`
        // per the documented MatchRest deferral.
        let ast = translate("case x\nin [*, a, *]\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(find_pattern"),
            "find_pattern expected: {sexp}"
        );
        assert!(
            sexp.contains("(match_var :a)"),
            "inner match_var expected: {sexp}"
        );
        // The `*` anchors in find_pattern go through translate_node (SplatNode path),
        // yielding `(splat nil)`. They are NOT converted to match_rest because
        // find_pattern anchor semantics differ from array_pattern rest.
        assert!(
            sexp.contains("(splat"),
            "splat anchors expected in find_pattern: {sexp}"
        );
    }

    #[test]
    fn translates_alternation_pattern() {
        // `1 | 2` — AlternationPatternNode: left/right.
        let ast = translate("case x\nin 1 | 2\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(sexp.contains("(match_alt"), "match_alt expected: {sexp}");
        assert!(
            sexp.contains("(int 1)") && sexp.contains("(int 2)"),
            "both arms expected: {sexp}"
        );
    }

    #[test]
    fn translates_hash_pattern_with_value() {
        // `{a: Integer}` — hash pattern where the value is a pattern, not a
        // literal; must route through translate_pattern, not translate_node.
        let ast = translate("case x\nin {a: Integer}\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(hash_pattern"),
            "hash_pattern expected: {sexp}"
        );
        assert!(
            sexp.contains("(const :Integer"),
            "const value expected: {sexp}"
        );
        assert!(
            !sexp.contains("(unknown)"),
            "no Unknown in hash pattern: {sexp}"
        );
    }

    #[test]
    fn translates_hash_pattern_shorthand() {
        // `{a:}` — shorthand binding: the value is SymbolNode (same as key)
        // in prism, indicating an implicit match_var capture.
        let ast = translate("case x\nin {a:}\n  a\nend\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(hash_pattern"),
            "hash_pattern expected: {sexp}"
        );
        assert!(
            sexp.contains("(match_var :a)"),
            "shorthand binding must become match_var: {sexp}"
        );
    }


    // ── murphy-j1j2 PM-C: one-liner pattern matching ─────────────────────────

    #[test]
    fn translates_match_pattern_p_integer() {
        // `x in Integer` — MatchPredicateNode → match_pattern_p.
        // value is an lvar, pattern is a const.
        let ast = translate("x = 1\nx in Integer\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(match_pattern_p"),
            "match_pattern_p expected: {sexp}"
        );
        assert!(
            sexp.contains("(lvar x)"),
            "value lvar expected: {sexp}"
        );
        assert!(
            sexp.contains("(const :Integer"),
            "pattern const expected: {sexp}"
        );
        assert!(
            !sexp.contains("(unknown)"),
            "no Unknown in match_pattern_p: {sexp}"
        );
    }

    #[test]
    fn translates_match_pattern_integer() {
        // `x => Integer` — MatchRequiredNode → match_pattern.
        // value is an lvar, pattern is a const.
        let ast = translate("x = 1\nx => Integer\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(match_pattern"),
            "match_pattern expected: {sexp}"
        );
        assert!(
            sexp.contains("(lvar x)"),
            "value lvar expected: {sexp}"
        );
        assert!(
            sexp.contains("(const :Integer"),
            "pattern const expected: {sexp}"
        );
        assert!(
            !sexp.contains("(unknown)"),
            "no Unknown in match_pattern: {sexp}"
        );
    }

    #[test]
    fn translates_match_pattern_p_with_hash_pattern() {
        // `{a: 1} in {a:}` — hash pattern with shorthand capture.
        let ast = translate("{a: 1} in {a:}\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(match_pattern_p"),
            "match_pattern_p expected: {sexp}"
        );
        assert!(
            sexp.contains("(hash_pattern"),
            "hash_pattern in match_pattern_p: {sexp}"
        );
        assert!(
            sexp.contains("(match_var :a)"),
            "shorthand binding becomes match_var: {sexp}"
        );
    }

    #[test]
    fn translates_match_pattern_with_array_pattern() {
        // `[1, 2] => [a, b]` — array pattern destructuring via =>.
        let ast = translate("[1, 2] => [a, b]\n", "t.rb");
        let sexp = murphy_ast::ast_to_sexp(&ast);
        assert!(
            sexp.contains("(match_pattern"),
            "match_pattern expected: {sexp}"
        );
        assert!(
            sexp.contains("(array_pattern"),
            "array_pattern in match_pattern: {sexp}"
        );
        assert!(
            sexp.contains("(match_var :a)") && sexp.contains("(match_var :b)"),
            "binding vars expected: {sexp}"
        );
    }
}