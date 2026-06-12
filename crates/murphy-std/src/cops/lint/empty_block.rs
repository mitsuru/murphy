//! `Lint/EmptyBlock` — flag a block (`{ }` / `do … end`) with no body.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EmptyBlock
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Message text aligned with RuboCop MSG ("Empty block detected.").
//!   AllowEmptyLambdas (default true) skips empty `-> {}`, `lambda {}`,
//!   `proc {}`, and `Proc.new {}` blocks, mirroring RuboCop's
//!   `lambda_or_proc?`. AllowComments (default true) skips a block whose
//!   source range contains a comment. Both options are read live via
//!   `cx.options_or_default`, so user overrides are honored at dispatch time.
//!   RuboCop's `allow_comment?` "line-comment disables the cop" branch is a
//!   documented divergence: Murphy treats any comment in the block range as
//!   allowing the empty block regardless of directive content.
//! ```
//!
//! Only `block` nodes are checked: numbered (`_1`) and `it` blocks reference
//! their implicit parameter, so they are never truly empty.

use murphy_plugin_api::{cop, CopOptions, Cx, NodeId};

#[derive(Default)]
pub struct EmptyBlock;

/// Cop options for [`EmptyBlock`], decoded live via `cx.options_or_default`.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        default = true,
        description = "When true, don't flag empty lambdas (`-> {}`) and procs (`proc {}`, `Proc.new {}`)."
    )]
    pub allow_empty_lambdas: bool,
    #[option(
        default = true,
        description = "When true, don't flag an empty block whose source range contains a comment."
    )]
    pub allow_comments: bool,
}

#[cop(
    name = "Lint/EmptyBlock",
    description = "Checks for blocks without a body.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl EmptyBlock {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.block_body(node).get().is_some() {
            return;
        }
        let opts = cx.options_or_default::<Options>();
        if opts.allow_empty_lambdas && is_lambda_or_proc(node, cx) {
            return;
        }
        if opts.allow_comments && !cx.comments_in_range(cx.range(node)).is_empty() {
            return;
        }
        cx.emit_offense(cx.range(node), "Empty block detected.", None);
    }
}

/// RuboCop's `lambda_or_proc?` — true for `-> {}`, `lambda {}`, `proc {}`,
/// and `Proc.new {}`. `cx.is_lambda` already covers the stabby and
/// `lambda`-call spellings; this adds the two `proc`/`Proc.new` shapes.
fn is_lambda_or_proc(node: NodeId, cx: &Cx<'_>) -> bool {
    if cx.is_lambda(node) {
        return true;
    }
    let Some(call) = cx.block_call(node).get() else {
        return false;
    };
    let Some(method) = cx.method_name(call) else {
        return false;
    };
    match method {
        // `proc {}` — receiverless `proc` call.
        "proc" => cx.call_receiver(call).get().is_none(),
        // `Proc.new {}` — `Proc` constant (bare or `::Proc`) receiver.
        "new" => cx
            .call_receiver(call)
            .get()
            .is_some_and(|recv| cx.is_global_const(recv, "Proc")),
        _ => false,
    }
}

murphy_plugin_api::submit_cop!(EmptyBlock);

#[cfg(test)]
mod tests {
    use super::{EmptyBlock, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_brace_block() {
        test::<EmptyBlock>().expect_offense(indoc! {r#"
            items.each { }
            ^^^^^^^^^^^^^^ Empty block detected.
        "#});
    }

    #[test]
    fn flags_empty_do_end_block_single_line() {
        // `do; end` keeps the whole block range on one source line so the
        // caret annotation can express it (RuboCop highlights the whole block).
        test::<EmptyBlock>().expect_offense(indoc! {r#"
            items.each do; end
            ^^^^^^^^^^^^^^^^^^ Empty block detected.
        "#});
    }

    #[test]
    fn accepts_block_with_body() {
        test::<EmptyBlock>().expect_no_offenses("items.each { |i| i }\n");
    }

    // AllowEmptyLambdas (default true).

    #[test]
    fn accepts_empty_stabby_lambda() {
        test::<EmptyBlock>().expect_no_offenses("-> {}\n");
    }

    #[test]
    fn accepts_empty_lambda_method() {
        test::<EmptyBlock>().expect_no_offenses("lambda {}\n");
    }

    #[test]
    fn accepts_empty_proc() {
        test::<EmptyBlock>().expect_no_offenses("proc {}\n");
    }

    #[test]
    fn accepts_empty_proc_new() {
        test::<EmptyBlock>().expect_no_offenses("Proc.new {}\n");
    }

    #[test]
    fn flags_empty_non_lambda_block_even_with_lambda_allowance() {
        // A plain block named like a method is still flagged.
        test::<EmptyBlock>().expect_offense(indoc! {r#"
            foo {}
            ^^^^^^ Empty block detected.
        "#});
    }

    // AllowComments (default true).

    #[test]
    fn accepts_empty_block_with_inner_comment() {
        test::<EmptyBlock>().expect_no_offenses(indoc! {r#"
            items.each do
              # noop
            end
        "#});
    }

    #[test]
    fn empty_block_with_multibyte_body_is_not_empty() {
        test::<EmptyBlock>().expect_no_offenses("items.each { |i| 名前 }\n");
    }

    #[test]
    fn allow_empty_lambdas_false_flags_empty_lambda() {
        // Override disables the lambda/proc allowance — the empty lambda fires.
        test::<EmptyBlock>()
            .with_options(&Options {
                allow_empty_lambdas: false,
                allow_comments: true,
            })
            .expect_offense(indoc! {r#"
                -> {}
                ^^^^^ Empty block detected.
            "#});
    }

    #[test]
    fn allow_comments_true_is_the_default_and_skips_comment_only_block() {
        // Explicitly setting the default still skips the comment-only block,
        // confirming the option is read live (not hardcoded to one value).
        test::<EmptyBlock>()
            .with_options(&Options {
                allow_empty_lambdas: true,
                allow_comments: true,
            })
            .expect_no_offenses(indoc! {r#"
                items.each do
                  # noop
                end
            "#});
    }

    #[test]
    fn offense_message_matches_rubocop_verbatim() {
        // Pins RuboCop's MSG = 'Empty block detected.'
        test::<EmptyBlock>().expect_offense(indoc! {r#"
            x.map { }
            ^^^^^^^^^ Empty block detected.
        "#});
    }
}
