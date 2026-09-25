//! `RSpec/NoExpectationExample` — examples must contain an expectation.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/NoExpectationExample
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_block` (`regular_or_focused_example?`: bare
//!   regular + focused examples only — skipped (`xit` / ...) and
//!   `pending` / `skip` never flag): the whole block flags per
//!   `add_offense(node)` with `No expectation found in this example.`
//!   unless some descendant bare send names an `Expectations.all`
//!   member (`expect` / `is_expected` / `should` / ... including
//!   `are_expected` / `expect_any_instance_of`) or matches one of
//!   `AllowedPatterns` (default `^expect_` / `^assert_`), or the body
//!   calls bare `pending` / `skip`, or the example send carries
//!   `skipped_in_metadata?` (`:skip` / `:pending` sym or a `skip:` /
//!   `pending:` pair with `true` / `Str` / `Dstr`). Verified vs 3.7.0,
//!   including the empty-body flag and the `not_expect_something`
//!   negative-pattern case. No autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on bare regular/focused example `Block`s:
//!
//! - `it do foo end` — no expectation, flagged (whole block).
//! - `it do expect(x).to eq(1) end` — clean.
//! - `it do expect_something end` — allowed pattern, clean.
//! - `xit do foo end` — skipped, never flagged.
//! - `it "x", :skip do foo end` — metadata, clean.
//! - `it do skip "x"; foo end` — inner skip, clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; adding the missing expectation needs
//! human judgement about intended behavior.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop, regex::Regex};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct NoExpectationExample;

#[derive(CopOptions)]
pub struct NoExpectationExampleOptions {
    #[option(
        name = "AllowedPatterns",
        default = ["^expect_", "^assert_"],
        description = "Regex patterns; matching bare calls count as expectations."
    )]
    pub allowed_patterns: Vec<String>,
}

#[cop(
    name = "RSpec/NoExpectationExample",
    description = "Checks if an example contains any expectation.",
    default_severity = "warning",
    default_enabled = true,
    options = NoExpectationExampleOptions
)]
impl NoExpectationExample {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        let NodeKind::Send {
            receiver,
            method,
            ..
        } = *cx.kind(call)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if !is_regular_or_focused_example_name(cx.symbol_str(method)) {
            return;
        }
        let opts = cx.options_or_default::<NoExpectationExampleOptions>();
        if includes_expectation(cx, node, &opts) {
            return;
        }
        if includes_skip_call(cx, node) {
            return;
        }
        if has_skip_metadata(cx, call) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "No expectation found in this example.",
            None,
        );
    }
}

/// `true` for `Examples.regular` + `Examples.focused`: `it` / `specify`
/// / `example` / `scenario` / `its` plus `fit` / `fspecify` / `fexample`
/// / `fscenario` / `focus`. Skipped (`xit` / ...) and `pending` /
/// `skip` are excluded upstream and never flag here either.
fn is_regular_or_focused_example_name(name: &str) -> bool {
    matches!(
        name,
        "it" | "specify"
            | "example"
            | "scenario"
            | "its"
            | "fit"
            | "fspecify"
            | "fexample"
            | "fscenario"
            | "focus"
    )
}

/// `true` when some descendant bare send names an expectation or matches
/// an allowed pattern.
///
/// Mirrors upstream `includes_expectation?` (`(send nil?
/// #Expectations.all ...)` plus `` (send nil? `#matches_allowed_pattern?
/// ...) ``).
fn includes_expectation(cx: &Cx<'_>, example: NodeId, opts: &NoExpectationExampleOptions) -> bool {
    for id in cx.descendants(example) {
        let NodeKind::Send {
            receiver,
            method,
            ..
        } = *cx.kind(id)
        else {
            continue;
        };
        if receiver != OptNodeId::NONE {
            continue;
        }
        let name = cx.symbol_str(method);
        if is_expectation_name(name) {
            return true;
        }
        if opts.allowed_patterns.iter().any(|pat| {
            Regex::new(pat).is_ok_and(|re| re.is_match(name))
        }) {
            return true;
        }
    }
    false
}

/// `Expectations` in rubocop-rspec's default config.
fn is_expectation_name(name: &str) -> bool {
    matches!(
        name,
        "are_expected"
            | "expect"
            | "expect_any_instance_of"
            | "is_expected"
            | "should"
            | "should_not"
            | "should_not_receive"
            | "should_receive"
    )
}

/// `true` when the example body calls bare `pending` / `skip`.
///
/// Mirrors upstream `includes_skip_example?` (`(send nil? {:pending
/// :skip} ...)`).
fn includes_skip_call(cx: &Cx<'_>, example: NodeId) -> bool {
    for id in cx.descendants(example) {
        let NodeKind::Send {
            receiver,
            method,
            ..
        } = *cx.kind(id)
        else {
            continue;
        };
        if receiver != OptNodeId::NONE {
            continue;
        }
        if matches!(cx.symbol_str(method), "pending" | "skip") {
            return true;
        }
    }
    false
}

/// Upstream `skipped_in_metadata?` on the example send: a bare `:skip` /
/// `:pending` sym arg, or a `skip:` / `pending:` pair with a `true` /
/// `Str` / `Dstr` value.
fn has_skip_metadata(cx: &Cx<'_>, send: NodeId) -> bool {
    let NodeKind::Send { args, .. } = *cx.kind(send) else {
        return false;
    };
    for arg in cx.list(args).iter().copied() {
        match *cx.kind(arg) {
            NodeKind::Sym(sym)
                if matches!(cx.symbol_str(sym), "skip" | "pending") =>
            {
                return true;
            }
            NodeKind::Hash(pairs) => {
                for pair_id in cx.list(pairs).iter().copied() {
                    let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
                        continue;
                    };
                    let is_skip_key = matches!(*cx.kind(key), NodeKind::Sym(s) if matches!(cx.symbol_str(s), "skip" | "pending"));
                    if !is_skip_key {
                        continue;
                    }
                    if matches!(*cx.kind(value), NodeKind::True_)
                        || matches!(*cx.kind(value), NodeKind::Str(_))
                        || matches!(*cx.kind(value), NodeKind::Dstr(_))
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{NoExpectationExample, NoExpectationExampleOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_example_without_expectation() {
        test::<NoExpectationExample>().expect_offense(indoc! {r#"
                it { foo }
                ^^^^^^^^^^ No expectation found in this example.
            "#});
    }

    #[test]
    fn flags_empty_example() {
        test::<NoExpectationExample>().expect_offense(indoc! {r#"
                it { }
                ^^^^^^ No expectation found in this example.
            "#});
    }

    #[test]
    fn ignores_example_with_expect() {
        test::<NoExpectationExample>().expect_no_offenses(indoc! {r#"
                it do
                  expect(a?).to be(true)
                end
            "#});
    }

    #[test]
    fn ignores_is_expected() {
        test::<NoExpectationExample>().expect_no_offenses(indoc! {r#"
                it do
                  is_expected.to be_truthy
                end
            "#});
    }

    #[test]
    fn ignores_allowed_expect_pattern() {
        test::<NoExpectationExample>().expect_no_offenses(indoc! {r#"
                it do
                  expect_something
                end
            "#});
    }

    #[test]
    fn ignores_allowed_assert_pattern() {
        test::<NoExpectationExample>().expect_no_offenses(indoc! {r#"
                it do
                  assert_something
                end
            "#});
    }

    #[test]
    fn flags_non_matching_pattern() {
        // `not_expect_something` matches neither `^expect_` nor
        // `^assert_` (verified vs 3.7.0).
        test::<NoExpectationExample>().expect_offense(indoc! {r#"
                it { not_expect_something }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ No expectation found in this example.
            "#});
    }

    #[test]
    fn ignores_skipped_example() {
        // `xit` is not regular/focused (verified vs 3.7.0).
        test::<NoExpectationExample>().expect_no_offenses(indoc! {r#"
                xit do
                  foo
                end
            "#});
    }

    #[test]
    fn ignores_skip_metadata() {
        test::<NoExpectationExample>().expect_no_offenses(indoc! {r#"
                it 'x', :skip do
                  foo
                end
            "#});
    }

    #[test]
    fn ignores_skip_inside_body() {
        test::<NoExpectationExample>().expect_no_offenses(indoc! {r#"
                it do
                  skip 'not ready'
                  foo
                end
            "#});
    }

    #[test]
    fn ignores_focused_with_expectation() {
        test::<NoExpectationExample>().expect_no_offenses(indoc! {r#"
                fit do
                  expect(1).to eq(1)
                end
            "#});
    }

    #[test]
    fn flags_focused_without_expectation() {
        test::<NoExpectationExample>().expect_offense(indoc! {r#"
                fit { foo }
                ^^^^^^^^^^^ No expectation found in this example.
            "#});
    }

    #[test]
    fn respects_custom_allowed_patterns() {
        let opts = NoExpectationExampleOptions {
            allowed_patterns: vec!["^custom_".to_owned()],
        };
        test::<NoExpectationExample>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                it do
                  custom_check
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(NoExpectationExample);
