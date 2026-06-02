//! `Style/MethodDefParentheses` — enforces consistent parentheses usage
//! around method definition arguments.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MethodDefParentheses
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   All three EnforcedStyle values are implemented:
//!   - require_parentheses (default): flag args without parens.
//!   - require_no_parentheses: flag args with parens (unless forced).
//!   - require_no_parentheses_except_multiline: no-parens for single-line
//!     args, require parens for multiline args.
//!   Forced parentheses cases (endless method, forward-all ..., anonymous
//!   *, **, &) are always skipped per RuboCop parity.
//!   Methods with no arguments are never flagged in require_parentheses style.
//!   Empty-paren methods `def foo()` are flagged in require_no_parentheses style.
//! ```
//!
//! ## Matched shapes
//!
//! `def` and `defs` (singleton method) nodes.
//!
//! ## Autocorrect
//!
//! - Missing parens (require_parentheses): insert `(` before and `)` after the
//!   args source range (from first arg to last arg).
//! - Unwanted parens (require_no_parentheses): replace `(` with a space and
//!   remove `)`.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG_PRESENT: &str = "Use def without parentheses.";
const MSG_MISSING: &str = "Use def with parentheses when there are parameters.";

/// Enforced parentheses style.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[allow(clippy::enum_variant_names)]
pub enum MethodDefParenthesesStyle {
    #[default]
    #[option(value = "require_parentheses")]
    RequireParentheses,
    #[option(value = "require_no_parentheses")]
    RequireNoParentheses,
    #[option(value = "require_no_parentheses_except_multiline")]
    RequireNoParenthesesExceptMultiline,
}

/// Configuration options.
#[derive(CopOptions)]
pub struct MethodDefParenthesesOptions {
    #[option(
        name = "EnforcedStyle",
        default = "require_parentheses",
        description = "Whether method definitions should have or not have parentheses."
    )]
    pub enforced_style: MethodDefParenthesesStyle,
}

/// Stateless unit struct.
#[derive(Default)]
pub struct MethodDefParentheses;

#[cop(
    name = "Style/MethodDefParentheses",
    description = "Enforce consistent parentheses in method definitions.",
    default_severity = "warning",
    default_enabled = true,
    options = MethodDefParenthesesOptions,
)]
impl MethodDefParentheses {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Find the method name token range by searching for the Other token
/// whose bytes equal the method name Symbol.
fn find_method_name_range(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let name_sym = match cx.kind(node) {
        NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => *name,
        _ => return None,
    };
    let name_str = cx.symbol_str(name_sym);
    let name_bytes = name_str.as_bytes();
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();

    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < node_range.start);
    // Search for the first Other token within the node range whose text
    // matches the method name. Walk forward from the def start.
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == name_bytes
        })
        .map(|t| t.range)
}

/// Returns `true` if the def/defs node's args are wrapped in parentheses.
///
/// - For non-empty args: checks if there is a LeftParen token between the
///   method name end and the first arg start.
/// - For empty args: checks if the immediate next token after the method name
///   is a LeftParen (handles `def foo()` vs `def foo`).
fn has_parentheses(node: NodeId, args: NodeId, cx: &Cx<'_>) -> bool {
    let Some(name_range) = find_method_name_range(node, cx) else {
        return false;
    };
    let NodeKind::Args(list) = *cx.kind(args) else {
        return false;
    };
    let children = cx.list(list);

    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < name_range.end);

    if let Some(&first_arg) = children.first() {
        // Non-empty args: look for LeftParen between name.end and first arg start.
        let first_start = cx.range(first_arg).start;
        return toks[idx..]
            .iter()
            .take_while(|t| t.range.start < first_start)
            .any(|t| t.kind == SourceTokenKind::LeftParen);
    }

    // Empty args: check if the very next token after the method name is `(`.
    toks.get(idx)
        .is_some_and(|t| t.kind == SourceTokenKind::LeftParen)
}

/// Returns the source range from the first to the last arg child (args
/// themselves, without surrounding parens).
fn args_children_range(args: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let NodeKind::Args(list) = *cx.kind(args) else {
        return None;
    };
    let children = cx.list(list);
    let first = children.first()?;
    let last = children.last()?;
    Some(Range {
        start: cx.range(*first).start,
        end: cx.range(*last).end,
    })
}

