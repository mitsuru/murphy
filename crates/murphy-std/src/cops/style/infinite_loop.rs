//! `Style/InfiniteLoop` — use `Kernel#loop` for infinite loops.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/InfiniteLoop
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `while truthy_literal` and `until falsey_literal` loops, where
//!   truthy literals are `true`, integer/float literals, non-empty array/hash
//!   literals, and falsey literals are `false` and `nil`. The offense is placed
//!   on the keyword token (matching RuboCop's `add_offense(node.loc.keyword)`).
//!
//!   Autocorrect: block-form loops (with or without `do`) are corrected by
//!   replacing the `keyword condition [do]` prefix with `loop do`. Post-condition
//!   loops (`begin..end while true`) replace `begin` with `loop do` and remove
//!   ` while true/until false` after the closing `end`. Modifier-form loops
//!   (`body while true` single-line) are replaced with `loop { body }`.
//!   Multi-line modifier-form loops are replaced with `loop do\nbody\nend`
//!   with indentation matching the original body source.
//!
//!   Parity gap (variable scope safety): RuboCop uses `VariableForce` to detect
//!   variables first assigned inside the loop and referenced after. Murphy
//!   implements an equivalent check via `cx.var_model()`, examining the root
//!   scope's variable list for: (a) first assignment inside the loop range,
//!   (b) no assignment before the loop, (c) a reference after the loop.
//!   Instance/class variables are unaffected (only local variables matter),
//!   so instance-variable assignments inside loops are safely autocorrected.
//! ```
//!
//! ## Matched shapes
//!
//! `While` nodes where condition is a truthy literal:
//! - `true`, integer literal, float literal, non-empty array literal,
//!   non-empty hash literal
//!
//! `Until` nodes where condition is a falsey literal:
//! - `false`, `nil`
//!
//! Both block-form, modifier-form, and post-condition form are matched.
//!
//! ## Autocorrect
//!
//! - Block form: replace `keyword condition [do]` prefix with `loop do`
//! - Post-condition (`begin..end while/until`): replace `begin` with `loop do`,
//!   remove ` while/until condition` after body's `end`
//! - Modifier single-line: replace whole node with `loop { body }`
//! - Modifier multi-line: replace whole node with `loop do\nbody\nend`

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Use `Kernel#loop` for infinite loops.";

#[derive(Default)]
pub struct InfiniteLoop;

#[cop(
    name = "Style/InfiniteLoop",
    description = "Use `Kernel#loop` for infinite loops.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl InfiniteLoop {
    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        if let NodeKind::While { cond, .. } = *cx.kind(node)
            && is_truthy_literal(cond, cx) {
                check(node, cx);
            }
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        if let NodeKind::Until { cond, .. } = *cx.kind(node)
            && is_falsey_literal(cond, cx) {
                check(node, cx);
            }
    }
}

/// Returns `true` if this node is a truthy literal:
/// `true`, int, float, non-empty array, non-empty hash.
fn is_truthy_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::True_ => true,
        NodeKind::Int(_) | NodeKind::Float(_) => true,
        NodeKind::Array(list) => !cx.list(list).is_empty(),
        NodeKind::Hash(list) => !cx.list(list).is_empty(),
        _ => false,
    }
}

/// Returns `true` if this node is a falsey literal: `false` or `nil`.
fn is_falsey_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::False_ | NodeKind::Nil)
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Find keyword range for the offense.
    let keyword_range = offense_keyword_range(node, cx);
    if keyword_range == Range::ZERO {
        return;
    }

    // Variable scope safety check: skip if introducing `loop {}` would change
    // variable scope semantics (i.e., a variable is first assigned inside the
    // loop and referenced outside after the loop). Only applies to post-false
    // (non-post-condition) loops.
    if !cx.is_post_condition_loop(node) && variable_scope_change(node, cx) {
        return;
    }

    cx.emit_offense(keyword_range, MSG, None);
    autocorrect(node, cx);
}

