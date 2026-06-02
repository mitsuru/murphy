//! `Style/RedundantReturn` — flags unnecessary `return` at the end of method
//! bodies.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantReturn
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Scope: only the last statement of the method body is checked. RuboCop
//!   also recurses into if/elsif/else and case/when branch tails; that
//!   recursive branch-tail checking is not implemented (conservative gap —
//!   no false positives).
//!   Rescue/Ensure: methods whose body is wrapped in Rescue or Ensure are
//!   skipped entirely (conservative; RuboCop recurses into rescue branches).
//!   AllowMultipleReturnValues: not implemented; multi-value returns
//!   (return a, b → Return(Array)) are always skipped.
//!   return [a, b] is indistinguishable from return a, b at the AST level;
//!   both are skipped.
//!   return(value): offense is emitted and autocorrect applies three surgical
//!   edits to delete `return`, `(`, and `)`, leaving just `value`.
//!   Autocorrect: `return value` → delete `return ` prefix (from return node
//!   start to value node start). Bare `return` → replace with `nil`.
//!   def/defs (singleton method definitions): both handled via separate hooks.
//! ```
//!
//! Subscribes to `NodeKind::Def` (instance methods including `def self.foo`)
//! and `NodeKind::Defs` (singleton method definitions). Checks whether the
//! last statement in the method body is a `return` node that can be removed.
//!
//! ## Offense conditions
//!
//! The last statement of the body (direct child of the body, or the body
//! itself if not a `Begin`) is a `Return` node:
//!
//! - `Return(None)` — bare `return` → offense; autocorrect replaces with `nil`.
//! - `Return(Some(value))` where `value` is NOT an `Array` → offense;
//!   autocorrect deletes the `return ` prefix.
//! - `Return(Some(Array(...)))` — multi-value / `return [arr]` → **skipped**
//!   (documented gap; indistinguishable from `return a, b` at AST level).
//!
//! ## Skips
//!
//! - Methods with no body.
//! - Methods whose body is a `Rescue` or `Ensure` node (conservative).
//! - Multi-value returns (see above).
//!
//! ## Autocorrect
//!
//! Two forms:
//!
//! - `return value`: delete from `return_node.range.start` to
//!   `value_node.range.start` (removes the `return ` prefix including space).
//! - bare `return`: replace the whole `return` node range with `nil`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedundantReturn;

#[cop(
    name = "Style/RedundantReturn",
    description = "Avoid redundant `return` at the end of a method body.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantReturn {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_method_body(cx.def_body(node), cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check_method_body(cx.def_body(node), cx);
    }
}

/// Check whether the last statement of `body` is a redundant `return`.
fn check_method_body(body: OptNodeId, cx: &Cx<'_>) {
    let Some(body_id) = body.get() else {
        return;
    };

    // Skip rescue/ensure-wrapped bodies (conservative; RuboCop recurses
    // into them but we don't for v1).
    if matches!(
        cx.kind(body_id),
        NodeKind::Rescue { .. } | NodeKind::Ensure { .. }
    ) {
        return;
    }

    // Find the last statement: if the body is a Begin block, take its last
    // child; otherwise the body itself is the single statement.
    let last_stmt = match cx.kind(body_id) {
        NodeKind::Begin(list) => {
            let children = cx.list(*list);
            match children.last() {
                Some(&last) => last,
                None => return,
            }
        }
        NodeKind::Kwbegin(list) => {
            let children = cx.list(*list);
            match children.last() {
                Some(&last) => last,
                None => return,
            }
        }
        _ => body_id,
    };

    check_return(last_stmt, cx);
}

