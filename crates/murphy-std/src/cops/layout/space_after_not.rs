//! `Layout/SpaceAfterNot` — flag a redundant space after the prefix `!`
//! operator.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAfterNot
//! upstream_version_checked: master
//! status: verified
//! gap_issues: []
//! notes: >
//!   Port of RuboCop's `on_send` for `prefix_bang?`. Fires on `! foo`
//!   (whitespace between the `!` operator and its argument) and removes the
//!   gap. The operator must be a literal prefix `!`: `not foo` (operator
//!   `not`), `foo.!` (postfix selector), and `x != y` (method `!=`) all lower
//!   to or near a `!` send but are excluded by checking that the selector
//!   source is exactly `!` and sits before the receiver in source order.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceAfterNot;

#[cop(
    name = "Layout/SpaceAfterNot",
    description = "Tracks redundant space after the `!` operator.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SpaceAfterNot {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Must be the unary `!` method with a receiver and no arguments.
        if cx.method_name(node) != Some("!") {
            return;
        }
        let Some(receiver) = cx.call_receiver(node).get() else {
            return;
        };
        if !cx.call_arguments(node).is_empty() {
            return;
        }

        // `prefix_bang?`: the selector must be a literal `!` that appears before
        // the receiver in source order. This excludes `foo.!` (selector after
        // the receiver) and `not foo` (selector source is `not`, not `!`).
        let selector = cx.node(node).loc.name;
        if selector.start >= cx.range(receiver).start {
            return;
        }
        if cx.raw_source(selector) != "!" {
            return;
        }

        // `whitespace_after_operator?`: a gap exists between the end of `!` and
        // the start of the receiver.
        let gap = Range {
            start: selector.end,
            end: cx.range(receiver).start,
        };
        if gap.start >= gap.end {
            return;
        }

        cx.emit_offense(
            cx.range(node),
            "Do not leave space between `!` and its argument.",
            None,
        );
        cx.emit_edit(gap, "");
    }
}

murphy_plugin_api::submit_cop!(SpaceAfterNot);

#[cfg(test)]
mod tests {
    use super::SpaceAfterNot;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_space_after_bang() {
        test::<SpaceAfterNot>().expect_correction(
            indoc! {r#"
                ! foo
                ^^^^^ Do not leave space between `!` and its argument.
            "#},
            "!foo\n",
        );
    }

    #[test]
    fn accepts_bang_without_space() {
        test::<SpaceAfterNot>().expect_no_offenses("!foo\n");
    }

    #[test]
    fn flags_space_after_bang_with_method_call() {
        test::<SpaceAfterNot>().expect_correction(
            indoc! {r#"
                ! foo.bar
                ^^^^^^^^^ Do not leave space between `!` and its argument.
            "#},
            "!foo.bar\n",
        );
    }

    #[test]
    fn ignores_not_equal_operator() {
        test::<SpaceAfterNot>().expect_no_offenses("x != y\n");
    }

    #[test]
    fn ignores_not_keyword() {
        // `not foo` uses the `not` keyword, not the `!` operator.
        test::<SpaceAfterNot>().expect_no_offenses("not foo\n");
    }

    #[test]
    fn ignores_postfix_bang_method() {
        test::<SpaceAfterNot>().expect_no_offenses("foo.!\n");
    }

    #[test]
    fn flags_nested_bang_with_spaces() {
        // Both `!` operators have a trailing space; each is its own offense.
        test::<SpaceAfterNot>().expect_correction(
            indoc! {r#"
                ! ! foo
                ^^^^^^^ Do not leave space between `!` and its argument.
                  ^^^^^ Do not leave space between `!` and its argument.
            "#},
            "!!foo\n",
        );
    }

    #[test]
    fn accepts_nested_bang_without_spaces() {
        test::<SpaceAfterNot>().expect_no_offenses("!!foo\n");
    }
}
