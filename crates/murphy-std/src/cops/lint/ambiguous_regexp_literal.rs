//! `Lint/AmbiguousRegexpLiteral` — flags ambiguous regexp literals in the
//! first argument of a method call without parentheses.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/AmbiguousRegexpLiteral
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   RuboCop drives this cop off the parser's `:ambiguous_regexp` diagnostic
//!   and then walks up the AST (`find_offense_node`) to locate the command
//!   call to parenthesize. Murphy exposes no parser-diagnostics surface, so
//!   the trigger is AST/loc based: a regexp literal whose enclosing
//!   `find_offense_node` walk lands on an unparenthesized command send. This
//!   is equivalent because prism only produces a regexp argument node for
//!   exactly the source the parser would flag — a local variable followed by
//!   `/.../ ` parses as division, so no regexp node appears (mirroring RuboCop
//!   emitting no diagnostic). The `find_offense_node` walk-up (including the
//!   `method_chain_to_regexp_receiver?` recursion) is ported so chains like
//!   `do_something /re/.foo bar` are handled. Autocorrect adds parentheses
//!   around the call's arguments (`add_parentheses`). The offense is reported
//!   at the regexp's opening `/` (a single column), matching the parser
//!   diagnostic's location. All RuboCop spec shapes are covered; `yield`/`super`
//!   command arguments (not Send nodes, and absent from RuboCop's spec) are not
//!   flagged.
//! ```
//!
//! ## Matched shapes
//! - `do_something /pattern/` — regexp is the unparenthesized first argument
//! - `obj.scan /pattern/` — same, through a method chain
//! - `do_something /pattern/.foo bar` — regexp begins a chained first argument
//!
//! ## Accepted shapes (no offense)
//! - `do_something(/pattern/)` — parenthesized call
//! - `x = /pattern/` — assignment, regexp is not a call argument
//! - `foo / pattern / 2` — actual division (no regexp node)

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Ambiguous regexp literal. Parenthesize the method arguments \
if it's surely a regexp literal, or add a whitespace to the right of the `/` \
if it should be a division.";

#[derive(Default)]
pub struct AmbiguousRegexpLiteral;

#[cop(
    name = "Lint/AmbiguousRegexpLiteral",
    description = "Checks for ambiguous regexp literals in the first argument of a method invocation without parentheses.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl AmbiguousRegexpLiteral {
    #[on_node(kind = "regexp")]
    fn check_regexp(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        // RuboCop: `find_offense_node(node.parent, node)`.
        let offense_node = find_offense_node(parent, node, cx);

        // The diagnostic only fires for an unparenthesized command send whose
        // first argument begins with the regexp. Guard against parenthesized
        // calls (`do_something(/re/)`) and non-call landing nodes (assignment).
        if !matches!(*cx.kind(offense_node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
            return;
        }
        if cx.is_parenthesized(offense_node) {
            return;
        }
        let Some(first_arg) = cx.first_argument(offense_node).get() else {
            return;
        };
        // The regexp must be at the very start of the first argument (either it
        // *is* the first arg, or it is the leftmost receiver of a chain that
        // forms the first arg).
        if cx.range(first_arg).start != cx.range(node).start {
            return;
        }

        // RuboCop reports at the parser diagnostic's location: the opening
        // `/` delimiter of the regexp (a single column), not the whole literal.
        let start = cx.range(node).start;
        let offense_range = Range {
            start,
            end: start + 1,
        };
        cx.emit_offense(offense_range, MSG, None);
        add_parentheses(offense_node, cx);
    }
}

/// Ports RuboCop's `find_offense_node(node, regexp_receiver)`: walk up the
/// chain from the regexp's parent until reaching the command call whose first
/// argument begins with the regexp.
fn find_offense_node(mut node: NodeId, regexp_receiver: NodeId, cx: &Cx<'_>) -> NodeId {
    loop {
        if first_argument_is_regexp(node, cx) {
            return node;
        }
        let Some(parent) = cx.parent(node).get() else {
            return node;
        };
        // `(node.parent.send_type? && node.receiver) ||
        //  method_chain_to_regexp_receiver?(node, regexp_receiver)`.
        let parent_is_send =
            matches!(*cx.kind(parent), NodeKind::Send { .. } | NodeKind::Csend { .. });
        let node_has_receiver = cx.call_receiver(node).get().is_some();
        if (parent_is_send && node_has_receiver)
            || method_chain_to_regexp_receiver(node, regexp_receiver, cx)
        {
            node = parent;
            continue;
        }
        return node;
    }
}

/// `first_argument_is_regexp?` — node is a send whose first argument is a
/// regexp literal.
fn first_argument_is_regexp(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return false;
    }
    cx.first_argument(node)
        .get()
        .is_some_and(|arg| matches!(*cx.kind(arg), NodeKind::Regexp { .. }))
}

/// `method_chain_to_regexp_receiver?(node, regexp_receiver)`:
/// ```ruby
/// parent = node.parent or return false
/// parent_receiver = parent.receiver or return false
/// parent.parent && parent_receiver.receiver == regexp_receiver
/// ```
fn method_chain_to_regexp_receiver(node: NodeId, regexp_receiver: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    let Some(parent_receiver) = cx.call_receiver(parent).get() else {
        return false;
    };
    cx.parent(parent).get().is_some()
        && cx.call_receiver(parent_receiver).get() == Some(regexp_receiver)
}

