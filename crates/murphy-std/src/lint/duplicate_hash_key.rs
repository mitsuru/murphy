use std::collections::HashMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct DuplicateHashKey;

#[cop(
    name = "Lint/DuplicateHashKey",
    description = "Flag duplicate literal hash keys.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicateHashKey {
    #[on_node(kind = "hash")]
    fn check_hash(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Hash(list) = *cx.kind(node) else {
            return;
        };
        let mut seen = HashMap::<String, NodeId>::new();
        for pair in cx.list(list) {
            let NodeKind::Pair { key, .. } = *cx.kind(*pair) else {
                continue;
            };
            let Some(k) = literal_key(cx, key) else {
                continue;
            };
            if seen.insert(k, key).is_some() {
                cx.emit_offense(cx.range(key), "Duplicate hash key", None);
            }
        }
    }
}

fn literal_key(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    match *cx.kind(node) {
        NodeKind::Sym(s) => Some(format!("sym:{}", cx.symbol_str(s))),
        NodeKind::Str(s) => Some(format!("str:{}", cx.string_str(s))),
        NodeKind::Int(i) => Some(format!("int:{i}")),
        NodeKind::Nil => Some("nil".to_string()),
        NodeKind::True_ => Some("true".to_string()),
        NodeKind::False_ => Some("false".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::DuplicateHashKey;
    use murphy_plugin_api::test_support::{expect_no_offenses, expect_offense, indoc};

    #[test]
    fn flags_duplicate_literal_keys() {
        expect_offense!(
            DuplicateHashKey,
            indoc! {r#"
            {
              a: 1,
              b: 2,
              a: 3,
              ^^ Duplicate hash key
              '名前' => 1,
              '名前' => 2,
              ^^^^ Duplicate hash key
            }
        "#}
        );
    }

    #[test]
    fn ignores_dynamic_keys_and_distinct_literal_types() {
        expect_no_offenses!(DuplicateHashKey, "{ a => 1, a => 2, :a => 1, 'a' => 2 }\n");
    }
}
