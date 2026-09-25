//! `Rails/DuplicateAssociation` — flag associations defined multiple times.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/DuplicateAssociation
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_class` with `active_record?`
//!   (`ApplicationRecord` / `ActiveRecord::Base`) gating and
//!   `class_send_nodes` (single-send body or direct `Begin` children).
//!   Name duplicates group by sym/str value (`to_sym`); `class_name`
//!   duplicates only fire for non-`belongs_to` associations with exactly one
//!   trailing hash arg containing a single `class_name:` pair, grouped by the
//!   value source. Autocorrect keeps the last duplicate (first replaced with
//!   last source, rest removed line-wise). Whole-send offense ranges.
//! ```
//!
//! ## Matched shapes
//!
//! - `class Foo < ApplicationRecord; belongs_to :foo; has_one :foo; end`
//!   → both `foo` associations flag.
//! - `class Foo < ApplicationRecord; has_many :a, class_name: "Foo"; has_many :b, class_name: "Foo"; end`
//!   → both `class_name: "Foo"` flag.
//!
//! Non-ActiveRecord classes and single associations do not flag.

use std::collections::HashMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const ASSOCIATION_METHODS: &[&str] = &[
    "belongs_to",
    "has_one",
    "has_many",
    "has_and_belongs_to_many",
];

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DuplicateAssociation;

#[cop(
    name = "Rails/DuplicateAssociation",
    description = "Don't repeat associations in a model.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl DuplicateAssociation {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class { superclass, .. } = *cx.kind(node) else {
            return;
        };
        let Some(super_id) = superclass.get() else {
            return;
        };
        // Upstream `active_record?`: `ApplicationRecord` or `ActiveRecord::Base`.
        let super_name = cx.const_name(super_id).unwrap_or_default();
        if super_name != "ApplicationRecord" && super_name != "ActiveRecord::Base" {
            return;
        }
        let sends = class_send_nodes(cx, node);
        let mut assoc_nodes: Vec<NodeId> = Vec::new();
        let mut name_of: HashMap<NodeId, String> = HashMap::new();
        for &send in &sends {
            if let Some(name) = association_name(cx, send) {
                assoc_nodes.push(send);
                name_of.insert(send, name);
            }
        }
        // Name duplicates.
        let mut by_name: HashMap<String, Vec<NodeId>> = HashMap::new();
        for &n in &assoc_nodes {
            by_name
                .entry(name_of.get(&n).cloned().unwrap_or_default())
                .or_default()
                .push(n);
        }
        for (name, nodes) in &by_name {
            if nodes.len() <= 1 {
                continue;
            }
            let msg = format!("Association `{name}` is defined multiple times. Don't repeat associations.");
            emit_group(cx, nodes, &msg);
        }
        // `class_name` duplicates (non-belongs_to, single hash arg).
        let mut by_class: HashMap<String, Vec<NodeId>> = HashMap::new();
        let mut class_display: HashMap<String, String> = HashMap::new();
        for &n in &assoc_nodes {
            if cx.method_name(n) == Some("belongs_to") {
                continue;
            }
            let Some((key, value_src)) = class_name_value(cx, n) else {
                continue;
            };
            by_class.entry(key.clone()).or_default().push(n);
            class_display.insert(key, value_src);
        }
        for (key, nodes) in &by_class {
            if nodes.len() <= 1 {
                continue;
            }
            let display = &class_display[key];
            let msg = format!("Association `class_name: {display}` is defined multiple times. Don't repeat associations.");
            emit_group(cx, nodes, &msg);
        }
    }
}

/// Upstream `class_send_nodes`: single-send body or direct `Begin` children.
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

/// Upstream `association`: bare send with association method and sym/str first arg.
/// Returns the normalized name (`to_sym` — sym and str compare equal).
fn association_name(cx: &Cx<'_>, node: NodeId) -> Option<String> {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return None;
    };
    if receiver.get().is_some() {
        return None;
    }
    if !ASSOCIATION_METHODS.contains(&cx.symbol_str(method)) {
        return None;
    }
    let args = cx.call_arguments(node);
    let first = *args.first()?;
    match *cx.kind(first) {
        NodeKind::Sym(s) => Some(cx.symbol_str(s).to_owned()),
        NodeKind::Str(sid) => Some(cx.string_str(sid).to_owned()),
        _ => None,
    }
}

