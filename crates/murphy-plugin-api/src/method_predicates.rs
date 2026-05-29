//! Method-identifier predicates — Murphy's port of RuboCop's
//! `RuboCop::AST::MethodIdentifierPredicates` mixin.
//!
//! These are **pure** functions of a method-name string: they classify a
//! selector (`==`, `foo?`, `[]=`, …) without consulting the AST. Cops that
//! already hold a selector `&str` call these directly; cops holding a
//! [`crate::NodeId`] use the thin [`crate::Cx`] wrappers (`cx.is_*_method`),
//! which resolve the node's selector first.
//!
//! Centralising the constant sets here is the point of the surface: before
//! this module each cop re-derived its own (often divergent) operator list —
//! e.g. `Style/RedundantSelf` shipped a hand-rolled set missing `!@`, `~@`,
//! and `` ` ``.
//!
//! ```murphy-parity
//! upstream: rubocop-ast
//! upstream_const: MethodIdentifierPredicates::{OPERATOR_METHODS, ENUMERATOR_METHODS,
//!   ENUMERABLE_METHODS, NONMUTATING_BINARY_OPERATOR_METHODS,
//!   NONMUTATING_UNARY_OPERATOR_METHODS, NONMUTATING_ARRAY_METHODS,
//!   NONMUTATING_HASH_METHODS, NONMUTATING_STRING_METHODS}, Node::COMPARISON_OPERATORS
//! upstream_version_checked: master (2026-05)
//! ruby_snapshot: ENUMERABLE_METHODS captured under MRI 3.3.5 (mise.toml pin)
//! ```
//!
//! Selector spellings match the parser gem (verified against Murphy's prism
//! translation via `murphy ast --format sexp`): unary minus is `-@`, the
//! index setter is `[]=`, the backtick method is `` ` ``.

/// `Node::COMPARISON_OPERATORS` — `%i[== === != <= >= > <]`.
const COMPARISON_METHODS: &[&str] = &["==", "===", "!=", "<=", ">=", ">", "<"];

/// `MethodIdentifierPredicates::OPERATOR_METHODS` —
/// `%i[| ^ & <=> == === =~ > >= < <= << >> + - * / % ** ~ +@ -@ !@ ~@ [] []= ! != !~ \`]`.
const OPERATOR_METHODS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/",
    "%", "**", "~", "+@", "-@", "!@", "~@", "[]", "[]=", "!", "!=", "!~", "`",
];

/// `MethodIdentifierPredicates::ENUMERATOR_METHODS`.
const ENUMERATOR_METHODS: &[&str] = &[
    "collect",
    "collect_concat",
    "detect",
    "downto",
    "each",
    "find",
    "find_all",
    "find_index",
    "inject",
    "loop",
    "map!",
    "map",
    "reduce",
    "reject",
    "reject!",
    "reverse_each",
    "select",
    "select!",
    "times",
    "upto",
];

/// `MethodIdentifierPredicates::ENUMERABLE_METHODS` —
/// `(Enumerable.instance_methods + [:each]).to_set`. This set is
/// **runtime-derived in RuboCop**, so it is version-dependent; this
/// snapshot is `Enumerable.instance_methods + [:each]` under MRI 3.3.5
/// (the version pinned in `mise.toml`). Re-snapshot if that pin moves.
const ENUMERABLE_METHODS: &[&str] = &[
    "all?",
    "any?",
    "chain",
    "chunk",
    "chunk_while",
    "collect",
    "collect_concat",
    "compact",
    "count",
    "cycle",
    "detect",
    "drop",
    "drop_while",
    "each",
    "each_cons",
    "each_entry",
    "each_slice",
    "each_with_index",
    "each_with_object",
    "entries",
    "filter",
    "filter_map",
    "find",
    "find_all",
    "find_index",
    "first",
    "flat_map",
    "grep",
    "grep_v",
    "group_by",
    "include?",
    "inject",
    "lazy",
    "map",
    "max",
    "max_by",
    "member?",
    "min",
    "min_by",
    "minmax",
    "minmax_by",
    "none?",
    "one?",
    "partition",
    "reduce",
    "reject",
    "reverse_each",
    "select",
    "slice_after",
    "slice_before",
    "slice_when",
    "sort",
    "sort_by",
    "sum",
    "take",
    "take_while",
    "tally",
    "to_a",
    "to_h",
    "to_set",
    "uniq",
    "zip",
];

