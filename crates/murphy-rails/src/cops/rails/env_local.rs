//! `Rails/EnvLocal` — replace `development? || test?` with `local?`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/EnvLocal
//! upstream_version_checked: 2.35.0
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 `on_or`/`on_and`: the `rails_env_local?`
//!   pattern requires zero-arg `Rails.env` + zero-arg `development?`/`test?`
//!   (`const_name` folds `::Rails`), so `development?(x)` never matches.
//!   Left-nested chains (`a || b || c`) collect rhs plus lhs.rhs exactly like
//!   upstream; the nested-lhs type checks (`or_type?` / `operator_keyword?`)
//!   cover both symbol (`||`/`&&`) and keyword (`or`/`and`) spellings on both
//!   sides since Murphy folds them to the same `Or`/`And` nodes. Parenthesised
//!   operands wrap in `Begin` and never match, matching upstream. The offense
//!   spans from the second collected operand to the end of the first, and the
//!   rewrite is `Rails.env.local?` (`!Rails.env.local?` for the negated
//!   `&&` form, even when the source used `not`). Gated on
//!   `TargetRailsVersion >= 7.1` (unset means newest). Upstream ships
//!   `Enabled: pending`; Murphy maps that to `default_enabled = false`.
//! ```
//!
//! ## Matched shapes
//!
//! - `Or` node: rhs and (lhs or lhs.rhs of a nested `or`) are the two local
//!   environments in any order — `Rails.env.development? ||
//!   Rails.env.test?` → `Rails.env.local?`. Longer chains collapse the
//!   matching pair only: `foo || dev? || test?` → `foo ||
//!   Rails.env.local?`.
//! - `And` node: same with `!`-negated operands — `!dev? && !test?` →
//!   `!Rails.env.local?`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct EnvLocal;

const MSG: &str = "Use `Rails.env.local?` instead.";
const MSG_NEGATED: &str = "Use `!Rails.env.local?` instead.";

#[cop(
    name = "Rails/EnvLocal",
    description = "Use `Rails.env.local?` instead of `Rails.env.development? || Rails.env.test?`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl EnvLocal {
    // Mirrors upstream `on_or`.
    #[on_node(kind = "or")]
    fn check_or(&self, node: NodeId, cx: &Cx<'_>) {
        // Upstream `minimum_target_rails_version 7.1` — unset means newest.
        if !cx.rails_version_at_least(7, 1) {
            return;
        }
        let NodeKind::Or { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        let Some(rhs_env) = rails_env_local(cx, rhs) else {
            return;
        };
        // `rails_env_local?(lhs)`, else `lhs.or_type? &&
        // rails_env_local?(lhs.rhs)` for left-nested chains.
        let lhs_node = match *cx.kind(lhs) {
            _ if rails_env_local(cx, lhs).is_some() => lhs,
            NodeKind::Or { rhs: inner_rhs, .. }
                if rails_env_local(cx, inner_rhs).is_some() =>
            {
                inner_rhs
            }
            _ => return,
        };
        let Some(lhs_env) = rails_env_local(cx, lhs_node) else {
            return;
        };
        if !is_local_pair(lhs_env, rhs_env) {
            return;
        }
        let range = offense_range(cx, lhs_node, rhs);
        cx.emit_offense(range, MSG, None);
        cx.emit_edit(range, "Rails.env.local?");
    }

    // Mirrors upstream `on_and`.
    #[on_node(kind = "and")]
    fn check_and(&self, node: NodeId, cx: &Cx<'_>) {
        // Upstream `minimum_target_rails_version 7.1` — unset means newest.
        if !cx.rails_version_at_least(7, 1) {
            return;
        }
        let NodeKind::And { lhs, rhs } = *cx.kind(node) else {
            return;
        };
        let Some(rhs_env) = not_rails_env_local(cx, rhs) else {
            return;
        };
        // `not_rails_env_local?(lhs)`, else `lhs.operator_keyword? &&
        // not_rails_env_local?(lhs.rhs)` — `operator_keyword?` is a node-type
        // check (`and`/`or`), so both `&&` and `and` spellings qualify, and
        // Murphy folds both to the same `And`/`Or` nodes.
        let lhs_node = match *cx.kind(lhs) {
            _ if not_rails_env_local(cx, lhs).is_some() => lhs,
            NodeKind::And { rhs: inner_rhs, .. }
            | NodeKind::Or { rhs: inner_rhs, .. }
                if not_rails_env_local(cx, inner_rhs).is_some() =>
            {
                inner_rhs
            }
            _ => return,
        };
        let Some(lhs_env) = not_rails_env_local(cx, lhs_node) else {
            return;
        };
        if !is_local_pair(lhs_env, rhs_env) {
            return;
        }
        let range = offense_range(cx, lhs_node, rhs);
        cx.emit_offense(range, MSG_NEGATED, None);
        cx.emit_edit(range, "!Rails.env.local?");
    }
}