/// Upstream `class_name` duplicate gate: exactly one trailing arg (`arguments.one?`)
/// which is a single-pair hash `(hash (pair (sym :class_name) $_))`.
/// Returns (group_key, display_source) where group_key is the value source
/// (upstream groups by `class_name.source`) and display is the same source.
fn class_name_value(cx: &Cx<'_>, node: NodeId) -> Option<(String, String)> {
    let args = cx.call_arguments(node);
    if args.len() < 2 {
        return None;
    }
    // `association(node).last` is `$...` (all args after name); `one?` means
    // exactly one trailing arg.
    let trailing = &args[1..];
    if trailing.len() != 1 {
        return None;
    }
    let hash_id = trailing[0];
    let NodeKind::Hash(pairs) = *cx.kind(hash_id) else {
        return None;
    };
    let list = cx.list(pairs);
    // Upstream `(hash (pair ...))` — exactly one pair.
    if list.len() != 1 {
        return None;
    }
    let pair_id = list[0];
    let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
        return None;
    };
    let NodeKind::Sym(k) = *cx.kind(key) else {
        return None;
    };
    if cx.symbol_str(k) != "class_name" {
        return None;
    }
    let src = cx.raw_source(cx.range(value)).to_owned();
    Some((src.clone(), src))
}

/// Upstream `register_offense`: first node replaced with last source, rest
/// removed whole-line (including the last node's original line).
fn emit_group(cx: &Cx<'_>, nodes: &[NodeId], msg: &str) {
    let Some(&first) = nodes.first() else {
        return;
    };
    let Some(&last) = nodes.last() else {
        return;
    };
    let last_src = cx.raw_source(cx.range(last)).to_owned();
    for (idx, &n) in nodes.iter().enumerate() {
        cx.emit_offense(cx.range(n), msg, None);
        if idx == 0 {
            cx.emit_edit(cx.range(first), &last_src);
        } else {
            let line_range = cx.range_by_whole_lines(cx.range(n), true);
            cx.emit_edit(line_range, "");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DuplicateAssociation;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_duplicate_name() {
        test::<DuplicateAssociation>().expect_offense(indoc! {r#"
            class Foo < ApplicationRecord
              belongs_to :foo
              ^^^^^^^^^^^^^^^ Association `foo` is defined multiple times. Don't repeat associations.
              belongs_to :bar
              has_one :foo
              ^^^^^^^^^^^^ Association `foo` is defined multiple times. Don't repeat associations.
            end
        "#});
    }

    #[test]
    fn flags_duplicate_class_name() {
        test::<DuplicateAssociation>().expect_offense(indoc! {r#"
            class Foo < ApplicationRecord
              has_many :foo, class_name: 'Foo'
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Association `class_name: 'Foo'` is defined multiple times. Don't repeat associations.
              has_many :bar, class_name: 'Foo'
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Association `class_name: 'Foo'` is defined multiple times. Don't repeat associations.
              has_one :baz
            end
        "#});
    }

    #[test]
    fn does_not_flag_single() {
        test::<DuplicateAssociation>().expect_no_offenses(indoc! {r#"
            class Foo < ApplicationRecord
              belongs_to :foo
              has_one :bar
            end
        "#});
    }

    #[test]
    fn does_not_flag_non_active_record() {
        test::<DuplicateAssociation>().expect_no_offenses(indoc! {r#"
            class Foo < FooBase
              belongs_to :foo
              has_one :foo
            end
        "#});
    }

    #[test]
    fn does_not_flag_belongs_to_class_name() {
        // `belongs_to` is excluded from class_name grouping upstream.
        test::<DuplicateAssociation>().expect_no_offenses(indoc! {r#"
            class Foo < ApplicationRecord
              belongs_to :foo, class_name: 'Foo'
              belongs_to :bar, class_name: 'Foo'
            end
        "#});
    }

    #[test]
    fn does_not_flag_multi_option_hash() {
        // Upstream `(hash (pair ...))` only matches single-pair hashes.
        test::<DuplicateAssociation>().expect_no_offenses(indoc! {r#"
            class Foo < ApplicationRecord
              has_many :foo, class_name: 'Foo', dependent: :destroy
              has_many :bar, class_name: 'Foo', dependent: :destroy
            end
        "#});
    }

    #[test]
    fn does_not_flag_active_record_base_mismatch() {
        test::<DuplicateAssociation>().expect_no_offenses(indoc! {r#"
            class Foo
              belongs_to :foo
              has_one :foo
            end
        "#});
    }

    #[test]
    fn flags_active_record_base() {
        test::<DuplicateAssociation>().expect_offense(indoc! {r#"
            class Foo < ActiveRecord::Base
              belongs_to :foo
              ^^^^^^^^^^^^^^^ Association `foo` is defined multiple times. Don't repeat associations.
              has_one :foo
              ^^^^^^^^^^^^ Association `foo` is defined multiple times. Don't repeat associations.
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(DuplicateAssociation);
