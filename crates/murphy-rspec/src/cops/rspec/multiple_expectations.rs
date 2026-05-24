//! `RSpec/MultipleExpectations` — caps the number of bare `expect(...)`
//! calls inside one example. Mirrors RuboCop-RSpec's cop of the same
//! name.
//!
//! ## Matched shapes
//!
//! Dispatched on `NodeKind::Block` and gates on
//! [`is_example_call`](crate::helpers::is_example_call) — the block's
//! call must be a bare `it` / `specify` / `example`. Hook blocks
//! (`before`, `after`, …) and grouping blocks (`describe`, `context`)
//! are skipped.
//!
//! ## What counts as an expectation
//!
//! Walks the body's descendants and counts `Send` nodes whose
//! `method` is `expect` AND whose receiver is empty
//! (`OptNodeId::NONE`). The receiver gate is load-bearing:
//! `obj.expect(x)` is a domain method on `obj`, not RSpec's matcher
//! entry point, and must not be counted (false positive territory).
//!
//! `aggregate_failures do ... end` (RuboCop's "count these as one")
//! and `expect_any_instance_of(...)` are intentionally out of scope
//! for v1; revisit when the rule sees real-world false positives.
//!
//! ## Option
//!
//! `max` (default `1`, matching RuboCop) — examples whose `expect`
//! count exceeds `max` are flagged. Runtime option wiring
//! (murphy-9cr.9) is not yet plumbed through `Cx`; v1 honours the
//! `Default` (same staging as `Style/StringLiterals`).
//!
//! ## No autocorrect
//!
//! Splitting an example into multiple `it` blocks is a refactor that
//! needs human judgement about isolation and shared setup.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

use super::helpers::is_example_call;

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct MultipleExpectations;

/// Cop options for [`MultipleExpectations`]. Schema is exported via
/// `#[derive(CopOptions)]`; runtime option access (murphy-9cr.9) is
/// not yet wired through `Cx`, so the `Default` (Max = 1) is what
/// fires at runtime today.
#[derive(CopOptions)]
pub struct MultipleExpectationsOptions {
    #[option(
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

        let opts = MultipleExpectationsOptions::default();
        let count = count_bare_expects(cx, body_id);
        if count <= opts.max as usize {
            return;
        }

        cx.emit_offense(
            cx.range(node),
            &format!(
                "Example has too many expectations ({count}/{max})",
                max = opts.max
            ),
            None,
        );
    }
}

/// Walk every descendant of `body` and count `Send` nodes whose
/// `method` is `expect` and whose receiver is empty. Nested example
/// blocks (`it { it { expect(…) } }`) attribute their inner expects
/// to the outer example — a tolerated false positive for v1 since the
/// shape is degenerate.
fn count_bare_expects(cx: &Cx<'_>, body: NodeId) -> usize {
    cx.descendants(body)
        .into_iter()
        .filter(|d| is_bare_expect_call(cx, *d))
        .count()
}

fn is_bare_expect_call(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(id)
    else {
        return false;
    };
    receiver == OptNodeId::NONE && cx.symbol_str(method) == "expect"
}

#[cfg(test)]
mod tests {
    use super::MultipleExpectations;
    use murphy_plugin_api::test_support::{indoc, run_cop};

    /// `run_cop` only dispatches the one cop type so every emission is
    /// already a `RSpec/MultipleExpectations` offense — no per-name
    /// filter needed.
    fn hits(source: &str) -> usize {
        run_cop::<MultipleExpectations>(source).len()
    }

    #[test]
    fn flags_two_expects() {
        let src = indoc! {r#"
            it "works" do
              expect(a).to eq(1)
              expect(b).to eq(2)
            end
        "#};
        assert_eq!(hits(src), 1);
    }

    #[test]
    fn does_not_flag_single_expect() {
        let src = indoc! {r#"
            it "works" do
              expect(a).to eq(1)
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn handles_specify_and_example_aliases() {
        let src = indoc! {r#"
            specify "x" do
              expect(a).to eq(1)
              expect(b).to eq(2)
            end
            example "y" do
              expect(a).to eq(1)
              expect(b).to eq(2)
            end
        "#};
        assert_eq!(hits(src), 2);
    }

    #[test]
    fn ignores_method_called_expect_on_receiver() {
        // `obj.expect(x)` is some domain method named `expect` — not
        // RSpec's matcher entry point. Only the bare `expect(c)` would
        // count, and there's just one, so no offense.
        let src = indoc! {r#"
            it "works" do
              obj.expect(a)
              obj.expect(b)
              expect(c).to eq(1)
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn ignores_hook_blocks() {
        // `before { ... }` is a hook, not an example; this rule does
        // not police hook bodies.
        let src = indoc! {r#"
            before do
              expect(setup).to eq(true)
              expect(other).to eq(true)
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn ignores_describe_blocks_holding_examples() {
        // `describe Widget do ... end` contains several `it` blocks
        // each with one expect — the describe itself is not an
        // example and must not aggregate its descendants' expects.
        let src = indoc! {r#"
            describe Widget do
              it "a" do
                expect(a).to eq(1)
              end
              it "b" do
                expect(b).to eq(1)
              end
            end
        "#};
        assert_eq!(hits(src), 0);
    }

    #[test]
    fn flags_brace_form_block() {
        // `it { ... }` parses to `NodeKind::Block` the same way as
        // `it do ... end`; the cop must count expects inside either
        // form.
        let src = indoc! {r#"
            it "works" {
              expect(a).to eq(1)
              expect(b).to eq(2)
            }
        "#};
        assert_eq!(hits(src), 1);
    }
}