/// Returns `true` if the def/defs node is an endless method
/// (no `end` keyword: `def foo = expr`).
fn is_endless(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.loc(node).end_keyword() == Range::ZERO
}

/// Returns `true` when the method has anonymous argument forms that
/// syntactically require parentheses (removing them would be a syntax error):
/// - `...` (ForwardArgs)
/// - anonymous `*` (Restarg with empty name)
/// - anonymous `**` (Kwrestarg with empty name)
/// - anonymous `&` (Blockarg with empty name)
fn has_anonymous_arguments(args: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Args(list) = *cx.kind(args) else {
        return false;
    };
    for &child in cx.list(list) {
        match cx.kind(child) {
            NodeKind::ForwardArgs => return true,
            NodeKind::Restarg(sym) if cx.symbol_str(*sym).is_empty() => return true,
            NodeKind::Kwrestarg(sym) if cx.symbol_str(*sym).is_empty() => return true,
            NodeKind::Blockarg(sym) if cx.symbol_str(*sym).is_empty() => return true,
            _ => {}
        }
    }
    false
}

/// Returns `true` if the def/defs node's args span multiple lines.
fn args_are_multiline(args: NodeId, cx: &Cx<'_>) -> bool {
    // Use the children range for multiline detection.
    if let Some(range) = args_children_range(args, cx) {
        let source = cx.source().as_bytes();
        // Check if there's a newline between first and last arg.
        source[range.start as usize..range.end as usize].contains(&b'\n')
    } else {
        false
    }
}

/// Returns `true` if parentheses are syntactically required regardless of style.
fn forced_parentheses(node: NodeId, args: NodeId, cx: &Cx<'_>) -> bool {
    is_endless(node, cx) || has_anonymous_arguments(args, cx)
}

/// Returns `true` if the style requires parentheses for these args.
fn require_parentheses(style: MethodDefParenthesesStyle, args: NodeId, cx: &Cx<'_>) -> bool {
    matches!(style, MethodDefParenthesesStyle::RequireParentheses)
        || (matches!(
            style,
            MethodDefParenthesesStyle::RequireNoParenthesesExceptMultiline
        ) && args_are_multiline(args, cx))
}

/// Find the LeftParen and RightParen tokens that wrap the def's args.
fn find_arg_parens(node: NodeId, args: NodeId, cx: &Cx<'_>) -> Option<(Range, Range)> {
    let name_range = find_method_name_range(node, cx)?;
    let node_range = cx.range(node);
    let NodeKind::Args(list) = *cx.kind(args) else {
        return None;
    };
    let children = cx.list(list);

    let toks = cx.sorted_tokens();

    // Find the LeftParen: first LeftParen token after the method name.
    let open_idx = toks.partition_point(|t| t.range.start < name_range.end);
    let open_paren = toks[open_idx..]
        .iter()
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| t.kind == SourceTokenKind::LeftParen)
        .copied()?;

    // Find the RightParen: first RightParen token after the last arg (or after open paren).
    let search_from = if let Some(&last_arg) = children.last() {
        cx.range(last_arg).end
    } else {
        open_paren.range.end
    };
    let close_idx = toks.partition_point(|t| t.range.start < search_from);
    let close_paren = toks[close_idx..]
        .iter()
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| t.kind == SourceTokenKind::RightParen)
        .copied()?;

    Some((open_paren.range, close_paren.range))
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<MethodDefParenthesesOptions>();
    let Some(args) = cx.def_arguments(node).get() else {
        return;
    };

    let NodeKind::Args(args_list) = *cx.kind(args) else {
        return;
    };
    let has_args = !cx.list(args_list).is_empty();
    let parens_present = has_parentheses(node, args, cx);

    if require_parentheses(opts.enforced_style, args, cx) {
        // Style requires parens. Flag if args are present but no parens.
        if has_args && !parens_present {
            let offense_range = args_children_range(args, cx).unwrap_or_else(|| cx.range(node));
            cx.emit_offense(offense_range, MSG_MISSING, None);
            autocorrect_add_parens(offense_range, cx);
        }
    } else if forced_parentheses(node, args, cx) {
        // Parentheses are syntactically required; no offense regardless of style.
    } else if parens_present {
        // Style does not require parens, but they are present → offense.
        // Offense range: covers the args including parens (from `(` to `)`).
        if let Some((open, close)) = find_arg_parens(node, args, cx) {
            let offense_range = Range { start: open.start, end: close.end };
            cx.emit_offense(offense_range, MSG_PRESENT, None);
            autocorrect_remove_parens(open, close, cx);
        }
    }
}

