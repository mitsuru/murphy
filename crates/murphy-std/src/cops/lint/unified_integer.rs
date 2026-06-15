//! `Lint/UnifiedInteger` — Checks for using `Fixnum` or `Bignum` constants.
//!
//! `Fixnum` and `Bignum` were unified into `Integer` in Ruby 2.4.
//! References to these constants will raise a `NameError` in modern Ruby.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnifiedInteger
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags bare and `::`-prefixed references to `Fixnum` and `Bignum`.
//!   Scoped references (`MyNamespace::Fixnum`) are correctly ignored.
//!   No autocorrect.
//! ```
//!
//! ## Matched shapes
//!
//! - `1.is_a?(Fixnum)` — bare reference.
//! - `1.is_a?(::Fixnum)` — toplevel constant (collapsed to scope:None in Murphy).
//!
//! ## No autocorrect
//!
//! There is no safe autocorrect.

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, def_node_matcher};

// RuboCop parity: RuboCop's `Lint/UnifiedInteger` matcher is
// `(:const {nil? (:cbase)} ${:Fixnum :Bignum})`. In Murphy's AST a `::`-prefixed
// const collapses to `Const{scope:None}`, so a single `nil?` scope already
// covers both bare and top-level forms — there is no separate `cbase` arm (and
// `cbase` is an unsupported pattern tag anyway). We split into two matchers
// instead of capturing the name symbol (atom-kind capture is not supported),
// which also keeps each offense message a plain constant. Equivalent to the
// prior `cx.is_global_const(node, "Fixnum"/"Bignum")` calls.
def_node_matcher!(is_global_fixnum, "(const nil? :Fixnum)");
def_node_matcher!(is_global_bignum, "(const nil? :Bignum)");

#[derive(Default)]
pub struct UnifiedInteger;

#[cop(
    name = "Lint/UnifiedInteger",
    description = "Checks for using `Fixnum` or `Bignum` constants.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UnifiedInteger {
    #[on_node(kind = "const")]
    fn check_const(&self, node: NodeId, cx: &Cx<'_>) {
        if is_global_fixnum(node, cx) {
            cx.emit_offense(cx.range(node), "Use `Integer` instead of `Fixnum`.", None);
        } else if is_global_bignum(node, cx) {
            cx.emit_offense(cx.range(node), "Use `Integer` instead of `Bignum`.", None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UnifiedInteger;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_fixnum() {
        test::<UnifiedInteger>().expect_offense(indoc! {r#"
            1.is_a?(Fixnum)
                    ^^^^^^ Use `Integer` instead of `Fixnum`.
        "#});
    }

    #[test]
    fn flags_bignum() {
        test::<UnifiedInteger>().expect_offense(indoc! {r#"
            1.is_a?(Bignum)
                    ^^^^^^ Use `Integer` instead of `Bignum`.
        "#});
    }

    #[test]
    fn flags_cbase_fixnum() {
        test::<UnifiedInteger>().expect_offense(indoc! {r#"
            1.is_a?(::Fixnum)
                    ^^^^^^^^ Use `Integer` instead of `Fixnum`.
        "#});
    }

    #[test]
    fn flags_cbase_bignum() {
        test::<UnifiedInteger>().expect_offense(indoc! {r#"
            1.is_a?(::Bignum)
                    ^^^^^^^^ Use `Integer` instead of `Bignum`.
        "#});
    }

    #[test]
    fn accepts_integer() {
        test::<UnifiedInteger>().expect_no_offenses(indoc! {"
            1.is_a?(Integer)
        "});
    }

    #[test]
    fn accepts_cbase_integer() {
        test::<UnifiedInteger>().expect_no_offenses(indoc! {"
            1.is_a?(::Integer)
        "});
    }

    #[test]
    fn accepts_scoped_fixnum() {
        test::<UnifiedInteger>().expect_no_offenses(indoc! {"
            1.is_a?(MyNamespace::Fixnum)
        "});
    }

    #[test]
    fn accepts_scoped_bignum() {
        test::<UnifiedInteger>().expect_no_offenses(indoc! {"
            1.is_a?(MyNamespace::Bignum)
        "});
    }

    // --- Boundary characterization (murphy-vn3o): pin the exact node set
    // `is_global_const(_, "Fixnum"/"Bignum")` matches, so the
    // `(const nil? :{Fixnum,Bignum})` refactor can be proven equivalent.

    #[test]
    fn boundary_fixnum_used_as_namespace_flags_inner_const() {
        // `Fixnum::Foo` — the inner top-level `Fixnum` const is still flagged
        // (scope:None, name Fixnum); the outer `Foo` (scope present) is not.
        test::<UnifiedInteger>().expect_offense(indoc! {r#"
            Fixnum::Foo
            ^^^^^^ Use `Integer` instead of `Fixnum`.
        "#});
    }

    #[test]
    fn boundary_lowercase_and_other_consts_not_flagged() {
        test::<UnifiedInteger>()
            .expect_no_offenses("Fixnums\n")
            .expect_no_offenses("MyFixnum\n");
    }
}

murphy_plugin_api::submit_cop!(UnifiedInteger);
