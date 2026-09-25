//! `Rails/HasManyOrHasOneDependent` — require `:dependent` on `has_many`/`has_one`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/HasManyOrHasOneDependent
//! upstream_version_checked: 2.35.0
//! version_added: "0.50"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:has_many, :has_one]
//!   gating (any receiver, so `base.has_many` flags), trailing-hash option
//!   validation (`dependent:` with any value including `nil`, or `through:`
//!   with a non-nil value), `**{...}` kwsplat-hash extraction (`**bar`
//!   stays invalid), `with_options` block validation including nesting
//!   (any ancestor `with_options` block with valid options suppresses),
//!   `ActiveResource::Base` exclusion via the enclosing class superclass,
//!   and `readonly? → true` exclusion. Lambda/extension-block forms are
//!   handled (trailing hash must be last; a `has_many ... do` extension
//!   block does not itself suppress). Selector-only offense, no autocorrect.
//!   Upstream Include (`**/app/models/**/*.rb`) has no file-path
//!   infrastructure in Murphy yet, so the cop fires in all files (audit
//!   tracked by murphy-4gd.1.15).
//! ```
//!
//! Looks for `has_many` or `has_one` associations that don't specify a
//! `:dependent` option.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct HasManyOrHasOneDependent;

#[cop(
    name = "Rails/HasManyOrHasOneDependent",
    description = "Define the dependent option to the `has_many` and `has_one` associations.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl HasManyOrHasOneDependent {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[has_many has_one]`.
    #[on_node(kind = "send", methods = ["has_many", "has_one"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Upstream `active_resource?(node.parent)` — enclosing class
        // inheriting `ActiveResource::Base` is excluded.
        if in_active_resource_class(cx, node) {
            return;
        }
        // Upstream `readonly_model?` — a `def readonly?; true; end` in the
        // enclosing type suppresses.
        if in_readonly_model(cx, node) {
            return;
        }
        // Upstream `!association_without_options? && valid_options?` gate.
        if has_valid_trailing_options(cx, node) {
            return;
        }
        // Upstream `valid_options_in_with_options_block?`.
        if in_valid_with_options_block(cx, node) {
            return;
        }
        cx.emit_offense(cx.selector(node), "Specify a `:dependent` option.", None);
    }
}

/// Any enclosing `class ... < ActiveResource::Base` (leading `::` folds via
/// `const_name`, mirroring the upstream `ActiveResource::Base` search).
fn in_active_resource_class(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        let NodeKind::Class { superclass, .. } = *cx.kind(anc) else {
            continue;
        };
        let Some(super_id) = superclass.get() else {
            continue;
        };
        if cx.const_name(super_id).as_deref() == Some("ActiveResource::Base") {
            return true;
        }
    }
    false
}

/// Enclosing class/module contains `def readonly?` with zero args and a
/// `true` body (mirrors upstream `(def :readonly? (args) (true))`).
fn in_readonly_model(cx: &Cx<'_>, node: NodeId) -> bool {
    // Find the nearest enclosing class/module for a scoped search.
    let mut scope: Option<NodeId> = None;
    for anc in cx.ancestors(node) {
        if matches!(
            *cx.kind(anc),
            NodeKind::Class { .. } | NodeKind::Module { .. }
        ) {
            scope = Some(anc);
            break;
        }
    }
    let Some(scope) = scope else {
        return false;
    };
    for desc in cx.descendants(scope) {
        let NodeKind::Def { name, args, body, .. } = *cx.kind(desc) else {
            continue;
        };
        if cx.symbol_str(name) != "readonly?" {
            continue;
        }
        // Zero args.
        let NodeKind::Args(params) = *cx.kind(args) else {
            continue;
        };
        if !cx.list(params).is_empty() {
            continue;
        }
        let Some(body) = body.get() else {
            continue;
        };
        if matches!(*cx.kind(body), NodeKind::True_) {
            return true;
        }
    }
    false
}

/// Trailing-hash validation (mirrors `association_with_options?` +
/// `valid_options?`). No trailing `Hash` arg means no options (invalid).
fn has_valid_trailing_options(cx: &Cx<'_>, node: NodeId) -> bool {
    let args = cx.call_arguments(node);
    let Some(&last) = args.last() else {
        return false;
    };
    let NodeKind::Hash(pairs) = *cx.kind(last) else {
        return false;
    };
    valid_option_pairs(cx, cx.list(pairs))
}

