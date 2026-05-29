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
//! upstream_const: MethodIdentifierPredicates::OPERATOR_METHODS, Node::COMPARISON_OPERATORS
//! upstream_version_checked: master (2026-05)
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
}
