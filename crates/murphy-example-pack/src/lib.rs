//! murphy-example-pack — demo plugin pack for plugin authors.
//!
//! Reborn under the single-surface ABI (ADR 0038, murphy-9cr.10.1). Ships
//! two cops that illustrate complementary authorship vectors:
//!
//! - [`Example/NoEval`](no_eval) — `Send` (CallNode) dispatch + receiver
//!   matching.
//! - [`Example/TodoFormat`](todo_format) — file-visit dispatch (`KINDS = &[]`)
//!   + `#[derive(CopOptions)]` (`Vec<String>` + `bool`).
//!
//! The pack is the canonical reference distribution for the e2e plugin
//! loading path (`crates/murphy-cli/tests/plugin_pack_e2e.rs`).

pub mod no_eval;
pub mod todo_format;

// cop の登録は各 cop ファイルの submit_cop!(T) が担う。
murphy_plugin_api::register_cops!(mode = dynamic);

#[cfg(test)]
mod tests {
    /// Dummy smoke test: ensures `cargo test --workspace` materialises
    /// the cdylib build artifact (the e2e test in
    /// `crates/murphy-cli/tests/plugin_pack_e2e.rs` reads it via dlopen).
    /// The Cargo dep graph already guarantees this through
    /// `murphy-cli`'s `[dev-dependencies]`, but the explicit test keeps
    /// the invariant local to this crate.
    #[test]
    fn smoke_compiles() {}

    /// Cross-crate guard for `def_node_matcher!` expansion (murphy-a70).
    ///
    /// `def_node_matcher!` originally emitted `::murphy_ast::NodeKind /
    /// NodeId / NodeKindTag`, which broke pioneer adoption in
    /// `murphy-rails` (the consuming crate had no `murphy-ast` dep —
    /// the single-surface ABI forbids it). Macro fix routes these
    /// paths through `::murphy_plugin_api::` re-exports.
    ///
    /// `murphy-plugin-macros/tests/node_pattern_behavior.rs` cannot
    /// catch the regression because it runs *inside* `murphy-plugin-
    /// macros`, which has direct access to `murphy_ast`. This test
    /// lives in `murphy-example-pack`, which depends on
    /// `murphy-plugin-api` only — exactly the same dep posture as any
    /// future external plugin pack. If the macro ever re-introduces a
    /// `::murphy_ast::...` reference, this test stops compiling.
    #[test]
    fn node_pattern_expands_via_plugin_api_reexports() {
        use murphy_plugin_api::def_node_matcher;
        def_node_matcher!(_probe_send, "(send nil? :probe)");
        // Take the fn pointer to force the matcher body to be emitted
        // (the const path resolution happens inside it). Signature is
        // pinned to plugin-api re-exports so a regression to
        // `::murphy_ast::NodeId` here would mismatch.
        let _: fn(murphy_plugin_api::NodeId, &murphy_plugin_api::Cx<'_>) -> bool = _probe_send;
    }
}
