//! `RSpec/BeEql` — prefer `be` over `eql` for identity-comparable values.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/BeEql
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `eql_type_with_identity` (`(send _ :to $(send nil?
//!   :eql {boolean int float sym nil}))`) with `RESTRICT_ON_SEND = [:to]`.
//!   Only `to` is checked (not `to_not` / `not_to`), and only identity
//!   literals (boolean / int / float / sym / nil) flag; `eql("str")` /
//!   `eql(obj)` do not. The offense range is the inner `eql` selector per
//!   `add_offense(eql.loc.selector)`. Detection is at parity; autocorrect
//!   (replace `eql` with `be`) is not ported in this batch — same
//!   convention as `RSpec/Focus` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["to"]`. Flags when any positional
//! arg is a bare `eql` with a single identity-literal arg:
//!
//! - `expect(foo).to eql(1)` — flagged (range is `eql`).
//! - `expect(foo).to eql(true)` / `eql(:bar)` / `eql(nil)` — flagged.
//! - `expect(foo).to eql("str")` — string, not flagged.
//! - `expect(foo).to_not eql(1)` — `to_not` host, not flagged.
//! - `expect(foo).to eq(1)` — different matcher, not flagged.
//!
//! ## No autocorrect
//!
//! Upstream replaces `eql` with `be`. This batch reports only; the edit is
//! trivial but out of scope for the port batch.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct BeEql;

#[cop(
    name = "RSpec/BeEql",
    description = "Check for expectations where be can replace eql.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl BeEql {
    #[on_node(kind = "send", methods = ["to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        for arg in cx.list(args).iter().copied() {
            let NodeKind::Send {
                receiver,
                method,
                args: inner_args,
            } = *cx.kind(arg)
            else {
                continue;
            };
            if receiver != OptNodeId::NONE {
                continue;
            }
            if cx.symbol_str(method) != "eql" {
                continue;
            }
            let inner = cx.list(inner_args);
            if inner.len() != 1 {
                continue;
            }
            if !is_identity_literal(cx, inner[0]) {
                continue;
            }
            cx.emit_offense(
                cx.node(arg).loc.name,
                "Prefer `be` over `eql`.",
                None,
            );
        }
    }
}

/// `true` for the `{boolean int float sym nil}` arg union: `True_` /
/// `False_` (boolean), `Int`, `Float`, `Sym`, `Nil`.
fn is_identity_literal(cx: &Cx<'_>, id: NodeId) -> bool {
    matches!(
        *cx.kind(id),
        NodeKind::True_
            | NodeKind::False_
            | NodeKind::Int(_)
            | NodeKind::Float(_)
            | NodeKind::Sym(_)
            | NodeKind::Nil
    )
}

#[cfg(test)]
mod tests {
    use super::BeEql;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_eql_int() {
        test::<BeEql>().expect_offense(indoc! {r#"
                expect(foo).to eql(1)
                               ^^^ Prefer `be` over `eql`.
            "#});
    }

    #[test]
    fn flags_eql_true() {
        test::<BeEql>().expect_offense(indoc! {r#"
                expect(foo).to eql(true)
                               ^^^ Prefer `be` over `eql`.
            "#});
    }

    #[test]
    fn flags_eql_sym() {
        test::<BeEql>().expect_offense(indoc! {r#"
                expect(foo).to eql(:bar)
                               ^^^ Prefer `be` over `eql`.
            "#});
    }

    #[test]
    fn flags_eql_nil() {
        test::<BeEql>().expect_offense(indoc! {r#"
                expect(foo).to eql(nil)
                               ^^^ Prefer `be` over `eql`.
            "#});
    }

    #[test]
    fn flags_eql_float() {
        test::<BeEql>().expect_offense(indoc! {r#"
                expect(foo).to eql(1.5)
                               ^^^ Prefer `be` over `eql`.
            "#});
    }

    #[test]
    fn does_not_flag_eql_string() {
        test::<BeEql>().expect_no_offenses(indoc! {r#"
                expect(foo).to eql("str")
            "#});
    }

    #[test]
    fn does_not_flag_to_not_eql() {
        // Upstream only checks `to`; `to_not` / `not_to` are out of scope
        // since `!eql?` is stricter than `!equal?`.
        test::<BeEql>().expect_no_offenses(indoc! {r#"
                expect(foo).to_not eql(1)
            "#});
    }

    #[test]
    fn does_not_flag_eq() {
        test::<BeEql>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq(1)
            "#});
    }
}

murphy_plugin_api::submit_cop!(BeEql);
