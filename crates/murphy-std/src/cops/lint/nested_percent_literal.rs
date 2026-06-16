//! `Lint/NestedPercentLiteral` — flag nested percent literals within percent
//! literals.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/NestedPercentLiteral
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-e9v9
//! notes: >
//!   Handles `%i`/`%I`/`%w`/`%W` (which map to Array nodes in Murphy).
//!   Non-array percent literal types (`%q`, `%Q`, `%x`, `%r`, `%s`) are not
//!   yet handled because they translate to different NodeKind variants
//!   (Str, Dstr, Xstr, Regexp, Sym) which need additional `on_node` handlers.
//! ```

use murphy_plugin_api::{Cx, NodeId, NoOptions, cop};

#[derive(Default)]
pub struct NestedPercentLiteral;

const PERCENT_PREFIXES: &[&str] = &[
    "%i", "%I", "%w", "%W", "%q", "%Q", "%x", "%r", "%s",
];

#[cop(
    name = "Lint/NestedPercentLiteral",
    description = "Flag nested percent literals.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl NestedPercentLiteral {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        let src = cx.raw_source(cx.range(node));
        if !PERCENT_PREFIXES.iter().any(|p| src.starts_with(p)) {
            return;
        }
        let elements = cx.array_elements(node);
        for &child in elements {
            let child_src = cx.raw_source(cx.range(child));
            if child_src.len() >= 3
                && child_src.chars().nth(2).is_some_and(|c| !c.is_alphanumeric())
                && PERCENT_PREFIXES.iter().any(|p| child_src.starts_with(p)) {
                cx.emit_offense(
                    cx.range(node),
                    "Within percent literals, nested percent literals do not function and may be unwanted in the result.",
                    None,
                );
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::NestedPercentLiteral;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_nested_percent_i_within_percent_i() {
        test::<NestedPercentLiteral>().expect_offense(indoc! {r#"
            %i[%i[a b]]
            ^^^^^^^^^^^ Within percent literals, nested percent literals do not function and may be unwanted in the result.
        "#});
    }

    #[test]
    fn ignores_flat_percent_i() {
        test::<NestedPercentLiteral>().expect_no_offenses("%i[a b]\n");
    }

    #[test]
    fn ignores_regular_array() {
        test::<NestedPercentLiteral>().expect_no_offenses("[:foo, :bar]\n");
    }

    #[test]
    fn flags_nested_percent_w() {
        test::<NestedPercentLiteral>().expect_offense(indoc! {r#"
            %w[%w[a b]]
            ^^^^^^^^^^^ Within percent literals, nested percent literals do not function and may be unwanted in the result.
        "#});
    }

    #[test]
    fn ignores_flat_percent_w() {
        test::<NestedPercentLiteral>().expect_no_offenses("%w[a b]\n");
    }
}
murphy_plugin_api::submit_cop!(NestedPercentLiteral);