/// RuboCop's `add_parentheses` for a send with arguments: remove the single
/// char after the selector, insert `(` there, and insert `)` after the last
/// argument.
fn add_parentheses(send: NodeId, cx: &Cx<'_>) {
    let args = cx.call_arguments(send);
    let Some(&last_arg) = args.last() else {
        return;
    };
    let Some(&first_arg) = args.first() else {
        return;
    };
    // `args_begin = selector.end.resize(1)` — the char right after the
    // selector (the space before the first argument). Replace it with `(`.
    let selector_end = cx.selector(send).end;
    let first_arg_start = cx.range(first_arg).start;
    cx.emit_edit(
        Range {
            start: selector_end,
            end: first_arg_start,
        },
        "(",
    );
    // Insert `)` after the last argument.
    let last_arg_end = cx.range(last_arg).end;
    cx.emit_edit(
        Range {
            start: last_arg_end,
            end: last_arg_end,
        },
        ")",
    );
}

murphy_plugin_api::submit_cop!(AmbiguousRegexpLiteral);

#[cfg(test)]
mod tests {
    use super::AmbiguousRegexpLiteral;
    use murphy_plugin_api::test_support::{indoc, test};

    // RuboCop reports a single-column offense at the opening `/` of the regexp.

    #[test]
    fn flags_single_argument() {
        test::<AmbiguousRegexpLiteral>()
            .expect_offense(indoc! {r#"
                p /pattern/
                  ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
            "#})
            .expect_correction(
                indoc! {r#"
                    p /pattern/
                      ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
                "#},
                "p(/pattern/)\n",
            );
    }

    #[test]
    fn flags_multiple_arguments() {
        test::<AmbiguousRegexpLiteral>()
            .expect_offense(indoc! {r#"
                p /pattern/, foo
                  ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
            "#})
            .expect_correction(
                indoc! {r#"
                    p /pattern/, foo
                      ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
                "#},
                "p(/pattern/, foo)\n",
            );
    }

    #[test]
    fn flags_method_sent_to_regexp() {
        test::<AmbiguousRegexpLiteral>()
            .expect_offense(indoc! {r#"
                p /pattern/.do_something
                  ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
            "#})
            .expect_correction(
                indoc! {r#"
                    p /pattern/.do_something
                      ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
                "#},
                "p(/pattern/.do_something)\n",
            );
    }

    #[test]
    fn flags_method_chain_sent_to_regexp() {
        test::<AmbiguousRegexpLiteral>()
            .expect_offense(indoc! {r#"
                p /pattern/.do_something.do_something
                  ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
            "#})
            .expect_correction(
                indoc! {r#"
                    p /pattern/.do_something.do_something
                      ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
                "#},
                "p(/pattern/.do_something.do_something)\n",
            );
    }

    #[test]
    fn flags_nested_command_argument() {
        // `puts line.grep /pattern/` — the inner `grep` command is the offense
        // node; only its arguments get parenthesized.
        test::<AmbiguousRegexpLiteral>()
            .expect_offense(indoc! {r#"
                puts line.grep /pattern/
                               ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
            "#})
            .expect_correction(
                indoc! {r#"
                    puts line.grep /pattern/
                                   ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
                "#},
                "puts line.grep(/pattern/)\n",
            );
    }

    #[test]
    fn flags_command_after_receiver_chain() {
        // `expect('x').to match /Cop/` — `match` is the unparenthesized command.
        test::<AmbiguousRegexpLiteral>()
            .expect_offense(indoc! {r#"
                expect('x').to match /Cop/
                                     ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
            "#})
            .expect_correction(
                indoc! {r#"
                    expect('x').to match /Cop/
                                         ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
                "#},
                "expect('x').to match(/Cop/)\n",
            );
    }

    #[test]
    fn flags_with_block_argument() {
        test::<AmbiguousRegexpLiteral>()
            .expect_offense(indoc! {r#"
                p /pattern/, foo do |arg|
                  ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
                end
            "#})
            .expect_correction(
                indoc! {r#"
                    p /pattern/, foo do |arg|
                      ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
                    end
                "#},
                "p(/pattern/, foo) do |arg|\nend\n",
            );
    }

    #[test]
    fn flags_with_regexp_flags() {
        test::<AmbiguousRegexpLiteral>()
            .expect_offense(indoc! {r#"
                p /pattern/i
                  ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
            "#})
            .expect_correction(
                indoc! {r#"
                    p /pattern/i
                      ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
                "#},
                "p(/pattern/i)\n",
            );
    }

    #[test]
    fn flags_safe_navigation_command() {
        // `obj&.scan /re/` — a csend with the regexp as its first argument.
        test::<AmbiguousRegexpLiteral>()
            .expect_offense(indoc! {r#"
                obj&.scan /pattern/
                          ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
            "#})
            .expect_correction(
                indoc! {r#"
                    obj&.scan /pattern/
                              ^ Ambiguous regexp literal. Parenthesize the method arguments if it's surely a regexp literal, or add a whitespace to the right of the `/` if it should be a division.
                "#},
                "obj&.scan(/pattern/)\n",
            );
    }

    #[test]
    fn accepts_parenthesized_call() {
        test::<AmbiguousRegexpLiteral>().expect_no_offenses("p(/pattern/)\n");
    }

    #[test]
    fn accepts_parenthesized_call_with_chained_regexp() {
        test::<AmbiguousRegexpLiteral>().expect_no_offenses("p(/pattern/.bar)\n");
    }

    #[test]
    fn accepts_regexp_not_in_first_position() {
        // The regexp is the second argument, so it does not start the arg list.
        test::<AmbiguousRegexpLiteral>().expect_no_offenses("p foo, /pattern/\n");
    }

    #[test]
    fn accepts_assignment() {
        test::<AmbiguousRegexpLiteral>().expect_no_offenses("x = /pattern/\n");
    }

    #[test]
    fn accepts_division() {
        // Real division: `/` operands separated by spaces, no regexp node.
        test::<AmbiguousRegexpLiteral>().expect_no_offenses(indoc! {r#"
            foo = 1
            foo / pattern / 2
        "#});
    }
}
