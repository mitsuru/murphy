//! `Rails/DuplicateScope` — flag scopes sharing the same expression.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/DuplicateScope
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_class` with `active_record?`
//!   (`ApplicationRecord` / `ActiveRecord::Base`) gating and
//!   `class_send_nodes` (single-send body or direct `Begin` children).
//!   Bare `scope` sends grouped by trailing-expression source
//!   (`scope(node)` `$...`); groups with 2+ members flag whole sends.
//!   No autocorrect upstream.
//! ```
//!
//! ## Matched shapes
//!
//! - `class Foo < ApplicationRecord; scope :visible, -> { where(visible: true) }; scope :hidden, -> { where(visible: true) }; end`
//!   → both scopes flag.
//!
//! Different bodies do not flag.

use std::collections::HashMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Multiple scopes share this same expression.";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DuplicateScope;

#[cop(
    name = "Rails/DuplicateScope",
    description = "Multiple scopes share this same expression.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl DuplicateScope {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class { superclass, .. } = *cx.kind(node) else {
            return;
        };
        let Some(super_id) = superclass.get() else {
            return;
        };
        let super_name = cx.const_name(super_id).unwrap_or_default();
        if super_name != "ApplicationRecord" && super_name != "ActiveRecord::Base" {
            return;
        }
        let sends = class_send_nodes(cx, node);
        let mut by_expr: HashMap<String, Vec<NodeId>> = HashMap::new();
        for &send in &sends {
            let Some(key) = scope_key(cx, send) else {
                continue;
            };
            by_expr.entry(key).or_default().push(send);
        }
        for nodes in by_expr.values() {
            if nodes.len() <= 1 {
                continue;
            }
            for &n in nodes {
                cx.emit_offense(cx.range(n), MSG, None);
            }
        }
    }
}

fn class_send_nodes(cx: &Cx<'_>, class_node: NodeId) -> Vec<NodeId> {
    let NodeKind::Class { body, .. } = *cx.kind(class_node) else {
        return Vec::new();
    };
    let Some(b) = body.get() else {
        return Vec::new();
    };
    match *cx.kind(b) {
        NodeKind::Send { .. } => vec![b],
        NodeKind::Begin(list) => cx
            .list(list)
            .iter()
            .filter(|&&id| matches!(*cx.kind(id), NodeKind::Send { .. }))
            .copied()
            .collect(),
        _ => Vec::new(),
    }
}

/// Upstream `scope`: bare `(send nil? :scope _ $...)`. Group key is the
/// trailing-expression source (all args after the name).
fn scope_key(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return None;
    };
    if receiver.get().is_some() {
        return None;
    }
    if cx.symbol_str(method) != "scope" {
        return None;
    }
    let args = cx.call_arguments(node);
    if args.is_empty() {
        return None;
    }
    // First arg is the name (`_`); rest is the expression (`$...`).
    let rest = &args[1..];
    if rest.is_empty() {
        return Some(String::new());
    }
    let parts: Vec<String> = rest
        .iter()
        .map(|&id| cx.raw_source(cx.range(id)).to_owned())
        .collect();
    Some(parts.join("\x1f"))
}

#[cfg(test)]
mod tests {
    use super::DuplicateScope;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_expression() {
        test::<DuplicateScope>().expect_offense(indoc! {r#"
            class Foo < ApplicationRecord
              scope :visible, -> { where(visible: true) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Multiple scopes share this same expression.
              scope :hidden, -> { where(visible: true) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Multiple scopes share this same expression.
            end
        "#});
    }

    #[test]
    fn does_not_flag_different_bodies() {
        test::<DuplicateScope>().expect_no_offenses(indoc! {r#"
            class Foo < ApplicationRecord
              scope :visible, -> { where(visible: true) }
              scope :hidden, -> { where(visible: false) }
            end
        "#});
    }

    #[test]
    fn does_not_flag_single_scope() {
        test::<DuplicateScope>().expect_no_offenses(indoc! {r#"
            class Foo < ApplicationRecord
              scope :visible, -> { where(visible: true) }
            end
        "#});
    }

    #[test]
    fn does_not_flag_non_active_record() {
        test::<DuplicateScope>().expect_no_offenses(indoc! {r#"
            class Foo
              scope :visible, -> { where(visible: true) }
              scope :hidden, -> { where(visible: true) }
            end
        "#});
    }

    #[test]
    fn flags_active_record_base() {
        test::<DuplicateScope>().expect_offense(indoc! {r#"
            class Foo < ActiveRecord::Base
              scope :visible, -> { where(visible: true) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Multiple scopes share this same expression.
              scope :hidden, -> { where(visible: true) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Multiple scopes share this same expression.
            end
        "#});
    }

    #[test]
    fn does_not_flag_receiver_scope() {
        test::<DuplicateScope>().expect_no_offenses(indoc! {r#"
            class Foo < ApplicationRecord
              foo.scope :visible, -> { where(visible: true) }
              foo.scope :hidden, -> { where(visible: true) }
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(DuplicateScope);
