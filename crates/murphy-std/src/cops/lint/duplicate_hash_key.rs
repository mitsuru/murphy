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

use std::collections::HashSet;

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
        let mut seen: HashSet<LiteralKey> = HashSet::new();
        for pair in cx.list(list) {
            let NodeKind::Pair { key, .. } = *cx.kind(*pair) else {
                continue;
            };
            let Some(k) = literal_key(cx, key) else {
                continue;
            };
            if !seen.insert(k) {
                cx.emit_offense(cx.range(key), "Duplicated key in hash literal.", None);
            }
        }
    }
}

/// Structured canonical form of a literal hash key. Using an `enum`
/// instead of a serialized `String` avoids element-boundary collisions
/// like `["a,str:b"]` vs `["a", "b"]` (both would have flattened to the
/// same string under naive `,`-joined encoding).
///
/// `f64` is normalized via `to_bits` so the variant can derive `Hash` /
/// `Eq`. Two `NaN` bit patterns therefore compare unequal — fine for
/// this cop because a literal `0.0/0.0` doesn't appear as a hash key.
#[derive(Hash, PartialEq, Eq, PartialOrd, Ord)]
enum LiteralKey {
    Sym(String),
    Str(String),
    Int(i64),
    /// Bit pattern of the source `f64`.
    Float(u64),
    Nil,
    True,
    False,
    Const {
        scope: Option<Box<LiteralKey>>,
        name: String,
    },
    Array(Vec<LiteralKey>),
    Hash(Vec<(LiteralKey, LiteralKey)>),
    Range {
        begin: Option<Box<LiteralKey>>,
        end: Option<Box<LiteralKey>>,
        exclusive: bool,
    },
    Regexp {
        source: String,
        opts: String,
    },
}

