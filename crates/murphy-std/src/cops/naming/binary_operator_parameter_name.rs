//! `Naming/BinaryOperatorParameterName` — binary-operator method definitions
//! should name their single parameter `other`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/BinaryOperatorParameterName
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detection is an exact match to rubocop 1.87.0, verified against the
//!   standalone binary. RuboCop's matcher is
//!   `(def [#op_method? $_] (args $(arg [!:other !:_other])) _)`:
//!     * `op_method?(name)` returns `false` for the EXCLUDED set
//!       (`+@ -@ [] []= << === ` =~`), otherwise true when the name does
//!       not start with a word character (a real operator) OR the name is
//!       one of OP_LIKE_METHODS (`eql?`, `equal?`). Murphy reproduces this
//!       with `is_op_method`.
//!     * `(args $(arg ...))` requires EXACTLY ONE positional `arg` — a
//!       single `Arg` node and nothing else. Multi-arg defs (`def +(a, b)`),
//!       splat/optional/keyword params, and zero-arg operator defs do not
//!       match. Murphy gates on `list.len() == 1` with a single `Arg`.
//!     * The arg name must not be `other` or `_other`.
//!   RuboCop registers `on_def` only — NOT `on_defs` — so singleton operator
//!   defs (`def self.+(x)`) are never flagged. In Murphy a singleton `def
//!   self.+` is a `NodeKind::Def` with a `Some` receiver (only `def obj.foo`
//!   becomes `Defs`), so we guard with `cx.def_receiver(node).get().is_none()`
//!   to match RuboCop's `on_def`-only scope.
//!
//!   Intentional gap: RuboCop ships an autocorrector (rename the parameter
//!   and every matching `lvar`/`lvasgn` in the body to `other`). Per the
//!   port issue (murphy-e7bz.33: "Safe: safe / no-autocorrect") detection is
//!   in scope and autocorrect is intentionally omitted. No behavioral gap in
//!   what is *flagged*; the only difference is the absence of a fix.
//! ```
//!
//! ## Offense range
//!
//! Mirrors RuboCop's `add_offense(arg, ...)`: the caret covers just the
//! parameter name node (e.g. `amount` in `def +(amount)`), not the whole
//! `def`.

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

/// Operator-like method names that look like words but are treated as binary
/// operators by RuboCop (`OP_LIKE_METHODS`).
const OP_LIKE_METHODS: [&str; 2] = ["eql?", "equal?"];

/// Method names RuboCop's `EXCLUDED` set skips: unary operators, indexers, the
/// shovel operator, case-equality, the backtick command operator, and the
/// match operator. These take an argument but are never required to name it
/// `other`.
const EXCLUDED: [&str; 8] = ["+@", "-@", "[]", "[]=", "<<", "===", "`", "=~"];

#[derive(Default)]
pub struct BinaryOperatorParameterName;

#[cop(
    name = "Naming/BinaryOperatorParameterName",
    description = "When defining binary operators, name the argument other.",
    default_severity = "warning",
    default_enabled = true
)]
impl BinaryOperatorParameterName {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        // RuboCop registers `on_def` only. In Murphy a singleton operator def
        // (`def self.+(x)`) is a `Def` with a `Some` receiver, so skip it to
        // avoid over-firing vs RuboCop's `on_def`-only scope.
        if cx.def_receiver(node).get().is_some() {
            return;
        }

        let Some(name) = cx.method_name(node) else {
            return;
        };
        if !is_op_method(name) {
            return;
        }

        // `(args $(arg ...))` — exactly one positional `Arg` and nothing else.
        let Some(args_id) = cx.def_arguments(node).get() else {
            return;
        };
        let NodeKind::Args(list) = *cx.kind(args_id) else {
            return;
        };
        let [arg_id] = cx.list(list) else {
            return;
        };
        let &NodeKind::Arg(arg_sym) = cx.kind(*arg_id) else {
            return;
        };

        let arg_name = cx.symbol_str(arg_sym);
        if arg_name == "other" || arg_name == "_other" {
            return;
        }

        // `loc.name` is the precise parameter-name range; mirror RuboCop's
        // `add_offense(arg, ...)`.
        let name_loc = cx.node(*arg_id).loc.name;
        let offense_range = if name_loc == murphy_plugin_api::Range::ZERO {
            cx.range(*arg_id)
        } else {
            name_loc
        };

        let message =
            format!("When defining the `{name}` operator, name its argument `other`.");
        cx.emit_offense(offense_range, &message, None);
    }
}

/// Mirrors RuboCop's `op_method?`: `false` for the EXCLUDED set, otherwise true
/// when `name` does not start with a word character (an actual operator) OR is
/// one of OP_LIKE_METHODS (`eql?`, `equal?`).
fn is_op_method(name: &str) -> bool {
    if EXCLUDED.contains(&name) {
        return false;
    }
    !starts_with_word_char(name) || OP_LIKE_METHODS.contains(&name)
}

