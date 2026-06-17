//! `Lint/UnreachableLoop` — Checks for loops that will have at most one
//! iteration because all paths through the body lead to a flow-terminating
//! statement.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnreachableLoop
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   while/until/for loops whose body always terminates (break / return /
//!   raise / exit / etc.) are flagged.  if-else where both branches
//!   terminate, and continue-statement guards (RuboCop's
//!   CONTINUE_KEYWORDS = next / redo) are respected. `retry` is NOT a
//!   continue keyword: it restarts the enclosing begin/rescue, not the
//!   loop, so a loop body of `begin; ...; rescue; retry; end; break` is
//!   still flagged. Enumerable block methods (on_block) and AllowedPatterns
//!   are not implemented; redo is not yet emitted by the translator.
//! ```
//!
//! ## Matched shapes
//!
//! - `while cond; body; break; end` — break unconditionally reached.
//! - `while cond; if x; break; else; raise; end; end` — both branches terminate.
//! - `for x in y; return; end` — return always fires.
//!
//! ## No autocorrect
//!
//! Determining the correct fix (refactoring away the loop) is a semantic
//! change that cannot be automated safely.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

const MSG: &str = "This loop will have at most one iteration.";

const FLOW_METHODS: &[&str] = &["raise", "fail", "throw", "exit", "exit!", "abort"];

#[derive(Default)]
pub struct UnreachableLoop;

#[cop(
    name = "Lint/UnreachableLoop",
    description = "Checks for loops that will have at most one iteration.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UnreachableLoop {
    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::While { body, .. } = *cx.kind(node) else { return; };
        self.check_loop(node, body, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Until { body, .. } = *cx.kind(node) else { return; };
        self.check_loop(node, body, cx);
    }

    #[on_node(kind = "for")]
    fn check_for(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::For { body, .. } = *cx.kind(node) else { return; };
        self.check_loop(node, body, cx);
    }
}

impl UnreachableLoop {
    fn check_loop(&self, node: NodeId, body: OptNodeId, cx: &Cx<'_>) {
        let Some(body_id) = body.get() else { return; };
        let statements = flat_statements(body_id, cx);

        let mut break_idx = None;
        for (i, &s) in statements.iter().enumerate() {
            if is_break_statement(s, cx) {
                break_idx = Some(i);
                break;
            }
        }
        let Some(idx) = break_idx else { return; };

        for &s in &statements[..idx] {
            if contains_next(s, cx) {
                return;
            }
        }

        if conditional_continue_keyword(statements[idx], cx) {
            return;
        }

        let keyword_range = cx.loc(node).keyword();
        let range = if keyword_range == Range::ZERO {
            cx.range(node)
        } else {
            keyword_range
        };
        cx.emit_offense(range, MSG, None);
    }
}

fn flat_statements(node: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    match cx.kind(node) {
        NodeKind::Begin(list) | NodeKind::Kwbegin(list) => cx.list(*list).to_vec(),
        _ => vec![node],
    }
}

fn is_break_statement(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Return(_) | NodeKind::Break(_) => true,

        NodeKind::Send { receiver, method, .. } => {
            let method_str = cx.symbol_str(*method);
            if *receiver == OptNodeId::NONE && FLOW_METHODS.contains(&method_str) {
                return true;
            }
            if let Some(recv_id) = receiver.get()
                && matches!(*cx.kind(recv_id), NodeKind::Const { scope, name } if {
                    let scope_is_root = scope.get().is_none_or(|sid| matches!(*cx.kind(sid), NodeKind::Cbase));
                    scope_is_root && cx.symbol_str(name) == "Kernel"
                }) && FLOW_METHODS.contains(&method_str)
            {
                return true;
            }
            false
        }

        NodeKind::If { then_, else_, .. } => {
            let Some(then_id) = then_.get() else { return false; };
            let Some(else_id) = else_.get() else { return false; };
            is_break_statement(then_id, cx) && is_break_statement(else_id, cx)
        }

        NodeKind::Begin(list) | NodeKind::Kwbegin(list) => {
            let children = cx.list(*list);
            let mut break_stmt = None;
            for &c in children.iter() {
                if is_break_statement(c, cx) {
                    break_stmt = Some(c);
                    break;
                }
            }
            let Some(bs) = break_stmt else { return false; };
            !preceded_by_continue(bs, children, cx)
        }

        _ => false,
    }
}

fn contains_next(node: NodeId, cx: &Cx<'_>) -> bool {
    // Mirror RuboCop's `CONTINUE_KEYWORDS = %i[next redo]`. `retry` is
    // deliberately excluded: a `retry` inside a `begin/rescue` restarts the
    // protected block, not the surrounding loop, so it does not let the outer
    // loop iterate more than once.
    if matches!(*cx.kind(node), NodeKind::Next(_) | NodeKind::Redo) {
        return true;
    }
    if is_block_or_loop(node, cx) {
        return false;
    }
    cx.children(node).iter().any(|&c| contains_next(c, cx))
}