/// `MethodIdentifierPredicates::NONMUTATING_BINARY_OPERATOR_METHODS`.
const NONMUTATING_BINARY_OPERATOR_METHODS: &[&str] = &[
    "*", "/", "%", "+", "-", "==", "===", "!=", "<", ">", "<=", ">=", "<=>",
];

/// `MethodIdentifierPredicates::NONMUTATING_UNARY_OPERATOR_METHODS`.
const NONMUTATING_UNARY_OPERATOR_METHODS: &[&str] = &["+@", "-@", "~", "!"];

/// `MethodIdentifierPredicates::NONMUTATING_ARRAY_METHODS`. Transcribed
/// verbatim from rubocop-ast master (set membership; an over-broad entry
/// is harmless, a missing one is the only risk worth re-checking).
const NONMUTATING_ARRAY_METHODS: &[&str] = &[
    "all?",
    "any?",
    "assoc",
    "at",
    "bsearch",
    "bsearch_index",
    "collect",
    "combination",
    "compact",
    "count",
    "cycle",
    "deconstruct",
    "difference",
    "dig",
    "drop",
    "drop_while",
    "each",
    "each_index",
    "empty?",
    "eql?",
    "fetch",
    "filter",
    "find_index",
    "first",
    "flatten",
    "hash",
    "include?",
    "index",
    "inspect",
    "intersection",
    "join",
    "last",
    "length",
    "map",
    "max",
    "min",
    "minmax",
    "none?",
    "one?",
    "pack",
    "permutation",
    "product",
    "rassoc",
    "reject",
    "repeated_combination",
    "repeated_permutation",
    "reverse",
    "reverse_each",
    "rindex",
    "rotate",
    "sample",
    "select",
    "shuffle",
    "size",
    "slice",
    "sort",
    "sum",
    "take",
    "take_while",
    "to_a",
    "to_ary",
    "to_h",
    "to_s",
    "transpose",
    "union",
    "uniq",
    "values_at",
    "zip",
    "|",
];

/// `MethodIdentifierPredicates::NONMUTATING_HASH_METHODS`. Transcribed
/// verbatim from rubocop-ast master.
const NONMUTATING_HASH_METHODS: &[&str] = &[
    "any?",
    "assoc",
    "compact",
    "dig",
    "each",
    "each_key",
    "each_pair",
    "each_value",
    "empty?",
    "eql?",
    "fetch",
    "fetch_values",
    "filter",
    "flatten",
    "has_key?",
    "has_value?",
    "hash",
    "include?",
    "inspect",
    "invert",
    "key",
    "key?",
    "keys?",
    "length",
    "member?",
    "merge",
    "rassoc",
    "rehash",
    "reject",
    "select",
    "size",
    "slice",
    "to_a",
    "to_h",
    "to_hash",
    "to_proc",
    "to_s",
    "transform_keys",
    "transform_values",
    "value?",
    "values",
    "values_at",
];

/// `MethodIdentifierPredicates::NONMUTATING_STRING_METHODS`. Transcribed
/// verbatim from rubocop-ast master.
const NONMUTATING_STRING_METHODS: &[&str] = &[
    "ascii_only?",
    "b",
    "bytes",
    "bytesize",
    "byteslice",
    "capitalize",
    "casecmp",
    "casecmp?",
    "center",
    "chars",
    "chomp",
    "chop",
    "chr",
    "codepoints",
    "count",
    "crypt",
    "delete",
    "delete_prefix",
    "delete_suffix",
    "downcase",
    "dump",
    "each_byte",
    "each_char",
    "each_codepoint",
    "each_grapheme_cluster",
    "each_line",
    "empty?",
    "encode",
    "encoding",
    "end_with?",
    "eql?",
    "getbyte",
    "grapheme_clusters",
    "gsub",
    "hash",
    "hex",
    "include",
    "index",
    "inspect",
    "intern",
    "length",
    "lines",
    "ljust",
    "lstrip",
    "match",
    "match?",
    "next",
    "oct",
    "ord",
    "partition",
    "reverse",
    "rindex",
    "rjust",
    "rpartition",
    "rstrip",
    "scan",
    "scrub",
    "size",
    "slice",
    "squeeze",
    "start_with?",
    "strip",
    "sub",
    "succ",
    "sum",
    "swapcase",
    "to_a",
    "to_c",
    "to_f",
    "to_i",
    "to_r",
    "to_s",
    "to_str",
    "to_sym",
    "tr",
    "tr_s",
    "unicode_normalize",
    "unicode_normalized?",
    "unpack",
    "unpack1",
    "upcase",
    "upto",
    "valid_encoding?",
];

