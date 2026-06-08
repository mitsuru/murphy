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

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

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
        let NodeKind::Const { scope, name } = *cx.kind(node) else {
            return;
        };
        if !scope.is_none() {
            return;
        }
        let name_str = cx.symbol_str(name);
        if name_str != "Fixnum" && name_str != "Bignum" {
            return;
        }
        let msg = format!("Use `Integer` instead of `{name_str}`.");
        cx.emit_offense(cx.range(node), &msg, None);
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
}

murphy_plugin_api::submit_cop!(UnifiedInteger);
