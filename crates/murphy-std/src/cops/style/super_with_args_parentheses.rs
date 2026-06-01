//! `Style/SuperWithArgsParentheses` — use parentheses for `super` with
//! arguments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SuperWithArgsParentheses
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `super` calls with explicit arguments that are not wrapped in
//!   parentheses. `super` with no arguments (zsuper) is never flagged.
//!   `super()` with empty parentheses is also never flagged (that is
//!   NodeKind::Super with an empty args list AND parentheses).
//!   Parenthesis detection uses a token scan for `LeftParen` starting
//!   exactly at keyword-end, because `cx.is_parenthesized` only covers
//!   Send/Csend nodes (call_closing_locs is not populated for SuperNode).
//!   Autocorrect: insert `(` between the keyword and first argument, and
//!   insert `)` after the last argument.
//! ```
//!
//! ## Detection
//!
//! A `Super` node is flagged when:
//! 1. It has at least one argument (non-empty args list).
//! 2. No `(` token starts at exactly `selector.end` (i.e. not parenthesized).
//!
//! `Zsuper` (bare `super`) is excluded because it maps to `NodeKind::Zsuper`.
//!
//! ## Autocorrect
//!
//! Two surgical edits:
//! - Replace the gap from `keyword.end` to `first_arg.start` with `(`.
//! - Insert `)` at `last_arg.end`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Use parentheses for `super` with arguments.";

#[derive(Default)]
pub struct SuperWithArgsParentheses;

#[cop(
    name = "Style/SuperWithArgsParentheses",
    description = "Use parentheses for `super` with arguments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SuperWithArgsParentheses {
    #[on_node(kind = "super")]
    fn check_super(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Super(args_list) = *cx.kind(node) else {
            return;
        };
        let args = cx.list(args_list);

        // No arguments: `super()` with empty parens or bare `super` without
        // parens — RuboCop does not flag either. `super` with no args and
        // no parens is `Zsuper` (excluded by on_node) so this guard covers
        // `super()` (Super with empty list).
        if args.is_empty() {
            return;
        }

        // Check whether there is a `(` token immediately after the keyword.
        let keyword_end = cx.selector(node).end;
        if has_left_paren_at(cx, keyword_end) {
            return;
        }

        let range = cx.range(node);
        cx.emit_offense(range, MSG, None);

        // Autocorrect: insert `(` right after the `super` keyword by replacing
        // the gap between keyword end and first arg start with `(`. Then
        // insert `)` after the last arg.
        let first_arg_start = cx.range(args[0]).start;
        let last_arg_end = cx.range(args[args.len() - 1]).end;

        // Replace keyword-end .. first-arg-start with `(`.
        cx.emit_edit(
            Range {
                start: keyword_end,
                end: first_arg_start,
            },
            "(",
        );
        // Insert `)` after the last argument (zero-width range at last_arg_end).
        cx.emit_edit(
            Range {
                start: last_arg_end,
                end: last_arg_end,
            },
            ")",
        );
    }
}

/// Returns `true` when there is a `LeftParen` token whose `range.start`
/// equals `offset` — i.e. a `(` starts exactly at the given position.
///
/// This is the token-scan alternative to `cx.is_parenthesized`, which only
/// covers `Send`/`Csend` nodes (whose `call_closing_locs` are populated by
/// the translator). `Super` nodes do not have a `call_closing_loc` entry.
fn has_left_paren_at(cx: &Cx<'_>, offset: u32) -> bool {
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < offset);
    if let Some(tok) = toks.get(idx) {
        tok.range.start == offset && tok.kind == SourceTokenKind::LeftParen
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::SuperWithArgsParentheses;
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- detection -----------------------------------------------------------

    #[test]
    fn flags_super_without_parens() {
        test::<SuperWithArgsParentheses>().expect_offense(indoc! {"
            def m
              super name, age
              ^^^^^^^^^^^^^^^ Use parentheses for `super` with arguments.
            end
        "});
    }

    #[test]
    fn flags_super_single_arg_no_parens() {
        test::<SuperWithArgsParentheses>().expect_offense(indoc! {"
            def m
              super name
              ^^^^^^^^^^ Use parentheses for `super` with arguments.
            end
        "});
    }

    #[test]
    fn accepts_super_with_parens() {
        test::<SuperWithArgsParentheses>().expect_no_offenses(indoc! {"
            def m
              super(name, age)
            end
        "});
    }

    #[test]
    fn accepts_bare_super_no_args() {
        // `super` with no arguments → `Zsuper`, never flagged.
        test::<SuperWithArgsParentheses>().expect_no_offenses(indoc! {"
            def m
              super
            end
        "});
    }

    #[test]
    fn accepts_super_empty_parens() {
        // `super()` — explicit empty parens, not flagged.
        test::<SuperWithArgsParentheses>().expect_no_offenses(indoc! {"
            def m
              super()
            end
        "});
    }

    // ---- autocorrect --------------------------------------------------------

    #[test]
    fn autocorrects_super_without_parens() {
        test::<SuperWithArgsParentheses>().expect_correction(
            indoc! {"
                def m
                  super name, age
                  ^^^^^^^^^^^^^^^ Use parentheses for `super` with arguments.
                end
            "},
            "def m\n  super(name, age)\nend\n",
        );
    }

    #[test]
    fn autocorrects_super_single_arg() {
        test::<SuperWithArgsParentheses>().expect_correction(
            indoc! {"
                def m
                  super name
                  ^^^^^^^^^^ Use parentheses for `super` with arguments.
                end
            "},
            "def m\n  super(name)\nend\n",
        );
    }

    #[test]
    fn autocorrect_is_idempotent() {
        test::<SuperWithArgsParentheses>().expect_no_offenses(indoc! {"
            def m
              super(name, age)
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(SuperWithArgsParentheses);
