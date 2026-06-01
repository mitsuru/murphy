//! `Lint/UnreachableCode` — flags sibling statements that follow a
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnreachableCode
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Translator-blocked items (not cop gaps): retry/redo nodes are not
//!   emitted by the translator (retry causes parse errors, redo translates
//!   to Unknown), so those keyword terminator arms are forward-compat only.
//!   case/in (CaseMatch) also translates to Unknown; a forward-compat arm
//!   mirrors the Case arm but is similarly untestable today. All other
//!   RuboCop parity items are implemented: message parity, first-only
//!   reporting, fail/throw/exit/exit!/abort method terminators, Kernel.*
//!   receiver form, if/case all-branches check, nested begin recursion,
//!   sibling def-redefinition suppression, and instance_eval suppression.
//! ```
//!
//! flow-terminator inside the *same* `Begin(NodeList)` container.
//!
//! A statement is unreachable when its **direct** sibling earlier in the
//! same `Begin` body is one of:
//!
//! - `Return(_)` — `return ...`
//! - `Break(_)` — `break ...`
//! - `Next(_)`  — `next ...`
//! - `Retry` / `Redo` — (forward-compat; translator currently emits Unknown)
//! - `Send { receiver: None, method: "raise"|"fail"|"throw"|"exit"|"exit!"|"abort", ... }`
//!   — receiver-less Kernel method calls, unless redefined as a sibling `def`.
//! - `Send { receiver: Const("Kernel")|Cbase, method: <redefinable>, ... }`
//!   — Kernel-qualified calls (e.g. `Kernel.exit`).
//! - `If { then_: Some(t), else_: Some(e) }` — when both branches terminate.
//! - `Case { whens: [When{body},...], else_: Some(e) }` — when all branches
//!   and the else terminate.
//! - `Begin([...])` — when any child terminates.
//!
//! "Direct sibling" is load-bearing: a `return` *inside* an `if` with only
//! a then-branch does not make code after the surrounding construct
//! unreachable, because the terminator does not always fire.
//!
//! Only the **first** unreachable sibling after a terminator is reported,
//! matching RuboCop's `each_cons(2)` contract.
//!
//! No autocorrect: deleting unreachable code is not always the user's
//! intent (the dead branch might be a stub waiting for implementation),
//! and the cop is `warning` severity so the lint run does not fail on
//! it. This matches RuboCop's `Lint/UnreachableCode` policy.

use std::collections::HashSet;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Symbol, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct UnreachableCode;

/// The set of receiver-less method names that act as flow terminators when
/// called without an explicit non-Kernel receiver, unless locally redefined.
const REDEFINABLE_FLOW_METHODS: &[&str] = &["raise", "fail", "throw", "exit", "exit!", "abort"];

#[cop(
    name = "Lint/UnreachableCode",
    description = "Flag the first statement following a terminator (return / break / next / raise / fail / exit / etc.) in the same begin block.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UnreachableCode {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Begin(_) = *cx.kind(node) else {
            return;
        };
        let in_instance_eval = is_inside_instance_eval(node, cx);
        let children = cx.children(node);

        // Track method names that are redefined as `def <name>; end` siblings
        // earlier in this same begin body, suppressing their terminator role.
        let mut redefined: HashSet<Symbol> = HashSet::new();

        for pair in children.windows(2) {
            let (expr1, expr2) = (pair[0], pair[1]);

            // Register any `def <name>` (receiver-less) that redefines a
            // redefinable flow method — must happen before the terminator
            // check so the *next* sibling's check sees it.
            if let NodeKind::Def { receiver, name, .. } = *cx.kind(expr1)
                && receiver == OptNodeId::NONE
                && REDEFINABLE_FLOW_METHODS.contains(&cx.symbol_str(name))
            {
                redefined.insert(name);
            }

            if is_flow_terminator(expr1, cx, &redefined, in_instance_eval) {
                cx.emit_offense(cx.range(expr2), "Unreachable code detected.", None);
                return; // report only the first unreachable sibling
            }
        }
    }
}

