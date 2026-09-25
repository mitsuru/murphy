//! `Rails/CreateTableWithTimestamps` — flag `create_table` without timestamps.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/CreateTableWithTimestamps
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:create_table] gating,
//!   bare-command gate, `id: false` exemption (`(pair (sym :id) (false))`
//!   anywhere in the call), block-form handling (offense on the `create_table` send
//!   when the body is missing or lacks time columns), block-pass
//!   `&:timestamps` exemption, and the `timestamps` / `datetime
//!   :created_at/:updated_at` time-column search. Whole-block (block form)
//!   or whole-send (bare form) offense range. `ActiveRecordMigrationsHelper`
//!   `create_table_with_block?` is approximated via `cx.block_node`. No
//!   autocorrect. File scope (`Include: ['db/**/*.rb']`, `Exclude:`
//!   active-storage migration globs) is enforced via the murphy-rails pack
//!   default.yml (engine `cop_applies_to_file` gate, verified vs
//!   rubocop-rails 2.38.0 default.yml, murphy-4gd.1.15).
//! ```
//!
//! ## Matched shapes
//!
//! - `create_table :users` → offense (no timestamps).
//! - `create_table :users do |t| t.string :name end` → offense (no `t.timestamps`).
//! - `create_table :users do |t| t.timestamps end` → no offense.
//! - `create_table :users, id: false do |t| ... end` → no offense (respects editing intention).
//! - `create_table :users, &:timestamps` → no offense (block-pass form).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct CreateTableWithTimestamps;

#[cop(
    name = "Rails/CreateTableWithTimestamps",
    description = "Add timestamps when creating a new table.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl CreateTableWithTimestamps {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[create_table]`.
    #[on_node(kind = "send", methods = ["create_table"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream `node.command?(:create_table)` — bare call.
        if receiver.get().is_some() {
            return;
        }
        // `id: false` exemption.
        if has_id_false_option(cx, node) {
            return;
        }
        // Block form: `create_table ... do |t| ... end`.
        if let Some(block) = cx.block_node(node).get() {
            let body = match *cx.kind(block) {
                NodeKind::Block { body, .. } => body.get(),
                NodeKind::Numblock { body, .. } => body.get(),
                NodeKind::Itblock { body, .. } => body.get(),
                _ => None,
            };
            let Some(body) = body else {
                cx.emit_offense(
                    cx.loc(node).name,
                    "Add timestamps when creating a new table.",
                    None,
                );
                return;
            };
            if !time_columns_included(cx, body) {
                cx.emit_offense(
                    cx.loc(node).name,
                    "Add timestamps when creating a new table.",
                    None,
                );
            }
            return;
        }
        // Block-pass `&:timestamps` exemption:
        // `(send nil? :create_table (sym _) ... (block-pass (sym :timestamps)))`.
        if has_timestamps_block_pass(cx, node) {
            return;
        }
        cx.emit_offense(
            cx.loc(node).name,
            "Add timestamps when creating a new table.",
            None,
        );
    }
}

/// `(pair (sym :id) (false))` anywhere in the call arguments (including
/// nested hashes).
fn has_id_false_option(cx: &Cx<'_>, node: NodeId) -> bool {
    for &arg in cx.call_arguments(node) {
        if hash_has_id_false(cx, arg) {
            return true;
        }
    }
    false
}

fn hash_has_id_false(cx: &Cx<'_>, node: NodeId) -> bool {
    // Direct hash scan plus one level of nesting via descendants.
    for desc in [node].into_iter().chain(cx.descendants(node)) {
        if let NodeKind::Pair { key, value } = *cx.kind(desc)
            && let NodeKind::Sym(sym) = *cx.kind(key)
                && cx.symbol_str(sym) == "id" && matches!(*cx.kind(value), NodeKind::False_) {
                    return true;
                }
    }
    // Only consider descendants of hash args to avoid false positives from
    // unrelated subexpressions? Upstream searches the whole send node, so
    // the broad search above matches. Constrain to hash-containing args:
    // if the arg itself is not a hash and contains no hash, the pair match
    // above could still hit e.g. `create_table(id_false_helper)` — but such
    // a pair node cannot exist outside a hash literal in valid Ruby, so the
    // broad match is safe.
    false
}

/// Last-argument `(block-pass (sym :timestamps))`.
fn has_timestamps_block_pass(cx: &Cx<'_>, node: NodeId) -> bool {
    for &arg in cx.call_arguments(node) {
        if let NodeKind::BlockPass(inner) = *cx.kind(arg)
            && let Some(sym_node) = inner.get()
                && let NodeKind::Sym(sym) = *cx.kind(sym_node)
                    && cx.symbol_str(sym) == "timestamps" {
                        return true;
                    }
    }
    false
}

/// `timestamps` or `datetime :created_at/:updated_at` (sym or str) anywhere
/// in the block body.
fn time_columns_included(cx: &Cx<'_>, body: NodeId) -> bool {
    let mut stack = vec![body];
    // Include the body node itself plus all descendants.
    stack.extend(cx.descendants(body));
    for id in stack {
        if let NodeKind::Send { method, .. } = *cx.kind(id) {
            let name = cx.symbol_str(method);
            if name == "timestamps" {
                return true;
            }
            if name == "datetime" && datetime_has_created_or_updated(cx, id) {
                return true;
            }
        }
    }
    false
}

fn datetime_has_created_or_updated(cx: &Cx<'_>, node: NodeId) -> bool {
    for &arg in cx.call_arguments(node) {
        match *cx.kind(arg) {
            NodeKind::Sym(sym) => {
                let s = cx.symbol_str(sym);
                if s == "created_at" || s == "updated_at" {
                    return true;
                }
            }
            NodeKind::Str(sid) => {
                let s = cx.string_str(sid);
                if s == "created_at" || s == "updated_at" {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::CreateTableWithTimestamps;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_bare_create_table() {
        test::<CreateTableWithTimestamps>().expect_offense(indoc! {r#"
            create_table :users
            ^^^^^^^^^^^^ Add timestamps when creating a new table.
        "#});
    }

    #[test]
    fn flags_block_without_timestamps() {
        test::<CreateTableWithTimestamps>().expect_offense(indoc! {r#"
            create_table :users do |t|
            ^^^^^^^^^^^^ Add timestamps when creating a new table.
              t.string :name
            end
        "#});
    }

    #[test]
    fn flags_empty_block() {
        test::<CreateTableWithTimestamps>().expect_offense(indoc! {r#"
            create_table :users do |t|
            ^^^^^^^^^^^^ Add timestamps when creating a new table.
            end
        "#});
    }

    #[test]
    fn does_not_flag_with_timestamps() {
        test::<CreateTableWithTimestamps>().expect_no_offenses(indoc! {r#"
            create_table :users do |t|
              t.string :name
              t.timestamps
            end
        "#});
    }

    #[test]
    fn does_not_flag_with_datetime_created_at() {
        test::<CreateTableWithTimestamps>().expect_no_offenses(indoc! {r#"
            create_table :users do |t|
              t.datetime :created_at, default: -> { 'CURRENT_TIMESTAMP' }
            end
        "#});
    }

    #[test]
    fn does_not_flag_id_false() {
        test::<CreateTableWithTimestamps>().expect_no_offenses(indoc! {r#"
            create_table :users, :articles, id: false do |t|
              t.integer :user_id
            end
        "#});
    }

    #[test]
    fn does_not_flag_receiver_form() {
        test::<CreateTableWithTimestamps>().expect_no_offenses("foo.create_table :users\n");
    }
}
murphy_plugin_api::submit_cop!(CreateTableWithTimestamps);