/// `dependent:` (any value, including `nil`) or `through:` with a non-nil
/// value counts as valid. Handles `**{...}` via kwsplat-hash extraction.
fn valid_option_pairs(cx: &Cx<'_>, pairs: &[NodeId]) -> bool {
    let effective: &[NodeId] = match pairs.first() {
        Some(&first) if matches!(*cx.kind(first), NodeKind::Kwsplat(_)) => {
            let NodeKind::Kwsplat(inner) = *cx.kind(first) else {
                return false;
            };
            let Some(inner) = inner.get() else {
                return false;
            };
            if !matches!(*cx.kind(inner), NodeKind::Hash(_)) {
                // `**bar` (variable) — no statically known options.
                return false;
            }
            let NodeKind::Hash(inner_pairs) = *cx.kind(inner) else {
                return false;
            };
            // Upstream only unwraps the leading kwsplat-hash; keep the
            // slice alive via a small static-free path: check the inner
            // pairs directly.
            return inner_valid(cx, cx.list(inner_pairs));
        }
        _ => pairs,
    };
    inner_valid(cx, effective)
}

fn inner_valid(cx: &Cx<'_>, pairs: &[NodeId]) -> bool {
    for &pair_id in pairs {
        let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
            continue;
        };
        let NodeKind::Sym(key_sym) = *cx.kind(key) else {
            continue;
        };
        let key_name = cx.symbol_str(key_sym);
        if key_name == "dependent" {
            // Any `dependent:` value (including `nil`) suppresses.
            return true;
        }
        if key_name == "through" && !matches!(*cx.kind(value), NodeKind::Nil) {
            return true;
        }
    }
    false
}

