//! `RSpec/RedundantPredicateMatcher` — prefer predicate matchers over `be_*`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/RedundantPredicateMatcher
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` with `RESTRICT_ON_SEND` (`be_all`,
//!   `be_cover`, `be_end_with`, `be_eql`, `be_equal`, `be_exist`,
//!   `be_exists`, `be_include`, `be_match`, `be_respond_to`,
//!   `be_start_with`): flags when the send has args, the parent is not a
//!   `Block` (`node.parent.block_type?`), and for `be_all` the first arg
//!   is a `Send` (`replaceable_arguments?`). The message is
//!   ``Use `<good>` instead of `<bad>`.`` with `be_exists` → `exist`
//!   and the rest `delete_prefix("be_")`. The offense range is the whole
//!   send per `add_offense(node)`. Detection is at parity (verified vs
//!   3.7.0, including `be_all(:sym)` clean, arg-less clean, and
//!   block-form clean); autocorrect (replace selector, except `be_all`)
//!   is not ported in this batch — same report-only convention as
//!   `RSpec/ReceiveCounts` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with the eleven `be_*` selectors:
//!
//! - `expect(foo).to be_exist(bar)` — flagged (→ `exist`).
//! - `expect(foo).not_to be_include(bar)` — flagged (→ `include`).
//! - `expect(foo).to be_all(bar)` — `bar` is a send, flagged (→ `all`).
//! - `expect(foo).to be_all(:sym)` — sym arg, clean.
//! - `expect(foo).to be_exist` — no args, clean.
//! - `expect(foo).to be_exist(bar) { baz }` — block parent, clean.
//!
//! ## No autocorrect
//!
//! Upstream replaces the selector with the predicate name (except
//! `be_all`). This batch reports only.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedundantPredicateMatcher;

#[cop(
    name = "RSpec/RedundantPredicateMatcher",
    description = "Checks for redundant predicate matcher.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RedundantPredicateMatcher {
    #[on_node(
        kind = "send",
        methods = [
            "be_all",
            "be_cover",
            "be_end_with",
            "be_eql",
            "be_equal",
            "be_exist",
            "be_exists",
            "be_include",
            "be_match",
            "be_respond_to",
            "be_start_with"
        ]
    )]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { method, args, .. } = *cx.kind(node) else {
            return;
        };
        let name = cx.symbol_str(method).to_owned();
        let arg_ids = cx.list(args);
        if arg_ids.is_empty() {
            return;
        }
        // Upstream `node.parent.block_type?`: a `Block` parent means the
        // `be_*` call takes a block — clean.
        if let Some(parent) = cx.parent(node).get()
            && matches!(*cx.kind(parent), NodeKind::Block { .. })
        {
            return;
        }
        // Upstream `replaceable_arguments?`: `be_all` needs a send arg.
        if name == "be_all" && !matches!(*cx.kind(arg_ids[0]), NodeKind::Send { .. }) {
            return;
        }
        let good = replaced_method_name(&name);
        cx.emit_offense(
            cx.range(node),
            &format!("Use `{good}` instead of `{name}`."),
            None,
        );
    }
}

fn replaced_method_name(method_name: &str) -> String {
    let name = method_name.strip_prefix("be_").unwrap_or(method_name);
    if name == "exists" {
        "exist".to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::RedundantPredicateMatcher;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_be_exist() {
        test::<RedundantPredicateMatcher>().expect_offense(indoc! {r#"
                expect(foo).to be_exist(bar)
                               ^^^^^^^^^^^^^ Use `exist` instead of `be_exist`.
            "#});
    }

    #[test]
    fn flags_be_include_not_to() {
        test::<RedundantPredicateMatcher>().expect_offense(indoc! {r#"
                expect(foo).not_to be_include(bar)
                                   ^^^^^^^^^^^^^^^ Use `include` instead of `be_include`.
            "#});
    }

    #[test]
    fn flags_be_all_with_send_arg() {
        test::<RedundantPredicateMatcher>().expect_offense(indoc! {r#"
                expect(foo).to be_all(bar)
                               ^^^^^^^^^^^ Use `all` instead of `be_all`.
            "#});
    }

    #[test]
    fn flags_be_exists_as_exist() {
        test::<RedundantPredicateMatcher>().expect_offense(indoc! {r#"
                expect(foo).to be_exists(bar)
                               ^^^^^^^^^^^^^^ Use `exist` instead of `be_exists`.
            "#});
    }

    #[test]
    fn does_not_flag_be_all_with_sym() {
        // `replaceable_arguments?`: first arg must be a send.
        test::<RedundantPredicateMatcher>().expect_no_offenses(indoc! {r#"
                expect(foo).to be_all(:sym)
            "#});
    }

    #[test]
    fn does_not_flag_without_args() {
        test::<RedundantPredicateMatcher>().expect_no_offenses(indoc! {r#"
                expect(foo).to be_exist
            "#});
    }

    #[test]
    fn does_not_flag_with_block() {
        test::<RedundantPredicateMatcher>().expect_no_offenses(indoc! {r#"
                expect(foo).to be_exist(bar) { baz }
            "#});
    }

    #[test]
    fn does_not_flag_unknown_be() {
        test::<RedundantPredicateMatcher>().expect_no_offenses(indoc! {r#"
                expect(foo).to be_foo(bar)
            "#});
    }
}

murphy_plugin_api::submit_cop!(RedundantPredicateMatcher);