/// Upstream `rails_env_local?`: `(send (send (const {cbase nil?} :Rails)
/// :env) {development? test?})` with zero args on both sends (the pattern
/// requires exact arity). Returns the environment method name.
fn rails_env_local<'a>(cx: &Cx<'a>, node: NodeId) -> Option<&'a str> {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return None;
    };
    let name = cx.symbol_str(method);
    if name != "development?" && name != "test?" {
        return None;
    }
    if !cx.call_arguments(node).is_empty() {
        return None;
    }
    let env = receiver.get()?;
    let NodeKind::Send {
        receiver: env_receiver,
        method: env_method,
        ..
    } = *cx.kind(env)
    else {
        return None;
    };
    if cx.symbol_str(env_method) != "env" {
        return None;
    }
    if !cx.call_arguments(env).is_empty() {
        return None;
    }
    let rails = env_receiver.get()?;
    if cx.const_name(rails).as_deref() != Some("Rails") {
        return None;
    }
    Some(name)
}

/// Upstream `not_rails_env_local?`: `(send #rails_env_local? :!)`.
/// Returns the inner environment method name.
fn not_rails_env_local<'a>(cx: &Cx<'a>, node: NodeId) -> Option<&'a str> {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return None;
    };
    if cx.symbol_str(method) != "!" {
        return None;
    }
    rails_env_local(cx, receiver.get()?)
}

/// The two operands are exactly `{development?, test?}` in any order.
fn is_local_pair(first: &str, second: &str) -> bool {
    (first == "development?" && second == "test?")
        || (first == "test?" && second == "development?")
}

/// Upstream `offense_range`: `nodes[1].begin.join(nodes[0].end)` — from the
/// start of the second collected operand to the end of the first (rhs).
fn offense_range(cx: &Cx<'_>, second: NodeId, first: NodeId) -> Range {
    Range {
        start: cx.range(second).start,
        end: cx.range(first).end,
    }
}

