//! `Style/IfUnlessModifier` — flags multi-line `if`/`unless` that could fit
//! on one line as a modifier form.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/IfUnlessModifier
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-y3h2
//! notes: >
//!   Murphy v1 handles the common case: a multi-line if/unless with no else
//!   branch, a single-statement body, where the modifier form fits in 120 chars.
//!   Gaps vs RuboCop: heredoc body handling, comment repositioning, Layout/LineLength
//!   and Layout/IndentationStyle config integration, endless methods, string
//!   interpolation scope detection, and pattern-matching / defined? guards.
//!   MaxLineLength is hardcoded at 120 (RuboCop reads it from Layout/LineLength).
//! ```
//!
//! ## Matched shapes
//!
//! Block-form `if`/`unless` nodes that:
//! - Are not modifier-form already (`is_modifier_form?`)
//! - Are not ternary
//! - Are not `elsif`
//! - Have no `else` branch (single-branch conditional)
//! - Have a non-nil, single-line, non-`Begin` body
//! - Would produce a modifier-form line that fits within 120 chars
//!
//! ## Unless AST shape
//!
//! Murphy represents `unless x; y; end` as `If { then_: None, else_: Some(y) }`.
//! The body for `unless` is in the else branch; for `if` it is in the then branch.
//!
//! ## Autocorrect
//!
//! Rewrites `if cond\n  body\nend` → `body if cond` (or `body unless cond`).
//! This is a structural rearrangement (whole-node interpolation), not a surgical edit.
//! The offense and autocorrect range cover the entire `if`/`unless` expression.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Maximum line length before the modifier form is rejected.
const MAX_LINE_LENGTH: usize = 120;

const MSG: &str = "Favor modifier `%s` usage when having a single-line body. Another good alternative is the usage of control flow `&&`/`||`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct IfUnlessModifier;