/// `comparison_method?` — the selector is one of `Node::COMPARISON_OPERATORS`.
pub fn is_comparison_method(name: &str) -> bool {
    COMPARISON_METHODS.contains(&name)
}

/// `operator_method?` — the selector is one of `OPERATOR_METHODS`.
pub fn is_operator_method(name: &str) -> bool {
    OPERATOR_METHODS.contains(&name)
}

/// `assignment_method?` — a setter selector: ends with `=` but is **not** a
/// comparison method (so `==`, `!=`, `<=`, `>=`, `===` are excluded, while
/// `foo=` and `[]=` qualify). The `!is_comparison_method` guard is the
/// load-bearing half of the definition.
pub fn is_assignment_method(name: &str) -> bool {
    !is_comparison_method(name) && name.ends_with('=')
}

/// `predicate_method?` — the selector ends with `?`.
pub fn is_predicate_method(name: &str) -> bool {
    name.ends_with('?')
}

/// `bang_method?` — the selector ends with `!`. Faithful to RuboCop, the bare
/// `!` selector counts (`!=` does not, as it ends with `=`).
pub fn is_bang_method(name: &str) -> bool {
    name.ends_with('!')
}

/// `camel_case_method?` — the selector starts with an ASCII upper-case letter
/// (RuboCop's `/\A[A-Z]/`). Unicode upper-case does not qualify.
pub fn is_camel_case_method(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
}

/// `enumerable_method?` — the selector is one of `ENUMERABLE_METHODS`.
/// The set is a snapshot of MRI 3.3.5 (see [`ENUMERABLE_METHODS`]).
pub fn is_enumerable_method(name: &str) -> bool {
    ENUMERABLE_METHODS.contains(&name)
}

/// `enumerator_method?` — the selector is one of `ENUMERATOR_METHODS`
/// **or** starts with `each_`.
pub fn is_enumerator_method(name: &str) -> bool {
    ENUMERATOR_METHODS.contains(&name) || name.starts_with("each_")
}

/// `nonmutating_binary_operator_method?`.
pub fn is_nonmutating_binary_operator_method(name: &str) -> bool {
    NONMUTATING_BINARY_OPERATOR_METHODS.contains(&name)
}

/// `nonmutating_unary_operator_method?`.
pub fn is_nonmutating_unary_operator_method(name: &str) -> bool {
    NONMUTATING_UNARY_OPERATOR_METHODS.contains(&name)
}

/// `nonmutating_operator_method?` — a non-mutating binary **or** unary
/// operator method.
pub fn is_nonmutating_operator_method(name: &str) -> bool {
    is_nonmutating_binary_operator_method(name) || is_nonmutating_unary_operator_method(name)
}

/// `nonmutating_array_method?`.
pub fn is_nonmutating_array_method(name: &str) -> bool {
    NONMUTATING_ARRAY_METHODS.contains(&name)
}

/// `nonmutating_hash_method?`.
pub fn is_nonmutating_hash_method(name: &str) -> bool {
    NONMUTATING_HASH_METHODS.contains(&name)
}