/// True when `node` is inside an `instance_eval` block anywhere in its
/// ancestor chain. In such a context, `self` is unknown, so receiver-less
/// redefinable method calls are suppressed (they may be dispatched on an
/// arbitrary object that has redefined them).
fn is_inside_instance_eval(node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        let kind = cx.kind(ancestor);
        match kind {
            NodeKind::Block { call, .. } | NodeKind::Numblock { send: call, .. } => {
                let is_ie = match cx.kind(*call) {
                    NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => {
                        cx.symbol_str(*method) == "instance_eval"
                    }
                    _ => false,
                };
                if is_ie {
                    return true;
                }
            }
            NodeKind::Itblock { send, .. } => {
                let is_ie = match cx.kind(*send) {
                    NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => {
                        cx.symbol_str(*method) == "instance_eval"
                    }
                    _ => false,
                };
                if is_ie {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// True when `node` represents a flow expression that prevents the next
/// sibling from running.
///
/// `redefined` — method names that have been redefined as a `def` sibling
/// earlier in the same begin body; suppresses their terminator semantics.
///
/// `in_instance_eval` — when true, receiver-less redefinable-method calls
/// are suppressed (unknown self type inside `instance_eval`).
fn is_flow_terminator(
    node: NodeId,
    cx: &Cx<'_>,
    redefined: &HashSet<Symbol>,
    in_instance_eval: bool,
) -> bool {
    match cx.kind(node) {
        NodeKind::Return(_) | NodeKind::Break(_) | NodeKind::Next(_) => true,

        // Forward-compat: translator currently emits Unknown for redo (and
        // parse-errors on retry), but these arms are correct when they arrive.
        NodeKind::Retry | NodeKind::Redo => true,

        NodeKind::Send {
            receiver, method, ..
        } => {
            let method_str = cx.symbol_str(*method);

            // Receiver-less call to a redefinable flow method.
            if *receiver == OptNodeId::NONE && REDEFINABLE_FLOW_METHODS.contains(&method_str) {
                // Suppress if the method was redefined as a sibling def.
                if redefined.contains(method) {
                    return false;
                }
                // Suppress if inside an instance_eval block (unknown self).
                if in_instance_eval {
                    return false;
                }
                return true;
            }

            // Kernel-qualified call: `Kernel.raise`, `Kernel.exit`, etc.
            // `::Kernel.raise` uses a Cbase receiver.
            if let Some(recv_id) = receiver.get() {
                let is_kernel = match cx.kind(recv_id) {
                    NodeKind::Const { scope, name } => {
                        let scope_is_root = match scope.get() {
                            None => true,
                            Some(sid) => matches!(*cx.kind(sid), NodeKind::Cbase),
                        };
                        scope_is_root && cx.symbol_str(*name) == "Kernel"
                    }
                    NodeKind::Cbase => true,
                    _ => false,
                };
                if is_kernel && REDEFINABLE_FLOW_METHODS.contains(&method_str) {
                    return true;
                }
            }

            false
        }

        // `if cond; <then>; else; <else>; end` — terminator iff both
        // branches exist and each terminates independently.
        NodeKind::If { then_, else_, .. } => {
            let Some(then_id) = then_.get() else {
                return false;
            };
            let Some(else_id) = else_.get() else {
                return false;
            };
            is_flow_terminator(then_id, cx, redefined, in_instance_eval)
                && is_flow_terminator(else_id, cx, redefined, in_instance_eval)
        }

        // `case subject; when ...; else ...; end` — terminator iff the else
        // branch exists and terminates, and every when body terminates.
        NodeKind::Case { whens, else_, .. } => {
            let Some(else_id) = else_.get() else {
                return false;
            };
            if !is_flow_terminator(else_id, cx, redefined, in_instance_eval) {
                return false;
            }
            for when_id in cx.list(*whens) {
                if let NodeKind::When { body, .. } = cx.kind(*when_id) {
                    let Some(body_id) = body.get() else {
                        return false;
                    };
                    if !is_flow_terminator(body_id, cx, redefined, in_instance_eval) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            true
        }

        // `case subject; in pattern; ...; else ...; end` — forward-compat arm.
        // CaseMatch currently translates to Unknown in the translator, so this
        // arm is never exercised today but mirrors the Case arm for parity.
        NodeKind::CaseMatch {
            in_patterns,
            else_body,
            ..
        } => {
            let Some(else_id) = else_body.get() else {
                return false;
            };
            if !is_flow_terminator(else_id, cx, redefined, in_instance_eval) {
                return false;
            }
            for in_id in cx.list(*in_patterns) {
                if let NodeKind::InPattern { body, .. } = cx.kind(*in_id) {
                    let Some(body_id) = body.get() else {
                        return false;
                    };
                    if !is_flow_terminator(body_id, cx, redefined, in_instance_eval) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            true
        }

        // `begin ... end` — terminator iff any child terminates.
        NodeKind::Begin(children) | NodeKind::Kwbegin(children) => cx
            .list(*children)
            .iter()
            .any(|&child| is_flow_terminator(child, cx, redefined, in_instance_eval)),

        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::UnreachableCode;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── existing baseline ──────────────────────────────────────────────

    #[test]
    fn flags_first_dead_sibling_after_return() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def foo
              return
              puts 'x'
              ^^^^^^^^^ Unreachable code detected.
            end
        "#});
    }

    #[test]
    fn does_not_flag_return_nested_in_if() {
        test::<UnreachableCode>().expect_no_offenses(indoc! {r#"
            def foo
              if x
                return
              end
              puts 'x'
            end
        "#});
    }

    #[test]
    fn flags_dead_code_after_raise() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def foo
              raise 'bad'
              puts 'x'
              ^^^^^^^^^ Unreachable code detected.
            end
        "#});
    }

    #[test]
    fn flags_dead_code_after_multiline_raise() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def foo
              raise(
                "boom"
              )
              bar
              ^^^ Unreachable code detected.
            end
        "#});
    }

    #[test]
    fn does_not_flag_explicit_receiver_raise() {
        test::<UnreachableCode>()
            .expect_no_offenses("def foo\n  obj.raise 'msg'\n  puts 'still runs'\nend\n");
    }

    // ── report only first unreachable sibling (RuboCop each_cons(2)) ──

    #[test]
    fn reports_only_first_unreachable_sibling() {
        // Two dead siblings; only the first one gets an offense.
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def foo
              return
              puts 'a'
              ^^^^^^^^ Unreachable code detected.
              puts 'b'
            end
        "#});
    }

    // ── message parity ────────────────────────────────────────────────

    #[test]
    fn message_matches_rubocop() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def foo
              break
              bar
              ^^^ Unreachable code detected.
            end
        "#});
    }

    // ── additional method terminators ─────────────────────────────────

    #[test]
    fn flags_dead_code_after_fail() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def foo
              fail
              bar
              ^^^ Unreachable code detected.
            end
        "#});
    }

    #[test]
    fn flags_dead_code_after_throw() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def foo
              throw
              bar
              ^^^ Unreachable code detected.
            end
        "#});
    }

    #[test]
    fn flags_dead_code_after_exit() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def foo
              exit
              bar
              ^^^ Unreachable code detected.
            end
        "#});
    }

    #[test]
    fn flags_dead_code_after_exit_bang() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def foo
              exit!
              bar
              ^^^ Unreachable code detected.
            end
        "#});
    }

    #[test]
    fn flags_dead_code_after_abort() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def foo
              abort
              bar
              ^^^ Unreachable code detected.
            end
        "#});
    }

    #[test]
    fn does_not_flag_after_operator_keyword_guard_clause() {
        test::<UnreachableCode>().expect_no_offenses(indoc! {r#"
            def foo
              ok or return
              bar
            end
        "#});
    }

    // ── Kernel-receiver terminators ───────────────────────────────────

    #[test]
    fn flags_dead_code_after_kernel_exit() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def exit; end
            Kernel.exit
            foo
            ^^^ Unreachable code detected.
        "#});
    }

    // ── if/else all-branches-terminate ────────────────────────────────

    #[test]
    fn flags_dead_code_after_if_both_branches_return() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def something
              array.each do |item|
                if cond
                  return
                else
                  return
                end
                bar
                ^^^ Unreachable code detected.
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_if_without_else() {
        test::<UnreachableCode>().expect_no_offenses(indoc! {r#"
            def something
              array.each do |item|
                if cond
                  return
                end
                bar
              end
            end
        "#});
    }

    #[test]
    fn flags_dead_code_after_if_elsif_else_all_return() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def something
              array.each do |item|
                if cond
                  something
                  return
                elsif cond2
                  something2
                  return
                else
                  something3
                  return
                end
                bar
                ^^^ Unreachable code detected.
              end
            end
        "#});
    }

    // ── case/when+else all-branches-terminate ─────────────────────────

    #[test]
    fn flags_dead_code_after_case_all_branches_exit() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def something
              array.each do |item|
                case cond
                when 1
                  something
                  exit
                when 2
                  something2
                  exit
                else
                  something3
                  exit
                end
                bar
                ^^^ Unreachable code detected.
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_case_without_else() {
        // Without an else branch, the case may not execute any when → bar
        // is still reachable.
        test::<UnreachableCode>().expect_no_offenses(indoc! {r#"
            def something
              array.each do |item|
                case cond
                when 1
                  exit
                when 2
                  exit
                end
                bar
              end
            end
        "#});
    }

    // ── explicit begin/end (kwbegin) ──────────────────────────────────

    #[test]
    fn flags_dead_code_in_explicit_begin_end() {
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def something
              array.each do |item|
                begin
                  return
                  bar
                  ^^^ Unreachable code detected.
                end
              end
            end
        "#});
    }

    // ── sibling def-redefinition suppression ─────────────────────────

    #[test]
    fn suppresses_raise_when_redefined_as_sibling_def() {
        test::<UnreachableCode>().expect_no_offenses(indoc! {r#"
            def something
              array.each do |item|
                def raise; end
                raise
                bar
              end
            end
        "#});
    }

    #[test]
    fn does_not_suppress_raise_for_self_dot_raise_def() {
        // `def self.raise` redefines on the singleton class, not the
        // receiver-less lookup path — should still fire.
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            def foo
              def self.raise; end
            end
            raise
            bar
            ^^^ Unreachable code detected.
        "#});
    }

    // ── instance_eval suppression ─────────────────────────────────────

    #[test]
    fn suppresses_exit_inside_instance_eval() {
        test::<UnreachableCode>().expect_no_offenses(indoc! {r#"
            class Dummy
              def exit; end
            end
            d = Dummy.new
            d.instance_eval do
              exit
              bar
            end
        "#});
    }

    #[test]
    fn does_not_suppress_return_inside_instance_eval() {
        // `return` is a keyword terminator — not redefinable, always
        // fires regardless of instance_eval context.
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            d.instance_eval do
              return
              bar
              ^^^ Unreachable code detected.
            end
        "#});
    }

    #[test]
    fn does_not_suppress_kernel_exit_inside_instance_eval() {
        // `Kernel.exit` is unambiguous — suppression only applies to
        // receiver-less calls where self is unknown.
        test::<UnreachableCode>().expect_offense(indoc! {r#"
            d.instance_eval do
              Kernel.exit
              bar
              ^^^ Unreachable code detected.
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(UnreachableCode);