#[cfg(test)]
mod tests {
    use super::EnvLocal;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_or_pair() {
        test::<EnvLocal>().expect_offense(indoc! {r#"
            Rails.env.development? || Rails.env.test?
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.env.local?` instead.
        "#});
    }

    #[test]
    fn flags_reversed_or_pair() {
        test::<EnvLocal>().expect_offense(indoc! {r#"
            Rails.env.test? || Rails.env.development?
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.env.local?` instead.
        "#});
    }

    #[test]
    fn flags_or_keyword_pair() {
        test::<EnvLocal>().expect_offense(indoc! {r#"
            Rails.env.development? or Rails.env.test?
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.env.local?` instead.
        "#});
    }

    #[test]
    fn flags_cbase_pair() {
        test::<EnvLocal>().expect_offense(indoc! {r#"
            ::Rails.env.development? || ::Rails.env.test?
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.env.local?` instead.
        "#});
    }

    #[test]
    fn flags_matching_pair_in_longer_chain() {
        // Only the matching pair span flags; the rest passes through.
        test::<EnvLocal>().expect_offense(indoc! {r#"
            foo || Rails.env.development? || Rails.env.test?
                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.env.local?` instead.
        "#});
    }

    #[test]
    fn flags_negated_and_pair() {
        test::<EnvLocal>().expect_offense(indoc! {r#"
            !Rails.env.development? && !Rails.env.test?
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!Rails.env.local?` instead.
        "#});
    }

    #[test]
    fn flags_negated_and_keyword_pair() {
        test::<EnvLocal>().expect_offense(indoc! {r#"
            !Rails.env.development? and !Rails.env.test?
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!Rails.env.local?` instead.
        "#});
    }

    #[test]
    fn flags_not_keyword_negation() {
        // `not x` folds to `Send(:!)`; the rewrite still uses `!`.
        test::<EnvLocal>().expect_offense(indoc! {r#"
            not Rails.env.development? and not Rails.env.test?
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!Rails.env.local?` instead.
        "#});
    }

    #[test]
    fn corrects_matching_pair_in_longer_and_chain() {
        test::<EnvLocal>()
            .expect_correction(
                indoc! {r#"
                    !Rails.env.development? && !Rails.env.test? && !foo
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!Rails.env.local?` instead.
                "#},
                "!Rails.env.local? && !foo\n",
            )
            .expect_no_offenses("!Rails.env.local? && !foo\n");
    }

    #[test]
    fn does_not_flag_duplicate_environment() {
        test::<EnvLocal>()
            .expect_no_offenses("Rails.env.development? || Rails.env.development?\n");
    }

    #[test]
    fn does_not_flag_other_environment() {
        test::<EnvLocal>()
            .expect_no_offenses("Rails.env.development? || Rails.env.production?\n");
    }

    #[test]
    fn does_not_flag_predicate_with_arguments() {
        test::<EnvLocal>()
            .expect_no_offenses("Rails.env.development?(x) || Rails.env.test?\n");
    }

    #[test]
    fn does_not_flag_parenthesised_operands() {
        test::<EnvLocal>()
            .expect_no_offenses("(Rails.env.development?) || (Rails.env.test?)\n");
    }

    #[test]
    fn does_not_flag_below_rails_71() {
        test::<EnvLocal>()
            .with_target_rails_version(7, 0)
            .expect_no_offenses("Rails.env.development? || Rails.env.test?\n");
    }

    #[test]
    fn fires_at_rails_71() {
        test::<EnvLocal>()
            .with_target_rails_version(7, 1)
            .expect_offense(indoc! {r#"
                Rails.env.development? || Rails.env.test?
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.env.local?` instead.
            "#});
    }

    #[test]
    fn corrects_or_pair() {
        test::<EnvLocal>()
            .expect_correction(
                indoc! {r#"
                    Rails.env.development? || Rails.env.test?
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.env.local?` instead.
                "#},
                "Rails.env.local?\n",
            )
            .expect_no_offenses("Rails.env.local?\n");
    }

    #[test]
    fn corrects_negated_and_pair() {
        test::<EnvLocal>()
            .expect_correction(
                indoc! {r#"
                    !Rails.env.development? && !Rails.env.test?
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `!Rails.env.local?` instead.
                "#},
                "!Rails.env.local?\n",
            )
            .expect_no_offenses("!Rails.env.local?\n");
    }

    #[test]
    fn corrects_matching_pair_in_longer_chain() {
        test::<EnvLocal>()
            .expect_correction(
                indoc! {r#"
                    foo || Rails.env.development? || Rails.env.test?
                           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `Rails.env.local?` instead.
                "#},
                "foo || Rails.env.local?\n",
            )
            .expect_no_offenses("foo || Rails.env.local?\n");
    }
}
murphy_plugin_api::submit_cop!(EnvLocal);
