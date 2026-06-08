//! `Lint/UnexpectedBlockArity` — Checks for blocks with the wrong number of arguments.
//!
//! Some methods (like `Enumerable#inject` and `Enumerable#reduce`) expect
//! at least two positional block arguments. This cop flags calls where the
//! block provides fewer positional arguments than expected.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnexpectedBlockArity
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Handles Block, Numblock, and Itblock nodes with hardcoded method-to-arity mapping.
//!   Numblock uses max_n as positional arity; Itblock uses arity 1.
//!   Configurable Methods option is not yet supported.
//! ```
//!
//! ## Matched shapes
//!
//! - `values.reduce {}` — block with no args, method expects 2.
//! - `values.reduce { |a| }` — block with 1 arg, method expects 2.
//!
//! ## No autocorrect
//!
//! The correct fix depends on intent (adjust block args or change the method).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const METHODS: &[(&str, usize)] = &[
    ("each_with_object", 2),
    ("inject", 2),
    ("max", 2),
    ("max_by", 1),
    ("min", 2),
    ("minmax_by", 1),
    ("reduce", 2),
];

fn expected_arity(method_name: &str) -> Option<usize> {
    METHODS
        .iter()
        .find_map(|(name, arity)| if *name == method_name { Some(*arity) } else { None })
}

#[derive(Default)]
pub struct UnexpectedBlockArity;

#[cop(
    name = "Lint/UnexpectedBlockArity",
    description = "Checks for blocks with the wrong number of arguments.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UnexpectedBlockArity {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, args, .. } = *cx.kind(node) else {
            return;
        };
        let Some((method_name, expected)) = method_and_expected(call, cx) else {
            return;
        };
        let actual = count_positional_args(cx, args);
        self.check_arity(node, &method_name, expected, actual, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, max_n, .. } = *cx.kind(node) else {
            return;
        };
        let Some((method_name, expected)) = method_and_expected(send, cx) else {
            return;
        };
        self.check_arity(node, &method_name, expected, max_n as usize, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Itblock { send, .. } = *cx.kind(node) else {
            return;
        };
        let Some((method_name, expected)) = method_and_expected(send, cx) else {
            return;
        };
        self.check_arity(node, &method_name, expected, 1, cx);
    }
}

impl UnexpectedBlockArity {
    fn check_arity(
        &self,
        node: NodeId,
        method_name: &str,
        expected: usize,
        actual: usize,
        cx: &Cx<'_>,
    ) {
        if actual >= expected {
            return;
        }
        let msg = format!(
            "`{method_name}` expects at least {expected} positional arguments, got {actual}."
        );
        cx.emit_offense(cx.range(node), &msg, None);
    }
}

fn method_and_expected(call: NodeId, cx: &Cx<'_>) -> Option<(String, usize)> {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(call) else {
        return None;
    };
    if receiver.is_none() {
        return None;
    }
    let method_name = cx.symbol_str(method).to_string();
    let expected = expected_arity(&method_name)?;
    Some((method_name, expected))
}

fn count_positional_args(cx: &Cx<'_>, args_node: NodeId) -> usize {
    let NodeKind::Args(list) = *cx.kind(args_node) else {
        return 0;
    };
    let mut count = 0;
    for &arg in cx.list(list).iter() {
        match *cx.kind(arg) {
            NodeKind::Arg(_) | NodeKind::Optarg { .. } | NodeKind::Mlhs(_) => count += 1,
            NodeKind::Restarg(_) => return usize::MAX,
            NodeKind::Unknown => count += 1,
            _ => {}
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::UnexpectedBlockArity;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn accepts_two_params() {
        test::<UnexpectedBlockArity>().expect_no_offenses(indoc! {"
            values.reduce { |a, b| a + b }
        "});
    }

    #[test]
    fn accepts_three_params() {
        test::<UnexpectedBlockArity>().expect_no_offenses(indoc! {"
            values.reduce { |a, b, c| a + b }
        "});
    }

    #[test]
    fn accepts_splat() {
        test::<UnexpectedBlockArity>().expect_no_offenses(indoc! {"
            values.reduce { |*x| x }
        "});
    }

    #[test]
    fn flags_no_params() {
        test::<UnexpectedBlockArity>().expect_offense(indoc! {r#"
            values.reduce { }
            ^^^^^^^^^^^^^^^^^ `reduce` expects at least 2 positional arguments, got 0.
        "#});
    }

    #[test]
    fn flags_one_param() {
        test::<UnexpectedBlockArity>().expect_offense(indoc! {r#"
            values.reduce { |a| a }
            ^^^^^^^^^^^^^^^^^^^^^^^ `reduce` expects at least 2 positional arguments, got 1.
        "#});
    }

    #[test]
    fn flags_only_keyword_args() {
        test::<UnexpectedBlockArity>().expect_offense(indoc! {r#"
            values.reduce { |a:, b:| a + b }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `reduce` expects at least 2 positional arguments, got 0.
        "#});
    }

    #[test]
    fn flags_only_keyword_splat() {
        test::<UnexpectedBlockArity>().expect_offense(indoc! {r#"
            values.reduce { |**kwargs| kwargs }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `reduce` expects at least 2 positional arguments, got 0.
        "#});
    }

    #[test]
    fn flags_destructuring_arity_one() {
        test::<UnexpectedBlockArity>().expect_offense(indoc! {r#"
            values.reduce { |(a, b)| a + b }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ `reduce` expects at least 2 positional arguments, got 1.
        "#});
    }

    #[test]
    fn accepts_destructuring_arity_two() {
        test::<UnexpectedBlockArity>().expect_no_offenses(indoc! {"
            values.reduce { |(a, b), c| a + b + c }
        "});
    }

    #[test]
    fn flags_optarg_arity_one() {
        test::<UnexpectedBlockArity>().expect_offense(indoc! {r#"
            values.reduce { |a = 1| a }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ `reduce` expects at least 2 positional arguments, got 1.
        "#});
    }

    #[test]
    fn accepts_optarg_arity_two() {
        test::<UnexpectedBlockArity>().expect_no_offenses(indoc! {"
            values.reduce { |a = 1, b = 2| a + b }
        "});
    }

    #[test]
    fn accepts_no_receiver() {
        test::<UnexpectedBlockArity>().expect_no_offenses(indoc! {"
            reduce { }
        "});
    }

    #[test]
    fn flags_inject() {
        test::<UnexpectedBlockArity>().expect_offense(indoc! {r#"
            values.inject { }
            ^^^^^^^^^^^^^^^^^ `inject` expects at least 2 positional arguments, got 0.
        "#});
    }

    #[test]
    fn flags_numblock_one_param() {
        test::<UnexpectedBlockArity>().expect_offense(indoc! {r#"
            values.reduce { _1 }
            ^^^^^^^^^^^^^^^^^^^^ `reduce` expects at least 2 positional arguments, got 1.
        "#});
    }

    #[test]
    fn accepts_numblock_two_params() {
        test::<UnexpectedBlockArity>().expect_no_offenses(indoc! {"
            values.reduce { _1 + _2 }
        "});
    }

    #[test]
    fn flags_itblock() {
        test::<UnexpectedBlockArity>().expect_offense(indoc! {r#"
            values.reduce { it }
            ^^^^^^^^^^^^^^^^^^^^ `reduce` expects at least 2 positional arguments, got 1.
        "#});
    }

    #[test]
    fn accepts_numblock_max_by_two_params() {
        test::<UnexpectedBlockArity>().expect_no_offenses(indoc! {"
            values.max_by { _1 + _2 }
        "});
    }
}

murphy_plugin_api::submit_cop!(UnexpectedBlockArity);
