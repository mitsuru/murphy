//! `RSpec/MultipleExpectations` — caps the number of bare `expect(...)`
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/MultipleExpectations
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Token-based example_call_range (murphy-ipxn) implemented.
//! ```
//!
//! calls inside one example. Mirrors RuboCop-RSpec's cop of the same
//! name.
//!
//! ## Matched shapes
//!
//! Dispatched on `NodeKind::Block` and gates on
//! [`is_example_call`](crate::helpers::is_example_call) — the block's
//! call must be a bare RSpec example alias such as `it`, `specify`,
//! `example`, `fit`, `xit`, `skip`, or `pending`. Hook blocks
//! (`before`, `after`, …) and grouping blocks (`describe`, `context`)
//! are skipped.
//!
//! ## What counts as an expectation
//!
//! Recursively walks the body and counts bare RSpec expectation
//! entrypoints (`expect`, `expect_any_instance_of`, `is_expected`,
//! `are_expected`, `should`, `should_not`, `should_receive`, and
//! `should_not_receive`). The receiver gate is load-bearing:
//! `obj.expect(x)` is a domain method on `obj`, not RSpec's matcher
//! entry point, and must not be counted (false positive territory).
//!
//! `aggregate_failures do ... end` counts as one expectation and its
//! body is not searched. Examples marked with `:aggregate_failures` or
//! `aggregate_failures: true` are ignored; explicit
//! `aggregate_failures: false` leaves normal counting enabled.
//! Degenerate nested blocks inside an example are still traversed, so
//! `it { it { expect(...) } }` attributes the inner expectation to the
//! outer example; this preserves the previous tolerated false positive.
//!
//! ## Option
//!
//! `max` (default `1`, matching RuboCop) — examples whose expectation
//! count exceeds `max` are flagged. Runtime option wiring is provided
//! by `Cx::options_or_default`, so tests and host config can exercise
//! non-default `Max` values.
//!
//! ## No autocorrect
//!
//! Splitting an example into multiple `it` blocks is a refactor that
//! needs human judgement about isolation and shared setup.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{example_call_range, is_example_call};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MultipleExpectations;

/// Cop options for [`MultipleExpectations`].
#[derive(CopOptions)]
pub struct MultipleExpectationsOptions {
    #[option(name = "Max", 
        default = 1,
        description = "Maximum number of expect(...) calls per example."
    )]
    pub max: i64,
}

#[cop(
    name = "RSpec/MultipleExpectations",
    description = "Caps the number of bare `expect(...)` calls inside one example.",
    default_severity = "warning",
    default_enabled = true,
    options = MultipleExpectationsOptions
)]
impl MultipleExpectations {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, body, .. } = *cx.kind(node) else {
            return;
        };
        if !is_example_call(cx, call) {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };

        if example_or_ancestor_with_aggregate_failures(cx, call) {
            return;
        }

        let opts = cx.options_or_default::<MultipleExpectationsOptions>();
        let count = count_expectations(cx, body_id);
        if count <= opts.max as usize {
            return;
        }

        cx.emit_offense(
            example_call_range(cx, call, body_id),
            &format!(
                "Example has too many expectations [{count}/{max}].",
                max = opts.max
            ),
            None,
        );
    }
}

/// Count expectation entrypoints below `node`. A nested
/// `aggregate_failures` block counts as one expectation and its body is
/// not searched, matching RuboCop-RSpec's aggregation semantics.
fn count_expectations(cx: &Cx<'_>, node: NodeId) -> usize {
    if is_aggregate_failures_block(cx, node) {
        return 1;
    }

    let own = usize::from(is_expectation_call(cx, node));
    own + cx
        .children(node)
        .into_iter()
        .map(|child| count_expectations(cx, child))
        .sum::<usize>()
}

fn is_expectation_call(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(id)
    else {
        return false;
    };
    let method = cx.symbol_str(method);
    if is_receiver_based_expectation_method(method) {
        return true;
    }
    receiver == OptNodeId::NONE && is_bare_expectation_method(method)
}

fn is_bare_expectation_method(method: &str) -> bool {
    matches!(
        method,
        "are_expected" | "expect" | "expect_any_instance_of" | "is_expected"
    )
}

fn is_receiver_based_expectation_method(method: &str) -> bool {
    matches!(
        method,
        "should" | "should_not" | "should_receive" | "should_not_receive"
    )
}

fn is_aggregate_failures_block(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Block { call, .. } = *cx.kind(id) else {
        return false;
    };
    is_bare_send_named(cx, call, "aggregate_failures")
}

