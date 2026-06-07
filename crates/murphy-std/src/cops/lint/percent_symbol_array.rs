//! `Lint/PercentSymbolArray` — check for colons and commas in `%i(...)` /
//! `%I(...)` arrays.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/PercentSymbolArray
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/PercentSymbolArray cop: within `%i`/`%I`, colons
//!   and commas are literal characters and should be avoided.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, Range, cop};

#[derive(Default)]
pub struct PercentSymbolArray;

const PERCENT_ARRAY_STARTS: &[&str] = &[
    "%i(", "%I(", "%i[", "%I[", "%i{", "%I{", "%i<", "%I<",
];

#[cop(
    name = "Lint/PercentSymbolArray",
    description = "Check for colons and commas in `%i`/`%I` arrays.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl PercentSymbolArray {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        let src = cx.raw_source(cx.range(node));
        if !PERCENT_ARRAY_STARTS.iter().any(|prefix| src.starts_with(prefix)) {
            return;
        }
        let elements = cx.array_elements(node);
        let mut has_offense = false;
        for &child in elements {
            let NodeKind::Sym(s) = *cx.kind(child) else { continue; };
            let val = cx.symbol_str(s);
            if !val.contains(|c: char| c.is_alphanumeric()) { continue; }
            if val.starts_with(':') || val.ends_with(',') {
                has_offense = true;
                let child_range = cx.range(child);
                if val.starts_with(':') {
                    cx.emit_edit(
                        Range { start: child_range.start, end: child_range.start + 1 },
                        "",
                    );
                }
                if val.ends_with(',') {
                    cx.emit_edit(
                        Range { start: child_range.end - 1, end: child_range.end },
                        "",
                    );
                }
            }
        }
        if has_offense {
            cx.emit_offense(
                cx.range(node),
                "Within `%i`/`%I`, ':' and ',' are unnecessary and may be unwanted in the resulting symbols.",
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PercentSymbolArray;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_colons_and_commas_in_percent_i() {
        test::<PercentSymbolArray>().expect_offense(indoc! {r#"
            %i(:foo, :bar, :baz)
            ^^^^^^^^^^^^^^^^^^^^^ Within `%i`/`%I`, ':' and ',' are unnecessary and may be unwanted in the resulting symbols.
        "#});
    }

    #[test]
    fn ignores_clean_percent_i() {
        test::<PercentSymbolArray>().expect_no_offenses("%i(foo bar baz)\n");
    }

    #[test]
    fn ignores_regular_array() {
        test::<PercentSymbolArray>().expect_no_offenses("[:foo, :bar]\n");
    }

    #[test]
    fn handles_percent_i_with_brackets() {
        test::<PercentSymbolArray>().expect_offense(indoc! {r#"
            %i[foo bar, baz]
            ^^^^^^^^^^^^^^^^^ Within `%i`/`%I`, ':' and ',' are unnecessary and may be unwanted in the resulting symbols.
        "#});
    }
}
murphy_plugin_api::submit_cop!(PercentSymbolArray);
