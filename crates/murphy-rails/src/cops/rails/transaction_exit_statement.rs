//! `Rails/TransactionExitStatement` — flag `return`/`break`/`throw` in transaction blocks.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/TransactionExitStatement
//! upstream_version_checked: 2.35.0
//! version_added: "2.14"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_send`: disabled on Rails >= 7.2
//!   (`rails_version_at_least(7, 2)` bails; unset means newest, so the cop
//!   is silent without an explicit `TargetRailsVersion < 7.2`), fires for
//!   `transaction`/`with_lock` plus configurable `TransactionMethods`. The
//!   send must be the call of a `Block`/`Numblock`/`Itblock` with a body,
//!   and no loop keyword (`while`/`until`/`for`) may sit among the send's
//!   right siblings — so a bare `while` body disables the whole check,
//!   matching upstream `in_transaction_block?`. Every `return`, `break`
//!   and bare-`throw` send in the body subtree (including the body itself)
//!   flags with the whole-statement range; only `break` is skipped when
//!   its nearest enclosing block is a non-transaction method (so `break`
//!   in a nested `loop`/`each`/`while`/`until` passes while `return` and
//!   `throw` there still flag). `next` and `raise` never flag. No
//!   autocorrect. Disabled upstream by default (`Enabled: pending`).
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct TransactionExitStatement;

#[derive(CopOptions)]
pub struct TransactionExitStatementOptions {
    #[option(
        name = "TransactionMethods",
        default = [],
        description = "Custom transaction methods (in addition to `transaction` and `with_lock`)."
    )]
    pub transaction_methods: Vec<String>,
}

#[cop(
    name = "Rails/TransactionExitStatement",
    description = "Avoid the usage of `return`, `break` and `throw` in transaction blocks.",
    default_severity = "warning",
    default_enabled = false,
    options = TransactionExitStatementOptions,
)]
impl TransactionExitStatement {
    // Mirrors upstream `on_send` (plain sends only; `&.` calls never match).
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Upstream `return if target_rails_version >= 7.2` — unset means newest.
    if cx.rails_version_at_least(7, 2) {
        return;
    }
    let NodeKind::Send { .. } = *cx.kind(node) else {
        return;
    };
    let Some(method) = cx.method_name(node) else {
        return;
    };
    let opts = cx.options_or_default::<TransactionExitStatementOptions>();
    if !is_transaction_method(&opts, method) {
        return;
    }
    // Upstream `in_transaction_block?`: the parent must be a block with a
    // body, and no loop keyword may follow the send among its siblings
    // (a bare `while`/`until`/`for` body disables the whole check).
    let Some(parent) = cx.parent(node).get() else {
        return;
    };
    let body = match *cx.kind(parent) {
        NodeKind::Block { call, body, .. } if call == node => body.get(),
        NodeKind::Numblock { send, body, .. } if send == node => body.get(),
        NodeKind::Itblock { send, body } if send == node => body.get(),
        _ => None,
    };
    let Some(body) = body else {
        return;
    };
    let kids = cx.children(parent);
    let Some(pos) = kids.iter().position(|&k| k == node) else {
        return;
    };
    if kids[pos + 1..].iter().any(|&k| cx.is_loop_keyword(k)) {
        return;
    }
    // Upstream `exit_statements(node.parent.body)`: every `return`, `break`
    // and bare-`throw` send in the body subtree, body itself included.
    let mut stmts = vec![body];
    stmts.extend(cx.descendants(body));
    for stmt in stmts {
        match *cx.kind(stmt) {
            NodeKind::Return(_) => {
                cx.emit_offense(cx.range(stmt), &message("return"), None);
            }
            NodeKind::Break(_) => {
                // Upstream `next if statement_node.break_type? &&
                // nested_block?(statement_node)`.
                if break_in_nested_block(cx, &opts, stmt) {
                    continue;
                }
                cx.emit_offense(cx.range(stmt), &message("break"), None);
            }
            NodeKind::Send { receiver, method, .. }
                if receiver.get().is_none() && cx.symbol_str(method) == "throw" =>
            {
                cx.emit_offense(cx.range(stmt), &message("throw"), None);
            }
            _ => {}
        }
    }
}

fn message(statement: &str) -> String {
    format!(
        "Exit statement `{statement}` is not allowed. Use `raise` (rollback) or `next` (commit)."
    )
}

