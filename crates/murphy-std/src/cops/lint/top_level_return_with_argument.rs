//! `Lint/TopLevelReturnWithArgument` — checks for top-level return with arguments.
//!
//! Top-level returns with arguments are always ignored in Ruby, making them
//! effectively dead code. This is detected automatically since Ruby 2.7.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/TopLevelReturnWithArgument
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   No autocorrect. Parity with RuboCop: flags return with arguments at
//!   top level of a file (outside any method/block). Returns inside `begin
//!   end` blocks at top level are also flagged, matching RuboCop's ancestor
//!   check.
//! ```
//!
//! ## Matched shapes
//!
//! - `return 1, 2, 3` at the top level of a file
//! - `return x if cond` at the top level
//!
//! ## Accepted
//!
//! - `return` without arguments (bare return)
//! - `return x` inside a method definition or block
//!
//! ## No autocorrect
//!
//! Autocorrect is not provided.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG: &str = "Top level return with argument detected.";

#[derive(Default)]
pub struct TopLevelReturnWithArgument;

#[cop(
    name = "Lint/TopLevelReturnWithArgument",
    description = "Checks for top-level return with arguments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl TopLevelReturnWithArgument {
    #[on_node(kind = "return")]
    fn check_return(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Return(value_opt) = *cx.kind(node) else {
            return;
        };

        if value_opt.get().is_none() {
            return;
        }

        if !is_top_level(node, cx) {
            return;
        }

        cx.emit_offense(cx.range(node), MSG, None);
    }
}

fn is_top_level(node: NodeId, cx: &Cx<'_>) -> bool {
    let mut current = node;
    loop {
        match cx.parent(current).get() {
            None => return true,
            Some(parent) => {
                if matches!(
                    cx.kind(parent),
                    NodeKind::Block { .. }
                        | NodeKind::Def { .. }
                        | NodeKind::Defs { .. }
                        | NodeKind::Numblock { .. }
                        | NodeKind::Itblock { .. }
                ) {
                    return false;
                }
                current = parent;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TopLevelReturnWithArgument;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_top_level_return_with_arguments() {
        test::<TopLevelReturnWithArgument>().expect_offense(indoc! {r#"
            return 1, 2, 3
            ^^^^^^^^^^^^^^ Top level return with argument detected.
        "#});
    }

    #[test]
    fn accepts_bare_return() {
        test::<TopLevelReturnWithArgument>().expect_no_offenses("return\n");
    }

    #[test]
    fn flags_multiple_top_level_returns() {
        test::<TopLevelReturnWithArgument>().expect_offense(indoc! {r#"
            return 1, 2, 3
            ^^^^^^^^^^^^^^ Top level return with argument detected.

            return 1, 2, 3
            ^^^^^^^^^^^^^^ Top level return with argument detected.

            return 1, 2, 3
            ^^^^^^^^^^^^^^ Top level return with argument detected.
        "#});
    }

    #[test]
    fn accepts_return_in_block() {
        test::<TopLevelReturnWithArgument>().expect_no_offenses(indoc! {"
            foo

            [1, 2, 3, 4, 5].each { |n| return n }

            return

            bar
        "});
    }

    #[test]
    fn flags_top_level_among_blocks() {
        test::<TopLevelReturnWithArgument>().expect_offense(indoc! {r#"
            foo

            [1, 2, 3, 4, 5].each { |n| return n }

            return 1, 2, 3
            ^^^^^^^^^^^^^^ Top level return with argument detected.

            bar
        "#});
    }

    #[test]
    fn flags_top_level_when_method_exists() {
        test::<TopLevelReturnWithArgument>().expect_offense(indoc! {r#"
            def method
              return 'Hello World'
            end

            return 1, 2, 3
            ^^^^^^^^^^^^^^ Top level return with argument detected.
        "#});
    }

    #[test]
    fn accepts_return_with_modifier_if_no_args() {
        test::<TopLevelReturnWithArgument>().expect_no_offenses(indoc! {"
            foo

            return if 1 == 1

            bar

            def method
              return 'Hello World' if 1 == 1
            end
        "});
    }

    #[test]
    fn flags_return_with_args_and_modifier_if() {
        test::<TopLevelReturnWithArgument>().expect_offense(indoc! {r#"
            foo
            return 1, 2, 3 if 1 == 1
            ^^^^^^^^^^^^^^ Top level return with argument detected.
            bar
            return 2
            ^^^^^^^^ Top level return with argument detected.
            return 3
            ^^^^^^^^ Top level return with argument detected.

            def method
              return 'Hello World' if 1 == 1
            end
        "#});
    }

    #[test]
    fn flags_return_with_args_in_semicolon_separated() {
        test::<TopLevelReturnWithArgument>().expect_offense(indoc! {r#"
            foo

            if a == b; warn 'hey'; return 42; end
                                   ^^^^^^^^^ Top level return with argument detected.

            bar
        "#});
    }

    #[test]
    fn accepts_bare_return_in_semicolon_separated() {
        test::<TopLevelReturnWithArgument>().expect_no_offenses(indoc! {"
            foo

            if a == b; warn 'hey'; return; end

            bar
        "});
    }
}

murphy_plugin_api::submit_cop!(TopLevelReturnWithArgument);
