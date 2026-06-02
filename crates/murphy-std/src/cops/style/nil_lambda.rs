//! `Style/NilLambda` — flags lambdas and procs that always return nil,
//! suggesting an empty lambda or proc instead.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NilLambda
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Disabled by default (matches RuboCop's `Enabled: pending` in default.yml).
//!   Only handles `Block` nodes; `Numblock` and `Itblock` are skipped (consistent
//!   with RuboCop's own `rubocop:disable InternalAffairs/NumblockHandler` comment).
//!   All four proc spellings are detected:
//!     - `-> { nil }` (stabby lambda)
//!     - `lambda { nil }` (method form)
//!     - `proc { nil }` (proc method)
//!     - `Proc.new { nil }` (Proc.new)
//!   Body patterns that trigger the cop:
//!     - bare `nil`
//!     - `return nil`
//!     - `next nil`
//!     - `break nil`
//!   The offense range covers the full block for single-line blocks, or just
//!   the opening line for multiline blocks (matching Murphy's test annotation
//!   convention).
//!   Autocorrect removes the body (with surrounding space for single-line blocks,
//!   whole lines for multiline blocks), leaving an empty block.
//! ```
//!
//! ## Matched shapes
//!
//! `Block` nodes that are lambdas or procs whose body is `nil`, `return nil`,
//! `next nil`, or `break nil`.
//!
//! ## Examples
//!
//! ```ruby
//! # bad
//! -> { nil }
//! lambda { return nil }
//! proc { nil }
//! Proc.new { next nil }
//!
//! # good
//! -> {}
//! lambda {}
//! proc {}
//! Proc.new {}
//!
//! # not flagged (conditional, may not always return nil)
//! -> (x) { nil if x }
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, SpaceRangeOptions, cop};

const MSG_LAMBDA: &str = "Use an empty lambda instead of always returning nil.";
const MSG_PROC: &str = "Use an empty proc instead of always returning nil.";

/// Stateless unit struct.
#[derive(Default)]
pub struct NilLambda;

#[cop(
    name = "Style/NilLambda",
    description = "Prefer empty lambdas/procs over those that always return nil.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl NilLambda {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { body, .. } = *cx.kind(node) else {
        return;
    };

    // Must be a lambda or proc.
    let is_lambda = cx.is_lambda(node);
    let is_proc = !is_lambda && is_proc_node(node, cx);
    if !is_lambda && !is_proc {
        return;
    }

    // Body must be present and must be a nil-returning pattern.
    let Some(body_id) = body.get() else {
        return;
    };
    if !is_nil_return(body_id, cx) {
        return;
    }

    let msg = if is_lambda { MSG_LAMBDA } else { MSG_PROC };

    // Offense range: the full block for single-line; the opening line for
    // multiline (Murphy's test annotation convention cannot represent ranges
    // that span multiple lines).
    let node_range = cx.range(node);
    let offense_range = first_line_range_of(node_range, cx.source().as_bytes());
    cx.emit_offense(offense_range, msg, None);

    // Autocorrect: remove the body.
    let body_range = cx.range(body_id);
    let delete_range = if cx.is_single_line(node) {
        // Single-line: expand through surrounding whitespace (spaces/tabs only,
        // not newlines) — turns `lambda { nil }` into `lambda {}`.
        cx.range_with_surrounding_space(
            body_range,
            SpaceRangeOptions {
                newlines: false,
                ..SpaceRangeOptions::default()
            },
        )
    } else {
        // Multiline: remove the entire body line(s) including the trailing newline.
        cx.range_by_whole_lines(body_range, true)
    };
    cx.emit_edit(delete_range, "");
}