fn example_or_ancestor_with_aggregate_failures(cx: &Cx<'_>, call: NodeId) -> bool {
    cx.ancestors(call)
        .filter(|ancestor| matches!(cx.kind(*ancestor), NodeKind::Block { .. }))
        .find_map(|block| {
            let NodeKind::Block { call, .. } = *cx.kind(block) else {
                return None;
            };
            send_aggregate_failures_setting(cx, call)
        })
        .unwrap_or(false)
}

fn send_aggregate_failures_setting(cx: &Cx<'_>, id: NodeId) -> Option<bool> {
    let NodeKind::Send { args, .. } = *cx.kind(id) else {
        return None;
    };

    let mut setting = None;
    for arg in cx.list(args).iter().copied() {
        match arg_aggregate_failures_setting(cx, arg) {
            Some(true) => return Some(true),
            Some(false) => setting = Some(false),
            None => {}
        }
    }
    setting
}

fn arg_aggregate_failures_setting(cx: &Cx<'_>, id: NodeId) -> Option<bool> {
    match *cx.kind(id) {
        NodeKind::Sym(sym) if cx.symbol_str(sym) == "aggregate_failures" => Some(true),
        NodeKind::Hash(pairs) => {
            let mut setting = None;
            for pair in cx.list(pairs).iter().copied() {
                if pair_is_aggregate_failures_true(cx, pair) {
                    return Some(true);
                }
                if pair_is_aggregate_failures_false(cx, pair) {
                    setting = Some(false);
                }
            }
            setting
        }
        _ => None,
    }
}

fn pair_is_aggregate_failures_true(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Pair { key, value } = *cx.kind(id) else {
        return false;
    };
    matches!(*cx.kind(key), NodeKind::Sym(sym) if cx.symbol_str(sym) == "aggregate_failures")
        && matches!(*cx.kind(value), NodeKind::True_)
}

fn pair_is_aggregate_failures_false(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Pair { key, value } = *cx.kind(id) else {
        return false;
    };
    matches!(*cx.kind(key), NodeKind::Sym(sym) if cx.symbol_str(sym) == "aggregate_failures")
        && matches!(*cx.kind(value), NodeKind::False_)
}

fn is_bare_send_named(cx: &Cx<'_>, id: NodeId, name: &str) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(id)
    else {
        return false;
    };
    receiver == OptNodeId::NONE && cx.symbol_str(method) == name
}

