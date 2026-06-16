//! prism AST → murphy-ast arena AST の変換層（murphy-9cr.15）。
//!
//! prism と parser-gem のノード分割差（collapse/split）を変換層の内部だけで
//! 吸収する（Route B）。対応する `NodeKind` が無い prism ノードは
//! [`murphy_ast::NodeKind::Unknown`] へ落とし、`translate` は決して panic
//! しない。

mod translate;

pub use translate::translate;

/// 翻訳層のバージョン。prism→arena 変換の挙動 (どの prism ノードが
/// どの [`murphy_ast::NodeKind`] にマップされるか) が変わるたびに **手動で
/// bump** する。arena バイナリキャッシュのキーの一部に焼かれるので、bump
/// すれば旧キャッシュは自動的に無視される (murphy-9cr.26 §3.3)。
///
/// **`NodeKind` の variant 追加・削除・並べ替えに伴って必ず bump。**
/// バイナリ形式そのものを変える場合は代わりに
/// [`murphy_ast::FORMAT_VERSION`] を bump する。
pub const LAYER_VERSION: u32 = 4;

#[cfg(test)]
mod lib_tests {
    use super::*;

    #[test]
    fn layer_version_is_initialized() {
        // Anchors the current value. Bump alongside any prism→arena
        // mapping change so cache invalidation kicks in. Last bumped to 4
        // when `retry` started mapping to NodeKind::Retry instead of
        // Unknown (murphy-l1iy).
        assert_eq!(LAYER_VERSION, 4);
    }
}
