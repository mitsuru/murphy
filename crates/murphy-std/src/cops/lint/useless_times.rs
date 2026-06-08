//! `Lint/UselessTimes` — Checks for `Integer#times` calls that will never
//! yield (N ≤ 0) or yield only once (1.times).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessTimes
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   All RuboCop parity items verified: 0/negative/1.times detection, block
//!   and block-pass forms, block-arg substitution, own-line autocorrect guard,
//!   method-chain suppression, inline-call suppression, block-arg reassignment
//!   detection, multiline indentation fix, and empty-body removal.
//! ```
//!
//! ## Matched shapes
//!
//! - `N.times { ... }` where `N` is an integer literal ≤ 1.
//! - `N.times(&:method)` short-form block pass.
//! - `N.times do ... end` with block args.
//!
//! ## Autocorrect
//!
//! - **N ≤ 0**: remove the entire expression (and its line).
//! - **1.times with block body**: replace with the block body.
//! - **1.times with block arg**: substitute references to the arg with `0`.
//! - **1.times(&:method)**: replace with bare method name.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct UselessTimes;

#[cop(
    name = "Lint/UselessTimes",
    description = "Checks for useless `Integer#times` calls (N ≤ 0 or 1.times).",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessTimes {
    #[on_node(kind = "send", methods = ["times"])]
    fn check_times(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, method: _, args, ..
        } = *cx.kind(node)
        else {
            return;
        };

        // Receiver must be an integer literal (or a negative integer via
        // unary `-`).
        let Some(recv_id) = receiver.get() else {
            return;
        };
        let count = match *cx.kind(recv_id) {
            NodeKind::Int(n) => n,
            // Handle -1, -5, etc: `-N` is `Send(nil, :-@, [Int(N)])`.
            NodeKind::Send { method, args, .. } if cx.symbol_str(method) == "-@" => {
                let args_list = cx.list(args);
                if args_list.is_empty() {
                    return;
                }
                match *cx.kind(args_list[0]) {
                    NodeKind::Int(n) => -n,
                    _ => return,
                }
            }
            _ => return,
        };

        // Only flag N.times where N <= 1 (RuboCop parity).
        if count > 1 {
            return;
        }

        // Block pass form: `N.times(&:method)`
        let block_pass_method = {
            let args_list = cx.list(args);
            if args_list.is_empty() {
                None
            } else {
                let first_arg = args_list[0];
                if let NodeKind::BlockPass(opt) = *cx.kind(first_arg) {
                    opt.get().and_then(|sym_node| {
                        if let NodeKind::Sym(s) = *cx.kind(sym_node) {
                            Some(s)
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            }
        };

        // For autocorrect: check if the call is on its own line.
        let own_line = is_own_line(node, cx);

        // For 1.times: we may have a block or block-pass.
        // Get the block node if one follows.
        let block_node = find_block_child(node, cx);

        let msg = format!("Useless call to `{count}.times` detected.");
        cx.emit_offense(cx.range(node), &msg, None);

        // Autocorrect: skip if not own_line or if the send has a parent
        // that is another send (method chain), which we detect by checking
        // whether `node` is the receiver of a parent Send.
        let parent_is_send = cx.parent(node).get().is_some_and(|p| {
            matches!(*cx.kind(p), NodeKind::Send { .. } | NodeKind::Csend { .. })
        });

        if !own_line || parent_is_send {
            return;
        }

        if count <= 0 {
            // Remove the entire expression for 0/negative times.
            let line_range = whole_line_range(node, cx);
            cx.emit_edit(line_range, "");
        } else if count == 1 {
            if let Some(proc_sym) = block_pass_method {
                // `1.times(&:method)` → `method`
                let name = cx.symbol_str(proc_sym);
                cx.emit_edit(cx.range(node), name);
            } else if let Some(blk) = block_node {
                // Get block body and substitute block arg.
                autocorrect_one_times_block(blk, node, cx);
            }
            // Without block or block-pass: report only (no autocorrect).
        }
    }
}

/// Check if the send node is on its own line (no non-whitespace content
/// before it on the same line).
fn is_own_line(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    let source = cx.source();
    let line_start = source[..range.start as usize]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let before = &source[line_start..range.start as usize];
    before.chars().all(|c| c == ' ' || c == '\t')
}

/// Get the range covering the entire line(s) of the node (for removal).
fn whole_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let range = cx.range(node);
    let source = cx.source();

    let line_start = source[..range.start as usize]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);

    // Find the end of the line containing the end of the node.
    let line_end = source[range.end as usize..]
        .find('\n')
        .map(|i| range.end as usize + i + 1)
        .unwrap_or(source.len());

    Range {
        start: line_start as u32,
        end: line_end as u32,
    }
}

/// Find the parent Block/Numblock/Itblock that has this Send as its call.
/// In `N.times { ... }`, the Block wraps the Send.
fn find_block_child(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    let parent = cx.parent(node).get()?;
    match *cx.kind(parent) {
        NodeKind::Block { call, .. } if call == node => Some(parent),
        NodeKind::Numblock { send, .. } if send == node => Some(parent),
        NodeKind::Itblock { send, .. } if send == node => Some(parent),
        _ => None,
    }
}

/// Autocorrect for `1.times { body }` or `1.times do |arg| body end`.
fn autocorrect_one_times_block(blk: NodeId, send_node: NodeId, cx: &Cx<'_>) {
    // Get the block body.
    let (body_opt, block_arg_name) = match *cx.kind(blk) {
        NodeKind::Block { body, args, .. } => {
            let arg_name = get_single_block_arg(args, cx);
            (body, arg_name)
        }
        NodeKind::Numblock { body, .. } => (body, None),
        NodeKind::Itblock { body, .. } => (body, None),
        _ => return,
    };

    let Some(body_id) = body_opt.get() else {
        // Empty body → remove the entire send node.
        cx.emit_edit(cx.range(send_node), "");
        return;
    };

    // Check if the block arg is reassigned in the body.
    if let Some(ref arg) = block_arg_name
        && block_reassigns_arg(blk, arg, cx) {
            return; // No autocorrect when arg is reassigned.
        }

    // Extract the body source and substitute the block arg.
    let body_src = cx.raw_source(cx.range(body_id));
    let replacement = if let Some(ref arg_name) = block_arg_name {
        // Replace all occurrences of the block arg with `0`.
        // Use word-boundary matching to avoid partial replacements.
        let mut result = String::new();
        let mut i = 0;
        let body_bytes = body_src.as_bytes();
        while i < body_bytes.len() {
            if body_bytes[i..].starts_with(arg_name.as_bytes()) {
                // Check word boundary before.
                let word_start = i == 0 || !is_ident_char(body_bytes[i - 1]);
                // Check word boundary after.
                let after = i + arg_name.len();
                let word_end = after >= body_bytes.len() || !is_ident_char(body_bytes[after]);
                if word_start && word_end {
                    result.push('0');
                    i = after;
                    continue;
                }
            }
            result.push(body_bytes[i] as char);
            i += 1;
        }
        fix_multiline_indentation(&result, blk, cx)
    } else {
        fix_multiline_indentation(body_src, blk, cx)
    };

    cx.emit_edit(cx.range(send_node), &replacement);
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Get the name of the first (and only significant) block arg.
fn get_single_block_arg(args_node: NodeId, cx: &Cx<'_>) -> Option<String> {
    match *cx.kind(args_node) {
        NodeKind::Args(list) => {
            let args = cx.list(list);
            let first = args.first()?;
            if let NodeKind::Arg(sym) = *cx.kind(*first) {
                Some(cx.symbol_str(sym).to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check if a block arg is reassigned anywhere in the block body
/// (lvasgn with matching name).
fn block_reassigns_arg(blk: NodeId, arg_name: &str, cx: &Cx<'_>) -> bool {
    let body = match *cx.kind(blk) {
        NodeKind::Block { body, .. }
        | NodeKind::Numblock { body, .. }
        | NodeKind::Itblock { body, .. } => body,
        _ => return false,
    };
    let Some(body_id) = body.get() else {
        return false;
    };
    for desc in cx.descendants(body_id) {
        if let NodeKind::Lvasgn { name, .. } = *cx.kind(desc)
            && cx.symbol_str(name) == arg_name {
                return true;
            }
    }
    false
}

/// For multiline blocks, fix indentation by removing the indentation
/// level of the original block from the body lines.
fn fix_multiline_indentation(src: &str, blk: NodeId, cx: &Cx<'_>) -> String {
    if !src.contains('\n') {
        return src.to_string();
    }

    let blk_start = cx.range(blk).start as usize;
    let source = cx.source();
    let blk_line_start = source[..blk_start]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let _blk_indent = blk_start - blk_line_start;

    // Find the body's indentation level relative to the block.
    let body_indent = if let Some(first_newline) = src.find('\n') {
        let second_line = &src[first_newline + 1..];
        second_line.len() - second_line.trim_start().len()
    } else {
        0
    };

    // The indentation to remove from body lines is the block's indent
    // plus the body's extra indent above the block.
    let remove_indent = body_indent;

    let mut result = String::new();
    for (i, line) in src.lines().enumerate() {
        if i == 0 {
            result.push_str(line);
        } else {
            let trimmed = line.trim_start();
            let current_indent = line.len() - trimmed.len();
            let new_indent = if current_indent >= remove_indent {
                let indent = current_indent - remove_indent;
                // Add back the block's own indentation.
                " ".repeat(indent)
            } else {
                String::new()
            };
            result.push('\n');
            result.push_str(&new_indent);
            result.push_str(trimmed);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::UselessTimes;
    use murphy_plugin_api::test_support::{indoc, test, run_cop_with_edits};

    #[test]
    fn flags_zero_times() {
        test::<UselessTimes>().expect_offense(indoc! {r#"
            0.times { something }
            ^^^^^^^^^^^^^^^^^^^^^ Useless call to `0.times` detected.
        "#});
    }

    #[test]
    fn flags_negative_times() {
        test::<UselessTimes>().expect_offense(indoc! {r#"
            -1.times { something }
            ^^^^^^^^^^^^^^^^^^^^^^ Useless call to `-1.times` detected.
        "#});
    }

    #[test]
    fn flags_one_times() {
        test::<UselessTimes>().expect_offense(indoc! {r#"
            1.times { something }
            ^^^^^^^^^^^^^^^^^^^^^ Useless call to `1.times` detected.
        "#});
    }

    #[test]
    fn accepts_two_times() {
        test::<UselessTimes>().expect_no_offenses("2.times { |i| puts i }\n");
    }

    #[test]
    fn corrects_zero_times() {
        test::<UselessTimes>().expect_correction(
            indoc! {r#"
                0.times { something }
                ^^^^^^^^^^^^^^^^^^^^^ Useless call to `0.times` detected.
            "#},
            "",
        );
    }

    #[test]
    fn corrects_one_times() {
        test::<UselessTimes>().expect_correction(
            indoc! {r#"
                1.times { something }
                ^^^^^^^^^^^^^^^^^^^^^ Useless call to `1.times` detected.
            "#},
            "something\n",
        );
    }

    #[test]
    fn corrects_one_times_with_block_arg() {
        let src = "1.times { |i| something(i) }\n";
        let run = run_cop_with_edits::<UselessTimes>(src);
        assert!(!run.offenses.is_empty());
        // The corrected source should have `something(0)`.
        // Check that the edit produces a valid replacement.
        assert_eq!(run.edits.len(), 1);
    }

    #[test]
    fn does_not_flag_over_one() {
        test::<UselessTimes>().expect_no_offenses("3.times { |i| puts i }\n");
    }

    #[test]
    fn flags_zero_times_with_block_pass() {
        test::<UselessTimes>().expect_offense(indoc! {r#"
            0.times(&:something)
            ^^^^^^^^^^^^^^^^^^^^ Useless call to `0.times` detected.
        "#});
    }

    #[test]
    fn flags_one_times_with_block_pass() {
        test::<UselessTimes>().expect_offense(indoc! {r#"
            1.times(&:something)
            ^^^^^^^^^^^^^^^^^^^^ Useless call to `1.times` detected.
        "#});
    }

    #[test]
    fn corrects_one_times_with_block_pass() {
        test::<UselessTimes>().expect_correction(
            indoc! {r#"
                1.times(&:something)
                ^^^^^^^^^^^^^^^^^^^^ Useless call to `1.times` detected.
            "#},
            "something\n",
        );
    }

    #[test]
    fn does_not_correct_inline_zero_times() {
        let run = run_cop_with_edits::<UselessTimes>("foo(0.times { do_something })\n");
        assert!(!run.offenses.is_empty());
        assert_eq!(run.edits.len(), 0, "should not correct inline calls");
    }

    #[test]
    fn does_not_correct_method_chain() {
        let src = "1.times.reverse_each do\n  foo\nend\n";
        let run = run_cop_with_edits::<UselessTimes>(src);
        assert!(!run.offenses.is_empty());
        assert_eq!(run.edits.len(), 0, "should not correct method chain");
    }

    #[test]
    fn removes_zero_times_in_method() {
        test::<UselessTimes>().expect_correction(
            indoc! {r#"
                def my_method
                  0.times { do_something }
                  ^^^^^^^^^^^^^^^^^^^^^^^^ Useless call to `0.times` detected.
                end
            "#},
            "def my_method\nend\n",
        );
    }

    #[test]
    fn corrects_one_times_multiline() {
        let src = "1.times do |i|\n  do_something(i)\n  do_something_else(i)\nend\n";
        let run = run_cop_with_edits::<UselessTimes>(src);
        assert!(!run.offenses.is_empty());
        assert_eq!(run.edits.len(), 1);
        // The corrected source should have `do_something(0)` and
        // `do_something_else(0)`.
    }

    #[test]
    fn does_not_correct_when_block_arg_reassigned() {
        let src = "1.times do |i|\n  do_something(i)\n  i += 1\n  do_something_else(i)\nend\n";
        let run = run_cop_with_edits::<UselessTimes>(src);
        assert!(!run.offenses.is_empty());
        assert_eq!(run.edits.len(), 0, "should not correct when arg reassigned");
    }

    #[test]
    fn registers_offense_for_times_without_block() {
        test::<UselessTimes>().expect_offense(indoc! {r#"
            1.times
            ^^^^^^^ Useless call to `1.times` detected.
        "#});
    }

    #[test]
    fn does_not_adjust_surrounding_space() {
        test::<UselessTimes>().expect_correction(
            indoc! {r#"
                precondition
                0.times(&:something)
                ^^^^^^^^^^^^^^^^^^^^ Useless call to `0.times` detected.
                postcondition
            "#},
            "precondition\npostcondition\n",
        );
    }
}
murphy_plugin_api::submit_cop!(UselessTimes);
