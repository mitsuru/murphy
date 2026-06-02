//! `Style/For` — checks use of `for` or `each` in multiline loops.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/For
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Both `each` (default) and `for` enforcement modes are implemented.
//!   Detection is complete for both modes.
//!   Autocorrect for `for`→`each` handles single-variable, multi-variable
//!   (mlhs), and range iterables with whole-node interpolation.
//!   Autocorrect for `each`→`for` rewrites multiline `.each` blocks.
//!   NOTE: This cop's autocorrection is unsafe because `for` leaks loop
//!   variables into the surrounding scope while `each` does not.
//!   Offense range covers the first line of the for/each construct, matching
//!   Murphy's convention for multi-line node offenses.
//! ```
//!
//! ## Matched shapes
//!
//! - `each` mode (default): `for … in … end` nodes → flags offense
//! - `for` mode: multiline `block`/`numblock`/`itblock` calling `.each`
//!   with no args and having a receiver
//!
//! ## Autocorrect
//!
//! - `for → each`: `for n in col\n  body\nend` → `col.each do |n|\n  body\nend`
//! - `each → for`: `col.each do |n|\n  body\nend` → `for n in col\n  body\nend`

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG_PREFER_EACH: &str = "Prefer `each` over `for`.";
const MSG_PREFER_FOR: &str = "Prefer `for` over `each`.";

#[derive(Default)]
pub struct For;

/// Enforced style for the `Style/For` cop.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ForStyle {
    /// Use `.each` with a block (Ruby idiomatic). Default.
    #[default]
    #[option(value = "each")]
    Each,
    /// Use `for` keyword loops.
    #[option(value = "for")]
    ForKeyword,
}

#[derive(CopOptions)]
pub struct ForOptions {
    #[option(
        name = "EnforcedStyle",
        default = "each",
        description = "Preferred iteration style: `each` (default) or `for`."
    )]
    pub enforced_style: ForStyle,
}