/// Autocorrect: insert `(` before the args range and `)` after it.
/// Also removes any whitespace between the method name and the first arg
/// so `def bar num1, num2` becomes `def bar(num1, num2)` not `def bar (...)`.
fn autocorrect_add_parens(args_range: Range, cx: &Cx<'_>) {
    let source = cx.source().as_bytes();
    // Find the start of any whitespace immediately before the first arg.
    let mut ws_start = args_range.start as usize;
    while ws_start > 0 && (source[ws_start - 1] == b' ' || source[ws_start - 1] == b'\t') {
        ws_start -= 1;
    }
    // Replace the whitespace (if any) + insert `(`.
    // This turns `bar num1` -> `bar(num1`.
    cx.emit_edit(
        Range { start: ws_start as u32, end: args_range.start },
        "(",
    );
    cx.emit_edit(Range { start: args_range.end, end: args_range.end }, ")");
}

/// Autocorrect: replace the `(` token with ` ` and remove the `)` token.
fn autocorrect_remove_parens(open: Range, close: Range, cx: &Cx<'_>) {
    // Replace `(` with a single space.
    cx.emit_edit(open, " ");
    // Remove `)`.
    cx.emit_edit(close, "");
}

#[cfg(test)]
mod tests {
    use super::{MethodDefParentheses, MethodDefParenthesesOptions, MethodDefParenthesesStyle};
    use murphy_plugin_api::test_support::{indoc, run_cop, test};

    // ----- require_parentheses (default) -----

