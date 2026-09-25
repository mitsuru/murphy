//! `Rails/EnvironmentComparison` — compare `Rails.env` with predicate methods.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/EnvironmentComparison
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:==, :!=] gating, the
//!   four NodePatterns (string comparison both sides, `to_sym` + symbol
//!   both sides) plus the two symbol-comparison patterns (which warn with
//!   SYM_MSG yet still autocorrect). The patterns require exact arity, so
//!   `Rails.env(x) == 'production'` does not flag. The predicate is built
//!   from the `Rails.env` source and the literal value (`bang` prefix for
//!   `!=`), replacing the whole comparison node. Interpolated strings
//!   (`dstr`) never match `$str`.
//! ```
//!
//! ## Matched shapes (Send node, `==`/`!=`, exactly one argument)
//!
//! - `Rails.env == 'production'` → `Rails.env.production?`.
//! - `Rails.env != 'production'` → `!Rails.env.production?`.
//! - `'production' == Rails.env` → `Rails.env.production?` (either side).
//! - `Rails.env.to_sym == :production` → `Rails.env.production?`.
//! - `Rails.env == :production` → same rewrite, but warns that comparing
//!   with a symbol always evaluates to `false`.
//!
//! `Rails.env == foo` (non-literal) and `Rails.env == 1` do not flag.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EnvironmentComparison;

const SYM_MSG: &str =
    "Do not compare `Rails.env` with a symbol, it will always evaluate to `false`.";

#[cop(
    name = "Rails/EnvironmentComparison",
    description = "Favor `Rails.env.production?` over `Rails.env == 'production'`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EnvironmentComparison {
    // Mirrors upstream `RESTRICT_ON_SEND = %i[== !=]`.
    #[on_node(kind = "send", methods = ["==", "!="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, .. } = *cx.kind(node) else {
            return;
        };
        let Some(recv) = receiver.get() else {
            return;
        };
        let args = cx.call_arguments(node);
        if args.len() != 1 {
            return;
        }
        let arg = args[0];
        let bang = if cx.method_name(node).is_some_and(|m| m == "!=") {
            "!"
        } else {
            ""
        };
        // Upstream `build_predicate_method`: `bang + receiver.source +
        // "." + argument.value + "?"`, trying each side as the Rails.env
        // side. `to_sym` receivers are unwrapped to the inner `Rails.env`.
        let shape = if let Some(env) = as_rails_env(cx, recv) {
            str_value(cx, arg)
                .map(|value| (env, value, false))
                .or_else(|| sym_value(cx, arg).map(|value| (env, value, true)))
        } else if let Some(env) = as_to_sym_env(cx, recv) {
            sym_value(cx, arg).map(|value| (env, value, false))
        } else if let Some(env) = as_rails_env(cx, arg) {
            str_value(cx, recv)
                .map(|value| (env, value, false))
                .or_else(|| sym_value(cx, recv).map(|value| (env, value, true)))
        } else if let Some(env) = as_to_sym_env(cx, arg) {
            sym_value(cx, recv).map(|value| (env, value, false))
        } else {
            None
        };
        let Some((env, value, symbolic)) = shape else {
            return;
        };
        let env_src = cx.raw_source(cx.range(env));
        let prefer = format!("{bang}{env_src}.{value}?");
        if symbolic {
            cx.emit_offense(cx.range(node), SYM_MSG, None);
        } else {
            let current = cx.raw_source(cx.range(node));
            cx.emit_offense(
                cx.range(node),
                &format!("Favor `{prefer}` over `{current}`."),
                None,
            );
        }
        cx.emit_edit(cx.range(node), &prefer);
    }
}

/// `(send (const {nil? cbase} :Rails) :env)` with zero args. Returns the
/// `Rails.env` send node. `const_name` folds `::Rails`, so the cbase form
/// matches identically.
fn as_rails_env(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return None;
    };
    if cx.symbol_str(method) != "env" {
        return None;
    }
    if !cx.call_arguments(node).is_empty() {
        return None;
    }
    let rails = receiver.get()?;
    if cx.const_name(rails).as_deref() != Some("Rails") {
        return None;
    }
    Some(node)
}

/// `(send #as_rails_env :to_sym)` with zero args. Returns the inner
/// `Rails.env` send node (the correction target's receiver source).
fn as_to_sym_env(cx: &Cx<'_>, node: NodeId) -> Option<NodeId> {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return None;
    };
    if cx.symbol_str(method) != "to_sym" {
        return None;
    }
    if !cx.call_arguments(node).is_empty() {
        return None;
    }
    as_rails_env(cx, receiver.get()?)
}

/// Plain string literal contents (`$str` never matches interpolation).
fn str_value<'a>(cx: &Cx<'a>, node: NodeId) -> Option<&'a str> {
    let NodeKind::Str(id) = *cx.kind(node) else {
        return None;
    };
    Some(cx.string_str(id))
}