/// `nonmutating_string_method?`.
pub fn is_nonmutating_string_method(name: &str) -> bool {
    NONMUTATING_STRING_METHODS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_methods_match_the_seven_operators() {
        for op in ["==", "===", "!=", "<=", ">=", ">", "<"] {
            assert!(
                is_comparison_method(op),
                "{op} should be a comparison method"
            );
        }
    }

    #[test]
    fn comparison_method_rejects_non_comparisons() {
        for name in ["+", "<<", "foo", "foo?", "[]", "=~", ""] {
            assert!(
                !is_comparison_method(name),
                "{name} is not a comparison method"
            );
        }
    }

    #[test]
    fn operator_methods_include_unary_and_backtick_and_setter() {
        // The exact forms the hand-rolled RedundantSelf set was missing.
        for op in [
            "+@", "-@", "!@", "~@", "`", "[]", "[]=", "<=>", "!", "!~", "=~",
        ] {
            assert!(is_operator_method(op), "{op} should be an operator method");
        }
    }

    #[test]
    fn operator_method_rejects_plain_identifiers() {
        for name in ["foo", "foo?", "foo!", "foo=", "Foo", ""] {
            assert!(
                !is_operator_method(name),
                "{name} is not an operator method"
            );
        }
    }

    #[test]
    fn assignment_method_is_setter_but_not_comparison() {
        for name in ["foo=", "[]=", "bar="] {
            assert!(
                is_assignment_method(name),
                "{name} should be an assignment method"
            );
        }
        // Comparison operators end with `=` but must not count.
        for name in ["==", "!=", "<=", ">=", "==="] {
            assert!(
                !is_assignment_method(name),
                "{name} must be excluded by the comparison guard"
            );
        }
        for name in ["foo", "foo?", "<<", ""] {
            assert!(
                !is_assignment_method(name),
                "{name} is not an assignment method"
            );
        }
    }

    #[test]
    fn predicate_method_ends_with_question() {
        assert!(is_predicate_method("foo?"));
        assert!(is_predicate_method("empty?"));
        assert!(!is_predicate_method("foo"));
        assert!(!is_predicate_method("foo!"));
        assert!(!is_predicate_method(""));
    }

    #[test]
    fn bang_method_ends_with_bang_including_bare_bang() {
        assert!(is_bang_method("foo!"));
        assert!(
            is_bang_method("!"),
            "bare ! is a bang method, faithful to RuboCop"
        );
        assert!(!is_bang_method("foo"));
        assert!(
            !is_bang_method("!="),
            "!= ends with = so it is not a bang method"
        );
        assert!(!is_bang_method(""));
    }

    #[test]
    fn camel_case_method_requires_ascii_capital() {
        assert!(is_camel_case_method("Foo"));
        assert!(is_camel_case_method("HTTPClient"));
        assert!(!is_camel_case_method("foo"));
        assert!(!is_camel_case_method("_foo"));
        assert!(!is_camel_case_method(""));
        // Unicode upper-case must not qualify (RuboCop pins to /\A[A-Z]/).
        assert!(!is_camel_case_method("Élan"));
    }

    #[test]
    fn enumerable_method_matches_the_snapshot() {
        for name in [
            "map",
            "select",
            "each",
            "inject",
            "flat_map",
            "find_index",
            "to_set",
        ] {
            assert!(
                is_enumerable_method(name),
                "{name} should be an enumerable method"
            );
        }
        for name in ["map!", "push", "foo", ""] {
            assert!(
                !is_enumerable_method(name),
                "{name} is not an enumerable method"
            );
        }
    }

    #[test]
    fn enumerator_method_matches_set_or_each_prefix() {
        // Members of the explicit set.
        for name in ["collect", "map!", "select!", "reduce", "times", "upto"] {
            assert!(
                is_enumerator_method(name),
                "{name} is in ENUMERATOR_METHODS"
            );
        }
        // `each_*` prefix rule — even when not in the set.
        for name in ["each_with_object", "each_slice", "each_foo"] {
            assert!(
                is_enumerator_method(name),
                "{name} matches the each_ prefix rule"
            );
        }
        // Bare `each` is in the set; `eacher` matches neither rule
        // (not in-set, and no `each_` prefix — there is no underscore).
        assert!(is_enumerator_method("each"));
        for name in ["push", "eacher", "foo", ""] {
            assert!(
                !is_enumerator_method(name),
                "{name} is not an enumerator method"
            );
        }
    }

    #[test]
    fn nonmutating_operator_method_unions_binary_and_unary() {
        // Binary.
        assert!(is_nonmutating_binary_operator_method("+"));
        assert!(is_nonmutating_binary_operator_method("<=>"));
        assert!(!is_nonmutating_binary_operator_method("+@"));
        // Unary.
        assert!(is_nonmutating_unary_operator_method("+@"));
        assert!(is_nonmutating_unary_operator_method("!"));
        assert!(!is_nonmutating_unary_operator_method("+"));
        // Union covers both, excludes mutating operators like `<<`.
        assert!(is_nonmutating_operator_method("+"));
        assert!(is_nonmutating_operator_method("~"));
        assert!(!is_nonmutating_operator_method("<<"));
        assert!(!is_nonmutating_operator_method("[]="));
    }

    #[test]
    fn nonmutating_collection_methods_match_their_tables() {
        // Array.
        assert!(is_nonmutating_array_method("map"));
        assert!(is_nonmutating_array_method("|"));
        assert!(!is_nonmutating_array_method("push"));
        // Hash.
        assert!(is_nonmutating_hash_method("merge"));
        assert!(is_nonmutating_hash_method("has_key?"));
        assert!(!is_nonmutating_hash_method("store"));
        // String.
        assert!(is_nonmutating_string_method("upcase"));
        assert!(is_nonmutating_string_method("start_with?"));
        assert!(!is_nonmutating_string_method("gsub!"));
        assert!(!is_nonmutating_string_method("concat"));
    }
}
