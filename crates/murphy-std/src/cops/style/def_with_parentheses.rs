//! `Style/DefWithParentheses` — flags empty `()` in zero-argument method definitions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/DefWithParentheses
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags def and defs (singleton methods) with empty parentheses when there
//!   are no arguments. Skips single-line non-endless methods (e.g.
//!   `def foo(); end` — a syntax error without parens). Skips endless methods
//!   where `)=` would immediately follow (e.g. `def foo()=bar`). Autocorrect
//!   removes the empty parentheses.
//! ```
//!
//! ## Matched shapes
//!
//! `def` and `defs` nodes with no arguments but with a `(` immediately
//! after the method name.
//!
//! ## Skip conditions
//!
//! - Single-line, non-endless: `def foo(); end` — parens required syntactically.
//! - Endless with `=` immediately after `)`: `def foo()=bar` — parens required.
//!
//! ## Autocorrect
//!
//! Removes the `()` range (both the open and close paren tokens).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Omit the parentheses in defs when the method doesn't accept any arguments.";

#[derive(Default)]
pub struct DefWithParentheses;

#[cop(
    name = "Style/DefWithParentheses",
    description = "Omit the parentheses in defs when the method doesn't accept any arguments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl DefWithParentheses {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Returns true if the def/defs node is an endless method (`def foo = expr`).
fn is_endless(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.loc(node).end_keyword() == Range::ZERO
}

/// Find the `(` and `)` tokens for the (empty) argument list.
/// Returns None if no `(` is found immediately after the method name token.
fn find_empty_parens(node: NodeId, cx: &Cx<'_>) -> Option<(Range, Range)> {
    // Find the method name token range.
    let name_sym = match cx.kind(node) {
        NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => *name,
        _ => return None,
    };
    let name_str = cx.symbol_str(name_sym);
    let name_bytes = name_str.as_bytes();
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    // Find the method name token.
    let idx = toks.partition_point(|t| t.range.start < node_range.start);
    let name_tok = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == name_bytes
        })?;

    // The token immediately after the name must be `(`.
    let after_idx = toks.partition_point(|t| t.range.start < name_tok.range.end);
    let open_tok = toks.get(after_idx)?;
    if open_tok.kind != SourceTokenKind::LeftParen {
        return None;
    }
    let open_range = open_tok.range;

    // The token immediately after `(` must be `)` (empty parens).
    let close_idx = toks.partition_point(|t| t.range.start < open_range.end);
    let close_tok = toks.get(close_idx)?;
    if close_tok.kind != SourceTokenKind::RightParen {
        return None;
    }

    Some((open_range, close_tok.range))
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Only flag when there are no arguments.
    let Some(args) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(list) = *cx.kind(args) else {
        return;
    };
    if !cx.list(list).is_empty() {
        return;
    }

    // Find the `()` tokens.
    let Some((open_range, close_range)) = find_empty_parens(node, cx) else {
        return;
    };

    let endless = is_endless(node, cx);

    // Skip single-line non-endless methods: `def foo(); end` or `def foo() x end`
    // (removing parens would be a syntax error).
    if cx.is_single_line(node) && !endless {
        return;
    }

    // Skip endless methods where `)` is immediately followed by `=`:
    // `def foo()=bar` — parens required.
    if endless {
        let source = cx.source().as_bytes();
        let after_close = close_range.end as usize;
        // Check the byte immediately after `)`.
        if source.get(after_close) == Some(&b'=') {
            return;
        }
    }

    let offense_range = Range {
        start: open_range.start,
        end: close_range.end,
    };
    cx.emit_offense(offense_range, MSG, None);

    // Autocorrect: remove both parens.
    cx.emit_edit(open_range, "");
    cx.emit_edit(close_range, "");
}

#[cfg(test)]
mod tests {
    use super::DefWithParentheses;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_parens_multiline() {
        test::<DefWithParentheses>().expect_correction(
            indoc! {"
                def foo()
                       ^^ Omit the parentheses in defs when the method doesn't accept any arguments.
                  do_something
                end
            "},
            indoc! {"
                def foo
                  do_something
                end
            "},
        );
    }

    #[test]
    fn flags_singleton_def_empty_parens() {
        test::<DefWithParentheses>().expect_correction(
            indoc! {"
                def Baz.foo()
                           ^^ Omit the parentheses in defs when the method doesn't accept any arguments.
                  do_something
                end
            "},
            indoc! {"
                def Baz.foo
                  do_something
                end
            "},
        );
    }

    #[test]
    fn accepts_def_without_parens() {
        test::<DefWithParentheses>().expect_no_offenses(indoc! {"
            def foo
              do_something
            end
        "});
    }

    #[test]
    fn accepts_def_with_args() {
        test::<DefWithParentheses>().expect_no_offenses(indoc! {"
            def foo(bar)
              do_something
            end
        "});
    }

    #[test]
    fn accepts_single_line_with_parens() {
        // Single-line non-endless: def foo(); end — parens required syntactically.
        test::<DefWithParentheses>().expect_no_offenses("def foo(); end\n");
    }

    #[test]
    fn flags_endless_def_with_parens() {
        // Endless method with parens and space before `=`.
        test::<DefWithParentheses>().expect_correction(
            "def foo() = do_something\n       ^^ Omit the parentheses in defs when the method doesn't accept any arguments.\n",
            "def foo = do_something\n",
        );
    }

    #[test]
    fn accepts_endless_def_with_parens_equals() {
        // def foo()=bar — parens syntactically required.
        test::<DefWithParentheses>().expect_no_offenses("def foo()=do_something\n");
    }
}
murphy_plugin_api::submit_cop!(DefWithParentheses);
