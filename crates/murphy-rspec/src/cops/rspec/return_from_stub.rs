//! `RSpec/ReturnFromStub` — consistent stub return style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/ReturnFromStub
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream dispatch (`on_send` gated to `and_return` via
//!   `RESTRICT_ON_SEND` for style `block`; `on_block` for stubs with
//!   a block body for style `and_return`) with `contains_stub?` (a
//!   bare `receive` send in the subtree) and `dynamic?` (negated
//!   `recursive_literal_or_const?` via the shared
//!   `is_recursive_literal_or_const` helper; a missing block body
//!   counts as static, matching upstream's `NULL_BLOCK_BODY`
//!   fallback). Style `and_return` (default) flags the block opener
//!   (`{` / `do`, mirroring `block.loc.begin`) with `Use `and_return`
//!   for static values.`; style `block` flags the `and_return`
//!   selector with `Use block for static values.`. Detection is at
//!   parity (verified vs 3.7.0, including dynamic-value and
//!   EnforcedStyle cases); autocorrect (rewrite between the
//!   spellings) is not ported in this batch — same convention as
//!   `RSpec/HookArgument` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Default style `and_return`:
//!
//! - `allow(Foo).to receive(:bar) { "baz" }` — flagged (the `{`).
//! - `expect(Foo).to receive(:bar) { "baz" }` — flagged.
//! - `allow(Foo).to receive(:bar) { bar.baz }` — dynamic, clean.
//! - `allow(Foo).to receive(:bar).and_return("baz")` — clean.
//!
//! Style `block` (`EnforcedStyle: block`):
//!
//! - `allow(Foo).to receive(:bar).and_return("baz")` — flagged (the
//!   `and_return` selector).
//! - `allow(Foo).to receive(:bar).and_return(bar.baz)` — dynamic,
//!   clean.
//! - `allow(Foo).to receive(:bar) { "baz" }` — clean.
//!
//! ## No autocorrect
//!
//! Upstream rewrites between `.and_return(x)` and `{ x }`. This
//! batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{block_open_token, is_recursive_literal_or_const};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct ReturnFromStub;

#[derive(CopOptions)]
pub struct ReturnFromStubOptions {
    #[option(
        name = "EnforcedStyle",
        default = "and_return",
        description = "Whether stub returns use and_return or block style."
    )]
    pub enforced_style: ReturnFromStubStyle,
}

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq)]
pub enum ReturnFromStubStyle {
    #[option(value = "and_return")]
    AndReturn,
    #[option(value = "block")]
    Block,
}

#[cop(
    name = "RSpec/ReturnFromStub",
    description = "Checks for consistent style of stub's return setting.",
    default_severity = "warning",
    default_enabled = true,
    options = ReturnFromStubOptions,
)]
impl ReturnFromStub {
    // Mirrors upstream `RESTRICT_ON_SEND = [:and_return]` — dispatch
    // only on candidate selectors; the body still gates on the stub
    // shape and the configured style.
    #[on_node(kind = "send", methods = ["and_return"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.options_or_default::<ReturnFromStubOptions>().enforced_style
            != ReturnFromStubStyle::Block
        {
            return;
        }
        if !contains_stub(cx, node) {
            return;
        }
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        // `and_return_value`: every value arg must be dynamic to skip;
        // an empty arg list counts as static upstream, so it flags.
        let dynamic = cx
            .list(args)
            .iter()
            .any(|&a| !is_recursive_literal_or_const(cx, a));
        if dynamic {
            return;
        }
        cx.emit_offense(
            cx.node(node).loc.name,
            "Use block for static values.",
            None,
        );
    }

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.options_or_default::<ReturnFromStubOptions>().enforced_style
            != ReturnFromStubStyle::AndReturn
        {
            return;
        }
        // `stub_with_block?`: the send (or its chain) contains a stub.
        if !core::iter::once(node)
            .chain(cx.descendants(node))
            .any(|id| is_bare_receive(cx, id))
        {
            return;
        }
        let NodeKind::Block { body, .. } = *cx.kind(node) else {
            return;
        };
        // `dynamic?(body)`: a missing body falls back to
        // `NULL_BLOCK_BODY` upstream and still flags.
        if body.get().is_some_and(|b| !is_recursive_literal_or_const(cx, b)) {
            return;
        }
        let Some(open) = block_open_token(cx, node) else {
            return;
        };
        cx.emit_offense(open, "Use `and_return` for static values.", None);
    }
}

/// `true` when `root` or any descendant is a bare `receive` send.
///
/// Mirrors upstream `contains_stub?` (`def_node_search` for
/// `(send nil? :receive (...))`).
fn contains_stub(cx: &Cx<'_>, root: NodeId) -> bool {
    core::iter::once(root)
        .chain(cx.descendants(root))
        .any(|id| is_bare_receive(cx, id))
}

fn is_bare_receive(cx: &Cx<'_>, id: NodeId) -> bool {
    let NodeKind::Send {
        receiver, method, ..
    } = *cx.kind(id)
    else {
        return false;
    };
    receiver == OptNodeId::NONE && cx.symbol_str(method) == "receive"
}

#[cfg(test)]
mod tests {
    use super::{ReturnFromStub, ReturnFromStubOptions, ReturnFromStubStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn block_style() -> ReturnFromStubOptions {
        ReturnFromStubOptions {
            enforced_style: ReturnFromStubStyle::Block,
        }
    }

    #[test]
    fn flags_block_return_default_style() {
        test::<ReturnFromStub>().expect_offense(indoc! {r#"
                allow(Foo).to receive(:bar) { "baz" }
                                            ^ Use `and_return` for static values.
            "#});
    }

    #[test]
    fn flags_expect_block_return() {
        test::<ReturnFromStub>().expect_offense(indoc! {r#"
                expect(Foo).to receive(:bar) { "baz" }
                                             ^ Use `and_return` for static values.
            "#});
    }

    #[test]
    fn ignores_dynamic_block_body() {
        // `bar.baz` is a dynamic value, so the block style is fine.
        test::<ReturnFromStub>().expect_no_offenses(indoc! {r#"
                allow(Foo).to receive(:bar) { bar.baz }
            "#});
    }

    #[test]
    fn ignores_and_return_default_style() {
        test::<ReturnFromStub>().expect_no_offenses(indoc! {r#"
                allow(Foo).to receive(:bar).and_return("baz")
            "#});
    }

    #[test]
    fn flags_and_return_block_style() {
        test::<ReturnFromStub>()
            .with_options(&block_style())
            .expect_offense(indoc! {r#"
                allow(Foo).to receive(:bar).and_return("baz")
                                            ^^^^^^^^^^ Use block for static values.
            "#});
    }

    #[test]
    fn ignores_dynamic_and_return_block_style() {
        test::<ReturnFromStub>()
            .with_options(&block_style())
            .expect_no_offenses(indoc! {r#"
                allow(Foo).to receive(:bar).and_return(bar.baz)
            "#});
    }

    #[test]
    fn ignores_block_return_block_style() {
        test::<ReturnFromStub>()
            .with_options(&block_style())
            .expect_no_offenses(indoc! {r#"
                allow(Foo).to receive(:bar) { "baz" }
            "#});
    }
}

murphy_plugin_api::submit_cop!(ReturnFromStub);
