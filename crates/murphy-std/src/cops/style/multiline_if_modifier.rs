//! `Style/MultilineIfModifier` — flags modifier-form `if`/`unless` when the
//! body spans multiple lines.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/MultilineIfModifier
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects modifier-form if/unless expressions (e.g. `body if cond`) where
//!   the body is multiline. Autocorrects to normal block form
//!   (`if cond\n  body\nend`). When nested modifier-form nodes both qualify,
//!   only the outermost emits an autocorrect edit to avoid overlapping edits
//!   (RuboCop uses ignore_node/part_of_ignored_node? for the same purpose).
//!   Gaps vs RuboCop: body indentation when the node is at non-zero column is
//!   handled with a best-effort approach using the raw body source.
//! ```
//!
//! ## Matched shapes
//!
//! Modifier-form `if`/`unless` nodes (`is_modifier_form?`) where:
//! - The body node is multiline (`body.multiline?`)
//!
//! ## Autocorrect
//!
//! Rewrites `body\nif_or_unless cond` → `if_or_unless cond\n  body\nend`,
//! re-indenting the body lines to be inside the block. This is a structural
//! rearrangement (whole-node replacement). When a node is nested inside
//! another flagged modifier-form node, the autocorrect edit is suppressed
//! on the inner node to avoid overlapping edits; the outer node's correction
//! subsumes the inner.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str =
    "Favor a normal `%s`-statement over a modifier clause in a multiline statement.";

#[derive(Default)]
pub struct MultilineIfModifier;

#[cop(
    name = "Style/MultilineIfModifier",
    description = "Favor a normal if/unless statement over a modifier clause in a multiline statement.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl MultilineIfModifier {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Must be modifier-form (e.g., `body if cond`, not `if cond; body; end`).
    if !cx.is_modifier_form(node) {
        return;
    }
    // Skip ternary.
    if cx.is_ternary(node) {
        return;
    }

    let keyword = cx.if_keyword(node);
    if keyword.is_empty() {
        return;
    }

    // Determine the body node based on keyword.
    // - `body if cond`     → then_ = Some(body), else_ = None
    // - `body unless cond` → then_ = None, else_ = Some(body)
    let body_opt = if keyword == "unless" {
        cx.if_else_branch(node)
    } else {
        cx.if_then_branch(node)
    };

    let Some(body) = body_opt.get() else {
        return;
    };

    // The body must be multiline.
    if !cx.is_multiline(body) {
        return;
    }

    // Emit offense on the keyword location.
    let node_range = cx.range(node);
    let keyword_loc = cx.if_keyword_loc(node);
    let offense_range = if keyword_loc != Range::ZERO {
        keyword_loc
    } else {
        node_range
    };

    let message = MSG.replacen("%s", keyword, 1);
    cx.emit_offense(offense_range, &message, None);

    // Autocorrect: rewrite to block form.
    // Skip when nested inside another flagged modifier-form if/unless to
    // avoid overlapping edits. (RuboCop uses ignore_node/part_of_ignored_node?
    // for the same purpose.) The outer node's correction subsumes the inner.
    let has_flagged_ancestor = cx.ancestors(node).any(|anc| {
        if !cx.is_modifier_form(anc) || cx.is_ternary(anc) {
            return false;
        }
        let anc_body_opt = if cx.if_keyword(anc) == "unless" {
            cx.if_else_branch(anc)
        } else {
            cx.if_then_branch(anc)
        };
        anc_body_opt.get().is_some_and(|b| cx.is_multiline(b))
    });
    if has_flagged_ancestor {
        return;
    }

    let NodeKind::If { cond, .. } = *cx.kind(node) else {
        return;
    };
    let replacement = to_normal_if(keyword, cond, body, node, cx);
    cx.emit_edit(node_range, &replacement);
}

