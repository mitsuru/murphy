//! 代表的な実 Ruby ファイルで `Unknown` 比率が受け入れ基準内かを検証する。
//! あわせて、稀構文を含む多様な入力でも `translate` が panic しないことを
//! 確認する fuzz テストを置く。

use murphy_ast::NodeKind;

#[test]
fn unknown_ratio_under_5_percent() {
    let src = include_str!("fixtures/realistic.rb");
    let ast = murphy_translate::translate(src, "realistic.rb");
    let total = ast.len();
    // 空 AST だと unknown=0 / ratio=0 で素通りし、realistic.rb が翻訳されない
    // 回帰を見逃す。ノードが 0 個なら明示的に失敗させる。
    assert!(total > 0, "translated AST is empty for realistic.rb");
    let unknown = (0..total)
        .filter(|&i| matches!(ast.kind(murphy_ast::NodeId(i as u32)), NodeKind::Unknown))
        .count();
    let ratio = unknown as f64 / total.max(1) as f64;
    assert!(
        ratio < 0.05,
        "Unknown ratio {:.1}% exceeds 5% ({unknown}/{total} nodes)",
        ratio * 100.0,
    );
}

#[test]
fn translate_never_panics_on_diverse_input() {
    // パターンマッチ・flip-flop・alias・lambda 等の稀構文を含む入力でも
    // panic しない（未対応ノードは Unknown に落ちる）。
    for src in [
        "case x; in [1, *rest]; end",
        "alias foo bar",
        "BEGIN { x }",
        "END { y }",
        "x = 1 if (a..b)",
        "->(x) { x }",
        "lambda { |a, b| a + b }",
        "h => { name: }",
        "x&.y&.z",
        "%i[a b c]",
        "%w[d e f]",
        "1r + 2i",
        "?A",
        "a = b = c = 0",
        "[*a, *b]",
        "foo(*args, **kwargs, &blk)",
        "module M; end; class C < M::Base; end",
        "def m(...) = other(...)",
        "x.tap { _1 }",
        "begin; rescue A, B => e; retry; end",
        "",
        "# only a comment\n",
        "=begin\nblock\n=end\n",
        "puts <<~HEREDOC\n  text #{value}\n HEREDOC\n",
        "/regex#{x}/im",
        "$global ||= []",
        "@@count &&= 0",
        "a, (b, c), *d = list",
        "next if done?",
    ] {
        let _ = murphy_translate::translate(src, "t.rb");
    }
}