fn is_transaction_method(opts: &TransactionExitStatementOptions, method: &str) -> bool {
    method == "transaction"
        || method == "with_lock"
        || opts.transaction_methods.iter().any(|m| m == method)
}

/// Upstream `nested_block?`: the nearest enclosing block's call is not a
/// transaction method.
fn break_in_nested_block(
    cx: &Cx<'_>,
    opts: &TransactionExitStatementOptions,
    stmt: NodeId,
) -> bool {
    for anc in cx.ancestors(stmt) {
        let call = match *cx.kind(anc) {
            NodeKind::Block { call, .. } => Some(call),
            NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => Some(send),
            _ => None,
        };
        if let Some(call) = call {
            // The transaction block itself always encloses `stmt`, so a
            // block ancestor is always found.
            return match cx.method_name(call) {
                Some(name) => !is_transaction_method(opts, name),
                None => true,
            };
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{TransactionExitStatement, TransactionExitStatementOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn rails70<T: murphy_plugin_api::NodeCop + Default>(
        t: murphy_plugin_api::test_support::Tester<T>,
    ) -> murphy_plugin_api::test_support::Tester<T> {
        t.with_target_rails_version(7, 0)
    }

    #[test]
    fn flags_return_in_transaction() {
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            ApplicationRecord.transaction do
              return if user.active?
              ^^^^^^ Exit statement `return` is not allowed. Use `raise` (rollback) or `next` (commit).
            end
        "#});
    }

    #[test]
    fn flags_return_in_with_lock() {
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            ApplicationRecord.with_lock do
              return if user.active?
              ^^^^^^ Exit statement `return` is not allowed. Use `raise` (rollback) or `next` (commit).
            end
        "#});
    }

    #[test]
    fn flags_receiverless_transaction() {
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            transaction do
              return if user.active?
              ^^^^^^ Exit statement `return` is not allowed. Use `raise` (rollback) or `next` (commit).
            end
        "#});
    }

    #[test]
    fn flags_break_in_transaction() {
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            ApplicationRecord.transaction do
              break if user.active?
              ^^^^^ Exit statement `break` is not allowed. Use `raise` (rollback) or `next` (commit).
            end
        "#});
    }

    #[test]
    fn flags_throw_in_transaction() {
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            ApplicationRecord.transaction do
              throw if user.active?
              ^^^^^ Exit statement `throw` is not allowed. Use `raise` (rollback) or `next` (commit).
            end
        "#});
    }

    #[test]
    fn flags_throw_with_arguments() {
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            ApplicationRecord.transaction do
              throw :abort if user.active?
              ^^^^^^^^^^^^ Exit statement `throw` is not allowed. Use `raise` (rollback) or `next` (commit).
            end
        "#});
    }

    #[test]
    fn does_not_flag_receiver_throw() {
        // Upstream `(send nil? :throw)`: an explicit receiver never matches.
        rails70(test::<TransactionExitStatement>()).expect_no_offenses(indoc! {r#"
            ApplicationRecord.transaction do
              obj.throw if user.active?
            end
        "#});
    }

    #[test]
    fn does_not_flag_next_or_raise() {
        rails70(test::<TransactionExitStatement>()).expect_no_offenses(indoc! {r#"
            ApplicationRecord.transaction do
              next if user.active?
            end
        "#});
        rails70(test::<TransactionExitStatement>()).expect_no_offenses(indoc! {r#"
            ApplicationRecord.transaction do
              raise if user.active?
            end
        "#});
    }

    #[test]
    fn flags_return_in_nested_loop() {
        // Only `break` is exempted in nested blocks; `return` still flags.
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            ApplicationRecord.transaction do
              loop do
                return if condition
                ^^^^^^ Exit statement `return` is not allowed. Use `raise` (rollback) or `next` (commit).
              end
            end
        "#});
    }

    #[test]
    fn flags_return_in_nested_each_numblock() {
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            ApplicationRecord.transaction do
              foo.each do
                return if _1
                ^^^^^^ Exit statement `return` is not allowed. Use `raise` (rollback) or `next` (commit).
              end
            end
        "#});
    }

    #[test]
    fn flags_throw_in_nested_loop() {
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            ApplicationRecord.transaction do
              loop do
                throw if condition
                ^^^^^ Exit statement `throw` is not allowed. Use `raise` (rollback) or `next` (commit).
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_break_in_nested_loop() {
        rails70(test::<TransactionExitStatement>()).expect_no_offenses(indoc! {r#"
            ApplicationRecord.transaction do
              loop do
                break if condition
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_break_in_while() {
        rails70(test::<TransactionExitStatement>()).expect_no_offenses(indoc! {r#"
            ApplicationRecord.transaction do
              while proceed_looping? do
                break if condition
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_break_in_until() {
        rails70(test::<TransactionExitStatement>()).expect_no_offenses(indoc! {r#"
            ApplicationRecord.transaction do
              until stop_looping? do
                break if condition
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_break_in_nested_each_numblock() {
        rails70(test::<TransactionExitStatement>()).expect_no_offenses(indoc! {r#"
            ApplicationRecord.transaction do
              foo.each do
                break if _1
              end
            end
        "#});
    }

    #[test]
    fn flags_return_in_rescue_body() {
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            ApplicationRecord.transaction do
            rescue
              return do_something
              ^^^^^^^^^^^^^^^^^^^ Exit statement `return` is not allowed. Use `raise` (rollback) or `next` (commit).
            end
        "#});
    }

    #[test]
    fn flags_return_beside_rescue() {
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            ApplicationRecord.transaction do
              return if user.active?
              ^^^^^^ Exit statement `return` is not allowed. Use `raise` (rollback) or `next` (commit).
            rescue
              pass
            end
        "#});
    }

    #[test]
    fn flags_bare_return_body() {
        // The body itself can be the exit statement.
        rails70(test::<TransactionExitStatement>()).expect_offense(indoc! {r#"
            ApplicationRecord.transaction do
              return
              ^^^^^^ Exit statement `return` is not allowed. Use `raise` (rollback) or `next` (commit).
            end
        "#});
    }

    #[test]
    fn does_not_flag_for_loop_body() {
        // `for` is a loop keyword too: the whole check is disabled when it
        // is the block body, matching upstream `in_transaction_block?`.
        rails70(test::<TransactionExitStatement>()).expect_no_offenses(indoc! {r#"
            ApplicationRecord.transaction do
              for x in items do
                return if x.done?
              end
            end
        "#});
    }

    #[test]
    fn does_not_flag_empty_block() {
        rails70(test::<TransactionExitStatement>()).expect_no_offenses(indoc! {r#"
            ApplicationRecord.transaction do
            end
        "#});
    }

    #[test]
    fn does_not_flag_chained_call() {
        rails70(test::<TransactionExitStatement>()).expect_no_offenses("transaction.foo\n");
    }

    #[test]
    fn does_not_flag_bare_call_without_block() {
        rails70(test::<TransactionExitStatement>()).expect_no_offenses("transaction\n");
    }

    #[test]
    fn flags_custom_transaction_method() {
        let opts = TransactionExitStatementOptions {
            transaction_methods: vec!["writable_transaction".to_string()],
        };
        test::<TransactionExitStatement>()
            .with_options(&opts)
            .with_target_rails_version(7, 0)
            .expect_offense(indoc! {r#"
                CustomModel.writable_transaction do
                  return if user.active?
                  ^^^^^^ Exit statement `return` is not allowed. Use `raise` (rollback) or `next` (commit).
                end
            "#});
    }

    #[test]
    fn does_not_flag_unknown_method() {
        rails70(test::<TransactionExitStatement>()).expect_no_offenses(indoc! {r#"
            CustomModel.writable_transaction do
              return if user.active?
            end
        "#});
    }

    #[test]
    fn does_not_flag_on_rails_72() {
        test::<TransactionExitStatement>()
            .with_target_rails_version(7, 2)
            .expect_no_offenses(indoc! {r#"
                ApplicationRecord.transaction do
                  return if user.active?
                end
            "#});
    }

    #[test]
    fn flags_on_rails_71() {
        test::<TransactionExitStatement>()
            .with_target_rails_version(7, 1)
            .expect_offense(indoc! {r#"
                ApplicationRecord.transaction do
                  return if user.active?
                  ^^^^^^ Exit statement `return` is not allowed. Use `raise` (rollback) or `next` (commit).
                end
            "#});
    }

    #[test]
    fn stays_silent_without_target_rails_version() {
        // Unset means newest (>= 7.2), so the cop bails, matching upstream
        // evaluated against a newest Rails.
        test::<TransactionExitStatement>().expect_no_offenses(indoc! {r#"
            ApplicationRecord.transaction do
              return if user.active?
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(TransactionExitStatement);
