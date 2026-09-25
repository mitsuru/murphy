//! `Rails/Env` — flag `Rails.env` predicate calls that should be feature flags.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/Env
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: on_send for `:env` with a `Rails`
//!   receiver (`const_name` folds `::Rails`), offense on the predicate-call
//!   parent (`is_predicate_method` covers both `Send` and `Csend` parents,
//!   matching upstream duck-typing), with the 2.35.0 `ALLOWED_LIST`
//!   verbatim. No arity checks on either send, matching upstream. When the
//!   predicate call takes a block, Murphy folds the block into the send's
//!   expression range, so the send-only end is recomputed from the selector
//!   and arguments. No autocorrect. Upstream ships `Enabled: false`; Murphy maps
//!   that to `default_enabled = false`. Drift note: rubocop-rails 2.38.0
//!   added `local?` to the allow list; this port targets 2.35.0 where
//!   `Rails.env.local?` still flags.
//! ```
//!
//! ## Matched shape (Send node)
//!
//! `Send(receiver=Const("Rails"), method=:env)` whose parent is a predicate
//! call (`production?`, `development?`, ...) outside the String-like
//! allowlist (`empty?`, `include?`, ...). The offense covers the whole
//! parent predicate call, e.g. `Rails.env.production?`.
//!
//! ## No autocorrect
//!
//! Replacing an environment gate with a feature flag requires product
//! context the cop cannot synthesise. Detect-only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Env;

const MSG: &str = "Use Feature Flags or config instead of `Rails.env`.";

// Derived from `(Rails.env.methods - Object.instance_methods)` ending in
// `?`, minus the environment-specific predicates — verbatim from
// rubocop-rails 2.35.0 (which, unlike 2.38.0, does not allow `local?`).
const ALLOWED_LIST: &[&str] = &[
    "unicode_normalized?",
    "exclude?",
    "empty?",
    "acts_like_string?",
    "include?",
    "is_utf8?",
    "casecmp?",
    "match?",
    "starts_with?",
    "ends_with?",
    "start_with?",
    "end_with?",
    "valid_encoding?",
    "ascii_only?",
    "between?",
];

#[cop(
    name = "Rails/Env",
    description = "Use Feature Flags or config instead of `Rails.env`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl Env {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[env]`.
    #[on_node(kind = "send", methods = ["env"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream `node.receiver&.const_name == 'Rails'` — no arity gate
        // on the `env` send itself.
        let Some(recv) = receiver.get() else {
            return;
        };
        if cx.const_name(recv).as_deref() != Some("Rails") {
            return;
        }
        // Upstream `parent.respond_to?(:predicate_method?) &&
        // parent.predicate_method?` — `is_predicate_method` covers every
        // method-bearing parent (`Send`, `Csend`, block-delegated) the
        // same duck-typed way.
        let Some(parent) = cx.parent(node).get() else {
            return;
        };
        if !cx.is_predicate_method(parent) {
            return;
        }
        let Some(name) = cx.method_name(parent) else {
            return;
        };
        if ALLOWED_LIST.contains(&name) {
            return;
        }
        cx.emit_offense(offense_range(cx, parent), MSG, None);
    }
}

/// Upstream `add_offense(parent)`: the whole predicate-send range. When the
/// predicate call takes a block, Murphy folds the block into the send's
/// expression range (a `Block` starts at its call), so recompute the
/// send-only end instead: the closing paren when the call is parenthesised,
/// else the last argument end (or the selector end when argument-less).
fn offense_range(cx: &Cx<'_>, parent: NodeId) -> Range {
    let range = cx.range(parent);
    if cx.block_node(parent).get().is_none() {
        return range;
    }
    let mut end = range.end;
    if cx.is_parenthesized(parent) {
        let sel_end = cx.selector(parent).end;
        if let Some(paren_end) = cx
            .tokens_in(range)
            .iter()
            .filter(|tok| {
                tok.kind == SourceTokenKind::RightParen && tok.range.start >= sel_end
            })
            .map(|tok| tok.range.end)
            .next()
        {
            end = paren_end;
        }
    } else if let Some(last) = cx.call_arguments(parent).last() {
        end = cx.range(*last).end;
    } else {
        end = cx.selector(parent).end;
    }
    Range {
        start: range.start,
        end: end.min(range.end),
    }
}

#[cfg(test)]
mod tests {
    use super::Env;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_production_predicate() {
        test::<Env>().expect_offense(indoc! {r#"
            Rails.env.production?
            ^^^^^^^^^^^^^^^^^^^^^ Use Feature Flags or config instead of `Rails.env`.
        "#});
    }

    #[test]
    fn flags_cbase_rails() {
        test::<Env>().expect_offense(indoc! {r#"
            ::Rails.env.production?
            ^^^^^^^^^^^^^^^^^^^^^^^ Use Feature Flags or config instead of `Rails.env`.
        "#});
    }

    #[test]
    fn flags_local_predicate() {
        // 2.35.0 has no `local?` exemption (added upstream in 2.38.0).
        test::<Env>().expect_offense(indoc! {r#"
            Rails.env.local?
            ^^^^^^^^^^^^^^^^ Use Feature Flags or config instead of `Rails.env`.
        "#});
    }

    #[test]
    fn flags_predicate_with_arguments() {
        // No arity gate on either send upstream.
        test::<Env>().expect_offense(indoc! {r#"
            Rails.env(x).production?
            ^^^^^^^^^^^^^^^^^^^^^^^^ Use Feature Flags or config instead of `Rails.env`.
        "#});
    }

    #[test]
    fn flags_assignment_value() {
        test::<Env>().expect_offense(indoc! {r#"
            x = Rails.env.production?
                ^^^^^^^^^^^^^^^^^^^^^ Use Feature Flags or config instead of `Rails.env`.
        "#});
    }

    #[test]
    fn flags_safe_navigation_parent() {
        test::<Env>().expect_offense(indoc! {r#"
            Rails.env&.production?
            ^^^^^^^^^^^^^^^^^^^^^^ Use Feature Flags or config instead of `Rails.env`.
        "#});
    }

    #[test]
    fn trims_block_from_offense_range() {
        test::<Env>().expect_offense(indoc! {r#"
            Rails.env.production? { foo }
            ^^^^^^^^^^^^^^^^^^^^^ Use Feature Flags or config instead of `Rails.env`.
        "#});
    }

    #[test]
    fn does_not_flag_allowlisted_empty() {
        test::<Env>().expect_no_offenses("Rails.env.empty?\n");
    }

    #[test]
    fn does_not_flag_allowlisted_include() {
        test::<Env>().expect_no_offenses("Rails.env.include?('x')\n");
    }

    #[test]
    fn does_not_flag_bare_env() {
        test::<Env>().expect_no_offenses("x = Rails.env\n");
    }

    #[test]
    fn does_not_flag_comparison_parent() {
        test::<Env>().expect_no_offenses("Rails.env == 'x'\n");
    }

    #[test]
    fn does_not_flag_non_predicate_parent() {
        test::<Env>().expect_no_offenses("Rails.env.fetch(:foo)\n");
    }
}
murphy_plugin_api::submit_cop!(Env);