/// `!/\A[[:word:]]/.match?(name)` — Ruby's `[[:word:]]` is `[A-Za-z0-9_]` plus
/// non-ASCII word characters. Operator names like `+`, `<=>`, `==` start with a
/// punctuation byte and thus do NOT start with a word character.
fn starts_with_word_char(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c == '_' || c.is_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::BinaryOperatorParameterName;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offending operator defs (ground-truth carets derived from
    //     rubocop 1.87.0 column/last_column). ---

    #[test]
    fn flags_plus_operator() {
        // rubocop: line 1, col 7 (`amount`).
        test::<BinaryOperatorParameterName>().expect_offense(indoc! {r#"
            def +(amount)
                  ^^^^^^ When defining the `+` operator, name its argument `other`.
              amount
            end
        "#});
    }

    #[test]
    fn flags_equality_operator() {
        // rubocop: col 8 (`foo`).
        test::<BinaryOperatorParameterName>().expect_offense(indoc! {r#"
            def ==(foo)
                   ^^^ When defining the `==` operator, name its argument `other`.
              foo
            end
        "#});
    }

    #[test]
    fn flags_spaceship_operator() {
        // rubocop: col 9 (`bar`).
        test::<BinaryOperatorParameterName>().expect_offense(indoc! {r#"
            def <=>(bar)
                    ^^^ When defining the `<=>` operator, name its argument `other`.
              bar
            end
        "#});
    }

    #[test]
    fn flags_minus_operator() {
        test::<BinaryOperatorParameterName>().expect_offense(indoc! {r#"
            def -(z)
                  ^ When defining the `-` operator, name its argument `other`.
              z
            end
        "#});
    }

    #[test]
    fn flags_shift_right_operator() {
        // The task hint claimed `>>` is excluded — it is NOT. EXCLUDED has
        // only `<<`. rubocop fires on `def >>(x)` at col 8.
        test::<BinaryOperatorParameterName>().expect_offense(indoc! {r#"
            def >>(x)
                   ^ When defining the `>>` operator, name its argument `other`.
              x
            end
        "#});
    }

    #[test]
    fn flags_power_operator() {
        test::<BinaryOperatorParameterName>().expect_offense(indoc! {r#"
            def **(y)
                   ^ When defining the `**` operator, name its argument `other`.
              y
            end
        "#});
    }

    // --- OP_LIKE_METHODS (word-named but treated as operators). ---

    #[test]
    fn flags_eql_predicate() {
        // rubocop: col 10 (`x`).
        test::<BinaryOperatorParameterName>().expect_offense(indoc! {r#"
            def eql?(x)
                     ^ When defining the `eql?` operator, name its argument `other`.
              x
            end
        "#});
    }

    #[test]
    fn flags_equal_predicate() {
        // rubocop: col 12 (`y`).
        test::<BinaryOperatorParameterName>().expect_offense(indoc! {r#"
            def equal?(y)
                       ^ When defining the `equal?` operator, name its argument `other`.
              y
            end
        "#});
    }

    // --- accepted: parameter already named other / _other. ---

    #[test]
    fn accepts_other() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def +(other)
              other
            end
        "#});
    }

    #[test]
    fn accepts_underscore_other() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def *(_other)
              1
            end
        "#});
    }

    // --- excluded operators (EXCLUDED set: never flagged). ---

    #[test]
    fn ignores_unary_plus() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def +@
              self
            end
        "#});
    }

    #[test]
    fn ignores_unary_minus() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def -@
              self
            end
        "#});
    }

    #[test]
    fn ignores_element_reference() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def [](index)
              index
            end
        "#});
    }

    #[test]
    fn ignores_element_assignment() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def []=(index, value)
              value
            end
        "#});
    }

    #[test]
    fn ignores_shovel_operator() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def <<(thing)
              thing
            end
        "#});
    }

    #[test]
    fn ignores_case_equality() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def ===(z)
              z
            end
        "#});
    }

    #[test]
    fn ignores_match_operator() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def =~(pattern)
              pattern
            end
        "#});
    }

    #[test]
    fn ignores_backtick() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def `(cmd)
              cmd
            end
        "#});
    }

    // --- non-operator / non-matching shapes. ---

    #[test]
    fn ignores_regular_method() {
        // `coerce` and `call` start with a word char and are not OP_LIKE.
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def coerce(thing)
              thing
            end
        "#});
    }

    #[test]
    fn ignores_call_method() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def call(thing)
              thing
            end
        "#});
    }

    #[test]
    fn ignores_operator_with_two_args() {
        // `(args $(arg ...))` requires exactly one positional arg.
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def +(a, b)
              a + b
            end
        "#});
    }

    #[test]
    fn ignores_operator_with_splat_arg() {
        // A `restarg` is not a single `arg` node.
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def +(*args)
              args
            end
        "#});
    }

    #[test]
    fn ignores_singleton_operator_def() {
        // RuboCop is `on_def`-only; singleton operator defs are never flagged.
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def self.+(amount)
              amount
            end
        "#});
    }

    #[test]
    fn no_offense_for_non_operator_code() {
        test::<BinaryOperatorParameterName>().expect_no_offenses(indoc! {r#"
            def say_hello(name)
              puts name
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(BinaryOperatorParameterName);