/// Recursive structural keying of a hash-key expression. `None` means
/// "not a basic / compound literal" — those keys can't be statically
/// compared, so the cop ignores them.
fn literal_key(cx: &Cx<'_>, node: NodeId) -> Option<LiteralKey> {
    Some(match *cx.kind(node) {
        NodeKind::Sym(s) => LiteralKey::Sym(cx.symbol_str(s).to_string()),
        NodeKind::Str(s) => LiteralKey::Str(cx.string_str(s).to_string()),
        NodeKind::Int(i) => LiteralKey::Int(i),
        NodeKind::Float(f) => LiteralKey::Float(f.to_bits()),
        NodeKind::Nil => LiteralKey::Nil,
        NodeKind::True_ => LiteralKey::True,
        NodeKind::False_ => LiteralKey::False,
        NodeKind::Const { scope, name } => LiteralKey::Const {
            scope: match scope.get() {
                Some(s) => Some(Box::new(literal_key(cx, s)?)),
                None => None,
            },
            name: cx.symbol_str(name).to_string(),
        },
        NodeKind::Array(list) => {
            let items: Option<Vec<LiteralKey>> = cx
                .list(list)
                .iter()
                .map(|&id| literal_key(cx, id))
                .collect();
            LiteralKey::Array(items?)
        }
        NodeKind::Hash(list) => {
            let pairs = cx.list(list);
            let mut items: Vec<(LiteralKey, LiteralKey)> = Vec::with_capacity(pairs.len());
            for &pair in pairs {
                let NodeKind::Pair { key, value } = *cx.kind(pair) else {
                    return None;
                };
                items.push((literal_key(cx, key)?, literal_key(cx, value)?));
            }
            // Ruby's `Hash#==` is order-independent, so two hashes with
            // the same `{key => value}` set should compare equal even
            // when literal order differs. Sort by `(key, value)` to give
            // them the same canonical form.
            items.sort();
            LiteralKey::Hash(items)
        }
        NodeKind::RangeExpr {
            begin_,
            end_,
            exclusive,
        } => LiteralKey::Range {
            begin: match begin_.get() {
                Some(id) => Some(Box::new(literal_key(cx, id)?)),
                None => None,
            },
            end: match end_.get() {
                Some(id) => Some(Box::new(literal_key(cx, id)?)),
                None => None,
            },
            exclusive,
        },
        NodeKind::Regexp { parts, opts } => {
            // Only non-interpolated regexps have a statically known value:
            // every part must be a plain `Str`.
            let mut source = String::new();
            for &part in cx.list(parts) {
                match *cx.kind(part) {
                    NodeKind::Str(id) => source.push_str(cx.string_str(id)),
                    _ => return None,
                }
            }
            LiteralKey::Regexp {
                source,
                opts: cx.symbol_str(opts).to_string(),
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::DuplicateHashKey;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_literal_keys() {
        test::<DuplicateHashKey>().expect_offense(indoc! {r#"
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
        "#});
    }

    #[test]
    fn ignores_dynamic_keys_and_distinct_literal_types() {
        test::<DuplicateHashKey>().expect_no_offenses("{ a => 1, a => 2, :a => 1, 'a' => 2 }\n");
    }

    // murphy-sn9r: const + compound literal key support.

    #[test]
    fn flags_duplicate_const_keys() {
        test::<DuplicateHashKey>().expect_offense(indoc! {r#"
                {
                  STATUS_OK => 1,
                  STATUS_OK => 2,
                  ^^^^^^^^^ Duplicated key in hash literal.
                }
            "#});
    }

    #[test]
    fn flags_duplicate_const_path_keys() {
        test::<DuplicateHashKey>().expect_offense(indoc! {r#"
                {
                  Foo::Bar => 1,
                  Foo::Bar => 2,
                  ^^^^^^^^ Duplicated key in hash literal.
                }
            "#});
    }

    #[test]
    fn const_vs_different_const_is_not_duplicate() {
        test::<DuplicateHashKey>().expect_no_offenses(indoc! {r#"
                {
                  STATUS_OK => 1,
                  STATUS_ERR => 2,
                }
            "#});
    }

    #[test]
    fn flags_duplicate_array_keys() {
        test::<DuplicateHashKey>().expect_offense(indoc! {r#"
                {
                  [1, 2] => :a,
                  [1, 2] => :b,
                  ^^^^^^ Duplicated key in hash literal.
                }
            "#});
    }

    #[test]
    fn flags_duplicate_range_keys() {
        test::<DuplicateHashKey>().expect_offense(indoc! {r#"
                {
                  1..3 => :a,
                  1..3 => :b,
                  ^^^^ Duplicated key in hash literal.
                }
            "#});
    }

    #[test]
    fn exclusive_range_differs_from_inclusive() {
        test::<DuplicateHashKey>().expect_no_offenses(indoc! {r#"
                {
                  1..3 => :a,
                  1...3 => :b,
                }
            "#});
    }

    #[test]
    fn flags_duplicate_regexp_keys() {
        test::<DuplicateHashKey>().expect_offense(indoc! {r#"
                {
                  /foo/i => 1,
                  /foo/i => 2,
                  ^^^^^^ Duplicated key in hash literal.
                }
            "#});
    }

    #[test]
    fn inner_hash_key_is_order_independent() {
        // Ruby's `Hash#==` ignores insertion order, so two hash keys
        // that hold the same pairs must compare equal even when the
        // literal order differs.
        test::<DuplicateHashKey>().expect_offense(indoc! {r#"
                {
                  { a: 1, b: 2 } => :x,
                  { b: 2, a: 1 } => :y,
                  ^^^^^^^^^^^^^^ Duplicated key in hash literal.
                }
            "#});
    }

    #[test]
    fn string_containing_serializer_delimiters_does_not_collide_with_array() {
        // Regression guard for the naive `,`-joined-string approach:
        // `["a,str:b"]` and `["a", "b"]` would have flattened to the
        // same `array:[str:a,str:b]` string and falsely flagged.
        test::<DuplicateHashKey>().expect_no_offenses(indoc! {r#"
                {
                  ["a,str:b"] => 1,
                  ["a", "b"] => 2,
                }
            "#});
    }

    #[test]
    fn array_with_non_literal_element_is_not_keyed() {
        // `[x, 1]` contains an Lvar; the compound can't be keyed
        // statically, so the cop must not flag.
        test::<DuplicateHashKey>().expect_no_offenses(indoc! {r#"
                {
                  [x, 1] => :a,
                  [x, 1] => :b,
                }
            "#});
    }
}