fn is_block_or_loop(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Itblock { .. }
            | NodeKind::While { .. }
            | NodeKind::Until { .. }
            | NodeKind::For { .. }
    )
}

fn preceded_by_continue(break_stmt: NodeId, siblings: &[NodeId], cx: &Cx<'_>) -> bool {
    let pos = siblings.iter().position(|&s| s == break_stmt);
    let Some(idx) = pos else { return false; };
    siblings[..idx].iter().any(|&s| {
        !is_loop_keyword_or_method(s, cx) && contains_next(s, cx)
    })
}

fn is_loop_keyword_or_method(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::While { .. } | NodeKind::Until { .. } | NodeKind::For { .. })
}

fn conditional_continue_keyword(break_stmt: NodeId, cx: &Cx<'_>) -> bool {
    let descendants = cx.descendants(break_stmt);
    let or_node = descendants
        .iter()
        .rev()
        .find(|&&d| matches!(*cx.kind(d), NodeKind::Or { .. }));
    let Some(&or_id) = or_node else { return false; };
    let NodeKind::Or { rhs, .. } = *cx.kind(or_id) else { return false; };
    matches!(*cx.kind(rhs), NodeKind::Next(_) | NodeKind::Redo)
}

#[cfg(test)]
mod tests {
    use super::UnreachableLoop;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_while_with_break() {
        test::<UnreachableLoop>().expect_offense(indoc! {r#"
            while x > 0
            ^^^^^ This loop will have at most one iteration.
              x += 1
              break
            end
        "#});
    }

    #[test]
    fn flags_while_with_if_else_both_break() {
        test::<UnreachableLoop>().expect_offense(indoc! {r#"
            while x > 0
            ^^^^^ This loop will have at most one iteration.
              if condition
                break
              else
                raise MyError
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_if_without_else() {
        test::<UnreachableLoop>().expect_no_offenses(indoc! {"
            while x > 0
              if condition
                break
              elsif other_condition
                raise MyError
              end
            end
        "});
    }

    #[test]
    fn does_not_flag_if_elsif_else_not_all_breaking() {
        test::<UnreachableLoop>().expect_no_offenses(indoc! {"
            while x > 0
              if condition
                break
              elsif other_condition
                do_something
              else
                raise MyError
              end
            end
        "});
    }

    #[test]
    fn does_not_flag_with_preceding_next() {
        test::<UnreachableLoop>().expect_no_offenses(indoc! {"
            while x > 0
              next if x.odd?
              x += 1
              break
            end
        "});
    }

    #[test]
    fn does_not_flag_with_preceding_next_in_if_else() {
        test::<UnreachableLoop>().expect_no_offenses(indoc! {"
            while x > 0
              next if x.odd?
              if condition
                break
              else
                raise MyError
              end
            end
        "});
    }

    #[test]
    fn flags_until_with_break() {
        test::<UnreachableLoop>().expect_offense(indoc! {r#"
            until x > 0
            ^^^^^ This loop will have at most one iteration.
              x -= 1
              break
            end
        "#});
    }

    #[test]
    fn flags_for_with_return() {
        test::<UnreachableLoop>().expect_offense(indoc! {r#"
            for x in values
            ^^^ This loop will have at most one iteration.
              return x
            end
        "#});
    }

    #[test]
    fn does_not_flag_if_branch_has_next_before_break() {
        test::<UnreachableLoop>().expect_no_offenses(indoc! {"
            while x > 0
              if y
                next if something
                break
              else
                break
              end
            end
        "});
    }

    #[test]
    fn does_not_flag_while_without_break() {
        test::<UnreachableLoop>().expect_no_offenses(indoc! {"
            while x > 0
              x -= 1
            end
        "});
    }

    #[test]
    fn does_not_flag_while_with_only_next() {
        test::<UnreachableLoop>().expect_no_offenses(indoc! {"
            while x
              next
            end
        "});
    }

    #[test]
    fn flags_while_with_rescue_retry_then_break() {
        // `retry` restarts the begin/rescue attempt, not the outer loop, so
        // the loop still runs at most once before the unconditional `break`.
        // RuboCop's CONTINUE_KEYWORDS is `%i[next redo]` — retry is excluded —
        // so this must still be flagged. (Regression: when `retry` lowered to
        // NodeKind::Retry, the dead `Retry` arm in `contains_next` went live
        // and suppressed this offense as if retry were a `next`.)
        test::<UnreachableLoop>().expect_offense(indoc! {r#"
            while cond
            ^^^^^ This loop will have at most one iteration.
              begin
                work
              rescue
                retry
              end
              break
            end
        "#});
    }

    #[test]
    fn flags_while_with_inner_block_next_then_break() {
        test::<UnreachableLoop>().expect_offense(indoc! {r#"
            while cond
            ^^^^^ This loop will have at most one iteration.
              xs.each { next if skip? }
              break
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(UnreachableLoop);
