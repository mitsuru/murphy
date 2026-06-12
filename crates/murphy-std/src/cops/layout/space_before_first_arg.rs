//! `Layout/SpaceBeforeFirstArg` — enforces exactly one space between a
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceBeforeFirstArg
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-889k
//! notes: >
//!   Mirrors RuboCop's `on_send`/`on_csend`: flags a parenthesis-less method
//!   call whose first argument is separated from the method name by zero or
//!   more-than-one space, and rewrites the gap to a single space. Operator
//!   methods, setter methods, and parenthesized calls are skipped. The
//!   zero-space case (`something'hello'`) emits a zero-length insert-point
//!   offense. murphy-889k: `AllowForAlignment` (default `true`) reuses
//!   Murphy's vertical-alignment heuristic (`is_alignment_at_column`), which
//!   keys on a non-whitespace character at the argument's column on an
//!   adjacent line; this is a simpler approximation of RuboCop's
//!   `PrecedingFollowingAlignment` token-aware alignment (which also tracks
//!   assignment/operator alignment groups across runs of lines). Cross-line
//!   first arguments are exempt, matching RuboCop's `same_line?` guard inside
//!   `expect_params_after_method_name?`.
//! ```
//!
//! method name and its first argument for calls without parentheses.
//! Mirrors RuboCop's same-named cop.

use murphy_plugin_api::{CopOptions, Cx, NodeId, Range, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceBeforeFirstArg;

#[derive(CopOptions)]
pub struct SpaceBeforeFirstArgOptions {
    #[option(
        name = "AllowForAlignment",
        default = true,
        description = "Allow extra spaces that vertically align the first argument."
    )]
    pub allow_for_alignment: bool,
}

#[cop(
    name = "Layout/SpaceBeforeFirstArg",
    description = "Put one space between the method name and the first argument.",
    default_severity = "warning",
    default_enabled = true,
    options = SpaceBeforeFirstArgOptions,
)]
impl SpaceBeforeFirstArg {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

const MSG: &str = "Put one space between the method name and the first argument.";

fn check(node: NodeId, cx: &Cx<'_>) {
    // `regular_method_call_with_arguments?`: has arguments, not an operator
    // method (`a + b`), not a setter method (`obj.foo = x`).
    let args = cx.call_arguments(node);
    let Some(&first_arg) = args.first() else {
        return;
    };
    if cx.is_operator_method(node) || cx.is_setter_method(node) {
        return;
    }
    // `return if node.parenthesized?`
    if cx.is_parenthesized(node) {
        return;
    }

    // The method-name selector range; without it we cannot locate the gap.
    let selector = cx.selector(node);
    if selector == Range::ZERO {
        return;
    }
    let name_end = selector.end as usize;
    let arg_start = cx.range(first_arg).start as usize;
    // Defensive: a malformed range where the argument precedes the selector.
    if arg_start < name_end {
        return;
    }

    let src = cx.source().as_bytes();
    // `space` = the whitespace immediately preceding the first argument, i.e.
    // `range_with_surrounding_space(first_arg, side: :left)` clipped to the
    // run of spaces/tabs directly before the arg. We start at `arg_start` and
    // walk left over spaces/tabs, but never past the method-name end.
    let mut space_start = arg_start;
    while space_start > name_end && matches!(src[space_start - 1], b' ' | b'\t') {
        space_start -= 1;
    }
    let space_len = arg_start - space_start;

    // `return if space.length == 1` — already exactly one space, clean.
    if space_len == 1 {
        return;
    }

    if !expect_params_after_method_name(cx, node, first_arg, space_start, arg_start) {
        return;
    }

    let space = Range {
        start: space_start as u32,
        end: arg_start as u32,
    };
    cx.emit_offense(space, MSG, None);
    cx.emit_edit(space, " ");
}

/// RuboCop's `expect_params_after_method_name?`:
/// - always expect when there is zero space between the method name and the
///   first argument (`something'hello'`);
/// - otherwise, only when the first argument is on the same line as the call
///   AND it is not exempted by `AllowForAlignment`.
fn expect_params_after_method_name(
    cx: &Cx<'_>,
    node: NodeId,
    first_arg: NodeId,
    space_start: usize,
    arg_start: usize,
) -> bool {
    // `no_space_between_method_name_and_first_argument?`
    if space_start == arg_start {
        return true;
    }

    // `same_line?(first_arg, node)` — both ends on the same source line. The
    // gap is whitespace-only (spaces/tabs) by construction, so the call name
    // and arg share a line iff there is no newline between the call start and
    // the arg. We approximate with the gap: a whitespace-only gap on one line.
    let src = cx.source().as_bytes();
    let call_start = cx.range(node).start as usize;
    let same_line = !src[call_start..arg_start].contains(&b'\n');
    if !same_line {
        return false;
    }

    // `!(allow_for_alignment? && aligned_with_something?(first_arg))`
    let opts = cx.options_or_default::<SpaceBeforeFirstArgOptions>();
    if opts.allow_for_alignment && crate::cops::util::is_alignment_at_column(src, arg_start) {
        let _ = first_arg;
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{SpaceBeforeFirstArg, SpaceBeforeFirstArgOptions};
    use murphy_plugin_api::test_support::{
        indoc, run_cop_with_options_and_edits, test,
    };

    #[test]
    fn flags_multiple_spaces_before_first_arg() {
        test::<SpaceBeforeFirstArg>().expect_correction(
            indoc! {r#"
                something  x
                         ^^ Put one space between the method name and the first argument.
            "#},
            "something x\n",
        );
    }

    #[test]
    fn flags_multiple_spaces_before_first_arg_with_multiple_args() {
        test::<SpaceBeforeFirstArg>().expect_correction(
            indoc! {r#"
                something   y, z
                         ^^^ Put one space between the method name and the first argument.
            "#},
            "something y, z\n",
        );
    }

    #[test]
    fn accepts_single_space_before_first_arg() {
        test::<SpaceBeforeFirstArg>()
            .expect_no_offenses("something x\nsomething y, z\nsomething 'hello'\n");
    }

    #[test]
    fn accepts_parenthesized_call() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses("something(  x)\n");
    }

    #[test]
    fn accepts_operator_method() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses("a  +  b\n");
    }

    #[test]
    fn accepts_setter_method() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses("obj.foo =  x\n");
    }

    #[test]
    fn accepts_call_without_arguments() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses("something\n");
    }

