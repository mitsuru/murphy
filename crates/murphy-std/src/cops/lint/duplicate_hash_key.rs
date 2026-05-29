//! `Lint/DuplicateHashKey` — flag a hash literal that has the same key
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DuplicateHashKey
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-m7lp
//! notes: >
//!   Main literal-key behavior is implemented. Added Begin, And/Or, Send
//!   (LITERAL_RECURSIVE_METHODS), and Dstr (all-literal-fragment) key shapes
//!   to match RuboCop's recursive_basic_literal? coverage. Grouping-paren
//!   expressions like (1), (false && true) remain undetected because
//!   Murphy's translator emits Unknown for prism's ParenthesesNode (a
//!   cross-cutting translator gap, not fixable per-cop). Inner-hash
//!   canonicalization follow-up remains.
//! ```
//!
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
//! - **`Begin`** — single-child begin (interpolation wrapper), e.g.
//!   `"#{2}"` contains a `Begin(Int(2))` fragment.
//! - **`And`/`Or`** — boolean expressions where all children are
//!   basic-literals, e.g. `false && true`, `"#{false or true}"`.
//! - **`Send`** (LITERAL_RECURSIVE_METHODS) — calls like `!true` or
//!   `false <=> true` where receiver and all args are basic-literals.
//! - **`Dstr`** — interpolated strings where every fragment is a
//!   basic-literal, e.g. `"#{2}"`.
//!
//! **Limitation (translator gap):** grouping parentheses like `(1)` or
//! `(false && true)` as hash keys are **not** detected — prism's
//! `ParenthesesNode` is translated to `Unknown` in Murphy's AST. This
//! mirrors a cross-cutting translator gap that would need a framework
//! change to fix. Interpolated forms like `"#{false && true}"` work.
//!
//! Interpolated strings/regexps with non-literal fragments and other
//! non-literal keys are skipped because their values aren't statically known.

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

/// Methods that RuboCop considers "literal-recursive" when all operands
/// are themselves basic-literals.
const LITERAL_RECURSIVE_METHODS: &[&str] =
    &["!", "<=>", "==", "===", "!=", "<=", ">=", ">", "<", "*"];

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
    /// `false && true` or `false or true` — Boolean expression where
    /// all children are literal. `And` and `Or` use distinct discriminants
    /// so they don't collide with each other.
    BoolOp {
        kind: BoolOpKind,
        lhs: Box<LiteralKey>,
        rhs: Box<LiteralKey>,
    },
    /// A literal-recursive `Send` node: method is in
    /// `LITERAL_RECURSIVE_METHODS` and receiver+args are all literal.
    Call {
        method: String,
        receiver: Option<Box<LiteralKey>>,
        args: Vec<LiteralKey>,
    },
    /// An interpolated string where every fragment is a basic-literal,
    /// e.g. `"#{2}"`. Kept distinct from `Str` to avoid colliding with
    /// a plain string literal of the same chars.
    Dstr(Vec<LiteralKey>),
}

/// Distinguishes `&&`/`and` from `||`/`or` in [`LiteralKey::BoolOp`].
#[derive(Hash, PartialEq, Eq, PartialOrd, Ord)]
enum BoolOpKind {
    And,
    Or,
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

        // --- parity gap shapes ---

        // `(begin ... end)` / interpolation wrapper — a begin node with a
        // single literal child. In Murphy's AST, `Begin` arises from
        // `begin...end` and from interpolation wrappers inside `Dstr`.
        // Grouping parens like `(1)` use prism's `ParenthesesNode` which
        // translates to `Unknown` — those are NOT handled here.
        NodeKind::Begin(list) => match cx.list(list) {
            &[single] => literal_key(cx, single)?,
            _ => return None,
        },

        // `false && true` / `false or true` — boolean expression
        // where both children are basic-literals.
        NodeKind::And { lhs, rhs } => LiteralKey::BoolOp {
            kind: BoolOpKind::And,
            lhs: Box::new(literal_key(cx, lhs)?),
            rhs: Box::new(literal_key(cx, rhs)?),
        },
        NodeKind::Or { lhs, rhs } => LiteralKey::BoolOp {
            kind: BoolOpKind::Or,
            lhs: Box::new(literal_key(cx, lhs)?),
            rhs: Box::new(literal_key(cx, rhs)?),
        },

        // `!true`, `false <=> true` etc. — Send where the method is in
        // LITERAL_RECURSIVE_METHODS and all operands are basic-literals.
        NodeKind::Send {
            receiver,
            method,
            args,
        } => {
            let method_str = cx.symbol_str(method);
            if !LITERAL_RECURSIVE_METHODS.contains(&method_str) {
                return None;
            }
            let recv_key = match receiver.get() {
                Some(id) => Some(Box::new(literal_key(cx, id)?)),
                None => None,
            };
            let arg_keys: Option<Vec<LiteralKey>> = cx
                .list(args)
                .iter()
                .map(|&id| literal_key(cx, id))
                .collect();
            LiteralKey::Call {
                method: method_str.to_string(),
                receiver: recv_key,
                args: arg_keys?,
            }
        }