/// Builds the block-form replacement string from a modifier-form `if`/`unless`.
///
/// Input:  `<body_src>\n<keyword> <cond_src>` (modifier form)
/// Output: `<keyword> <cond_src>\n<indented_body>\nend`
fn to_normal_if(keyword: &str, cond: NodeId, body: NodeId, node: NodeId, cx: &Cx<'_>) -> String {
    let cond_src = cx.raw_source(cx.range(cond));
    let body_src = cx.raw_source(cx.range(body));
    let node_range = cx.range(node);
    let source = cx.source();
    let source_bytes = source.as_bytes();

    // Compute the leading whitespace of the node on its starting line.
    let start = node_range.start as usize;
    let line_start = source_bytes[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    let node_indent = &source[line_start..start];
    let body_indent = format!("{node_indent}  ");

    // Re-indent the body: each non-blank line gets `body_indent` prepended.
    // The body_src itself may already have its own indentation; we prepend
    // `node_indent` (matching RuboCop's `"#{offset(node)}#{body.source}"`)
    // and then replace the leading `node_indent` prefix with `body_indent`.
    let normalized_first_line = format!("{node_indent}{body_src}");
    let indented_body = normalized_first_line
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else if let Some(stripped) = line.strip_prefix(node_indent) {
                format!("{body_indent}{stripped}")
            } else {
                // Line has less indentation than the node itself — keep as-is
                // with `body_indent` prefix (shouldn't happen in well-indented
                // Ruby, but be safe).
                format!("{body_indent}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let condition_line = format!("{keyword} {cond_src}");
    let end_line = format!("{node_indent}end");

    format!("{condition_line}\n{indented_body}\n{end_line}")
}

#[cfg(test)]
mod tests {
    use super::MultilineIfModifier;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Flagged cases -----

    #[test]
    fn flags_multiline_body_with_modifier_unless() {
        test::<MultilineIfModifier>().expect_correction(
            indoc! {"
                {
                  result: 'this should not happen'
                } unless cond
                  ^^^^^^ Favor a normal `unless`-statement over a modifier clause in a multiline statement.
            "},
            "unless cond\n  {\n    result: 'this should not happen'\n  }\nend\n",
        );
    }

    #[test]
    fn flags_multiline_body_with_modifier_if() {
        test::<MultilineIfModifier>().expect_correction(
            indoc! {"
                foo
                  .bar if condition
                       ^^ Favor a normal `if`-statement over a modifier clause in a multiline statement.
            "},
            "if condition\n  foo\n    .bar\nend\n",
        );
    }

    #[test]
    fn nested_modifier_ifs_both_offenses_no_overlapping_edits() {
        // Both the outer and inner modifier-if have multiline bodies.
        // Both offenses are reported; only the outer edit is emitted to
        // avoid overlapping edits. `if cond1` is the innermost (body = `{...}`);
        // `if cond2` is the outermost (body = `{...} if cond1`).
        // The outer `if cond2` offense is at col 11; the inner `if cond1`
        // at col 2 on the `} if cond1 if cond2` line.
        test::<MultilineIfModifier>().expect_offense(indoc! {"
            {
              result: 'bad'
            } if cond1 if cond2
              ^^ Favor a normal `if`-statement over a modifier clause in a multiline statement.
                       ^^ Favor a normal `if`-statement over a modifier clause in a multiline statement.
        "});
    }

    // ----- Allowed cases -----

    #[test]
    fn accepts_single_line_modifier_if() {
        test::<MultilineIfModifier>().expect_no_offenses("do_something if condition\n");
    }

    #[test]
    fn accepts_single_line_modifier_unless() {
        test::<MultilineIfModifier>().expect_no_offenses("do_something unless condition\n");
    }

    #[test]
    fn accepts_block_form_if() {
        test::<MultilineIfModifier>().expect_no_offenses(indoc! {"
            if condition
              do_something
            end
        "});
    }

    #[test]
    fn accepts_ternary() {
        test::<MultilineIfModifier>().expect_no_offenses("x ? y : z\n");
    }
}

murphy_plugin_api::submit_cop!(MultilineIfModifier);