/// Check whether `node` is a redundant `return` and emit an offense if so.
fn check_return(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Return(value) = *cx.kind(node) else {
        return;
    };

    match value.get() {
        None => {
            // Bare `return` — offense; autocorrect replaces with `nil`.
            let range = cx.range(node);
            cx.emit_offense(range, "Redundant `return` detected.", None);
            cx.emit_edit(range, "nil");
        }
        Some(val_id) => {
            // Skip multi-value returns: Return(Array(...)) is either
            // `return a, b` or `return [a, b]` — indistinguishable at the
            // AST level; skip both to avoid a meaning-changing autocorrect.
            if matches!(cx.kind(val_id), NodeKind::Array(_)) {
                return;
            }

            // `return value` — offense; autocorrect deletes "return " prefix.
            let return_range = cx.range(node);
            let value_range = cx.range(val_id);
            cx.emit_offense(return_range, "Redundant `return` detected.", None);

            use crate::cops::util::is_parenthesized;
            if is_parenthesized(val_id, cx) {
                // `return(expr)`: three surgical edits to remove return, (, and ).
                // Use token ranges for ( and ) to be explicit about what is deleted.
                let lp = cx.token_after(value_range.start);
                let rp = cx.token_before(value_range.end);
                // Edit 1: delete "return" (return node start to begin node start).
                cx.emit_edit(
                    Range { start: return_range.start, end: value_range.start },
                    "",
                );
                // Edit 2: delete opening `(`.
                if let Some(t) = lp {
                    cx.emit_edit(t.range, "");
                }
                // Edit 3: delete closing `)`.
                if let Some(t) = rp {
                    cx.emit_edit(t.range, "");
                }
            } else {
                // `return expr`: delete "return " prefix.
                let keyword_range = Range {
                    start: return_range.start,
                    end: value_range.start,
                };
                cx.emit_edit(keyword_range, "");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RedundantReturn;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Basic offense cases ---

    #[test]
    fn flags_return_as_last_stmt_simple_body() {
        test::<RedundantReturn>().expect_offense(indoc! {r#"
            def foo
              return 1
              ^^^^^^^^ Redundant `return` detected.
            end
        "#});
    }

    #[test]
    fn flags_return_as_only_stmt_in_body() {
        test::<RedundantReturn>().expect_offense(indoc! {r#"
            def bar
              return 'hello'
              ^^^^^^^^^^^^^^ Redundant `return` detected.
            end
        "#});
    }

    #[test]
    fn flags_return_with_method_call_value() {
        test::<RedundantReturn>().expect_offense(indoc! {r#"
            def compute
              x = 1
              return x.to_s
              ^^^^^^^^^^^^^ Redundant `return` detected.
            end
        "#});
    }

    // --- Bare return ---

    #[test]
    fn flags_bare_return_at_end() {
        test::<RedundantReturn>().expect_offense(indoc! {r#"
            def side_effect
              do_something
              return
              ^^^^^^ Redundant `return` detected.
            end
        "#});
    }

    // --- Autocorrect cases ---

    #[test]
    fn corrects_return_value() {
        test::<RedundantReturn>().expect_correction(
            indoc! {r#"
                def foo
                  return 1
                  ^^^^^^^^ Redundant `return` detected.
                end
            "#},
            indoc! {r#"
                def foo
                  1
                end
            "#},
        );
    }

    #[test]
    fn corrects_bare_return_to_nil() {
        test::<RedundantReturn>().expect_correction(
            indoc! {r#"
                def side_effect
                  do_something
                  return
                  ^^^^^^ Redundant `return` detected.
                end
            "#},
            indoc! {r#"
                def side_effect
                  do_something
                  nil
                end
            "#},
        );
    }

    #[test]
    fn corrects_return_string_value() {
        test::<RedundantReturn>().expect_correction(
            indoc! {r#"
                def greet
                  return 'hello'
                  ^^^^^^^^^^^^^^ Redundant `return` detected.
                end
            "#},
            indoc! {r#"
                def greet
                  'hello'
                end
            "#},
        );
    }

    // --- No-offense cases ---

    #[test]
    fn no_offense_return_in_middle() {
        test::<RedundantReturn>().expect_no_offenses(indoc! {r#"
            def foo
              return x if condition
              y
            end
        "#});
    }

    #[test]
    fn no_offense_no_return_at_end() {
        test::<RedundantReturn>().expect_no_offenses(indoc! {r#"
            def foo
              x = 1
              x
            end
        "#});
    }

    #[test]
    fn no_offense_empty_body() {
        test::<RedundantReturn>().expect_no_offenses(indoc! {r#"
            def foo
            end
        "#});
    }

    #[test]
    fn no_offense_multi_value_return() {
        // return a, b → Return(Array) — skipped (gap: indistinguishable
        // from return [a, b] at AST level).
        test::<RedundantReturn>().expect_no_offenses(indoc! {r#"
            def multi
              return a, b
            end
        "#});
    }

    #[test]
    fn no_offense_return_array_literal() {
        // return [a, b] — same AST as multi-value, also skipped.
        test::<RedundantReturn>().expect_no_offenses(indoc! {r#"
            def multi
              return [a, b]
            end
        "#});
    }

    #[test]
    fn no_offense_with_rescue_body() {
        // Bodies wrapped in rescue are skipped (conservative).
        test::<RedundantReturn>().expect_no_offenses(indoc! {r#"
            def foo
              begin
                return 1
              rescue
                nil
              end
            end
        "#});
    }

    // --- def self.foo (receiver on Def) ---

    #[test]
    fn flags_return_in_singleton_def_self() {
        test::<RedundantReturn>().expect_offense(indoc! {r#"
            def self.foo
              return 42
              ^^^^^^^^^ Redundant `return` detected.
            end
        "#});
    }

    // --- defs (NodeKind::Defs singleton method definition) ---

    #[test]
    fn no_offense_inside_non_last_stmt_middle_return() {
        // return is not the last statement — must not fire.
        test::<RedundantReturn>().expect_no_offenses(indoc! {r#"
            def foo
              return 1 if something
              do_something_else
            end
        "#});
    }

    // --- Parenthesised return form ---

    #[test]
    fn flags_and_autocorrects_return_with_parens() {
        // `return(value)` lowers to Return(Begin([Int(42)])) via is_parenthesized.
        // Autocorrect applies three surgical edits: delete "return", "(", and ")"
        // leaving just `42`.
        test::<RedundantReturn>().expect_correction(
            indoc! {r#"
                def foo
                  return(42)
                  ^^^^^^^^^^ Redundant `return` detected.
                end
            "#},
            indoc! {r#"
                def foo
                  42
                end
            "#},
        );
    }
}
murphy_plugin_api::submit_cop!(RedundantReturn);