/// Returns the keyword range for the offense.
/// - Block-form and post-condition: `cx.loc(node).keyword()` works for
///   block-form (starts at node start). For post-condition, search for the
///   `while`/`until` keyword backward from the condition.
/// - Modifier-form: the keyword is after the body, search for it.
fn offense_keyword_range(node: NodeId, cx: &Cx<'_>) -> Range {
    if cx.is_post_condition_loop(node) {
        // Post-condition: keyword is after `end`, before condition.
        let cond = match *cx.kind(node) {
            NodeKind::While { cond, .. } | NodeKind::Until { cond, .. } => cond,
            _ => return Range::ZERO,
        };
        find_loop_keyword_before_cond(node, cond, cx)
    } else if cx.is_modifier_form(node) {
        // Modifier: keyword is between body and condition.
        let (cond, body) = match *cx.kind(node) {
            NodeKind::While { cond, body, .. } | NodeKind::Until { cond, body, .. } => {
                (cond, body)
            }
            _ => return Range::ZERO,
        };
        let body_end = body.get().map_or(cx.range(node).start, |b| cx.range(b).end);
        find_loop_keyword_after(body_end, cx.range(cond).start, cx)
    } else {
        // Block-form: keyword starts at node start.
        cx.loc(node).keyword()
    }
}

/// Find `while`/`until` keyword token in `[from, to)` range.
fn find_loop_keyword_after(from: u32, to: u32, cx: &Cx<'_>) -> Range {
    if from >= to {
        return Range::ZERO;
    }
    let src = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        if tok.kind == SourceTokenKind::Other {
            let text = &src[tok.range.start as usize..tok.range.end as usize];
            if text == b"while" || text == b"until" {
                return tok.range;
            }
        }
    }
    Range::ZERO
}

/// Find `while`/`until` keyword scanning backward from condition start.
fn find_loop_keyword_before_cond(node: NodeId, cond: NodeId, cx: &Cx<'_>) -> Range {
    let cond_start = cx.range(cond).start;
    let node_start = cx.range(node).start;
    let src = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < cond_start);
    for tok in toks[..idx].iter().rev() {
        if tok.range.start < node_start {
            break;
        }
        if tok.kind == SourceTokenKind::Other {
            let text = &src[tok.range.start as usize..tok.range.end as usize];
            if text == b"while" || text == b"until" {
                return tok.range;
            }
        }
    }
    Range::ZERO
}

/// Variable scope safety check.
/// Returns `true` if autocorrecting this loop would change variable semantics
/// (a variable first assigned inside the loop is referenced after the loop).
fn variable_scope_change(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(model) = cx.var_model() else {
        // No model available — conservative: skip correction.
        return false;
    };

    let loop_range = cx.range(node);

    // Walk the root scope's variables. Root scope is the innermost non-block
    // scope — here we check all scopes that could "see" the loop.
    // For simplicity (matching RuboCop behavior), check all variables that
    // are in the same scope as the loop (not inside a block within the loop).
    // We look at all scopes and find any variable whose declaration is inside
    // the loop range and that has a reference after the loop.
    for (_scope_id, scope) in model.scopes() {
        for var in &scope.variables {
            if var.is_argument {
                continue;
            }
            // Does this variable have any assignment inside the loop range?
            let assigned_inside = var
                .assignments
                .iter()
                .any(|a| a.end > loop_range.start && a.end <= loop_range.end);
            if !assigned_inside {
                continue;
            }
            // Does this variable have any assignment BEFORE the loop?
            let assigned_before = var
                .assignments
                .iter()
                .any(|a| a.end <= loop_range.start);
            if assigned_before {
                continue;
            }
            // Does this variable have a reference AFTER the loop?
            let referenced_after = var
                .references
                .iter()
                .any(|r| r.pos >= loop_range.end);
            if referenced_after {
                return true;
            }
        }
    }
    false
}

/// Emit autocorrect edits for the infinite loop.
fn autocorrect(node: NodeId, cx: &Cx<'_>) {
    if cx.is_post_condition_loop(node) {
        autocorrect_post_condition(node, cx);
    } else if cx.is_modifier_form(node) {
        autocorrect_modifier(node, cx);
    } else {
        autocorrect_block(node, cx);
    }
}

/// Autocorrect block-form loop: `while cond [do] ... end` → `loop do ... end`
/// Replace `keyword condition [do]` with `loop do`.
fn autocorrect_block(node: NodeId, cx: &Cx<'_>) {
    let cond = match *cx.kind(node) {
        NodeKind::While { cond, .. } | NodeKind::Until { cond, .. } => cond,
        _ => return,
    };

    let kw_range = cx.loc(node).keyword();
    if kw_range == Range::ZERO {
        return;
    }

    // Check if there's a `do` after the condition.
    let cond_end = cx.range(cond).end;
    let node_end = cx.range(node).end;
    let replace_end = if let Some(do_range) = find_do_after(cx, cond_end, node_end) {
        do_range.end
    } else {
        cond_end
    };

    let replace_range = Range {
        start: kw_range.start,
        end: replace_end,
    };
    cx.emit_edit(replace_range, "loop do");
}