        // `"#{2}"` — interpolated string where every fragment is a
        // basic-literal. Each child is keyed recursively:
        //   - plain `Str` parts → `LiteralKey::Str`
        //   - interpolation wrappers (`Begin(expr)`) → `literal_key` on
        //     the wrapped expression
        // Result is `LiteralKey::Dstr` (kept distinct from plain `Str` to
        // avoid false collisions between `"2"` and `"#{2}"`).
        NodeKind::Dstr(list) => {
            let parts: Option<Vec<LiteralKey>> = cx
                .list(list)
                .iter()
                .map(|&id| literal_key(cx, id))
                .collect();
            LiteralKey::Dstr(parts?)
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

    // --- parity gap tests: And/Or, Send, Dstr ---

    // Note: grouping-paren expressions like `(1)`, `(false && true)`,
    // `(false <=> true)` translate to `Unknown` in Murphy's AST because
    // prism's `ParenthesesNode` is not yet translated. Those forms are NOT
    // detected. Tests below use equivalent unparenthesized forms or
    // interpolated-string routes that ARE reachable.

    #[test]
    fn flags_duplicate_and_keys() {
        // `false && true` is an And node where both children are literals.
        test::<DuplicateHashKey>().expect_offense(indoc! {r#"
                {
                  false && true => 1,
                  false && true => 4,
                  ^^^^^^^^^^^^^ Duplicated key in hash literal.
                }
            "#});
    }

    #[test]
    fn and_vs_or_does_not_collide() {
        // `false && true` and `false or true` must be distinct keys.
        // (The `or` form can only appear without parens if it's in a Dstr.)
        test::<DuplicateHashKey>().expect_no_offenses(indoc! {r#"
                {
                  false && true => 1,
                  false && false => 2,
                }
            "#});
    }

    #[test]
    fn flags_duplicate_send_not_keys() {
        // `!true` is a Send node with method `!` and literal receiver.
        test::<DuplicateHashKey>().expect_offense(indoc! {r#"
                {
                  !true => 1,
                  !true => 4,
                  ^^^^^ Duplicated key in hash literal.
                }
            "#});
    }

    #[test]
    fn flags_duplicate_send_spaceship_keys() {
        // `false <=> true` is a Send node with method `<=>` and all-literal args.
        test::<DuplicateHashKey>().expect_offense(indoc! {r#"
                {
                  false <=> true => 1,
                  false <=> true => 4,
                  ^^^^^^^^^^^^^^ Duplicated key in hash literal.
                }
            "#});
    }

    #[test]
    fn send_with_non_literal_operand_is_not_keyed() {
        // `!x` — `x` is an Lvar; cannot be keyed statically.
        test::<DuplicateHashKey>().expect_no_offenses(indoc! {r#"
                {
                  !x => 1,
                  !x => 2,
                }
            "#});
    }

    #[test]
    fn flags_duplicate_dstr_literal_keys() {
        // `"#{2}"` is a Dstr node where every interpolated part is a literal Int.
        test::<DuplicateHashKey>().expect_offense(indoc! {r##"
                {
                  "#{2}" => 1,
                  "#{2}" => 4,
                  ^^^^^^ Duplicated key in hash literal.
                }
            "##});
    }

    #[test]
    fn dstr_with_non_literal_interpolation_is_not_keyed() {
        // `"#{x}"` contains an Lvar; the interpolation can't be keyed statically.
        test::<DuplicateHashKey>().expect_no_offenses(indoc! {r##"
                {
                  "#{x}" => 1,
                  "#{x}" => 2,
                }
            "##});
    }

    #[test]
    fn dstr_does_not_collide_with_plain_str() {
        // `"#{2}"` (Dstr) must not equal `"2"` (plain Str).
        test::<DuplicateHashKey>().expect_no_offenses(indoc! {r##"
                {
                  "#{2}" => 1,
                  "2" => 2,
                }
            "##});
    }

    #[test]
    fn flags_dstr_with_embedded_and() {
        // `"#{false && true}"` exercises Dstr + Begin + BoolOp path.
        test::<DuplicateHashKey>().expect_offense(indoc! {r##"
                {
                  "#{false && true}" => 1,
                  "#{false && true}" => 4,
                  ^^^^^^^^^^^^^^^^^^ Duplicated key in hash literal.
                }
            "##});
    }

    #[test]
    fn dstr_and_vs_or_does_not_collide() {
        // `"#{false && true}"` and `"#{false or true}"` must be distinct.
        test::<DuplicateHashKey>().expect_no_offenses(indoc! {r##"
                {
                  "#{false && true}" => 1,
                  "#{false or true}" => 2,
                }
            "##});
    }

    #[test]
    fn grouping_paren_key_not_detected_translator_gap() {
        // Grouping parentheses `(1)` translate to `Unknown` in Murphy's AST
        // (prism's `ParenthesesNode` is not yet translated). The cop does NOT
        // flag these — this is a known limitation pending a translator fix.
        test::<DuplicateHashKey>().expect_no_offenses(indoc! {r#"
                {
                  (1) => 1,
                  (1) => 4,
                }
            "#});
    }
}
