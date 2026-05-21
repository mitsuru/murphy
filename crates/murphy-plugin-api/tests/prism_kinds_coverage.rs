//! Drift guard for `murphy_plugin_api::kinds::ALL`.
//!
//! `murphy_core::cop::prism_node_kind` is an exhaustive `match` on
//! `ruby_prism::Node`, so any prism upgrade that adds or renames a variant
//! triggers a `non_exhaustive_patterns` compile error in murphy-core. This
//! test makes the *plugin-api* side feel the same drift: it parses the
//! function body out of murphy-core's source and asserts that the set of
//! snake_case wire names declared there matches `kinds::ALL` exactly.
//!
//! Workflow on prism upgrade:
//! 1. Update `murphy-core/src/cop.rs::prism_node_kind` to cover any new
//!    variant (this build will fail loudly until you do).
//! 2. Re-run the kinds.rs generator (see header comment in `kinds.rs`).
//! 3. This test goes green again.

/// Source text of murphy-core's cop.rs, embedded at compile time so the
/// drift check needs no filesystem access at runtime.
const CORE_COP_SOURCE: &str = include_str!("../../murphy-core/src/cop.rs");

#[test]
fn kinds_all_matches_murphy_core_prism_node_kind() {
    let body = extract_prism_node_kind_body(CORE_COP_SOURCE)
        .expect("prism_node_kind function found in murphy-core/src/cop.rs");
    let mut wire_names = extract_byte_literals(body);
    wire_names.sort();
    wire_names.dedup();

    let mut kinds_sorted: Vec<&str> = murphy_plugin_api::kinds::ALL.to_vec();
    kinds_sorted.sort();
    kinds_sorted.dedup();

    assert_eq!(
        kinds_sorted, wire_names,
        "murphy_plugin_api::kinds::ALL drifted from murphy_core::cop::prism_node_kind"
    );
    assert_eq!(murphy_plugin_api::kinds::COUNT, kinds_sorted.len());
}

fn extract_prism_node_kind_body(source: &str) -> Option<&str> {
    let signature = "pub fn prism_node_kind(node: &ruby_prism::Node<'_>) -> &'static [u8] {";
    let start = source.find(signature)?;
    let tail = &source[start..];
    // The matching brace closes at the start of a line followed by `}`.
    let end_rel = tail.find("\n}\n")?;
    Some(&tail[..end_rel])
}

fn extract_byte_literals(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut cursor = body;
    while let Some(idx) = cursor.find("b\"") {
        let rest = &cursor[idx + 2..];
        let Some(end) = rest.find('"') else {
            break;
        };
        let candidate = &rest[..end];
        if !candidate.is_empty()
            && candidate
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_')
        {
            out.push(candidate);
        }
        cursor = &rest[end + 1..];
    }
    out
}
