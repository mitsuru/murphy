//! `Style/PredicateWithKind` — prefer `any?(Klass)` to `any? { |x| x.is_a?(Klass) }`.
//!
//! Looks for uses of `any?`, `all?`, `none?`, or `one?` with a block containing
//! only an `is_a?`, `kind_of?`, or `instance_of?` check, and suggests using the
//! predicate method with the class argument directly.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/PredicateWithKind
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full parity. Handles regular blocks, numblocks (_1) and itblocks (it)
//!   across any?/all?/none?/one?, including safe navigation (&.). Offense
//!   spans the whole block node; autocorrect replaces selector..block-end
//!   with `method(Klass)`. Autocorrect is unsafe (instance_of? + === subclass
//!   semantics), matching RuboCop's SafeAutoCorrect: false.
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct PredicateWithKind;

const KIND_METHODS: [&str; 3] = ["is_a?", "kind_of?", "instance_of?"];
const PREDICATE_METHODS: [&str; 4] = ["any?", "all?", "none?", "one?"];

#[cop(
    name = "Style/PredicateWithKind",
    description = "Prefer `any?(Klass)` to `any? { |x| x.is_a?(Klass) }`.",
    default_severity = "warning",
    default_enabled = false,
    safe_autocorrect = false,
    options = murphy_plugin_api::NoOptions
)]
impl PredicateWithKind {
    /// Regular block: `array.any? { |x| x.is_a?(Integer) }`.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, args, body } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        // The block must take exactly one named parameter.
        let arg_children = cx.children(args);
        let [arg_id] = arg_children.as_slice() else {
            return;
        };
        let NodeKind::Arg(param_sym) = *cx.kind(*arg_id) else {
            return;
        };
        check_predicate_block(node, call, body_id, cx.symbol_str(param_sym), cx);
    }

    /// Numbered-parameter block: `array.any? { _1.is_a?(Integer) }`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, max_n, body } = *cx.kind(node) else {
            return;
        };
        if max_n != 1 {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        check_predicate_block(node, send, body_id, "_1", cx);
    }

    /// `it`-parameter block: `array.any? { it.is_a?(Integer) }`.
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Itblock { send, body } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        check_predicate_block(node, send, body_id, "it", cx);
    }
}

/// Shared check for all three block kinds.
///
/// `block_node` is the whole `Block`/`Numblock`/`Itblock` node, `call` its
/// underlying predicate call (`Send`/`Csend`), `body` its single body
/// expression, and `param` the block's parameter name (`x`, `_1`, or `it`).
fn check_predicate_block(
    block_node: NodeId,
    call: NodeId,
    body: NodeId,
    param: &str,
    cx: &Cx<'_>,
) {
    // The predicate call must be `any?`/`all?`/`none?`/`one?` with no argument.
    let Some(method) = cx.method_name(call) else {
        return;
    };
    if !PREDICATE_METHODS.contains(&method) {
        return;
    }
    if !cx.call_arguments(call).is_empty() {
        return;
    }

    // The body must be a single kind-check `Send` (not `Csend`, not `Begin`):
    // `<param>.is_a?(Klass)`. RuboCop's pattern is `(send (lvar _) %KIND _)`.
    let NodeKind::Send {
        receiver: kind_recv,
        method: kind_method,
        args: kind_args,
    } = *cx.kind(body)
    else {
        return;
    };
    if !KIND_METHODS.contains(&cx.symbol_str(kind_method)) {
        return;
    }

    // The kind-check receiver must be the block parameter (an `Lvar`), not an
    // external variable or method call.
    let Some(recv_id) = kind_recv.get() else {
        return;
    };
    let NodeKind::Lvar(recv_sym) = *cx.kind(recv_id) else {
        return;
    };
    if cx.symbol_str(recv_sym) != param {
        return;
    }

    // Exactly one argument — the class to pass to the predicate.
    let [klass] = cx.list(kind_args) else {
        return;
    };

    let klass_src = cx.raw_source(cx.range(*klass));
    let replacement = format!("{method}({klass_src})");
    let message =
        format!("Prefer `{replacement}` to `{method} {{ ... }}` with a kind check.");

    // Offense spans the whole block node (matches RuboCop's `add_offense(block_node)`).
    cx.emit_offense(cx.range(block_node), &message, None);

    // Autocorrect replaces selector-begin..block-end with `method(Klass)`,
    // preserving the receiver and dot that sit before the selector.
    let edit_range = Range {
        start: cx.selector(block_node).start,
        end: cx.range(block_node).end,
    };
    cx.emit_edit(edit_range, &replacement);
}

