//! prism→arena 変換コストゲート（murphy-9cr.16、設計 §3/§8）。
//!
//! 設計 §3 は arena 方式を続けるか「prism ノードを薄くラップ」へ後退するかの
//! 判断点。素の `translate / parse` 比だけでは、後退案でも dispatch のために
//! 必要な「木の走査」コストまで arena の純増に数えてしまう。そこで corpus
//! ごとに 4 ベンチを取り、実パイプライン同士で比較する:
//!
//! - `parse`: `prism::parse` のみ（ベースライン）。
//! - `prism_walk`: parse + prism 木の素の DFS（`Visit` トレイト）。
//!   ＝ 後退案のパイプライン（parse + dispatch 走査）。
//! - `translate`: parse + DFS + arena 構築（`murphy_translate::translate`）。
//! - `arena_walk`: translate + arena ノード配列のリニアスキャン。
//!   ＝ arena 案のパイプライン（parse + 変換 + dispatch 走査）。
//!
//! ここから導かれる量:
//! - `translate − prism_walk` … arena 構築の純増（木の走査は両案で共通なので
//!   差し引いた値）。
//! - `arena_walk` 対 `prism_walk` … 実パイプライン同士のネット比較。ゲートが
//!   本来見るべき数字。
//!
//! 全ベンチとも返り値（`ParseResult` / `Ast`）を毎反復 drop し、解放コストを
//! 公平に計上する。各 corpus を `BenchmarkGroup` 化し `Throughput::Bytes` で
//! MB/s 化する。計測結果とゲート判定は
//! `docs/decisions/0039-arena-translation-cost-gate.md`。

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use murphy_ast::NodeId;
use murphy_translate::translate;
use ruby_prism::{self as prism, Node, Visit};
use std::hint::black_box;

// 既存の変換テスト fixture。小さめのファイルは criterion / ファイル単位の固定費
// が支配的になりやすいので、ヘッドラインは steady-state 合成側（realistic_x*）。
const CONTROL_FLOW: &str = include_str!("../tests/fixtures/control_flow.rb");
const METHOD_DEF: &str = include_str!("../tests/fixtures/method_def.rb");
const MIXED: &str = include_str!("../tests/fixtures/mixed.rb");
const REALISTIC: &str = include_str!("../tests/fixtures/realistic.rb");

/// prism 木を全ノード DFS する素のビジター。`Visit` のデフォルト走査が木を
/// 1 回辿り、各ノードで enter コールバックを呼ぶ。dispatch が「全ノードを
/// 1 回触る」コストの代理。走査機構（子ノードの FFI 取得）が支配的なので、
/// ノードあたりの作業はカウンタ加算のみで足りる。
#[derive(Default)]
struct CountingVisitor {
    nodes: u64,
}

impl<'pr> Visit<'pr> for CountingVisitor {
    fn visit_branch_node_enter(&mut self, _node: Node<'pr>) {
        self.nodes += 1;
    }

    fn visit_leaf_node_enter(&mut self, _node: Node<'pr>) {
        self.nodes += 1;
    }
}

/// 1 つの corpus に対し parse / prism_walk / translate / arena_walk を登録する。
fn bench_corpus(c: &mut Criterion, name: &str, source: &str) {
    let mut group = c.benchmark_group(name);
    group.throughput(Throughput::Bytes(source.len() as u64));

    // ベースライン: prism parse のみ。
    group.bench_function("parse", |b| {
        b.iter(|| prism::parse(black_box(source.as_bytes())));
    });

    // 後退案パイプライン: parse + prism 木の素の DFS。
    group.bench_function("prism_walk", |b| {
        b.iter(|| {
            let result = prism::parse(black_box(source.as_bytes()));
            let mut visitor = CountingVisitor::default();
            visitor.visit(&result.node());
            black_box(visitor.nodes)
        });
    });

    // 変換: parse + 1 パス DFS 変換 + finish。
    group.bench_function("translate", |b| {
        b.iter(|| translate(black_box(source), "bench.rb"));
    });

    // arena 案パイプライン: translate + arena ノード配列のリニアスキャン。
    // 各ノードの `kind` を読むことでフラット配列を実際に走査する（読まないと
    // ループが消去され走査コストが計上されない）。
    group.bench_function("arena_walk", |b| {
        b.iter(|| {
            let ast = translate(black_box(source), "bench.rb");
            for i in 0..ast.len() {
                black_box(ast.kind(NodeId(i as u32)));
            }
            ast
        });
    });

    group.finish();
}

fn translate_cost(c: &mut Criterion) {
    // 既存 fixture（28〜161 行）。
    bench_corpus(c, "control_flow", CONTROL_FLOW);
    bench_corpus(c, "method_def", METHOD_DEF);
    bench_corpus(c, "mixed", MIXED);
    bench_corpus(c, "realistic", REALISTIC);

    // steady-state: realistic.rb を連結し criterion / 固定費を償却する。×10 と
    // ×50 の 2 段で、コストが入力サイズに対し O(n) であることも確認できる。
    let realistic_x10 = REALISTIC.repeat(10);
    let realistic_x50 = REALISTIC.repeat(50);
    bench_corpus(c, "realistic_x10", &realistic_x10);
    bench_corpus(c, "realistic_x50", &realistic_x50);
}

criterion_group!(benches, translate_cost);
criterion_main!(benches);
