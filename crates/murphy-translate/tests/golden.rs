//! prism→arena 翻訳の S 式ゴールデンテスト。
//!
//! `BLESS=1 cargo test -p murphy-translate --test golden` で snapshot 再生成。
//!
//! S 式プリンタ本体は [`murphy_ast::ast_to_sexp`] に置かれている
//! (`NodeKind` の網羅 match を 2 箇所に分散させないため)。本テストはその
//! 出力に末尾改行を 1 個付けた文字列を `snapshots/<name>.sexp` と照合する。

use murphy_ast::ast_to_sexp;
use std::path::PathBuf;

/// `fixtures/<name>.rb` を翻訳し、S 式を `snapshots/<name>.sexp` と照合する。
/// `BLESS` 環境変数があれば snapshot を上書きする。
fn check(name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let src = std::fs::read_to_string(dir.join("fixtures").join(format!("{name}.rb"))).unwrap();
    let ast = murphy_translate::translate(&src, format!("{name}.rb"));
    let got = format!("{}\n", ast_to_sexp(&ast));
    let snap_path = dir.join("snapshots").join(format!("{name}.sexp"));
    if std::env::var("BLESS").is_ok() {
        std::fs::write(&snap_path, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&snap_path).unwrap_or_default();
    assert_eq!(
        got, want,
        "snapshot mismatch for {name}; BLESS=1 to re-bless"
    );
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

#[test]
fn golden_case_in() {
    check("case_in");
}
