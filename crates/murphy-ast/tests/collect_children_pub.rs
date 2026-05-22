//! `collect_children` must be reachable as a public `murphy-ast` item so
//! `murphy-plugin-api`'s `Cx::children` can delegate to it (murphy-9cr.20).

use murphy_ast::{AstBuilder, NodeKind, Range, collect_children};

#[test]
fn collect_children_is_public_and_enumerates_children() {
    let mut b = AstBuilder::new("x", "x");
    let leaf = b.push(NodeKind::Nil, Range { start: 0, end: 1 });
    let root = b.push(
        NodeKind::Return(murphy_ast::OptNodeId::some(leaf)),
        Range { start: 0, end: 1 },
    );
    let ast = b.finish(root);
    let mut out = Vec::new();
    collect_children(ast.kind(root), &[], &mut out);
    assert_eq!(out, vec![leaf]);
}