    /// `on_csend` dispatch: a safe-navigation call `foo&.bar  baz` is treated
    /// the same as `on_send`.
    #[test]
    fn flags_multiple_spaces_before_first_arg_on_csend() {
        test::<SpaceBeforeFirstArg>().expect_correction(
            indoc! {r#"
                foo&.bar  baz
                        ^^ Put one space between the method name and the first argument.
            "#},
            "foo&.bar baz\n",
        );
    }

    /// Zero space between method name and first argument (`something'hello'`).
    /// The offense range is a zero-length insert point, which the caret
    /// annotation format cannot represent, so we verify via run_cop + edits.
    #[test]
    fn flags_zero_space_before_string_argument() {
        let opts = SpaceBeforeFirstArgOptions {
            allow_for_alignment: true,
        };
        let result = run_cop_with_options_and_edits::<SpaceBeforeFirstArg>("something'hello'\n", &opts);
        assert_eq!(
            result.offenses.len(),
            1,
            "expected 1 offense, got {:?}",
            result.offenses
        );
        assert_eq!(
            result.offenses[0].message,
            "Put one space between the method name and the first argument."
        );
        assert_eq!(result.edits.len(), 1, "expected 1 edit");
        assert_eq!(result.edits[0].replacement, " ");
    }

    /// `AllowForAlignment: true` (default) exempts an argument aligned with a
    /// non-whitespace character directly above it.
    #[test]
    fn accepts_aligned_argument_when_allow_for_alignment() {
        test::<SpaceBeforeFirstArg>().expect_no_offenses(indoc! {r#"
            foo    1
            foobar 2
        "#});
    }

    /// With `AllowForAlignment: false`, alignment spacing is flagged.
    #[test]
    fn flags_aligned_argument_when_disallow_for_alignment() {
        let opts = SpaceBeforeFirstArgOptions {
            allow_for_alignment: false,
        };
        test::<SpaceBeforeFirstArg>()
            .with_options(&opts)
            .expect_correction(
                indoc! {r#"
                    foo    1
                       ^^^^ Put one space between the method name and the first argument.
                    foobar 2
                "#},
                "foo 1\nfoobar 2\n",
            );
    }
}

murphy_plugin_api::submit_cop!(SpaceBeforeFirstArg);
