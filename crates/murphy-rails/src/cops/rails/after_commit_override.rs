//! `Rails/AfterCommitOverride` — only one `after_*_commit :name` per model.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/AfterCommitOverride
//! upstream_version_checked: 2.35.0
//! version_added: "2.8"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_class` over `ClassSendNodeHelper`:
//!   top-level `send` children of the class body whose method is one of
//!   `after_commit`, `after_create_commit`, `after_update_commit`,
//!   `after_save_commit`, `after_destroy_commit` with a leading `sym`
//!   argument. The second and later uses of the same callback name flag
//!   with `` There can only be one `after_*_commit :<name>` hook defined
//!   for a model. ``. Sends without args, with non-sym first args
//!   (lambdas), `csend`, or nested inside `def` do not flag. No
//!   autocorrect upstream. No file-scope gating upstream.
//! ```
//!
//! ## Matched shapes
//!
//! - `class U; after_create_commit :a; after_commit :a, on: :update; end` →
//!   offense on the second (and later) `:a` sends
//! - Different names, non-commit callbacks (`before_save`), bare
//!   `after_commit`, or lambda callbacks → no offense

use std::collections::HashSet;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct AfterCommitOverride;

const CALLBACKS: &[&str] = &[
    "after_commit",
    "after_create_commit",
    "after_update_commit",
    "after_save_commit",
    "after_destroy_commit",
];

#[cop(
    name = "Rails/AfterCommitOverride",
    description = "There can only be one `after_*_commit` hook defined for a model.",
    default_enabled = false,
    options = NoOptions,
)]
impl AfterCommitOverride {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn is_after_commit_callback(cx: &Cx<'_>, node: NodeId) -> bool {
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return false;
    }
    let Some(method) = cx.method_name(node) else {
        return false;
    };
    if !CALLBACKS.contains(&method) {
        return false;
    }
    // Upstream `named_callback?`: first argument exists and is `sym_type?`.
    let args = cx.call_arguments(node);
    let Some(&first) = args.first() else {
        return false;
    };
    matches!(*cx.kind(first), NodeKind::Sym(_))
}

fn callback_name(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    let args = cx.call_arguments(node);
    let first = args.first()?;
    let NodeKind::Sym(sym) = *cx.kind(*first) else {
        return None;
    };
    Some(cx.symbol_str(sym).to_string())
}

fn check(class_node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Class { body, .. } = *cx.kind(class_node) else {
        return;
    };
    let Some(b) = body.get() else {
        return;
    };
    // Mirrors `ClassSendNodeHelper#class_send_nodes`: single-send body or
    // immediate `send` children of a `Begin` body. Nested sends (inside
    // `def`, blocks, conditionals) do not flag upstream.
    let sends: Vec<NodeId> = match *cx.kind(b) {
        NodeKind::Begin(list) => cx
            .list(list)
            .iter()
            .copied()
            .filter(|&s| matches!(*cx.kind(s), NodeKind::Send { .. }))
            .collect(),
        _ => {
            if matches!(*cx.kind(b), NodeKind::Send { .. }) {
                vec![b]
            } else {
                Vec::new()
            }
        }
    };
    let mut seen: HashSet<String> = HashSet::new();
    for node in sends {
        if !is_after_commit_callback(cx, node) {
            continue;
        }
        let Some(name) = callback_name(cx, node) else {
            continue;
        };
        if seen.contains(&name) {
            let msg = format!(
                "There can only be one `after_*_commit :{name}` hook defined for a model."
            );
            cx.emit_offense(cx.range(node), &msg, None);
        } else {
            seen.insert(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AfterCommitOverride;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_callback_name() {
        test::<AfterCommitOverride>().expect_offense(indoc! {r#"
            class User < ApplicationRecord
              after_create_commit :log_action
              after_commit :log_action, on: :update
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ There can only be one `after_*_commit :log_action` hook defined for a model.
              after_destroy_commit :log_action
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ There can only be one `after_*_commit :log_action` hook defined for a model.
            end
        "#});
    }

    #[test]
    fn allows_different_names() {
        test::<AfterCommitOverride>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              after_create_commit :log_create_action
              after_destroy_commit :log_destroy_action
            end
        "#});
    }

    #[test]
    fn allows_different_callback_types_same_name() {
        test::<AfterCommitOverride>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              before_save :log_action
              after_create_commit :log_action
            end
        "#});
    }

    #[test]
    fn allows_lambda_callbacks() {
        test::<AfterCommitOverride>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              after_commit -> { foo }
              after_destroy_commit -> { foo }
            end
        "#});
    }

    #[test]
    fn ignores_bare_calls_without_arguments() {
        test::<AfterCommitOverride>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              after_commit
              after_destroy_commit
            end
        "#});
    }

    #[test]
    fn ignores_nested_callbacks_inside_def() {
        test::<AfterCommitOverride>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              def setup
                after_commit :log_action
              end
              after_commit :other_action
            end
        "#});
    }

    #[test]
    fn ignores_csend_callbacks() {
        test::<AfterCommitOverride>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              after_commit :log_action
              obj&.after_commit(:log_action)
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(AfterCommitOverride);
