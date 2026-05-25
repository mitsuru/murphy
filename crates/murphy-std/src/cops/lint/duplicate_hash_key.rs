//! `Lint/DuplicateHashKey` — flag a hash literal that has the same key
//! twice (the second binding wins, so the first is dead).
//!
//! ## Keys covered
//!
//! Matches RuboCop's `recursive_basic_literal?` plus `const_type?`:
//!
//! - Atomic basic literals: `sym`, `str`, `int`, `float`, `nil`,
//!   `true`, `false`.
//! - **`Const`** — `STATUS_OK => 1, STATUS_OK => 2`, including nested
//!   constant paths (`Foo::Bar`).
//! - **Compound literals** — `Array`, `Hash`, `RangeExpr`, and a
//!   non-interpolated `Regexp`. A compound literal counts only when
//!   every element is itself a basic-literal-or-const.
//!
//! Interpolated strings/regexps and other non-literal keys are skipped
//! because their values aren't statically known.

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
                cx.emit_offense(cx.range(key), "Duplicated key in hash literal.", None);
            }
        }
    }
}

/// Recursive serialization of a hash-key expression to a canonical
/// string. `None` means "not a basic / compound literal" — those keys
/// can't be statically compared, so the cop ignores them.
fn literal_key(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    match *cx.kind(node) {
        NodeKind::Sym(s) => Some(format!("sym:{}", cx.symbol_str(s))),
        NodeKind::Str(s) => Some(format!("str:{}", cx.string_str(s))),
        NodeKind::Int(i) => Some(format!("int:{i}")),
        NodeKind::Float(f) => Some(format!("float:{f:?}")),
        NodeKind::Nil => Some("nil".to_string()),
        NodeKind::True_ => Some("true".to_string()),
        NodeKind::False_ => Some("false".to_string()),
        NodeKind::Const { scope, name } => {
            let scope_part = match scope.get() {
                Some(s) => literal_key(cx, s)?,
                None => String::new(),
            };
            Some(format!("const:{scope_part}::{}", cx.symbol_str(name)))
        }
        NodeKind::Array(list) => {
            let items: Option<Vec<String>> = cx
                .list(list)
                .iter()
                .map(|&id| literal_key(cx, id))
                .collect();
            Some(format!("array:[{}]", items?.join(",")))
        }
        NodeKind::Hash(list) => {
            let pairs = cx.list(list);
            let mut items: Vec<String> = Vec::with_capacity(pairs.len());
            for &pair in pairs {
                let NodeKind::Pair { key, value } = *cx.kind(pair) else {
                    return None;
                };
                items.push(format!(
                    "{}=>{}",
                    literal_key(cx, key)?,
                    literal_key(cx, value)?
                ));
            }
            Some(format!("hash:{{{}}}", items.join(",")))
        }
        NodeKind::RangeExpr {
            begin_,
            end_,
            exclusive,
        } => {
            let b = match begin_.get() {
                Some(id) => literal_key(cx, id)?,
                None => String::new(),
            };
            let e = match end_.get() {
                Some(id) => literal_key(cx, id)?,
                None => String::new(),
            };
            let sep = if exclusive { "..." } else { ".." };
            Some(format!("range:{b}{sep}{e}"))
        }
        NodeKind::Regexp { parts, opts } => {
            // Only non-interpolated regexps have a statically known value:
            // every part must be a plain `Str`.
            let mut s = String::new();
            for &part in cx.list(parts) {
                match *cx.kind(part) {
                    NodeKind::Str(id) => s.push_str(cx.string_str(id)),
                    _ => return None,
                }
            }
            Some(format!("regexp:{s}/{}", cx.symbol_str(opts)))
        }
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
              ^^ Duplicated key in hash literal.
              '名前' => 1,
              '名前' => 2,
              ^^^^ Duplicated key in hash literal.
              1.0 => 1,
              1.0 => 2,
              ^^^ Duplicated key in hash literal.
            }
        "#}
        );
    }

    #[test]
    fn ignores_dynamic_keys_and_distinct_literal_types() {
        expect_no_offenses!(DuplicateHashKey, "{ a => 1, a => 2, :a => 1, 'a' => 2 }\n");
    }

    // murphy-sn9r: const + compound literal key support.

    #[test]
    fn flags_duplicate_const_keys() {
        expect_offense!(
            DuplicateHashKey,
            indoc! {r#"
                {
                  STATUS_OK => 1,
                  STATUS_OK => 2,
                  ^^^^^^^^^ Duplicated key in hash literal.
                }
            "#}
        );
    }

    #[test]
    fn flags_duplicate_const_path_keys() {
        expect_offense!(
            DuplicateHashKey,
            indoc! {r#"
                {
                  Foo::Bar => 1,
                  Foo::Bar => 2,
                  ^^^^^^^^ Duplicated key in hash literal.
                }
            "#}
        );
    }

    #[test]
    fn const_vs_different_const_is_not_duplicate() {
        expect_no_offenses!(
            DuplicateHashKey,
            indoc! {r#"
                {
                  STATUS_OK => 1,
                  STATUS_ERR => 2,
                }
            "#}
        );
    }

    #[test]
    fn flags_duplicate_array_keys() {
        expect_offense!(
            DuplicateHashKey,
            indoc! {r#"
                {
                  [1, 2] => :a,
                  [1, 2] => :b,
                  ^^^^^^ Duplicated key in hash literal.
                }
            "#}
        );
    }

    #[test]
    fn flags_duplicate_range_keys() {
        expect_offense!(
            DuplicateHashKey,
            indoc! {r#"
                {
                  1..3 => :a,
                  1..3 => :b,
                  ^^^^ Duplicated key in hash literal.
                }
            "#}
        );
    }

    #[test]
    fn exclusive_range_differs_from_inclusive() {
        expect_no_offenses!(
            DuplicateHashKey,
            indoc! {r#"
                {
                  1..3 => :a,
                  1...3 => :b,
                }
            "#}
        );
    }

    #[test]
    fn flags_duplicate_regexp_keys() {
        expect_offense!(
            DuplicateHashKey,
            indoc! {r#"
                {
                  /foo/i => 1,
                  /foo/i => 2,
                  ^^^^^^ Duplicated key in hash literal.
                }
            "#}
        );
    }

    #[test]
    fn array_with_non_literal_element_is_not_keyed() {
        // `[x, 1]` contains an Lvar; the compound can't be keyed
        // statically, so the cop must not flag.
        expect_no_offenses!(
            DuplicateHashKey,
            indoc! {r#"
                {
                  [x, 1] => :a,
                  [x, 1] => :b,
                }
            "#}
        );
    }
}
