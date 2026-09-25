//! `Rails/DelegateAllowBlank` — flag `allow_blank` in favour of `allow_nil`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/DelegateAllowBlank
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:delegate] gating,
//!   bare `(send nil? :delegate _ (hash <(pair (sym :allow_blank) true) ...>))`
//!   shape with pair-only offense range and key-replacement autocorrect
//!   (`allow_blank: true` → `allow_nil: true`). Only `true` flags;
//!   `allow_blank: false` and multi-method `delegate :a, :b, ...` do not flag.
//! ```
//!
//! ## Matched shapes
//!
//! - `delegate :foo, to: :bar, allow_blank: true` → `allow_nil: true`.
//!
//! `delegate :foo, to: :bar, allow_blank: false` and
//! `delegate :foo, :bar, to: :baz, allow_blank: true` do not flag.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "`allow_blank` is not a valid option, use `allow_nil`.";

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DelegateAllowBlank;

#[cop(
    name = "Rails/DelegateAllowBlank",
    description = "Do not use allow_blank as an option to delegate.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl DelegateAllowBlank {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[delegate]`.
    #[on_node(kind = "send", methods = ["delegate"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream `(send nil? :delegate ...)` — bare call only.
        if receiver.get().is_some() {
            return;
        }
        let args = cx.call_arguments(node);
        // Upstream pattern is exactly 2 args: method name + options hash.
        // Multi-method `delegate :a, :b, to: :x` has 3+ args and does not flag.
        if args.len() != 2 {
            return;
        }
        let hash_id = args[1];
        let NodeKind::Hash(pairs) = *cx.kind(hash_id) else {
            return;
        };
        for &pair_id in cx.list(pairs) {
            let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
                continue;
            };
            let NodeKind::Sym(sym) = *cx.kind(key) else {
                continue;
            };
            if cx.symbol_str(sym) != "allow_blank" {
                continue;
            }
            // Upstream `(pair (sym :allow_blank) true)` — only `true`.
            if !matches!(*cx.kind(value), NodeKind::True_) {
                continue;
            }
            cx.emit_offense(cx.range(pair_id), MSG, None);
            // Upstream replaces only the key; replacing the whole pair with
            // the same spacing is equivalent for the label form and also
            // normalises `=>` to `:`.
            let replacement = if cx.raw_source(cx.range(pair_id)).contains("=>") {
                ":allow_nil => true".to_owned()
            } else {
                "allow_nil: true".to_owned()
            };
            cx.emit_edit(cx.range(pair_id), &replacement);
            // Only one `allow_blank` pair can exist; stop after first.
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DelegateAllowBlank;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_allow_blank_true() {
        test::<DelegateAllowBlank>().expect_offense(indoc! {r#"
            delegate :foo, to: :bar, allow_blank: true
                                     ^^^^^^^^^^^^^^^^^ `allow_blank` is not a valid option, use `allow_nil`.
        "#});
    }

    #[test]
    fn does_not_flag_allow_nil() {
        test::<DelegateAllowBlank>()
            .expect_no_offenses("delegate :foo, to: :bar, allow_nil: true\n");
    }

    #[test]
    fn does_not_flag_allow_blank_false() {
        test::<DelegateAllowBlank>()
            .expect_no_offenses("delegate :foo, to: :bar, allow_blank: false\n");
    }

    #[test]
    fn does_not_flag_without_options() {
        test::<DelegateAllowBlank>().expect_no_offenses("delegate :foo, to: :bar\n");
    }

    #[test]
    fn does_not_flag_receiver() {
        test::<DelegateAllowBlank>()
            .expect_no_offenses("foo.delegate :bar, to: :baz, allow_blank: true\n");
    }

    #[test]
    fn does_not_flag_multi_method() {
        test::<DelegateAllowBlank>()
            .expect_no_offenses("delegate :foo, :bar, to: :baz, allow_blank: true\n");
    }

    #[test]
    fn corrects_allow_blank() {
        test::<DelegateAllowBlank>()
            .expect_correction(
                indoc! {r#"
                    delegate :foo, to: :bar, allow_blank: true
                                             ^^^^^^^^^^^^^^^^^ `allow_blank` is not a valid option, use `allow_nil`.
                "#},
                "delegate :foo, to: :bar, allow_nil: true\n",
            )
            .expect_no_offenses("delegate :foo, to: :bar, allow_nil: true\n");
    }

    #[test]
    fn corrects_among_other_options() {
        test::<DelegateAllowBlank>()
            .expect_correction(
                indoc! {r#"
                    delegate :foo, to: :bar, allow_blank: true, prefix: true
                                             ^^^^^^^^^^^^^^^^^ `allow_blank` is not a valid option, use `allow_nil`.
                "#},
                "delegate :foo, to: :bar, allow_nil: true, prefix: true\n",
            )
            .expect_no_offenses("delegate :foo, to: :bar, allow_nil: true, prefix: true\n");
    }
}
murphy_plugin_api::submit_cop!(DelegateAllowBlank);
