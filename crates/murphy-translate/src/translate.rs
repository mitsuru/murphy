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

    /// 任意の prism ノードを翻訳して NodeId を返す。未対応は Unknown。
    fn translate_node(&mut self, node: &prism::Node<'_>) -> NodeId {
        let range = Self::node_range(node);
        // Task 2 以降、ここに各ノード種の arm を足していく。
        self.builder.push(NodeKind::Unknown, range)
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
        // NilNode→Nil mapping lands in Task 2; for now NilNode falls to
        // Unknown, which is still sufficient to verify the no-Begin-wrap
        // semantic. The `children().count() == 0` clause is what
        // discriminates this from the multi-statement (Begin) case.
        let ast = translate("nil", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Unknown));
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
        // Task 1 時点で IntegerNode は未対応。
        let ast = translate("1", "t.rb");
        assert!(matches!(ast.kind(ast.root()), NodeKind::Unknown));
    }
}
