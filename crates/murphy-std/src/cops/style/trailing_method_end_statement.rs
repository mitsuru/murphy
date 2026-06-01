//! `Style/TrailingMethodEndStatement` — flags `end` on the same line as the
//! method body in a multiline method definition.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/TrailingMethodEndStatement
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Endless methods (`def foo = expr`) are skipped (no `end` keyword).
//!   Only multiline defs are checked; single-line `def foo; bar; end` is
//!   accepted (consistent with RuboCop).
//!   Autocorrect: inserts `\n` + def-keyword column indentation before the
//!   trailing `end`.
//! ```
//!
//! ## Offense condition
//!
//! A `def` or `defs` node is flagged when:
//!
//! 1. The method has a body (non-empty).
//! 2. The method is multiline (its full range spans more than one line).
//! 3. The `end` keyword is on the same line as the last part of the body
//!    (no newline between body end and `end` keyword start).
//!
//! ## Autocorrect
//!
//! Insert `\n<indent>` before the `end` keyword, where `<indent>` is the
//! leading whitespace of the line containing `def`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct TrailingMethodEndStatement;

#[cop(
    name = "Style/TrailingMethodEndStatement",
    description = "Place the end statement of a multi-line method on its own line.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl TrailingMethodEndStatement {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_method(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check_method(node, cx);
    }
}

fn check_method(node: NodeId, cx: &Cx<'_>) {
    // Only check multiline definitions.
    if !cx.is_multiline(node) {
        return;
    }

    // Must have a body.
    let Some(body_id) = cx.def_body(node).get() else {
        return;
    };

    // Find the `end` keyword range.
    let end_kw = cx.loc(node).end_keyword();
    if end_kw == Range::ZERO {
        // Endless method (`def foo = expr`) — no `end`.
        return;
    }

    // Check if body end and `end` keyword are on the same line.
    let body_range = cx.range(body_id);
    let between = Range {
        start: body_range.end,
        end: end_kw.start,
    };
    let between_src = cx.raw_source(between);
    if between_src.contains('\n') {
        // `end` is already on its own line.
        return;
    }

    // Offense: the `end` keyword.
    cx.emit_offense(
        end_kw,
        "Place the end statement of a multi-line method on its own line.",
        None,
    );

    // Autocorrect: replace the gap between body end and `end` keyword start
    // with `\n<def_indent>`.
    let def_indent = def_keyword_indent(node, cx);
    let insert_text = format!("\n{}", def_indent);
    cx.emit_edit(between, &insert_text);
}

/// Returns the leading whitespace of the line containing the `def` keyword.
///
/// Only the leading spaces/tabs are returned, not any other content (e.g.
/// `private `) that may precede `def` on the same line.
fn def_keyword_indent<'a>(node: NodeId, cx: &Cx<'a>) -> &'a str {
    let source = cx.source();
    let def_start = cx.range(node).start as usize;
    // Find start of the def's line.
    let line_start = source[..def_start]
        .bytes()
        .rposition(|b| b == b'\n')
        .map_or(0, |p| p + 1);
    // Collect only leading spaces/tabs as the indent.
    let indent_len = source[line_start..def_start]
        .bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .count();
    &source[line_start..line_start + indent_len]
}

#[cfg(test)]
mod tests {
    use super::TrailingMethodEndStatement;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No offense: single-line def ---

    #[test]
    fn no_offense_single_line_def() {
        test::<TrailingMethodEndStatement>().expect_no_offenses("def foo; bar; end\n");
    }

    #[test]
    fn no_offense_single_line_def_no_body() {
        test::<TrailingMethodEndStatement>().expect_no_offenses("def foo; end\n");
    }

    // --- No offense: properly formatted multiline def ---

    #[test]
    fn no_offense_multiline_def_end_on_own_line() {
        test::<TrailingMethodEndStatement>().expect_no_offenses(indoc! {r#"
            def foo
              bar
            end
        "#});
    }

    #[test]
    fn no_offense_multiline_def_empty_body() {
        test::<TrailingMethodEndStatement>().expect_no_offenses(indoc! {r#"
            def foo
            end
        "#});
    }

    // --- Offense: multiline def with end on same line as body ---

    #[test]
    fn flags_end_on_same_line_as_body() {
        test::<TrailingMethodEndStatement>().expect_offense(indoc! {r#"
            def some_method
            do_stuff; end
                      ^^^ Place the end statement of a multi-line method on its own line.
        "#});
    }

    #[test]
    fn flags_end_on_same_line_as_block_body() {
        test::<TrailingMethodEndStatement>().expect_offense(indoc! {r#"
            def do_this(x)
              baz.map { |b| b.this(x) } end
                                        ^^^ Place the end statement of a multi-line method on its own line.
        "#});
    }

    #[test]
    fn flags_end_on_same_line_as_nested_block_end() {
        test::<TrailingMethodEndStatement>().expect_offense(indoc! {r#"
            def foo
              block do
                bar
              end end
                  ^^^ Place the end statement of a multi-line method on its own line.
        "#});
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_trailing_end_to_own_line() {
        test::<TrailingMethodEndStatement>().expect_correction(
            indoc! {r#"
                def some_method
                do_stuff; end
                          ^^^ Place the end statement of a multi-line method on its own line.
            "#},
            indoc! {r#"
                def some_method
                do_stuff
                end
            "#},
        );
    }

    #[test]
    fn corrects_end_with_indentation() {
        test::<TrailingMethodEndStatement>().expect_correction(
            indoc! {r#"
                def do_this(x)
                  baz.map { |b| b.this(x) } end
                                            ^^^ Place the end statement of a multi-line method on its own line.
            "#},
            indoc! {r#"
                def do_this(x)
                  baz.map { |b| b.this(x) }
                end
            "#},
        );
    }

    // --- defs (singleton methods) ---

    #[test]
    fn flags_trailing_end_in_defs() {
        test::<TrailingMethodEndStatement>().expect_offense(indoc! {r#"
            def self.foo
              bar end
                  ^^^ Place the end statement of a multi-line method on its own line.
        "#});
    }

    #[test]
    fn corrects_end_preserves_only_leading_whitespace_not_modifier_prefix() {
        // When `def` is on a line like `  private def bar`, only the two leading
        // spaces should be used as the indent — not `  private ` (which would
        // produce wrong output like `  private end`).
        test::<TrailingMethodEndStatement>().expect_correction(
            indoc! {r#"
                private def bar
                  baz end
                      ^^^ Place the end statement of a multi-line method on its own line.
            "#},
            indoc! {r#"
                private def bar
                  baz
                end
            "#},
        );
    }
}
murphy_plugin_api::submit_cop!(TrailingMethodEndStatement);
