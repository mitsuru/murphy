//! `Rails/ActiveRecordCallbacksOrder` — declare callbacks in execution order.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/ActiveRecordCallbacksOrder
//! upstream_version_checked: 2.35.0
//! version_added: "2.7"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_class` over `CALLBACKS_IN_ORDER`
//!   (19 callbacks): each out-of-order callback send flags with
//!   `` `<current>` is supposed to appear before `<previous>`. `` and swaps
//!   with its nearest preceding callback sibling via
//!   `insert_before(previous) + remove(current)` over
//!   `range_with_comments_and_lines` ranges (own-line comments travel with
//!   the moved line). Upstream loops corrections to a fixpoint; Murphy
//!   applies one pass per run, so multi-offense files converge over repeated
//!   `--fix` runs. Offense detection is exact in a single pass. File scope
//!   (`Include: models`) is enforced via the murphy-rails pack default.yml.
//! ```
//!
//! ## Matched shapes
//!
//! - `class U < ApplicationRecord; after_commit :a; after_save :b; end` →
//!   offense on `after_save`, swapped before `after_commit`
//! - Correctly ordered callbacks, non-callback macros, and `def`s → no offense
//!
//! ## Autocorrect
//!
//! Move the flagged line (with its own-line comments) before the previous
//! callback sibling's line; the flagged line itself is removed.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ActiveRecordCallbacksOrder;

/// Upstream `CALLBACKS_IN_ORDER`.
const CALLBACKS_IN_ORDER: &[&str] = &[
    "after_initialize",
    "before_validation",
    "after_validation",
    "before_save",
    "around_save",
    "before_create",
    "around_create",
    "after_create",
    "before_update",
    "around_update",
    "after_update",
    "before_destroy",
    "around_destroy",
    "after_destroy",
    "after_save",
    "after_commit",
    "after_rollback",
    "after_find",
    "after_touch",
];

fn callback_index(method: &str) -> Option<usize> {
    CALLBACKS_IN_ORDER.iter().position(|&m| m == method)
}

#[cop(
    name = "Rails/ActiveRecordCallbacksOrder",
    description = "Order callback declarations in the order in which they will be executed.",
    default_enabled = false,
    options = NoOptions,
)]
impl ActiveRecordCallbacksOrder {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Upstream `callback?`: a `send` whose method is a known callback.
/// (`send_type?` only — `csend` never matches upstream.)
fn is_callback(cx: &Cx<'_>, node: NodeId) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return false;
    }
    cx.method_name(node)
        .is_some_and(|m| callback_index(m).is_some())
}

fn check(class_node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Class { body, .. } = *cx.kind(class_node) else {
        return;
    };
    let Some(b) = body.get() else {
        return;
    };
    // Class body statements in source order (single-send body or `Begin`).
    let stmts: Vec<NodeId> = match *cx.kind(b) {
        NodeKind::Begin(list) => cx.list(list).to_vec(),
        _ => vec![b],
    };
    let callbacks: Vec<NodeId> = stmts.into_iter().filter(|&s| is_callback(cx, s)).collect();
    let mut previous_index: Option<usize> = None;
    let mut previous_callback: &str = "";
    for (i, &node) in callbacks.iter().enumerate() {
        let method = cx.method_name(node).unwrap_or_default().to_owned();
        let index = callback_index(&method).unwrap_or(usize::MAX);
        if previous_index.is_some_and(|prev| index < prev) {
            let msg = format!(
                "`{method}` is supposed to appear before `{previous_callback}`."
            );
            cx.emit_offense(cx.range(node), &msg, None);
            // Upstream `autocorrect`: swap with the nearest preceding
            // callback sibling (`left_siblings.reverse_each.find`).
            if i > 0 {
                let previous = callbacks[i - 1];
                let current_range = cx.range_with_comments_and_lines(node);
                let previous_range = cx.range_with_comments_and_lines(previous);
                let current_src = cx.raw_source(current_range).to_owned();
                // `insert_before(previous_range, current source)` as a
                // zero-width insert; the current line is removed below.
                // Boundary touches are not overlaps, so both edits apply.
                cx.emit_edit(
                    murphy_plugin_api::Range {
                        start: previous_range.start,
                        end: previous_range.start,
                    },
                    &current_src,
                );
                cx.emit_edit(current_range, "");
            }
        }
        previous_index = Some(index);
        previous_callback = cx.method_name(node).unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::ActiveRecordCallbacksOrder;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_single_swap() {
        test::<ActiveRecordCallbacksOrder>().expect_correction(
            indoc! {r#"
                class User < ApplicationRecord
                  after_commit :after_commit_callback
                  after_save :after_save_callback
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `after_save` is supposed to appear before `after_commit`.
                end
            "#},
            "class User < ApplicationRecord\n  after_save :after_save_callback\n  after_commit :after_commit_callback\nend\n",
        );
    }

    #[test]
    fn flags_multiple_out_of_order() {
        test::<ActiveRecordCallbacksOrder>().expect_offense(indoc! {r#"
            class User < ApplicationRecord
              scope :admins, -> { where(admin: true) }

              after_commit :after_commit_callback
              after_save :after_save_callback
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `after_save` is supposed to appear before `after_commit`.

              def some_method
              end

              before_validation :before_validation_callback
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `before_validation` is supposed to appear before `after_save`.
              some_other_macros :foo
            end
        "#});
    }

    #[test]
    fn corrects_with_preceding_comment() {
        test::<ActiveRecordCallbacksOrder>().expect_correction(
            indoc! {r#"
                class User < ApplicationRecord
                  # This is a
                  # multiline
                  # comment for after_commit.
                  after_commit :after_commit_callback
                  # This is another
                  # multiline
                  # comment for after_save.
                  after_save :after_save_callback
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `after_save` is supposed to appear before `after_commit`.
                end
            "#},
            "class User < ApplicationRecord\n  # This is another\n  # multiline\n  # comment for after_save.\n  after_save :after_save_callback\n  # This is a\n  # multiline\n  # comment for after_commit.\n  after_commit :after_commit_callback\nend\n",
        );
    }

    #[test]
    fn ignores_correct_order() {
        test::<ActiveRecordCallbacksOrder>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              scope :admins, -> { where(admin: true) }

              before_validation :before_validation_callback
              after_save :after_save_callback

              def some_method
              end

              after_commit :after_commit_callback
            end
        "#});
    }

    #[test]
    fn ignores_same_type_adjacent() {
        test::<ActiveRecordCallbacksOrder>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              after_save :after_save_callback1
              after_save :after_save_callback2
            end
        "#});
    }

    #[test]
    fn ignores_no_callbacks() {
        test::<ActiveRecordCallbacksOrder>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              def some_method
              end
            end
        "#});
    }

    #[test]
    fn ignores_csend_callback() {
        // Upstream `callback?` is `send_type?`-only.
        test::<ActiveRecordCallbacksOrder>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              after_commit :after_commit_callback
              obj&.after_save(:after_save_callback)
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(ActiveRecordCallbacksOrder);
