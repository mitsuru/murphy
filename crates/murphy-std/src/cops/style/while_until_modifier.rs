//! `Style/WhileUntilModifier` — flags multi-line `while`/`until` that could
//! fit on one line as a modifier form.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/WhileUntilModifier
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Murphy v1 handles the common case: a multi-line while/until with a
//!   single-statement body where the modifier form fits in 120 chars.
//!   Gaps vs RuboCop: heredoc body handling, comment repositioning,
//!   Layout/LineLength config integration.
//!   MaxLineLength is hardcoded at 120 (RuboCop reads it from Layout/LineLength).
//! ```
//!
//! ## Matched shapes
//!
//! Block-form `while`/`until` nodes (not modifier-form, not post-condition)
//! that:
//! - Have a non-nil, single-line, non-`Begin` body
//! - The body is not itself a conditional or loop keyword
//! - The condition is single-line
//! - The node is multi-line
//! - The modifier form candidate fits within 120 chars
//! - The node contains no comments
//!
//! ## Autocorrect
//!
//! Rewrites `while cond\n  body\nend` to `body while cond`
//! (or `body until cond`). Whole-node interpolation because this is a
//! structural rearrangement.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Maximum line length before the modifier form is rejected.
const MAX_LINE_LENGTH: usize = 120;

const MSG: &str = "Favor modifier `%s` usage when having a single-line body.";

/// Stateless unit struct.
#[derive(Default)]
pub struct WhileUntilModifier;

#[cop(
    name = "Style/WhileUntilModifier",
    description = "Favor modifier `while`/`until` for single-line loops.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl WhileUntilModifier {
    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must be a pre-condition block form (not modifier, not post/do-while).
    if cx.is_modifier_form(node) {
        return;
    }
    if cx.is_post_condition_loop(node) {
        return;
    }

    let (keyword, cond, body_opt) = match *cx.kind(node) {
        NodeKind::While { cond, body, .. } => ("while", cond, body),
        NodeKind::Until { cond, body, .. } => ("until", cond, body),
        _ => return,
    };

    let Some(body) = body_opt.get() else {
        // No body — nothing to turn into modifier form.
        return;
    };

    // Body must be single-line.
    if !cx.is_single_line(body) {
        return;
    }

    // Condition must be single-line. A multi-line condition would produce
    // broken Ruby in modifier form.
    if !cx.is_single_line(cond) {
        return;
    }

    // Body must not be a Begin node (multi-statement body).
    if matches!(cx.kind(body), NodeKind::Begin(..)) {
        return;
    }

    // Body must not be a conditional or loop. Nesting modifier forms
    // produces `body while cond2 while cond1` which is a syntax error.
    if cx.is_conditional(body) || cx.is_loop_keyword(body) {
        return;
    }

    // The whole while node must be multi-line.
    if cx.is_single_line(node) {
        return;
    }

    // Skip when the node contains comments. Moving comments when converting
    // to modifier form requires non-trivial repositioning logic not yet
    // implemented in v1.
    if !cx.comments_for_node(node).is_empty() {
        return;
    }

    // Build the modifier-form candidate to check length.
    let cond_src = cx.raw_source(cx.range(cond));
    let body_src = cx.raw_source(cx.range(body));

    // Compute the indentation of the node on its starting line.
    // Use char counts (not byte counts) so multi-byte characters are handled
    // correctly — a line with Unicode content would otherwise appear longer
    // than its visible width.
    let node_range = cx.range(node);
    let source = cx.source();
    let source_bytes = source.as_bytes();
    let start = node_range.start as usize;
    let line_start = source_bytes[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    let indent_str = &source[line_start..start];
    let indent_col = indent_str.chars().count();

    // Candidate: "<indent><body_src> <keyword> <cond_src>"
    let candidate_len =
        indent_col + body_src.chars().count() + 1 + keyword.len() + 1 + cond_src.chars().count();
    if candidate_len > MAX_LINE_LENGTH {
        return;
    }

    // Build message with the keyword substituted.
    let message = MSG.replacen("%s", keyword, 1);

    // Offense range is the keyword token of the while/until.
    let keyword_loc = cx.loc(node).keyword();
    let offense_range = if keyword_loc != Range::ZERO {
        keyword_loc
    } else {
        node_range
    };

    cx.emit_offense(offense_range, &message, None);

    // Autocorrect: whole-node replacement with modifier form.
    let replacement = format!("{body_src} {keyword} {cond_src}");
    cx.emit_edit(node_range, &replacement);
}

#[cfg(test)]
mod tests {
    use super::WhileUntilModifier;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- `while` cases -----

    #[test]
    fn flags_multiline_while() {
        test::<WhileUntilModifier>().expect_correction(
            indoc! {"
                while x < 10
                ^^^^^ Favor modifier `while` usage when having a single-line body.
                  x += 1
                end
            "},
            "x += 1 while x < 10\n",
        );
    }

    #[test]
    fn flags_multiline_while_method_body() {
        test::<WhileUntilModifier>().expect_correction(
            indoc! {"
                while condition
                ^^^^^ Favor modifier `while` usage when having a single-line body.
                  do_something
                end
            "},
            "do_something while condition\n",
        );
    }

    // ----- `until` cases -----

    #[test]
    fn flags_multiline_until() {
        test::<WhileUntilModifier>().expect_correction(
            indoc! {"
                until x > 10
                ^^^^^ Favor modifier `until` usage when having a single-line body.
                  x += 1
                end
            "},
            "x += 1 until x > 10\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_modifier_while_already() {
        test::<WhileUntilModifier>().expect_no_offenses("x += 1 while x < 10\n");
    }

    #[test]
    fn accepts_modifier_until_already() {
        test::<WhileUntilModifier>().expect_no_offenses("x += 1 until x > 10\n");
    }

    #[test]
    fn accepts_multiline_body() {
        test::<WhileUntilModifier>().expect_no_offenses(indoc! {"
            while condition
              do_something
              do_other
            end
        "});
    }

    #[test]
    fn accepts_single_line_while() {
        test::<WhileUntilModifier>().expect_no_offenses("while x < 10; x += 1; end\n");
    }

    #[test]
    fn accepts_line_too_long() {
        let long_body = "a".repeat(60);
        let long_cond = "b".repeat(60);
        let src = format!("while {long_cond}\n  {long_body}\nend\n");
        test::<WhileUntilModifier>().expect_no_offenses(&src);
    }

    #[test]
    fn accepts_body_that_is_conditional() {
        // `do_something if cond while cond2` is a syntax error.
        test::<WhileUntilModifier>().expect_no_offenses(indoc! {"
            while condition
              do_something if other_condition
            end
        "});
    }

    #[test]
    fn accepts_body_that_is_while_loop() {
        test::<WhileUntilModifier>().expect_no_offenses(indoc! {"
            while condition
              i += 1 while i < 10
            end
        "});
    }

    #[test]
    fn accepts_multiline_condition() {
        test::<WhileUntilModifier>().expect_no_offenses(indoc! {"
            while foo(
              bar
            )
              baz
            end
        "});
    }

    #[test]
    fn accepts_while_with_comment() {
        test::<WhileUntilModifier>().expect_no_offenses(indoc! {"
            while condition
              # comment
              do_something
            end
        "});
    }

    #[test]
    fn accepts_do_while_post_condition() {
        test::<WhileUntilModifier>().expect_no_offenses(indoc! {"
            begin
              x += 1
            end while x < 10
        "});
    }
}
murphy_plugin_api::submit_cop!(WhileUntilModifier);