#[cop(
    name = "Style/IfUnlessModifier",
    description = "Favor modifier `if`/`unless` for single-line conditionals.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl IfUnlessModifier {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must be a block-form if/unless (not modifier, not ternary).
    if cx.is_modifier_form(node) {
        return;
    }
    if cx.is_ternary(node) {
        return;
    }
    // Skip elsif chains.
    if cx.is_elsif(node) {
        return;
    }
    // Must not have an else branch.
    if cx.is_else(node) {
        return;
    }

    let keyword = cx.if_keyword(node);
    if keyword.is_empty() {
        return;
    }

    // Determine the body node based on keyword.
    // - `if cond; body; end`   → then_ = Some(body), else_ = None
    // - `unless cond; body; end` → then_ = None, else_ = Some(body)
    let body_opt = if keyword == "unless" {
        cx.if_else_branch(node)
    } else {
        cx.if_then_branch(node)
    };

    let Some(body) = body_opt.get() else {
        // No body — nothing to turn into modifier form.
        return;
    };

    // Body must be single-line.
    if !cx.is_single_line(body) {
        return;
    }

    // Condition must also be single-line. A multi-line condition (e.g. a
    // method call spanning multiple lines) would produce invalid/broken Ruby
    // in modifier form: `baz if foo(\n  bar\n)`.
    let NodeKind::If { cond, .. } = *cx.kind(node) else {
        return;
    };
    if !cx.is_single_line(cond) {
        return;
    }

    // Body must not be a Begin node (multi-statement body).
    if matches!(cx.kind(body), NodeKind::Begin(..)) {
        return;
    }

    // Body must not be a conditional or loop. Nesting modifier forms produces
    // `do_something if cond2 if cond1` which is a syntax error in Ruby.
    if cx.is_conditional(body) || cx.is_loop_keyword(body) {
        return;
    }

    // The whole if node must be multi-line (otherwise it already fits on one line
    // in block form and we don't need to suggest modifier form).
    if cx.is_single_line(node) {
        return;
    }

    // Skip when the node contains comments. Moving comments when converting
    // to modifier form requires non-trivial repositioning logic that is not
    // yet implemented in v1. Skipping is safer than silently dropping comments
    // during autocorrect.
    if !cx.comments_for_node(node).is_empty() {
        return;
    }

    // RuboCop's StatementModifier exempts a condition that assigns a local
    // variable anywhere in its subtree (`condition.each_node.any?(&:lvasgn_type?)`).
    // The condition is often parenthesised (`if (batch = next_batch)`), so the
    // lvasgn lives below a `Begin` wrapper — a descendant walk (and a self-check)
    // is required.
    if crate::cops::util::condition_contains_lvasgn(cond, cx) {
        return;
    }

    // RuboCop's StatementModifier exempts nodes spanning more than 3 nonempty
    // physical lines (`nonempty_line_count > 3`). A single-line body that pulls
    // in a multi-line heredoc, for example, makes the whole `if` too tall to be
    // a sensible modifier even though the body node is "single-line".
    if nonempty_line_count(node, cx) > 3 {
        return;
    }

    // Build the modifier-form candidate to check length.
    // `cond` was already extracted earlier in this function.
    let cond_src = cx.raw_source(cx.range(cond));
    let body_src = cx.raw_source(cx.range(body));

    // Compute the indentation of the node on its starting line.
    let node_range = cx.range(node);
    let source = cx.source();
    let source_bytes = source.as_bytes();
    let start = node_range.start as usize;
    let line_start = source_bytes[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    let indent_col = start - line_start;

    // Candidate: "<indent><body_src> <keyword> <cond_src>"
    let candidate_len = indent_col + body_src.len() + 1 + keyword.len() + 1 + cond_src.len();
    if candidate_len > MAX_LINE_LENGTH {
        return;
    }

    // Build message with the keyword substituted.
    let message = MSG.replacen("%s", keyword, 1);

    // Offense range is the keyword token of the if/unless.
    let keyword_loc = cx.if_keyword_loc(node);
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

/// Count physical lines in `node`'s source that are not blank (whitespace-only).
///
/// Mirrors RuboCop's `nonempty_line_count` — `source.lines.grep_v(/\A\s*\z/).size`.
fn nonempty_line_count(node: NodeId, cx: &Cx<'_>) -> usize {
    cx.raw_source(cx.range(node))
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

#[cfg(test)]
mod tests {
    use super::IfUnlessModifier;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- `if` cases -----

    #[test]
    fn flags_multiline_if() {
        test::<IfUnlessModifier>().expect_correction(
            indoc! {"
                if condition
                ^^ Favor modifier `if` usage when having a single-line body. Another good alternative is the usage of control flow `&&`/`||`.
                  do_something
                end
            "},
            "do_something if condition\n",
        );
    }

    #[test]
    fn flags_multiline_if_with_method_call() {
        test::<IfUnlessModifier>().expect_correction(
            indoc! {"
                if x > 0
                ^^ Favor modifier `if` usage when having a single-line body. Another good alternative is the usage of control flow `&&`/`||`.
                  foo
                end
            "},
            "foo if x > 0\n",
        );
    }

    // ----- `unless` cases -----

    #[test]
    fn flags_multiline_unless() {
        test::<IfUnlessModifier>().expect_correction(
            indoc! {"
                unless condition
                ^^^^^^ Favor modifier `unless` usage when having a single-line body. Another good alternative is the usage of control flow `&&`/`||`.
                  do_something
                end
            "},
            "do_something unless condition\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_modifier_if_already() {
        test::<IfUnlessModifier>().expect_no_offenses("do_something if condition\n");
    }

    #[test]
    fn accepts_modifier_unless_already() {
        test::<IfUnlessModifier>().expect_no_offenses("do_something unless condition\n");
    }

    #[test]
    fn accepts_if_with_else() {
        test::<IfUnlessModifier>().expect_no_offenses(indoc! {"
            if condition
              do_something
            else
              do_other
            end
        "});
    }

    #[test]
    fn accepts_multiline_body() {
        test::<IfUnlessModifier>().expect_no_offenses(indoc! {"
            if condition
              do_something
              do_other
            end
        "});
    }

    #[test]
    fn accepts_ternary() {
        test::<IfUnlessModifier>().expect_no_offenses("x ? y : z\n");
    }

    #[test]
    fn accepts_elsif() {
        test::<IfUnlessModifier>().expect_no_offenses(indoc! {"
            if x
              a
            elsif y
              b
            end
        "});
    }

    #[test]
    fn accepts_empty_body() {
        // if with no body (unusual but valid in Murphy's AST — then_ = None for `if`)
        test::<IfUnlessModifier>().expect_no_offenses(indoc! {"
            if condition
            end
        "});
    }

    #[test]
    fn accepts_line_too_long() {
        // Body + keyword + condition > 120 chars: not flagged.
        let long_body = "a".repeat(60);
        let long_cond = "b".repeat(60);
        let src = format!("if {long_cond}\n  {long_body}\nend\n");
        test::<IfUnlessModifier>().expect_no_offenses(&src);
    }

    #[test]
    fn accepts_body_that_is_modifier_if() {
        // `do_something if cond2 if cond1` is a syntax error in Ruby.
        test::<IfUnlessModifier>().expect_no_offenses(indoc! {"
            if condition
              do_something if other_condition
            end
        "});
    }

    #[test]
    fn accepts_body_that_is_while_loop() {
        // Nested modifier loop is a syntax error.
        test::<IfUnlessModifier>().expect_no_offenses(indoc! {"
            if condition
              i += 1 while i < 10
            end
        "});
    }

    #[test]
    fn accepts_multiline_condition() {
        // A multi-line condition would produce broken modifier form.
        test::<IfUnlessModifier>().expect_no_offenses(indoc! {"
            if foo(
              bar
            )
              baz
            end
        "});
    }

    #[test]
    fn accepts_if_with_comment_in_body() {
        // Comments inside the if block must not be dropped during autocorrect.
        // v1 conservatively skips any if/unless that contains a comment.
        test::<IfUnlessModifier>().expect_no_offenses(indoc! {"
            if condition
              # required side-effect explanation
              do_something
            end
        "});
    }

    #[test]
    fn accepts_unless_with_comment() {
        test::<IfUnlessModifier>().expect_no_offenses(indoc! {"
            unless condition
              # comment
              do_something
            end
        "});
    }

    #[test]
    fn accepts_lvasgn_in_parenthesized_condition() {
        // RuboCop's StatementModifier exempts conditions that assign a local
        // variable: `if (batch = ...)` reads worse as a modifier and is a
        // common intentional idiom.
        test::<IfUnlessModifier>().expect_no_offenses(indoc! {"
            if (batch = Thread.current[:batch])
              batch.add_jobs([msg])
            end
        "});
    }

    #[test]
    fn accepts_node_spanning_more_than_three_nonempty_lines() {
        // A heredoc-bearing body makes the whole `if` node span >3 nonempty
        // physical lines, so RuboCop's `nonempty_line_count > 3` exempts it.
        test::<IfUnlessModifier>().expect_no_offenses(indoc! {"
            if ENV.key?('WHITELIST_MODE')
              warn(<<~MESSAGE.squish)
                one
                two
              MESSAGE
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(IfUnlessModifier);
