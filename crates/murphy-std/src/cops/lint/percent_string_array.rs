//! `Lint/PercentStringArray` — flags unnecessary quotes and commas in `%w`
//! / `%W` arrays.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/PercentStringArray
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's Lint/PercentStringArray cop: within `%w`/`%W`, quotes
//!   and commas are literal characters and should be avoided.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, NoOptions, Range, cop};

#[derive(Default)]
pub struct PercentStringArray;

#[cop(
    name = "Lint/PercentStringArray",
    description = "Flags unnecessary quotes and commas in `%w`/`%W` arrays.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl PercentStringArray {
    #[on_node(kind = "array")]
    fn check_array(&self, node: NodeId, cx: &Cx<'_>) {
        let src = cx.raw_source(cx.range(node));
        if !src.starts_with("%w") && !src.starts_with("%W") {
            return;
        }
        let elements = cx.array_elements(node);
        let mut has_offense = false;
        for &child in elements {
            let NodeKind::Str(s) = *cx.kind(child) else { continue; };
            let val = cx.string_str(s);
            // False positive guard: skip values with no alphanumeric chars
            // (e.g. `%w(' " # )` where quotes are intentional content).
            if !val.contains(|c: char| c.is_alphanumeric()) { continue; }
            let child_range = cx.range(child);
            // Remove leading quote.
            if val.starts_with('\'') || val.starts_with('"') {
                has_offense = true;
                cx.emit_edit(
                    Range { start: child_range.start, end: child_range.start + 1 },
                    "",
                );
            }
            // Remove trailing quote and/or comma.
            let trailing_len = count_trailing_quote_or_comma(val) as u32;
            if trailing_len > 0 {
                has_offense = true;
                cx.emit_edit(
                    Range { start: child_range.end - trailing_len, end: child_range.end },
                    "",
                );
            }
        }
        if has_offense {
            cx.emit_offense(
                cx.range(node),
                "Within `%w`/`%W`, quotes and ',' are unnecessary and may be unwanted in the resulting strings.",
                None,
            );
        }
    }
}

fn count_trailing_quote_or_comma(val: &str) -> usize {
    let bytes = val.as_bytes();
    if bytes.is_empty() {
        return 0;
    }
    let last = bytes[bytes.len() - 1];
    if last == b',' {
        if bytes.len() > 1 {
            let second_last = bytes[bytes.len() - 2];
            if second_last == b'\'' || second_last == b'"' {
                return 2; // e.g. `foo',`
            }
        }
        return 1; // e.g. `foo,`
    }
    if last == b'\'' || last == b'"' {
        return 1; // e.g. `foo'`
    }
    0
}

#[cfg(test)]
mod tests {
    use super::PercentStringArray;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_quotes_and_commas_in_percent_w() {
        test::<PercentStringArray>().expect_offense(indoc! {r#"
            %w('foo', 'bar', 'baz')
            ^^^^^^^^^^^^^^^^^^^^^^^ Within `%w`/`%W`, quotes and ',' are unnecessary and may be unwanted in the resulting strings.
        "#});
    }

    #[test]
    fn ignores_clean_percent_w() {
        test::<PercentStringArray>().expect_no_offenses("%w(foo bar baz)\n");
    }

    #[test]
    fn ignores_regular_array() {
        test::<PercentStringArray>().expect_no_offenses("['foo', 'bar']\n");
    }

    #[test]
    fn handles_percent_w_with_parens_and_mixed_quotes() {
        test::<PercentStringArray>().expect_offense(indoc! {r#"
            %w('foo' "bar" 'baz')
            ^^^^^^^^^^^^^^^^^^^^^ Within `%w`/`%W`, quotes and ',' are unnecessary and may be unwanted in the resulting strings.
        "#});
    }

    #[test]
    fn flags_single_quoted_token_without_commas() {
        test::<PercentStringArray>().expect_offense(indoc! {r#"
            %w('foo' bar baz)
            ^^^^^^^^^^^^^^^^^ Within `%w`/`%W`, quotes and ',' are unnecessary and may be unwanted in the resulting strings.
        "#});
    }

    #[test]
    fn flags_comma_without_quotes() {
        test::<PercentStringArray>().expect_offense(indoc! {r#"
            %w(foo, bar baz)
            ^^^^^^^^^^^^^^^^ Within `%w`/`%W`, quotes and ',' are unnecessary and may be unwanted in the resulting strings.
        "#});
    }

    #[test]
    fn handles_percent_upper_w() {
        test::<PercentStringArray>().expect_offense(indoc! {r#"
            %W('foo' bar)
            ^^^^^^^^^^^^^ Within `%w`/`%W`, quotes and ',' are unnecessary and may be unwanted in the resulting strings.
        "#});
    }

    #[test]
    fn ignores_symbol_only_false_positive() {
        test::<PercentStringArray>().expect_no_offenses("%w(' \" ! = # ,)\n");
    }

    #[test]
    fn ignores_regular_array_never_triggers() {
        test::<PercentStringArray>().expect_no_offenses("['foo', 'bar']\n");
    }
}
murphy_plugin_api::submit_cop!(PercentStringArray);