/// Symbol literal name.
fn sym_value<'a>(cx: &Cx<'a>, node: NodeId) -> Option<&'a str> {
    let NodeKind::Sym(sym) = *cx.kind(node) else {
        return None;
    };
    Some(cx.symbol_str(sym))
}

#[cfg(test)]
mod tests {
    use super::EnvironmentComparison;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_string_comparison() {
        test::<EnvironmentComparison>().expect_offense(indoc! {r#"
            Rails.env == 'production'
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `Rails.env.production?` over `Rails.env == 'production'`.
        "#});
    }

    #[test]
    fn flags_string_inequality() {
        test::<EnvironmentComparison>().expect_offense(indoc! {r#"
            Rails.env != 'production'
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `!Rails.env.production?` over `Rails.env != 'production'`.
        "#});
    }

    #[test]
    fn flags_string_on_lhs() {
        test::<EnvironmentComparison>().expect_offense(indoc! {r#"
            'staging' != Rails.env
            ^^^^^^^^^^^^^^^^^^^^^^ Favor `!Rails.env.staging?` over `'staging' != Rails.env`.
        "#});
    }

    #[test]
    fn flags_to_sym_comparison() {
        test::<EnvironmentComparison>().expect_offense(indoc! {r#"
            Rails.env.to_sym == :production
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `Rails.env.production?` over `Rails.env.to_sym == :production`.
        "#});
    }

    #[test]
    fn flags_symbol_on_lhs_of_to_sym() {
        test::<EnvironmentComparison>().expect_offense(indoc! {r#"
            :production == Rails.env.to_sym
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `Rails.env.production?` over `:production == Rails.env.to_sym`.
        "#});
    }

    #[test]
    fn flags_symbol_comparison_with_sym_message() {
        test::<EnvironmentComparison>().expect_offense(indoc! {r#"
            Rails.env == :production
            ^^^^^^^^^^^^^^^^^^^^^^^^ Do not compare `Rails.env` with a symbol, it will always evaluate to `false`.
        "#});
    }

    #[test]
    fn flags_symbol_inequality_with_sym_message() {
        test::<EnvironmentComparison>().expect_offense(indoc! {r#"
            Rails.env != :test
            ^^^^^^^^^^^^^^^^^^ Do not compare `Rails.env` with a symbol, it will always evaluate to `false`.
        "#});
    }

    #[test]
    fn flags_cbase_rails() {
        test::<EnvironmentComparison>().expect_offense(indoc! {r#"
            ::Rails.env == 'production'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `::Rails.env.production?` over `::Rails.env == 'production'`.
        "#});
    }

    #[test]
    fn does_not_flag_non_literal() {
        test::<EnvironmentComparison>().expect_no_offenses("Rails.env == foo\n");
    }

    #[test]
    fn does_not_flag_integer() {
        test::<EnvironmentComparison>().expect_no_offenses("Rails.env == 1\n");
    }

    #[test]
    fn does_not_flag_interpolated_string() {
        test::<EnvironmentComparison>().expect_no_offenses("Rails.env == \"#{x}\"\n");
    }

    #[test]
    fn does_not_flag_env_with_arguments() {
        test::<EnvironmentComparison>().expect_no_offenses("Rails.env(x) == 'production'\n");
    }

    #[test]
    fn corrects_string_comparison() {
        test::<EnvironmentComparison>()
            .expect_correction(
                indoc! {r#"
                    Rails.env == 'production'
                    ^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `Rails.env.production?` over `Rails.env == 'production'`.
                "#},
                "Rails.env.production?\n",
            )
            .expect_no_offenses("Rails.env.production?\n");
    }

    #[test]
    fn corrects_inequality() {
        test::<EnvironmentComparison>()
            .expect_correction(
                indoc! {r#"
                    Rails.env != 'production'
                    ^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `!Rails.env.production?` over `Rails.env != 'production'`.
                "#},
                "!Rails.env.production?\n",
            )
            .expect_no_offenses("!Rails.env.production?\n");
    }

    #[test]
    fn corrects_symbol_comparison() {
        // SYM_MSG cases still autocorrect upstream.
        test::<EnvironmentComparison>()
            .expect_correction(
                indoc! {r#"
                    Rails.env == :production
                    ^^^^^^^^^^^^^^^^^^^^^^^^ Do not compare `Rails.env` with a symbol, it will always evaluate to `false`.
                "#},
                "Rails.env.production?\n",
            )
            .expect_no_offenses("Rails.env.production?\n");
    }

    #[test]
    fn corrects_to_sym_comparison() {
        test::<EnvironmentComparison>()
            .expect_correction(
                indoc! {r#"
                    Rails.env.to_sym != :staging
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor `!Rails.env.staging?` over `Rails.env.to_sym != :staging`.
                "#},
                "!Rails.env.staging?\n",
            )
            .expect_no_offenses("!Rails.env.staging?\n");
    }
}
murphy_plugin_api::submit_cop!(EnvironmentComparison);