/// Returns `true` if `node` is a proc block:
/// - `proc { }` — receiverless send with method `proc`
/// - `Proc.new { }` — send with method `new` on const `Proc`
fn is_proc_node(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(node) else {
        return false;
    };

    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return false;
    };

    let method_name = cx.symbol_str(method);

    // `proc { }` — receiverless call to `proc`
    if receiver.get().is_none() && method_name == "proc" {
        return true;
    }

    // `Proc.new { }` — call to `new` on const `Proc` (with or without explicit cbase)
    if method_name == "new" {
        if let Some(recv_id) = receiver.get() {
            if let NodeKind::Const { name, scope } = *cx.kind(recv_id) {
                let const_name = cx.symbol_str(name);
                // Match `Proc` and `::Proc`
                if const_name == "Proc" {
                    let scope_is_none_or_cbase = match scope.get() {
                        None => true,
                        Some(scope_id) => matches!(cx.kind(scope_id), NodeKind::Cbase),
                    };
                    if scope_is_none_or_cbase {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Returns `true` if the body node is a nil-returning pattern:
/// - bare `nil`
/// - `return nil`
/// - `next nil`
/// - `break nil`
fn is_nil_return(body: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(body) {
        NodeKind::Nil => true,
        NodeKind::Return(val) | NodeKind::Next(val) | NodeKind::Break(val) => {
            is_nil_value(val, cx)
        }
        _ => false,
    }
}

/// Returns `true` if `opt` is present and its value is a `nil` literal.
fn is_nil_value(opt: OptNodeId, cx: &Cx<'_>) -> bool {
    opt.get().is_some_and(|id| matches!(cx.kind(id), NodeKind::Nil))
}

/// Returns the range from `range.start` to the first newline (exclusive),
/// or `range` itself if the range is entirely on one line.
///
/// This is used to produce an offense range that fits within one source line,
/// which the Murphy test annotation format requires.
fn first_line_range_of(range: Range, source: &[u8]) -> Range {
    let end = source[range.start as usize..range.end as usize]
        .iter()
        .position(|&b| b == b'\n')
        .map(|p| range.start + p as u32)
        .unwrap_or(range.end);
    Range {
        start: range.start,
        end,
    }
}

#[cfg(test)]
mod tests {
    use super::NilLambda;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- lambda forms -----

    #[test]
    fn flags_stabby_lambda_nil() {
        test::<NilLambda>().expect_correction(
            indoc! {"
                -> { nil }
                ^^^^^^^^^^ Use an empty lambda instead of always returning nil.
            "},
            "-> {}\n",
        );
    }

    #[test]
    fn flags_lambda_method_nil() {
        test::<NilLambda>().expect_correction(
            indoc! {"
                lambda { nil }
                ^^^^^^^^^^^^^^ Use an empty lambda instead of always returning nil.
            "},
            "lambda {}\n",
        );
    }

    #[test]
    fn flags_lambda_return_nil() {
        test::<NilLambda>().expect_correction(
            indoc! {"
                lambda { return nil }
                ^^^^^^^^^^^^^^^^^^^^^ Use an empty lambda instead of always returning nil.
            "},
            "lambda {}\n",
        );
    }

    #[test]
    fn flags_lambda_multiline_next_nil() {
        test::<NilLambda>().expect_correction(
            indoc! {"
                lambda do
                ^^^^^^^^^ Use an empty lambda instead of always returning nil.
                  next nil
                end
            "},
            indoc! {"
                lambda do
                end
            "},
        );
    }

    #[test]
    fn flags_lambda_multiline_nil() {
        test::<NilLambda>().expect_correction(
            indoc! {"
                lambda do
                ^^^^^^^^^ Use an empty lambda instead of always returning nil.
                  nil
                end
            "},
            indoc! {"
                lambda do
                end
            "},
        );
    }

    // ----- proc forms -----

    #[test]
    fn flags_proc_nil() {
        test::<NilLambda>().expect_correction(
            indoc! {"
                proc { nil }
                ^^^^^^^^^^^^ Use an empty proc instead of always returning nil.
            "},
            "proc {}\n",
        );
    }

    #[test]
    fn flags_proc_new_nil() {
        test::<NilLambda>().expect_correction(
            indoc! {"
                Proc.new { nil }
                ^^^^^^^^^^^^^^^^ Use an empty proc instead of always returning nil.
            "},
            "Proc.new {}\n",
        );
    }

    #[test]
    fn flags_proc_break_nil() {
        test::<NilLambda>().expect_correction(
            indoc! {"
                proc { break nil }
                ^^^^^^^^^^^^^^^^^^ Use an empty proc instead of always returning nil.
            "},
            "proc {}\n",
        );
    }

    #[test]
    fn flags_proc_new_multiline_break_nil() {
        test::<NilLambda>().expect_correction(
            indoc! {"
                Proc.new do
                ^^^^^^^^^^^ Use an empty proc instead of always returning nil.
                  break nil
                end
            "},
            indoc! {"
                Proc.new do
                end
            "},
        );
    }

    // ----- negative cases -----

    #[test]
    fn accepts_empty_lambda() {
        test::<NilLambda>().expect_no_offenses("-> {}\n");
    }

    #[test]
    fn accepts_lambda_with_non_nil_body() {
        test::<NilLambda>().expect_no_offenses("-> { 1 }\n");
    }

    #[test]
    fn accepts_conditional_nil() {
        // Conditional nil — may not always return nil.
        test::<NilLambda>().expect_no_offenses("-> (x) { nil if x }\n");
    }

    #[test]
    fn accepts_lambda_return_without_nil() {
        // `return` (bare) — not `return nil`.
        test::<NilLambda>().expect_no_offenses("lambda { return }\n");
    }

    #[test]
    fn accepts_plain_block_nil() {
        // Ordinary block — not a lambda or proc.
        test::<NilLambda>().expect_no_offenses("[1, 2].each { nil }\n");
    }

    #[test]
    fn accepts_proc_next_without_nil() {
        // `next` with a non-nil argument.
        test::<NilLambda>().expect_no_offenses("proc { next 1 }\n");
    }

    // ----- idempotency -----

    #[test]
    fn corrected_empty_lambda_is_idempotent() {
        test::<NilLambda>().expect_no_offenses("-> {}\n");
    }

    #[test]
    fn corrected_empty_proc_is_idempotent() {
        test::<NilLambda>().expect_no_offenses("proc {}\n");
    }
}

murphy_plugin_api::submit_cop!(NilLambda);
