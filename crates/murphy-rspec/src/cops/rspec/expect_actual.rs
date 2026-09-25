//! `RSpec/ExpectActual` — provide the actual value to `expect(...)`, not a literal.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ExpectActual
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `expect_literal`
//!   (`(send (send nil? :expect $#literal?) #Runners.all
//!   ${(send (send nil? $:be) :== $_) (send nil? $_ $_ ...)})`) with
//!   `RESTRICT_ON_SEND = Runners.all` (`to` / `to_not` / `not_to`).
//!   The `expect` receiver must be bare with a single literal arg
//!   (`SIMPLE_LITERALS`: true / false / nil / int / float / str / sym /
//!   complex / rational / regopt; `COMPLEX_LITERALS`: array / hash / pair /
//!   irange / erange / regexp with all-literal children). The matcher is
//!   either `be == expected` (bare zero-arg `be` receiver) or
//!   `matcher(expected, ...)` (bare receiver). `route_to` / `be_routable`
//!   are skipped per `SKIPPED_MATCHERS`. The offense range is the `expect`
//!   literal per `add_offense(actual)`. Detection is at parity;
//!   autocorrect (swap actual / expected for `eq` / `eql` / `equal` / `be`)
//!   is not ported in this batch — same convention as `RSpec/Eq`
//!   (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["to", "to_not", "not_to"]`.
//!
//! - `expect(5).to eq(price)` — flagged (actual `5`).
//! - `expect("John").to eq(name)` — flagged.
//! - `expect(false).to eq(true)` — both literals, still flagged (no
//!   autocorrect upstream).
//! - `expect(price).to eq(5)` — actual not literal, not flagged.
//! - `expect(5).to route_to(foo)` — skipped matcher, not flagged.
//! - `expect(5).to be == price` — `be ==` form, flagged.
//!
//! ## No autocorrect
//!
//! Upstream swaps actual / expected for correctable matchers when the
//! expected is not a literal. This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ExpectActual;

#[cop(
    name = "RSpec/ExpectActual",
    description = "Checks for expect(...) calls containing literal values.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl ExpectActual {
    #[on_node(kind = "send", methods = ["to", "to_not", "not_to"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, args, ..
        } = *cx.kind(node)
        else {
            return;
        };
        let Some(expect_id) = receiver.get() else {
            return;
        };
        let NodeKind::Send {
            receiver: exp_recv,
            method: exp_method,
            args: exp_args,
        } = *cx.kind(expect_id)
        else {
            return;
        };
        if exp_recv != OptNodeId::NONE {
            return;
        }
        if cx.symbol_str(exp_method) != "expect" {
            return;
        }
        let exp_arg_ids = cx.list(exp_args);
        if exp_arg_ids.len() != 1 {
            return;
        }
        let actual_id = exp_arg_ids[0];
        if !is_literal(cx, actual_id) {
            return;
        }
        for arg in cx.list(args).iter().copied() {
            let NodeKind::Send {
                receiver: m_recv,
                method: m_method,
                args: m_args,
            } = *cx.kind(arg)
            else {
                continue;
            };
            // Arm 1: `be == expected`
            if cx.symbol_str(m_method) == "==" {
                let m_arg_ids = cx.list(m_args);
                if m_arg_ids.len() != 1 {
                    continue;
                }
                let Some(be_id) = m_recv.get() else {
                    continue;
                };
                let NodeKind::Send {
                    receiver: be_recv,
                    method: be_method,
                    args: be_args,
                } = *cx.kind(be_id)
                else {
                    continue;
                };
                if be_recv != OptNodeId::NONE {
                    continue;
                }
                if cx.symbol_str(be_method) != "be" {
                    continue;
                }
                if !cx.list(be_args).is_empty() {
                    continue;
                }
                // `be` is not skipped; `==` arm always reports.
                cx.emit_offense(
                    cx.range(actual_id),
                    "Provide the actual value you are testing to `expect(...)`.",
                    None,
                );
                return;
            }
            // Arm 2: `matcher(expected, ...)` with bare receiver.
            if m_recv != OptNodeId::NONE {
                continue;
            }
            let matcher = cx.symbol_str(m_method).to_owned();
            if matches!(matcher.as_str(), "route_to" | "be_routable") {
                continue;
            }
            let m_arg_ids = cx.list(m_args);
            if m_arg_ids.is_empty() {
                continue;
            }
            cx.emit_offense(
                cx.range(actual_id),
                "Provide the actual value you are testing to `expect(...)`.",
                None,
            );
            return;
        }
    }
}

/// `true` when `id` is a literal per upstream `literal?`.
fn is_literal(cx: &Cx<'_>, id: NodeId) -> bool {
    match *cx.kind(id) {
        NodeKind::Nil
        | NodeKind::True_
        | NodeKind::False_
        | NodeKind::Int(_)
        | NodeKind::Float(_)
        | NodeKind::Str(_)
        | NodeKind::Sym(_)
        | NodeKind::Complex(_)
        | NodeKind::Rational(_)
        | NodeKind::Regopt(_) => true,
        NodeKind::Array(list) | NodeKind::Hash(list) => {
            cx.list(list).iter().all(|&child| is_literal(cx, child))
        }
        NodeKind::Pair { key, value } => is_literal(cx, key) && is_literal(cx, value),
        NodeKind::RangeExpr { begin_, end_, .. } => {
            let begin_ok = begin_.get().is_none_or(|b| is_literal(cx, b));
            let end_ok = end_.get().is_none_or(|e| is_literal(cx, e));
            begin_ok && end_ok
        }
        NodeKind::Regexp { parts, .. } => {
            // `/foo/` parts are `Str`; interpolated parts are `Dstr`-ish
            // and fail the literal check, matching upstream's
            // all-children-literal rule.
            cx.list(parts).iter().all(|&child| is_literal(cx, child))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::ExpectActual;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_expect_literal_to_eq() {
        test::<ExpectActual>().expect_offense(indoc! {r#"
                expect(5).to eq(price)
                       ^ Provide the actual value you are testing to `expect(...)`.
            "#});
    }

    #[test]
    fn flags_expect_string_to_eq() {
        test::<ExpectActual>().expect_offense(indoc! {r#"
                expect("John").to eq(name)
                       ^^^^^^ Provide the actual value you are testing to `expect(...)`.
            "#});
    }

    #[test]
    fn flags_both_literals() {
        test::<ExpectActual>().expect_offense(indoc! {r#"
                expect(false).to eq(true)
                       ^^^^^ Provide the actual value you are testing to `expect(...)`.
            "#});
    }

    #[test]
    fn flags_be_eq_form() {
        test::<ExpectActual>().expect_offense(indoc! {r#"
                expect(5).to be == price
                       ^ Provide the actual value you are testing to `expect(...)`.
            "#});
    }

    #[test]
    fn does_not_flag_actual_not_literal() {
        test::<ExpectActual>().expect_no_offenses(indoc! {r#"
                expect(price).to eq(5)
            "#});
    }

    #[test]
    fn does_not_flag_skipped_matcher() {
        test::<ExpectActual>().expect_no_offenses(indoc! {r#"
                expect(5).to route_to(foo)
            "#});
    }

    #[test]
    fn does_not_flag_expect_with_var() {
        test::<ExpectActual>().expect_no_offenses(indoc! {r#"
                expect(foo).to eq(bar)
            "#});
    }
}

murphy_plugin_api::submit_cop!(ExpectActual);
