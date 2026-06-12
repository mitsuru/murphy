//! `Layout/SpaceAfterColon` — require a space after a colon in a hash pair or
//! optional keyword argument.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAfterColon
//! upstream_version_checked: master
//! status: complete
//! gap_issues: []
//! notes: >
//!   Port of RuboCop's `on_pair` + `on_kwoptarg`. Colon-style hash pairs
//!   (`a: 1`) and optional keyword params (`def f(k: 1)`) require a trailing
//!   space after the colon. Hash-rocket pairs (`a => 1`) and value-omission
//!   pairs (`{a:}`) are skipped, matching RuboCop's `node.colon?` /
//!   `value_omission?` guards. Required keyword params (`def f(k:)`) lower to
//!   `Kwarg`, which is value-omitted and has no `on_kwarg` handler in RuboCop,
//!   so they are skipped here too. Ternary `a ? b : c` and symbol literals
//!   `:foo` never reach this cop because dispatch is on `Pair`/`Kwoptarg`
//!   nodes, not a colon token scan.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceAfterColon;

#[cop(
    name = "Layout/SpaceAfterColon",
    description = "Use spaces after colons.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SpaceAfterColon {
    #[on_node(kind = "pair")]
    fn check_pair(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Pair { key, value } = *cx.kind(node) else {
            return;
        };
        // Value omission `{a:}` lowers the value to `Unknown`; RuboCop skips it.
        if matches!(cx.kind(value), NodeKind::Unknown) {
            return;
        }

        // A colon-style pair (`a: 1`) has the key range *include* the trailing
        // `:`, so the colon ends at `key.end`. A hash-rocket pair (`a => 1`)
        // keeps the `=>` outside the key range, so its key does not end with
        // `:` — that distinguishes the two without a token scan.
        let key_src = cx.raw_source(cx.range(key));
        if !key_src.ends_with(':') {
            return;
        }
        check_colon(cx, cx.range(key).end, cx.range(value).start);
    }

    #[on_node(kind = "kwoptarg")]
    fn check_kwoptarg(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Kwoptarg { default, .. } = *cx.kind(node) else {
            return;
        };
        // `loc.name` covers the parameter name *and* the trailing colon
        // (`bar:` for `bar: 1`), so the colon ends at `loc.name.end` — exactly
        // like a colon-style hash pair's key range.
        let name = cx.node(node).loc.name;
        if !cx.raw_source(name).ends_with(':') {
            return;
        }
        check_colon(cx, name.end, cx.range(default).start);
    }
}

/// Emit an offense when the colon ending at `colon_end` is not followed by a
/// space before `value_start`. The offense range is the colon itself (1 char).
fn check_colon(cx: &Cx<'_>, colon_end: u32, value_start: u32) {
    if colon_end >= value_start {
        // Colon directly abuts the value: missing space.
        let colon = Range {
            start: colon_end - 1,
            end: colon_end,
        };
        cx.emit_offense(colon, "Space missing after colon.", None);
        cx.emit_edit(
            Range {
                start: colon_end,
                end: colon_end,
            },
            " ",
        );
    }
    // colon_end < value_start: at least one byte (a space) separates them — OK.
}

murphy_plugin_api::submit_cop!(SpaceAfterColon);

#[cfg(test)]
mod tests {
    use super::SpaceAfterColon;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_missing_space_after_colon_in_hash() {
        test::<SpaceAfterColon>().expect_correction(
            indoc! {r#"
                { a:1 }
                   ^ Space missing after colon.
            "#},
            "{ a: 1 }\n",
        );
    }

    #[test]
    fn accepts_space_after_colon_in_hash() {
        test::<SpaceAfterColon>().expect_no_offenses("{ a: 1 }\n");
    }

    #[test]
    fn flags_missing_space_after_colon_in_kwoptarg() {
        test::<SpaceAfterColon>().expect_correction(
            indoc! {r#"
                def foo(bar:1)
                           ^ Space missing after colon.
                end
            "#},
            "def foo(bar: 1)\nend\n",
        );
    }

    #[test]
    fn accepts_space_after_colon_in_kwoptarg() {
        test::<SpaceAfterColon>().expect_no_offenses("def foo(bar: 1)\nend\n");
    }

    #[test]
    fn ignores_hash_rocket_pairs() {
        test::<SpaceAfterColon>().expect_no_offenses("{ :a =>1 }\n");
    }

    #[test]
    fn ignores_value_omission() {
        test::<SpaceAfterColon>().expect_no_offenses("x = 1\n{ x: }\n");
    }

    #[test]
    fn ignores_required_keyword_argument() {
        test::<SpaceAfterColon>().expect_no_offenses("def foo(bar:)\nend\n");
    }

    #[test]
    fn ignores_ternary() {
        test::<SpaceAfterColon>().expect_no_offenses("x = cond ? a:b\n");
    }

    #[test]
    fn flags_multiple_pairs() {
        test::<SpaceAfterColon>().expect_correction(
            indoc! {r#"
                { a:1, b:2 }
                   ^ Space missing after colon.
                        ^ Space missing after colon.
            "#},
            "{ a: 1, b: 2 }\n",
        );
    }

    // ── RuboCop spec parity ───────────────────────────────────────────────────

    /// RuboCop parity: a symbol literal `:a` is never a colon pair.
    #[test]
    fn ignores_symbol_literal() {
        test::<SpaceAfterColon>().expect_no_offenses("x = :a\n");
    }

    /// RuboCop parity: a colon inside a string literal is not flagged.
    #[test]
    fn ignores_colon_in_string() {
        test::<SpaceAfterColon>().expect_no_offenses("x = \"str << ':'\"\n");
    }

    /// RuboCop parity: required keyword args `def f(x:, y:)` are value-omitted.
    #[test]
    fn ignores_required_keyword_args() {
        test::<SpaceAfterColon>().expect_no_offenses("def f(x:, y:)\nend\n");
    }

    /// RuboCop parity: method-call value omission `foo(table:, nodes:)`.
    #[test]
    fn ignores_method_call_value_omission() {
        test::<SpaceAfterColon>().expect_no_offenses("foo(table:, nodes:)\n");
    }

    /// RuboCop parity: mixed optional kwargs — only the spaceless one flags.
    #[test]
    fn flags_only_spaceless_optional_kwarg() {
        test::<SpaceAfterColon>().expect_correction(
            indoc! {r#"
                def m(var:1, other_var: 2)
                         ^ Space missing after colon.
                end
            "#},
            "def m(var: 1, other_var: 2)\nend\n",
        );
    }

    /// RuboCop parity: hash-value omission `{x:, y:}`.
    #[test]
    fn ignores_hash_value_omission() {
        test::<SpaceAfterColon>().expect_no_offenses("x = 1\ny = 2\n{ x:, y: }\n");
    }
}
