//! `Rails/AttributeDefaultBlockValue` — flag `attribute ..., default: <mutable/call>` in favour of a block.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/AttributeDefaultBlockValue
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:attribute] gating,
//!   the `(send nil? :attribute _ ?_ (hash <(pair (sym :default) ...) ...>))`
//!   shape (bare receiver, trailing options hash with a `:default` pair),
//!   the `send`/`array`/`hash` offender types, value-only offense range,
//!   and `-> { <source> }` autocorrect. Non-offender literals (sym, str,
//!   int, float, boolean, nil, const) pass through untouched. File scope
//!   (`Include: ['**/app/models/**/*']`) is enforced via the murphy-rails
//!   pack default.yml (engine `cop_applies_to_file` gate, verified vs
//!   rubocop-rails 2.38.0 default.yml, murphy-4gd.1.15).
//! ```
//!
//! ## Matched shapes
//!
//! Bare `Send` with method `attribute` whose last argument is a `Hash`
//! containing a `default:` pair whose value is a `Send` (method call),
//! `Array`, or `Hash`:
//!
//! - `attribute :confirmed_at, :datetime, default: Time.zone.now` → wrap in `-> { }`
//! - `attribute :roles, :string, array: true, default: []` → wrap in `-> { }`
//! - `attribute :configuration, default: {}` → wrap in `-> { }`
//!
//! `attribute :role, :string, default: :customer` (sym) and
//! `attribute :login_count, :integer, default: 0` (int) do not flag.
//!
//! ## Autocorrect
//!
//! Replace the default value only with `-> { <value source> }`; the
//! attribute name, type, and surrounding options pass through untouched.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct AttributeDefaultBlockValue;

#[cop(
    name = "Rails/AttributeDefaultBlockValue",
    description = "Pass method in a block to `:default` option.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl AttributeDefaultBlockValue {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[attribute]`.
    #[on_node(kind = "send", methods = ["attribute"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream pattern requires a bare (`nil?`) receiver.
        if receiver.get().is_some() {
            return;
        }
        let Some(value) = find_default_value(cx, node) else {
            return;
        };
        // Upstream `TYPE_OFFENDERS = %i[send array hash]`.
        if !matches!(
            *cx.kind(value),
            NodeKind::Send { .. } | NodeKind::Array(_) | NodeKind::Hash(_)
        ) {
            return;
        }
        cx.emit_offense(
            cx.range(value),
            "Pass method in a block to `:default` option.",
            None,
        );
        cx.emit_edit(
            cx.range(value),
            &format!("-> {{ {} }}", cx.raw_source(cx.range(value))),
        );
    }
}

/// Find the value of the `:default` pair in the trailing options hash.
/// Mirrors upstream's `(hash <(pair (sym :default) $_) ...>)` tail: only the
/// last argument is considered, and it must be a `Hash`.
fn find_default_value(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    let args = cx.call_arguments(node);
    let &last = args.last()?;
    let NodeKind::Hash(pairs) = *cx.kind(last) else {
        return None;
    };
    for &pair_id in cx.list(pairs) {
        let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
            continue;
        };
        let NodeKind::Sym(key_sym) = *cx.kind(key) else {
            continue;
        };
        if cx.symbol_str(key_sym) == "default" {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::AttributeDefaultBlockValue;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_method_call_default() {
        test::<AttributeDefaultBlockValue>().expect_offense(indoc! {r#"
            attribute :confirmed_at, :datetime, default: Time.zone.now
                                                         ^^^^^^^^^^^^^ Pass method in a block to `:default` option.
        "#});
    }

    #[test]
    fn flags_array_default() {
        test::<AttributeDefaultBlockValue>().expect_offense(indoc! {r#"
            attribute :roles, :string, array: true, default: []
                                                             ^^ Pass method in a block to `:default` option.
        "#});
    }

    #[test]
    fn flags_hash_default() {
        test::<AttributeDefaultBlockValue>().expect_offense(indoc! {r#"
            attribute :configuration, default: {}
                                               ^^ Pass method in a block to `:default` option.
        "#});
    }

    #[test]
    fn does_not_flag_symbol_default() {
        test::<AttributeDefaultBlockValue>()
            .expect_no_offenses("attribute :role, :string, default: :customer\n");
    }

    #[test]
    fn does_not_flag_boolean_default() {
        test::<AttributeDefaultBlockValue>()
            .expect_no_offenses("attribute :activated, :boolean, default: false\n");
    }

    #[test]
    fn does_not_flag_integer_default() {
        test::<AttributeDefaultBlockValue>()
            .expect_no_offenses("attribute :login_count, :integer, default: 0\n");
    }

    #[test]
    fn does_not_flag_const_default() {
        test::<AttributeDefaultBlockValue>().expect_no_offenses(
            "FOO = 123\nattribute :custom_attribute, :integer, default: FOO\n",
        );
    }

    #[test]
    fn does_not_flag_string_default() {
        // `str` is not in upstream `TYPE_OFFENDERS`.
        test::<AttributeDefaultBlockValue>()
            .expect_no_offenses("attribute :name, :string, default: \"anon\"\n");
    }

    #[test]
    fn does_not_flag_receiver_attribute() {
        // `obj.attribute(...)` is not the bare class-method form.
        test::<AttributeDefaultBlockValue>()
            .expect_no_offenses("obj.attribute :roles, :string, default: []\n");
    }

    #[test]
    fn corrects_method_call_default() {
        test::<AttributeDefaultBlockValue>()
            .expect_correction(
                indoc! {r#"
                    attribute :confirmed_at, :datetime, default: Time.zone.now
                                                                 ^^^^^^^^^^^^^ Pass method in a block to `:default` option.
                "#},
                "attribute :confirmed_at, :datetime, default: -> { Time.zone.now }\n",
            )
            .expect_no_offenses(
                "attribute :confirmed_at, :datetime, default: -> { Time.zone.now }\n",
            );
    }

    #[test]
    fn corrects_array_default() {
        test::<AttributeDefaultBlockValue>()
            .expect_correction(
                indoc! {r#"
                    attribute :roles, :string, array: true, default: []
                                                                     ^^ Pass method in a block to `:default` option.
                "#},
                "attribute :roles, :string, array: true, default: -> { [] }\n",
            )
            .expect_no_offenses("attribute :roles, :string, array: true, default: -> { [] }\n");
    }

    #[test]
    fn corrects_hash_default() {
        test::<AttributeDefaultBlockValue>()
            .expect_correction(
                indoc! {r#"
                    attribute :configuration, default: {}
                                                       ^^ Pass method in a block to `:default` option.
                "#},
                "attribute :configuration, default: -> { {} }\n",
            )
            .expect_no_offenses("attribute :configuration, default: -> { {} }\n");
    }
}
murphy_plugin_api::submit_cop!(AttributeDefaultBlockValue);