#[cfg(test)]
mod tests {
    use super::{MultipleExpectations, MultipleExpectationsOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_two_expects() {
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                it "works" do
                ^^^^^^^^^^ Example has too many expectations [2/1].
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
            "#});
    }

    #[test]
    fn does_not_flag_single_expect() {
        test::<MultipleExpectations>().expect_no_offenses(indoc! {r#"
                it "works" do
                  expect(a).to eq(1)
                end
            "#});
    }

    #[test]
    fn handles_specify_and_example_aliases() {
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                specify "x" do
                ^^^^^^^^^^^ Example has too many expectations [2/1].
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
                example "y" do
                ^^^^^^^^^^^ Example has too many expectations [2/1].
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
            "#});
    }

    #[test]
    fn handles_focused_skipped_and_pending_example_aliases() {
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                fit "focused" do
                ^^^^^^^^^^^^^ Example has too many expectations [2/1].
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
                xit "skipped" do
                ^^^^^^^^^^^^^ Example has too many expectations [2/1].
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
                pending "pending" do
                ^^^^^^^^^^^^^^^^^ Example has too many expectations [2/1].
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
            "#});
    }

    #[test]
    fn counts_rspec_expectation_aliases() {
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                it "works" do
                ^^^^^^^^^^ Example has too many expectations [7/1].
                  expect_any_instance_of(User).to receive(:save)
                  is_expected.to be_valid
                  are_expected.to contain_exactly(1, 2)
                  should be_valid
                  should_not be_nil
                  should_receive(:save)
                  should_not_receive(:destroy)
                end
            "#});
    }

    #[test]
    fn counts_receiver_based_should_expectations() {
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                it "works" do
                ^^^^^^^^^^ Example has too many expectations [4/1].
                  user.should be_valid
                  user.should_not be_nil
                  user.should_receive(:save)
                  user.should_not_receive(:destroy)
                end
            "#});
    }

    #[test]
    fn honors_max_option() {
        test::<MultipleExpectations>()
            .with_options(&MultipleExpectationsOptions { max: 2 })
            .expect_no_offenses(indoc! {r#"
                    it "works" do
                      expect(a).to eq(1)
                      expect(b).to eq(2)
                    end
                "#});
    }

    #[test]
    fn aggregate_failures_block_counts_as_one_expectation() {
        test::<MultipleExpectations>().expect_no_offenses(indoc! {r#"
                it "works" do
                  aggregate_failures do
                    expect(a).to eq(1)
                    expect(b).to eq(2)
                  end
                end
            "#});
    }

    #[test]
    fn example_metadata_aggregate_failures_true_is_ignored() {
        test::<MultipleExpectations>().expect_no_offenses(indoc! {r#"
                it "works", :aggregate_failures do
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end

                it "also works", aggregate_failures: true do
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
            "#});
    }

    #[test]
    fn example_metadata_aggregate_failures_symbol_wins_after_false_hash() {
        test::<MultipleExpectations>().expect_no_offenses(indoc! {r#"
                it "works", { aggregate_failures: false }, :aggregate_failures do
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
            "#});
    }

    #[test]
    fn example_metadata_aggregate_failures_false_still_counts() {
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                it "works", aggregate_failures: false do
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Example has too many expectations [2/1].
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
            "#});
    }

    #[test]
    fn example_metadata_aggregate_failures_false_overrides_parent_true() {
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                describe User, :aggregate_failures do
                  it "works", aggregate_failures: false do
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Example has too many expectations [2/1].
                    expect(a).to eq(1)
                    expect(b).to eq(2)
                  end
                end
            "#});
    }

    #[test]
    fn ignores_method_called_expect_on_receiver() {
        // `obj.expect(x)` is some domain method named `expect` — not
        // RSpec's matcher entry point. Only the bare `expect(c)` would
        // count, and there's just one, so no offense.
        test::<MultipleExpectations>().expect_no_offenses(indoc! {r#"
                it "works" do
                  obj.expect(a)
                  obj.expect(b)
                  expect(c).to eq(1)
                end
            "#});
    }

    #[test]
    fn ignores_hook_blocks() {
        // `before { ... }` is a hook, not an example; this rule does
        // not police hook bodies.
        test::<MultipleExpectations>().expect_no_offenses(indoc! {r#"
                before do
                  expect(setup).to eq(true)
                  expect(other).to eq(true)
                end
            "#});
    }

    #[test]
    fn ignores_describe_blocks_holding_examples() {
        // `describe Widget do ... end` contains several `it` blocks
        // each with one expect — the describe itself is not an
        // example and must not aggregate its descendants' expects.
        test::<MultipleExpectations>().expect_no_offenses(indoc! {r#"
                describe Widget do
                  it "a" do
                    expect(a).to eq(1)
                  end
                  it "b" do
                    expect(b).to eq(1)
                  end
                end
            "#});
    }

    #[test]
    fn flags_two_expects_single_line_block() {
        // The offense follows RuboCop-RSpec and points at the example
        // call, not the full block body.
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                it "x" do expect(a).to eq(1); expect(b).to eq(2) end
                ^^^^^^ Example has too many expectations [2/1].
            "#});
    }

    #[test]
    fn flags_brace_form_block() {
        // `it { ... }` parses to `NodeKind::Block` the same way as
        // `it do ... end`; the cop must count expects inside either
        // form.
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                it "works" {
                ^^^^^^^^^^ Example has too many expectations [2/1].
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                }
            "#});
    }

    // ------------------------------------------------------------------
    // Token-based example_call_range tests (murphy-ipxn)
    // ------------------------------------------------------------------

    #[test]
    fn offense_range_excludes_block_args_before_body() {
        // `it "works" do |x|` — block parameters appear between `do` and
        // the body. The offense range must cover only the call (`it "works"`),
        // not `do |x|`. The heuristic strip_suffix("do") fails here because
        // the trimmed text ends with `|x|` not `do`.
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                it "works" do |x|
                ^^^^^^^^^^ Example has too many expectations [2/1].
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
            "#});
    }

    #[test]
    fn offense_range_excludes_block_opener_with_brace_hash_arg() {
        // When the example call has an explicit brace-hash argument,
        // the `{` of the hash must NOT be mistaken for the block opener.
        // The offense range should cover the whole call including the
        // hash arg, but not the `do` block opener.
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                it "works", { aggregate_failures: false } do
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Example has too many expectations [2/1].
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
            "#});
    }

    #[test]
    fn offense_range_includes_parens_in_call() {
        // `it("works") do` -- the parens are part of the call node's
        // expression range and must be included in the offense range.
        test::<MultipleExpectations>().expect_offense(indoc! {r#"
                it("works") do
                ^^^^^^^^^^^ Example has too many expectations [2/1].
                  expect(a).to eq(1)
                  expect(b).to eq(2)
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(MultipleExpectations);
