//! `Layout/FirstMethodParameterLineBreak` — requires a line break before the
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/FirstMethodParameterLineBreak
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Ports `on_def`/`on_defs` + the shared `FirstElementLineBreak` mixin's
//!   `check_method_line_break`. Murphy folds `def self.foo` into
//!   `NodeKind::Def`, so a single `def` handler covers both. Fires when a
//!   parenthesised method-parameter list spans multiple lines but the first
//!   parameter shares the `def`'s opening line. Paren-less defs are skipped
//!   (RuboCop's `method_uses_parens?`). `AllowMultilineFinalElement` is
//!   honoured. Autocorrect inserts a newline before the first parameter.
//! ```
//!
//! first parameter of a multi-line method-parameter definition. Mirrors
//! RuboCop's same-named cop.

use crate::cops::util::check_children_line_break;
use murphy_plugin_api::{CopOptions, Cx, NodeId, cop};

const MSG: &str = "Add a line break before the first parameter of a multi-line method parameter list.";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct FirstMethodParameterLineBreak;

/// Options for [`FirstMethodParameterLineBreak`]. Matches RuboCop's key.
#[derive(CopOptions)]
pub struct FirstMethodParameterLineBreakOptions {
    #[option(
        name = "AllowMultilineFinalElement",
        default = false,
        description = "Allow the final parameter to span multiple lines without a leading line break."
    )]
    pub allow_multiline_final_element: bool,
}

#[cop(
    name = "Layout/FirstMethodParameterLineBreak",
    description = "Checks for a line break before the first parameter in a multi-line method parameter definition.",
    default_severity = "warning",
    // RuboCop ships this cop `Enabled: false` (opt-in). The `default.yml`
    // layer also disables it; this fallback keeps every config path faithful.
    default_enabled = false,
    options = FirstMethodParameterLineBreakOptions,
)]
impl FirstMethodParameterLineBreak {
    // Murphy folds `def self.foo` into `NodeKind::Def` (with a `self`
    // receiver), so one `def` handler covers RuboCop's `on_def` + `on_defs`.
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<FirstMethodParameterLineBreakOptions>();
        check_method_line_break(node, cx, opts.allow_multiline_final_element);
    }
}

/// Port of `FirstElementLineBreak#check_method_line_break`: only proceed
/// when the parameter list is parenthesised (RuboCop's `method_uses_parens?`).
fn check_method_line_break(node: NodeId, cx: &Cx<'_>, ignore_last: bool) {
    let Some(args) = cx.def_arguments(node).get() else {
        return;
    };
    let params = cx.children(args);
    let Some(&first) = params.first() else {
        return;
    };

    if !method_uses_parens(node, first, cx) {
        return;
    }

    check_children_line_break(cx, cx.range(node).start, params.as_slice(), ignore_last, MSG);
}

/// RuboCop's `method_uses_parens?`: the `def`'s source line, sliced up to the
/// first parameter, ends with `(` (allowing trailing whitespace). Byte-based
/// to stay multi-byte safe; the parameter's column on its own line is
/// irrelevant because the `line != first.first_line` guard already handles a
/// first parameter that begins on a later line.
fn method_uses_parens(node: NodeId, first_param: NodeId, cx: &Cx<'_>) -> bool {
    let limit = cx.range(first_param).start as usize;
    let src = cx.source().as_bytes();
    if limit > src.len() {
        return false;
    }
    let node_start = cx.range(node).start as usize;
    let line_start = src[..limit]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(node_start.min(limit), |i| i + 1);
    let prefix = &src[line_start..limit];
    // Equivalent to /\s*\(\s*$/ — trailing whitespace, then `(`.
    matches!(
        prefix.iter().rev().find(|&&b| b != b' ' && b != b'\t'),
        Some(&b'(')
    )
}

#[cfg(test)]
mod tests {
    use super::{FirstMethodParameterLineBreak, FirstMethodParameterLineBreakOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_first_parameter_on_def_line() {
        test::<FirstMethodParameterLineBreak>().expect_offense(indoc! {r#"
            def foo(bar,
                    ^^^ Add a line break before the first parameter of a multi-line method parameter list.
                baz)
            end
        "#});
    }

    #[test]
    fn corrects_first_parameter_on_def_line() {
        test::<FirstMethodParameterLineBreak>().expect_correction(
            indoc! {r#"
                def foo(bar,
                        ^^^ Add a line break before the first parameter of a multi-line method parameter list.
                    baz)
                end
            "#},
            "def foo(\nbar,\n    baz)\nend\n",
        );
    }

    #[test]
    fn accepts_first_parameter_on_own_line() {
        test::<FirstMethodParameterLineBreak>().expect_no_offenses(indoc! {r#"
            def foo(
              bar,
              baz)
            end
        "#});
    }

    #[test]
    fn accepts_single_line_parameters() {
        test::<FirstMethodParameterLineBreak>().expect_no_offenses(indoc! {r#"
            def foo(bar, baz)
            end
        "#});
    }

    #[test]
    fn ignores_parenless_method() {
        // `def foo bar,\n baz` has no parentheses — RuboCop's
        // `method_uses_parens?` returns false, so no offense.
        test::<FirstMethodParameterLineBreak>().expect_no_offenses(indoc! {r#"
            def foo bar,
              baz
            end
        "#});
    }

    #[test]
    fn flags_singleton_method() {
        // `def self.foo` folds into NodeKind::Def in Murphy.
        test::<FirstMethodParameterLineBreak>().expect_offense(indoc! {r#"
            def self.foo(bar,
                         ^^^ Add a line break before the first parameter of a multi-line method parameter list.
                baz)
            end
        "#});
    }

    #[test]
    fn accepts_no_parameters() {
        test::<FirstMethodParameterLineBreak>().expect_no_offenses(indoc! {r#"
            def foo
            end
        "#});
    }

    #[test]
    fn accepts_multiline_final_parameter_when_allowed() {
        test::<FirstMethodParameterLineBreak>()
            .with_options(&FirstMethodParameterLineBreakOptions {
                allow_multiline_final_element: true,
            })
            .expect_no_offenses(indoc! {r#"
                def foo(bar, baz = {
                  a: 1
                })
                end
            "#});
    }

    #[test]
    fn flags_multiline_final_parameter_by_default() {
        test::<FirstMethodParameterLineBreak>().expect_offense(indoc! {r#"
            def foo(bar, baz = {
                    ^^^ Add a line break before the first parameter of a multi-line method parameter list.
              a: 1
            })
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(FirstMethodParameterLineBreak);