    #[test]
    fn flags_def_args_without_parens() {
        test::<MethodDefParentheses>().expect_offense(indoc! {"
            def bar num1, num2
                    ^^^^^^^^^^ Use def with parentheses when there are parameters.
              num1 + num2
            end
        "});
    }

    #[test]
    fn corrects_def_args_without_parens() {
        test::<MethodDefParentheses>().expect_correction(
            indoc! {"
                def bar num1, num2
                        ^^^^^^^^^^ Use def with parentheses when there are parameters.
                  num1 + num2
                end
            "},
            indoc! {"
                def bar(num1, num2)
                  num1 + num2
                end
            "},
        );
    }

    #[test]
    fn accepts_def_with_parens() {
        test::<MethodDefParentheses>().expect_no_offenses(indoc! {"
            def bar(num1, num2)
              num1 + num2
            end
        "});
    }

    #[test]
    fn accepts_def_no_args() {
        test::<MethodDefParentheses>().expect_no_offenses(indoc! {"
            def bar
            end
        "});
    }

    #[test]
    fn accepts_def_no_args_with_empty_parens() {
        // Empty parens with no args: acceptable in require_parentheses style
        // (no args to require parens around).
        test::<MethodDefParentheses>().expect_no_offenses("def bar(); end\n");
    }

    // ----- require_no_parentheses -----

    #[test]
    fn flags_def_with_unwanted_parens() {
        test::<MethodDefParentheses>()
            .with_options(&MethodDefParenthesesOptions {
                enforced_style: MethodDefParenthesesStyle::RequireNoParentheses,
            })
            .expect_offense(indoc! {"
                def bar(num1, num2)
                       ^^^^^^^^^^^^ Use def without parentheses.
                  num1 + num2
                end
            "});
    }

    #[test]
    fn corrects_def_with_unwanted_parens() {
        test::<MethodDefParentheses>()
            .with_options(&MethodDefParenthesesOptions {
                enforced_style: MethodDefParenthesesStyle::RequireNoParentheses,
            })
            .expect_correction(
                indoc! {"
                    def bar(num1, num2)
                           ^^^^^^^^^^^^ Use def without parentheses.
                      num1 + num2
                    end
                "},
                indoc! {"
                    def bar num1, num2
                      num1 + num2
                    end
                "},
            );
    }

    #[test]
    fn accepts_def_without_parens_no_parens_style() {
        test::<MethodDefParentheses>()
            .with_options(&MethodDefParenthesesOptions {
                enforced_style: MethodDefParenthesesStyle::RequireNoParentheses,
            })
            .expect_no_offenses(indoc! {"
                def bar num1, num2
                  num1 + num2
                end
            "});
    }

    // ----- Forced parentheses cases -----

    #[test]
    fn accepts_endless_method_no_parens_style() {
        test::<MethodDefParentheses>()
            .with_options(&MethodDefParenthesesOptions {
                enforced_style: MethodDefParenthesesStyle::RequireNoParentheses,
            })
            .expect_no_offenses("def foo(a) = bar(a)\n");
    }

    #[test]
    fn accepts_forward_args_no_parens_style() {
        test::<MethodDefParentheses>()
            .with_options(&MethodDefParenthesesOptions {
                enforced_style: MethodDefParenthesesStyle::RequireNoParentheses,
            })
            .expect_no_offenses("def foo(...); end\n");
    }

    #[test]
    fn accepts_anonymous_rest_no_parens_style() {
        test::<MethodDefParentheses>()
            .with_options(&MethodDefParenthesesOptions {
                enforced_style: MethodDefParenthesesStyle::RequireNoParentheses,
            })
            .expect_no_offenses("def foo(*); end\n");
    }

    #[test]
    fn accepts_anonymous_kwrest_no_parens_style() {
        test::<MethodDefParentheses>()
            .with_options(&MethodDefParenthesesOptions {
                enforced_style: MethodDefParenthesesStyle::RequireNoParentheses,
            })
            .expect_no_offenses("def foo(**); end\n");
    }

    #[test]
    fn accepts_anonymous_blockarg_no_parens_style() {
        test::<MethodDefParentheses>()
            .with_options(&MethodDefParenthesesOptions {
                enforced_style: MethodDefParenthesesStyle::RequireNoParentheses,
            })
            .expect_no_offenses("def foo(&); end\n");
    }

    // ----- require_no_parentheses_except_multiline -----

    #[test]
    fn flags_single_line_args_with_parens_multiline_except_style() {
        test::<MethodDefParentheses>()
            .with_options(&MethodDefParenthesesOptions {
                enforced_style: MethodDefParenthesesStyle::RequireNoParenthesesExceptMultiline,
            })
            .expect_offense(indoc! {"
                def bar(num1, num2)
                       ^^^^^^^^^^^^ Use def without parentheses.
                  num1 + num2
                end
            "});
    }

    #[test]
    fn accepts_multiline_args_with_parens_multiline_except_style() {
        test::<MethodDefParentheses>()
            .with_options(&MethodDefParenthesesOptions {
                enforced_style: MethodDefParenthesesStyle::RequireNoParenthesesExceptMultiline,
            })
            .expect_no_offenses(indoc! {"
                def foo(descriptive_var_name,
                        another_descriptive_var_name)
                  do_something
                end
            "});
    }

    #[test]
    fn flags_multiline_args_without_parens_multiline_except_style() {
        // Multi-line args range: use run_cop to avoid annotated-format limitations.
        let offenses = run_cop::<MethodDefParentheses>(
            "def foo descriptive_var_name,\n        another_descriptive_var_name\n  do_something\nend\n",
        );
        // TODO: This test currently has no options override, but
        // the multiline_except style needs to be configured.
        // Skip for now and use a direct approach.
        let _ = offenses;
    }

    // Multiline args without parens: verify via run_cop_with_options.
    #[test]
    fn detects_multiline_args_without_parens_multiline_except_style() {
        let opts = MethodDefParenthesesOptions {
            enforced_style: MethodDefParenthesesStyle::RequireNoParenthesesExceptMultiline,
        };
        let offenses = murphy_plugin_api::test_support::run_cop_with_options::<MethodDefParentheses>(
            "def foo descriptive_var_name,\n        another_descriptive_var_name\n  do_something\nend\n",
            &opts,
        );
        assert_eq!(offenses.len(), 1);
        assert!(
            offenses[0].message.contains("Use def with parentheses"),
            "expected missing-parens message, got: {}",
            offenses[0].message
        );
    }

    // ----- Singleton methods (defs) -----

    #[test]
    fn flags_singleton_def_without_parens() {
        test::<MethodDefParentheses>().expect_offense(indoc! {"
            def self.bar num1, num2
                         ^^^^^^^^^^ Use def with parentheses when there are parameters.
              num1 + num2
            end
        "});
    }

    #[test]
    fn accepts_singleton_def_with_parens() {
        test::<MethodDefParentheses>().expect_no_offenses(indoc! {"
            def self.bar(num1, num2)
              num1 + num2
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(MethodDefParentheses);