#[cop(
    name = "Style/For",
    description = "Checks use of `for` or `each` in multiline loops.",
    default_severity = "warning",
    default_enabled = true,
    options = ForOptions,
)]
impl For {
    #[on_node(kind = "for")]
    fn check_for(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ForOptions>();
        if opts.enforced_style != ForStyle::Each {
            return;
        }
        // Offense on the opening line of the for node.
        cx.emit_offense(first_line_range(node, cx), MSG_PREFER_EACH, None);
        autocorrect_for_to_each(node, cx);
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_each_block(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_each_block(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_each_block(node, cx);
    }
}

/// Check a block node for `for` enforcement.
fn check_each_block(node: NodeId, cx: &Cx<'_>) {
    let opts = cx.options_or_default::<ForOptions>();
    if opts.enforced_style != ForStyle::ForKeyword {
        return;
    }
    if !suspect_enumerable(node, cx) {
        return;
    }
    // Offense on the opening line of the block.
    cx.emit_offense(first_line_range(node, cx), MSG_PREFER_FOR, None);
    autocorrect_each_to_for(node, cx);
}

/// Returns the range covering only the first line of a node.
fn first_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let start = node_range.start as usize;
    let end = source[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|p| (start + p) as u32)
        .unwrap_or(node_range.end);
    Range {
        start: node_range.start,
        end,
    }
}

/// Returns true if the block is a multiline `.each` call with no send
/// arguments, has a receiver, and has at least one block argument.
/// A block with no arguments cannot be rewritten as a valid `for` loop.
fn suspect_enumerable(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_single_line(node) {
        return false;
    }
    let (call, block_args) = match *cx.kind(node) {
        NodeKind::Block { call, args, .. } => (call, Some(args)),
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => (send, None),
        _ => return false,
    };
    // Block must have at least one argument for a valid `for` rewrite.
    if let Some(args_node) = block_args
        && let NodeKind::Args(l) = *cx.kind(args_node)
        && cx.list(l).is_empty()
    {
        return false;
    }
    let NodeKind::Send { method, .. } = *cx.kind(call) else {
        return false;
    };
    if cx.call_receiver(call).get().is_none() {
        return false;
    }
    cx.symbol_str(method) == "each" && cx.call_arguments(call).is_empty()
}

/// Autocorrect: `for n in col\n  body\nend` → `col.each do |n|\n  body\nend`
fn autocorrect_for_to_each(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::For { var, iter, body } = *cx.kind(node) else {
        return;
    };

    let iter_src = cx.raw_source(cx.range(iter));
    let var_src = cx.raw_source(cx.range(var));

    // Build block param(s): handle `mlhs` (multi-variable `for a, b in`).
    let params = match *cx.kind(var) {
        NodeKind::Mlhs(_) => {
            format!("|({})|", inner_mlhs_src(var, cx))
        }
        _ => {
            format!("|{}|", var_src)
        }
    };

    let body_str = if let Some(b) = body.get() {
        cx.raw_source(cx.range(b)).to_string()
    } else {
        String::new()
    };

    let iter_with_parens = if needs_parens_for_each(iter, cx) {
        format!("({})", iter_src)
    } else {
        iter_src.to_string()
    };

    let indent = node_indent(node, cx);
    let inner_indent = format!("{}  ", indent);

    let replacement = if body_str.is_empty() {
        format!("{iter_with_parens}.each do {params}\n{indent}end")
    } else {
        let indented_body = indent_body(&body_str, &inner_indent);
        format!("{iter_with_parens}.each do {params}\n{indented_body}\n{indent}end")
    };

    cx.emit_edit(cx.range(node), &replacement);
}

/// Returns the inner variable list source for an `mlhs` node (e.g. `a, b`).
fn inner_mlhs_src(node: NodeId, cx: &Cx<'_>) -> String {
    let NodeKind::Mlhs(list) = *cx.kind(node) else {
        return cx.raw_source(cx.range(node)).to_string();
    };
    cx.list(list)
        .iter()
        .map(|&child| cx.raw_source(cx.range(child)).to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Returns true if the iterable needs parentheses when used as a receiver.
fn needs_parens_for_each(iter: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(iter), NodeKind::RangeExpr { .. })
}

/// Autocorrect: `col.each do |n|\n  body\nend` → `for n in col\n  body\nend`
fn autocorrect_each_to_for(node: NodeId, cx: &Cx<'_>) {
    let (call, args_node, body) = match *cx.kind(node) {
        NodeKind::Block { call, args, body } => (call, args, body),
        _ => return,
    };

    let Some(recv) = cx.call_receiver(call).get() else {
        return;
    };

    let recv_src = cx.raw_source(cx.range(recv));
    let indent = node_indent(node, cx);

    // Build block variable(s) from block args.
    let args_list = match *cx.kind(args_node) {
        NodeKind::Args(l) => cx.list(l),
        _ => return,
    };

    let var_str = if args_list.is_empty() {
        String::new()
    } else if args_list.len() == 1 {
        cx.raw_source(cx.range(args_list[0])).to_string()
    } else {
        args_list
            .iter()
            .map(|&a| cx.raw_source(cx.range(a)).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };

    let body_str = if let Some(b) = body.get() {
        cx.raw_source(cx.range(b)).to_string()
    } else {
        String::new()
    };

    let inner_indent = format!("{}  ", indent);
    let for_header = if var_str.is_empty() {
        format!("for in {recv_src}")
    } else {
        format!("for {var_str} in {recv_src}")
    };

    let replacement = if body_str.is_empty() {
        format!("{for_header}\n{indent}end")
    } else {
        let indented_body = indent_body(&body_str, &inner_indent);
        format!("{for_header}\n{indented_body}\n{indent}end")
    };

    cx.emit_edit(cx.range(node), &replacement);
}

/// Return the indentation whitespace of a node.
fn node_indent(node: NodeId, cx: &Cx<'_>) -> String {
    let source = cx.source();
    let source_bytes = source.as_bytes();
    let start = cx.range(node).start as usize;
    let line_start = source_bytes[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    source[line_start..start].to_string()
}

/// Re-indent body source to target_indent.
fn indent_body(body: &str, target_indent: &str) -> String {
    // Find original indentation from the first non-empty line using strip_suffix
    // to safely extract the prefix without manual byte slicing.
    let orig_indent = body
        .lines()
        .find(|l| !l.trim().is_empty())
        .and_then(|l| {
            let trimmed = l.trim_start();
            l.strip_suffix(trimmed)
        })
        .unwrap_or("");

    body.lines()
        .map(|line| {
            if line.trim().is_empty() {
                line.to_string()
            } else if let Some(rest) = line.strip_prefix(orig_indent) {
                format!("{target_indent}{rest}")
            } else {
                format!("{target_indent}{}", line.trim_start())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{For, ForOptions, ForStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn option_defaults_match_rubocop() {
        let opts = ForOptions::default();
        assert_eq!(opts.enforced_style, ForStyle::Each);
    }

    // --- `each` mode (default) ---

    #[test]
    fn flags_for_loop_in_each_mode() {
        test::<For>().expect_offense(indoc! {"
            for n in [1, 2, 3]
            ^^^^^^^^^^^^^^^^^^ Prefer `each` over `for`.
              puts n
            end
        "});
    }

    #[test]
    fn corrects_for_loop_to_each() {
        test::<For>().expect_correction(
            indoc! {"
                for n in [1, 2, 3]
                ^^^^^^^^^^^^^^^^^^ Prefer `each` over `for`.
                  puts n
                end
            "},
            "[1, 2, 3].each do |n|\n  puts n\nend\n",
        );
    }

    #[test]
    fn corrects_for_loop_with_range_to_each() {
        test::<For>().expect_correction(
            indoc! {"
                for n in 1..10
                ^^^^^^^^^^^^^^ Prefer `each` over `for`.
                  puts n
                end
            "},
            "(1..10).each do |n|\n  puts n\nend\n",
        );
    }

    #[test]
    fn corrects_for_loop_with_mlhs() {
        test::<For>().expect_correction(
            indoc! {"
                for a, b in [[1, 2], [3, 4]]
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `each` over `for`.
                  puts a
                end
            "},
            "[[1, 2], [3, 4]].each do |(a, b)|\n  puts a\nend\n",
        );
    }

    #[test]
    fn accepts_each_block_in_each_mode() {
        test::<For>().expect_no_offenses("[1, 2, 3].each do |n|\n  puts n\nend\n");
    }

    #[test]
    fn accepts_single_line_each_block() {
        test::<For>().expect_no_offenses("[1, 2, 3].each { |n| puts n }\n");
    }

    // --- `for` mode ---

    #[test]
    fn flags_each_block_in_for_mode() {
        test::<For>()
            .with_options(&ForOptions {
                enforced_style: ForStyle::ForKeyword,
            })
            .expect_offense(indoc! {"
                [1, 2, 3].each do |n|
                ^^^^^^^^^^^^^^^^^^^^^ Prefer `for` over `each`.
                  puts n
                end
            "});
    }

    #[test]
    fn corrects_each_to_for() {
        test::<For>()
            .with_options(&ForOptions {
                enforced_style: ForStyle::ForKeyword,
            })
            .expect_correction(
                indoc! {"
                    [1, 2, 3].each do |n|
                    ^^^^^^^^^^^^^^^^^^^^^ Prefer `for` over `each`.
                      puts n
                    end
                "},
                "for n in [1, 2, 3]\n  puts n\nend\n",
            );
    }

    #[test]
    fn accepts_for_loop_in_for_mode() {
        test::<For>()
            .with_options(&ForOptions {
                enforced_style: ForStyle::ForKeyword,
            })
            .expect_no_offenses("for n in [1, 2, 3]\n  puts n\nend\n");
    }

    #[test]
    fn skips_single_line_each_in_for_mode() {
        test::<For>()
            .with_options(&ForOptions {
                enforced_style: ForStyle::ForKeyword,
            })
            .expect_no_offenses("[1, 2, 3].each { |n| puts n }\n");
    }

    #[test]
    fn skips_each_with_send_args_in_for_mode() {
        test::<For>()
            .with_options(&ForOptions {
                enforced_style: ForStyle::ForKeyword,
            })
            .expect_no_offenses("[1, 2, 3].each_with_object([]) do |n, acc|\n  acc << n\nend\n");
    }
}
murphy_plugin_api::submit_cop!(For);
