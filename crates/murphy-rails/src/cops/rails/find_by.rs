//! `Rails/FindBy` — prefer `find_by` over `where.take` / `where.first`.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/FindBy
//! upstream_version_checked: 2.35.0
//! version_added: "0.30"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND first/take, zero-arg
//!   outer gate, `where` receiver gate (Send or Csend, block-carrying
//!   `where` excluded via block_node), offense range from the `where`
//!   selector to the outer selector, `where{dot}{method}` message with
//!   `.`/`&.` from the call operator, and take-only autocorrect
//!   (selector rename + tail delete). `IgnoreWhereFirst` (default true)
//!   suppresses `where.first` unless disabled.
//! ```
//!
//! Identifies usages of `where.take` and changes them to `find_by`.
//! `where(...).first` can return different results from `find_by`
//! (different ordering), so it is ignored by default via
//! `IgnoreWhereFirst: true`.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct FindBy;

#[derive(CopOptions)]
pub struct FindByOptions {
    #[option(
        name = "IgnoreWhereFirst",
        default = true,
        description = "Whether to ignore `where.first` (which can differ from `find_by` by ordering)."
    )]
    pub ignore_where_first: bool,
}

#[cop(
    name = "Rails/FindBy",
    description = "Prefer find_by over where.first.",
    default_severity = "warning",
    default_enabled = true,
    options = FindByOptions,
)]
impl FindBy {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let outer_method = match cx.method_name(node) {
        Some(m) => m.to_owned(),
        None => return,
    };
    // Upstream `node.arguments.empty?`.
    if !cx.call_arguments(node).is_empty() {
        return;
    }
    if outer_method == "first" {
        let opts = cx.options_or_default::<FindByOptions>();
        if opts.ignore_where_first {
            return;
        }
    } else if outer_method != "take" {
        return;
    }
    // Upstream `where_method?`: receiver responds to method? and is `where`,
    // and is not any block type.
    let Some(recv) = cx.call_receiver(node).get() else {
        return;
    };
    let recv_method = match cx.method_name(recv) {
        Some(m) => m.to_owned(),
        None => return,
    };
    if recv_method != "where" {
        return;
    }
    if !matches!(*cx.kind(recv), NodeKind::Send { .. } | NodeKind::Csend { .. }) {
        return;
    }
    if cx.block_node(recv).get().is_some() {
        return;
    }
    // Offense range: receiver selector begin to outer selector end.
    let range = Range {
        start: cx.selector(recv).start,
        end: cx.selector(node).end,
    };
    let dot = cx
        .call_operator_loc(node)
        .map(|r| cx.raw_source(r).to_owned())
        .unwrap_or_else(|| ".".to_owned());
    let msg = format!("Use `find_by` instead of `where{dot}{outer_method}`.");
    cx.emit_offense(range, &msg, None);
    // Autocorrect only for `take` (upstream returns early for `first`).
    if outer_method == "first" {
        return;
    }
    cx.emit_edit(cx.selector(recv), "find_by");
    let tail = Range {
        start: cx.range(recv).end,
        end: cx.selector(node).end,
    };
    cx.emit_edit(tail, "");
}

#[cfg(test)]
mod tests {
    use super::{FindBy, FindByOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_where_take() {
        test::<FindBy>().expect_offense(indoc! {r#"
            User.where(name: 'Bruce').take
                 ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `find_by` instead of `where.take`.
        "#});
    }

    #[test]
    fn autocorrects_where_take() {
        test::<FindBy>().expect_correction(
            indoc! {r#"
                User.where(name: 'Bruce').take
                     ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `find_by` instead of `where.take`.
            "#},
            "User.find_by(name: 'Bruce')\n",
        );
    }

    #[test]
    fn ignores_where_first_by_default() {
        test::<FindBy>().expect_no_offenses("User.where(name: 'Bruce').first\n");
    }

    #[test]
    fn flags_where_first_when_ignore_disabled() {
        let opts = FindByOptions {
            ignore_where_first: false,
        };
        test::<FindBy>().with_options(&opts).expect_offense(indoc! {r#"
            User.where(name: 'Bruce').first
                 ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `find_by` instead of `where.first`.
        "#});
    }

    #[test]
    fn where_first_has_no_autocorrect() {
        use murphy_plugin_api::test_support::run_cop_with_options_and_edits;
        let opts = FindByOptions {
            ignore_where_first: false,
        };
        let run = run_cop_with_options_and_edits::<FindBy>(
            "User.where(name: 'Bruce').first\n",
            &opts,
        );
        assert_eq!(run.offenses.len(), 1);
        assert!(run.edits.is_empty());
    }

    #[test]
    fn flags_safe_navigation_take() {
        test::<FindBy>().expect_offense(indoc! {r#"
            User.where(name: 'Bruce')&.take
                 ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `find_by` instead of `where&.take`.
        "#});
    }

    #[test]
    fn does_not_flag_take_with_args() {
        test::<FindBy>().expect_no_offenses("User.where(name: 'x').take(2)\n");
    }

    #[test]
    fn does_not_flag_bare_take() {
        test::<FindBy>().expect_no_offenses("take\n");
    }

    #[test]
    fn does_not_flag_where_with_block() {
        test::<FindBy>().expect_no_offenses("User.where { |x| x }.take\n");
    }
}
murphy_plugin_api::submit_cop!(FindBy);