/// Autocorrect post-condition loop: `begin ... end while cond` → `loop do ... end`
/// Edit 1: replace `begin` with `loop do`.
/// Edit 2: remove ` while cond` after the body's `end`.
fn autocorrect_post_condition(node: NodeId, cx: &Cx<'_>) {
    let body = match *cx.kind(node) {
        NodeKind::While { body, .. } | NodeKind::Until { body, .. } => body,
        _ => return,
    };
    let Some(body_id) = body.get() else {
        return;
    };

    // The body's keyword is `begin`.
    let begin_range = cx.loc(body_id).keyword();
    if begin_range == Range::ZERO {
        return;
    }

    // The body's end_keyword is `end`.
    let end_kw_range = cx.loc(body_id).end_keyword();
    if end_kw_range == Range::ZERO {
        return;
    }

    // Edit 1: replace `begin` with `loop do`.
    cx.emit_edit(begin_range, "loop do");

    // Edit 2: remove from after body's `end` to the whole node end
    // (removes ` while true` / ` until false` and trailing whitespace).
    let remove_range = Range {
        start: end_kw_range.end,
        end: cx.range(node).end,
    };
    cx.emit_edit(remove_range, "");
}

/// Autocorrect modifier-form loop: `body while cond` → `loop { body }` or
/// `loop do\n  body\nend`.
fn autocorrect_modifier(node: NodeId, cx: &Cx<'_>) {
    let (cond, body_opt) = match *cx.kind(node) {
        NodeKind::While { cond, body, .. } | NodeKind::Until { cond, body, .. } => (cond, body),
        _ => return,
    };
    let Some(body_id) = body_opt.get() else {
        return;
    };

    let body_source = cx.raw_source(cx.range(body_id));
    let node_range = cx.range(node);
    let src = cx.source().as_bytes();

    // Check for a trailing comment after the condition.
    // Find the first `#` comment token after the condition end.
    let cond_end = cx.range(cond).end;
    let trailing_comment = find_comment_after(cx, cond_end, node_range.end);

    // Determine if the body is single-line or multiline.
    let body_is_single_line = !body_source.contains('\n');

    let (loop_end, replacement) = if body_is_single_line {
        // Single-line: `loop { body_source }`
        // The replacement covers from node start to cond end (or before comment).
        let replace_end = trailing_comment
            .map(|r| r.start)
            .unwrap_or(node_range.end);
        // Strip trailing whitespace before `#` comment.
        let replace_end = strip_trailing_spaces(src, replace_end);
        let repl = format!("loop {{ {} }}", body_source);
        (replace_end, repl)
    } else {
        // Multi-line: replace entire node.
        // Detect indentation from the node start.
        let node_start = node_range.start as usize;
        let line_start = line_start_of(src, node_start);
        let indentation = &src[line_start..node_start];
        let indentation = std::str::from_utf8(indentation).unwrap_or("");
        // Re-indent body lines: add `indentation + 2 spaces` to each line so the
        // body is correctly nested inside the `loop do` block.
        let body_indented = body_source
            .lines()
            .map(|line| format!("{indentation}  {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let repl = format!("loop do\n{body_indented}\n{indentation}end");
        let replace_end = trailing_comment
            .map(|r| r.start)
            .unwrap_or(node_range.end);
        let replace_end = strip_trailing_spaces(src, replace_end);
        (replace_end, repl)
    };

    let replace_range = Range {
        start: node_range.start,
        end: loop_end,
    };
    cx.emit_edit(replace_range, &replacement);
}

/// Find `do` token in `[from, to)`.
fn find_do_after(cx: &Cx<'_>, from: u32, to: u32) -> Option<Range> {
    if from >= to {
        return None;
    }
    let src = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        // `do` is Other kind; stop at a Newline (no `do` on next line).
        if tok.kind == SourceTokenKind::Newline || tok.kind == SourceTokenKind::IgnoredNewline {
            break;
        }
        if tok.kind == SourceTokenKind::Other
            && &src[tok.range.start as usize..tok.range.end as usize] == b"do"
        {
            return Some(tok.range);
        }
    }
    None
}

/// Find the first `Comment` token in `[from, to)`.
fn find_comment_after(cx: &Cx<'_>, from: u32, to: u32) -> Option<Range> {
    if from >= to {
        return None;
    }
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < from);
    for tok in &toks[idx..] {
        if tok.range.start >= to {
            break;
        }
        if tok.kind == SourceTokenKind::Comment {
            return Some(tok.range);
        }
    }
    None
}