#[cfg(test)]
mod tests {
    use super::PredicateWithKind;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_is_a_block() {
        test::<PredicateWithKind>().expect_offense(indoc! {"
            array.any? { |x| x.is_a?(Integer) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `any?(Integer)` to `any? { ... }` with a kind check.
        "});
    }

    #[test]
    fn flags_kind_of_block() {
        test::<PredicateWithKind>().expect_offense(indoc! {"
            array.all? { |x| x.kind_of?(String) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `all?(String)` to `all? { ... }` with a kind check.
        "});
    }

    #[test]
    fn flags_instance_of_block() {
        test::<PredicateWithKind>().expect_offense(indoc! {"
            array.one? { |x| x.instance_of?(Float) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `one?(Float)` to `one? { ... }` with a kind check.
        "});
    }

    #[test]
    fn flags_namespaced_class() {
        test::<PredicateWithKind>().expect_offense(indoc! {"
            array.none? { |x| x.is_a?(ActiveRecord::Base) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `none?(ActiveRecord::Base)` to `none? { ... }` with a kind check.
        "});
    }

    #[test]
    fn flags_multiline_block() {
        test::<PredicateWithKind>().expect_offense(indoc! {"
            array.any? do |x|; x.is_a?(Integer); end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `any?(Integer)` to `any? { ... }` with a kind check.
        "});
    }

    #[test]
    fn flags_without_receiver() {
        test::<PredicateWithKind>().expect_offense(indoc! {"
            any? { |x| x.is_a?(Integer) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `any?(Integer)` to `any? { ... }` with a kind check.
        "});
    }

    #[test]
    fn flags_numblock() {
        test::<PredicateWithKind>().expect_offense(indoc! {"
            array.any? { _1.is_a?(Integer) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `any?(Integer)` to `any? { ... }` with a kind check.
        "});
    }

    #[test]
    fn flags_itblock() {
        test::<PredicateWithKind>().expect_offense(indoc! {"
            array.any? { it.is_a?(Integer) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `any?(Integer)` to `any? { ... }` with a kind check.
        "});
    }

    #[test]
    fn flags_safe_navigation() {
        test::<PredicateWithKind>().expect_offense(indoc! {"
            array&.any? { |x| x.is_a?(Integer) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `any?(Integer)` to `any? { ... }` with a kind check.
        "});
    }

    #[test]
    fn corrects_is_a_block() {
        test::<PredicateWithKind>().expect_correction(
            indoc! {"
                array.any? { |x| x.is_a?(Integer) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `any?(Integer)` to `any? { ... }` with a kind check.
            "},
            "array.any?(Integer)\n",
        );
    }

    #[test]
    fn corrects_multiline_block() {
        test::<PredicateWithKind>().expect_correction(
            indoc! {"
                array.all? do |x|; x.kind_of?(String); end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `all?(String)` to `all? { ... }` with a kind check.
            "},
            "array.all?(String)\n",
        );
    }

    #[test]
    fn corrects_numblock() {
        test::<PredicateWithKind>().expect_correction(
            indoc! {"
                array.none? { _1.instance_of?(Float) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `none?(Float)` to `none? { ... }` with a kind check.
            "},
            "array.none?(Float)\n",
        );
    }

    #[test]
    fn corrects_safe_navigation() {
        test::<PredicateWithKind>().expect_correction(
            indoc! {"
                array&.any? { |x| x.is_a?(Integer) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `any?(Integer)` to `any? { ... }` with a kind check.
            "},
            "array&.any?(Integer)\n",
        );
    }

    #[test]
    fn accepts_no_block() {
        test::<PredicateWithKind>().expect_no_offenses("array.any?\n");
    }

    #[test]
    fn accepts_non_kind_block() {
        test::<PredicateWithKind>().expect_no_offenses("array.any? { |x| x.even? }\n");
    }

    #[test]
    fn accepts_multiple_expressions() {
        test::<PredicateWithKind>().expect_no_offenses(indoc! {"
            array.any? do |x|
              next if x.nil?
              x.is_a?(Integer)
            end
        "});
    }

    #[test]
    fn accepts_external_variable() {
        test::<PredicateWithKind>().expect_no_offenses("array.any? { |x| y.is_a?(Integer) }\n");
    }

    #[test]
    fn accepts_predicate_with_argument() {
        test::<PredicateWithKind>().expect_no_offenses("array.any?(Integer)\n");
    }

    #[test]
    fn accepts_two_block_args() {
        test::<PredicateWithKind>().expect_no_offenses("array.any? { |x, y| x.is_a?(Integer) }\n");
    }

    #[test]
    fn accepts_unrelated_method() {
        test::<PredicateWithKind>().expect_no_offenses("array.select? { |x| x.is_a?(Integer) }\n");
    }
}
murphy_plugin_api::submit_cop!(PredicateWithKind);