/// Any ancestor `with_options <hash> do` block with valid options suppresses
/// (mirrors `contain_valid_options_in_with_options_block?` including nesting;
/// `begin`/extension-block intermediaries are covered by the full-ancestor walk).
fn in_valid_with_options_block(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        let NodeKind::Block { call, args, .. } = *cx.kind(anc) else {
            continue;
        };
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(call)
        else {
            continue;
        };
        if receiver.get().is_some() {
            continue;
        }
        if cx.symbol_str(method) != "with_options" {
            continue;
        }
        // Block args `_?` — zero or one parameter.
        let NodeKind::Args(params) = *cx.kind(args) else {
            continue;
        };
        if cx.list(params).len() > 1 {
            continue;
        }
        let call_args = cx.call_arguments(call);
        let Some(&first) = call_args.first() else {
            continue;
        };
        let NodeKind::Hash(pairs) = *cx.kind(first) else {
            continue;
        };
        if valid_option_pairs(cx, cx.list(pairs)) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::HasManyOrHasOneDependent;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_has_one_without_options() {
        test::<HasManyOrHasOneDependent>().expect_offense(indoc! {r#"
            class Person < ApplicationRecord
              has_one :foo
              ^^^^^^^ Specify a `:dependent` option.
            end
        "#});
    }

    #[test]
    fn flags_has_many_without_options() {
        test::<HasManyOrHasOneDependent>().expect_offense(indoc! {r#"
            class Person < ApplicationRecord
              has_many :foo
              ^^^^^^^^ Specify a `:dependent` option.
            end
        "#});
    }

    #[test]
    fn flags_with_class_name_only() {
        test::<HasManyOrHasOneDependent>().expect_offense(indoc! {r#"
            class Person < ApplicationRecord
              has_one :foo, class_name: 'bar'
              ^^^^^^^ Specify a `:dependent` option.
            end
        "#});
    }

    #[test]
    fn allows_dependent() {
        test::<HasManyOrHasOneDependent>().expect_no_offenses(indoc! {r#"
            class Person < ApplicationRecord
              has_one :foo, dependent: :destroy
            end
        "#});
    }

    #[test]
    fn allows_dependent_nil() {
        test::<HasManyOrHasOneDependent>().expect_no_offenses(indoc! {r#"
            class Person < ApplicationRecord
              has_many :foo, dependent: nil
            end
        "#});
    }

    #[test]
    fn allows_through() {
        test::<HasManyOrHasOneDependent>()
            .expect_no_offenses("has_many :foo, through: :bars\n");
    }

    #[test]
    fn flags_through_nil() {
        test::<HasManyOrHasOneDependent>().expect_offense(indoc! {r#"
            class Person < ApplicationRecord
              has_many :foo, through: nil
              ^^^^^^^^ Specify a `:dependent` option.
            end
        "#});
    }

    #[test]
    fn allows_kwsplat_hash() {
        test::<HasManyOrHasOneDependent>().expect_no_offenses(indoc! {r#"
            class Person < ApplicationRecord
              has_one :foo, **{dependent: :destroy}
            end
        "#});
    }

    #[test]
    fn flags_kwsplat_variable() {
        test::<HasManyOrHasOneDependent>().expect_offense(indoc! {r#"
            class Person < ApplicationRecord
              has_one :foo, **bar
              ^^^^^^^ Specify a `:dependent` option.
            end
        "#});
    }

    #[test]
    fn flags_lambda_without_options() {
        test::<HasManyOrHasOneDependent>().expect_offense(indoc! {r#"
            class User < ApplicationRecord
              has_many :articles, -> { where(active: true) }
              ^^^^^^^^ Specify a `:dependent` option.
            end
        "#});
    }

    #[test]
    fn allows_lambda_with_dependent() {
        test::<HasManyOrHasOneDependent>().expect_no_offenses(indoc! {r#"
            class User < ApplicationRecord
              has_many :articles, -> { where(active: true) }, dependent: :destroy
            end
        "#});
    }

    #[test]
    fn allows_with_options_dependent() {
        test::<HasManyOrHasOneDependent>().expect_no_offenses(indoc! {r#"
            class Person < ApplicationRecord
              with_options dependent: :destroy do
                has_one :foo
              end
            end
        "#});
    }

    #[test]
    fn flags_with_options_through_nil() {
        test::<HasManyOrHasOneDependent>().expect_offense(indoc! {r#"
            class Person < ApplicationRecord
              with_options through: nil do
                has_many :foo
                ^^^^^^^^ Specify a `:dependent` option.
              end
            end
        "#});
    }

    #[test]
    fn allows_nested_with_options() {
        test::<HasManyOrHasOneDependent>().expect_no_offenses(indoc! {r#"
            class Article < ApplicationRecord
              with_options dependent: :destroy do
                has_many :tags
                with_options class_name: 'Tag' do
                  has_many :special_tags, foreign_key: :special_id, inverse_of: :special
                end
              end
            end
        "#});
    }

    #[test]
    fn allows_extension_block_in_valid_with_options() {
        test::<HasManyOrHasOneDependent>().expect_no_offenses(indoc! {r#"
            class Person < ApplicationRecord
              with_options dependent: :destroy do
                has_many :foo do
                  def bar
                  end
                end
              end
            end
        "#});
    }

    #[test]
    fn flags_receiver_without_options() {
        test::<HasManyOrHasOneDependent>().expect_offense(indoc! {r#"
            module Foo
              def self.included(base)
                base.has_many :bazs
                     ^^^^^^^^ Specify a `:dependent` option.
              end
            end
        "#});
    }

    #[test]
    fn allows_active_resource() {
        test::<HasManyOrHasOneDependent>().expect_no_offenses(indoc! {r#"
            class User < ActiveResource::Base
              has_many :projects, class_name: 'API::Project'
            end
        "#});
    }

    #[test]
    fn allows_readonly_true() {
        test::<HasManyOrHasOneDependent>().expect_no_offenses(indoc! {r#"
            class Person < ActiveRecord::Base
              has_one :foo

              def readonly?
                true
              end
            end
        "#});
    }

    #[test]
    fn flags_readonly_false() {
        test::<HasManyOrHasOneDependent>().expect_offense(indoc! {r#"
            class Person < ActiveRecord::Base
              has_one :foo
              ^^^^^^^ Specify a `:dependent` option.

              def readonly?
                false
              end
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(HasManyOrHasOneDependent);