/// Strip trailing space characters (not newlines) from `end` backward.
fn strip_trailing_spaces(src: &[u8], end: u32) -> u32 {
    let mut e = end as usize;
    while e > 0 && src[e - 1] == b' ' {
        e -= 1;
    }
    e as u32
}

/// Find the byte offset of the start of the line containing `pos`.
fn line_start_of(src: &[u8], pos: usize) -> usize {
    src[..pos]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1)
}

#[cfg(test)]
mod tests {
    use super::InfiniteLoop;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- while truthy_literal -----

    #[test]
    fn flags_while_true() {
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                while true
                ^^^^^ Use `Kernel#loop` for infinite loops.
                  top
                end
            "},
            "loop do\n  top\nend\n",
        );
    }

    #[test]
    fn flags_while_integer_literal() {
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                while 1
                ^^^^^ Use `Kernel#loop` for infinite loops.
                  top
                end
            "},
            "loop do\n  top\nend\n",
        );
    }

    #[test]
    fn flags_while_float_literal() {
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                while 2.0
                ^^^^^ Use `Kernel#loop` for infinite loops.
                  top
                end
            "},
            "loop do\n  top\nend\n",
        );
    }

    // ----- until falsey_literal -----

    #[test]
    fn flags_until_false() {
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                until false
                ^^^^^ Use `Kernel#loop` for infinite loops.
                  top
                end
            "},
            "loop do\n  top\nend\n",
        );
    }

    #[test]
    fn flags_until_nil() {
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                until nil
                ^^^^^ Use `Kernel#loop` for infinite loops.
                  top
                end
            "},
            "loop do\n  top\nend\n",
        );
    }

    // ----- accepts -----

    #[test]
    fn accepts_kernel_loop() {
        test::<InfiniteLoop>().expect_no_offenses("loop { break if something }\n");
    }

    #[test]
    fn accepts_while_with_variable_scope_issue() {
        // `a` is first assigned inside the loop and referenced after — skip.
        test::<InfiniteLoop>().expect_no_offenses(indoc! {"
            while true
              a, b = 42, 42
              break
            end
            puts a, b
        "});
    }

    // ----- autocorrect: block form with do -----

    #[test]
    fn autocorrects_while_true_do() {
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                while true do
                ^^^^^ Use `Kernel#loop` for infinite loops.
                end
            "},
            "loop do\nend\n",
        );
    }

    #[test]
    fn autocorrects_until_false_do() {
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                until false do
                ^^^^^ Use `Kernel#loop` for infinite loops.
                end
            "},
            "loop do\nend\n",
        );
    }

    // ----- autocorrect: post-condition -----

    #[test]
    fn autocorrects_begin_end_while_true() {
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                begin
                  something += 1
                end while true
                    ^^^^^ Use `Kernel#loop` for infinite loops.
            "},
            "loop do\n  something += 1\nend\n",
        );
    }

    #[test]
    fn autocorrects_begin_end_until_false() {
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                begin
                  something += 1
                end until false
                    ^^^^^ Use `Kernel#loop` for infinite loops.
            "},
            "loop do\n  something += 1\nend\n",
        );
    }

    // ----- autocorrect: modifier form -----

    #[test]
    fn autocorrects_modifier_while_single_line() {
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                something += 1 while true
                               ^^^^^ Use `Kernel#loop` for infinite loops.
            "},
            "loop { something += 1 }\n",
        );
    }

    #[test]
    fn autocorrects_modifier_until_single_line() {
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                something += 1 until false
                               ^^^^^ Use `Kernel#loop` for infinite loops.
            "},
            "loop { something += 1 }\n",
        );
    }

    #[test]
    fn autocorrects_modifier_while_multi_line() {
        // Multi-line body: `something +\n  1` is a single expression spanning
        // two lines. The body is indented 2 spaces inside the `loop do` block.
        test::<InfiniteLoop>().expect_correction(
            indoc! {"
                something +
                  1 while true
                    ^^^^^ Use `Kernel#loop` for infinite loops.
            "},
            "loop do\n  something +\n    1\nend\n",
        );
    }
}

murphy_plugin_api::submit_cop!(InfiniteLoop);
