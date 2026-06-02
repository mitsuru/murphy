//! `Style/RedundantArrayConstructor` — flags redundant `Array` constructor
//! calls and replaces them with an array literal.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantArrayConstructor
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   The cop is disabled by default (Enabled: pending in RuboCop).
//!   Three redundant patterns are detected:
//!     - Array.new([...]) / ::Array.new([...]) → [...]
//!     - Array['a', 'b'] / ::Array['a', 'b'] → ['a', 'b']
//!     - Array(['a', 'b']) (Kernel method) → ['a', 'b']
//!   Not flagged:
//!     - Array.new(3, 'foo') (size + default value form)
//!     - Array.new(3) { 'foo' } (size + block form, block node wraps the send)
//!     - Array.new([...]) { ... } (send wrapped as block call)
//!     - Array.new (no args)
//!     - Array('foo') (Kernel conversion of non-array)
//!     - Foo::Array.new([]) (namespaced constant)
//!     - Array.[]('a') (explicit dot bracket form, rare but valid Ruby)
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! Array.new([])
//! Array[]
//! Array([])
//! Array.new(['foo', 'foo', 'foo'])
//! Array['foo', 'foo', 'foo']
//! Array(['foo', 'foo', 'foo'])
//!
//! # good
//! []
//! ['foo', 'foo', 'foo']
//! Array.new(3, 'foo')
//! Array.new(3) { 'foo' }
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Remove the redundant `Array` constructor.";

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantArrayConstructor;

#[cop(
    name = "Style/RedundantArrayConstructor",
    description = "Checks for the instantiation of array using redundant `Array` constructor.",
    default_severity = "warning",
    default_enabled = false,
    options = murphy_plugin_api::NoOptions,
)]
impl RedundantArrayConstructor {
    #[on_node(kind = "send", methods = ["new", "[]", "Array"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(node)
    else {
        return;
    };

    let method_name = cx.symbol_str(method);
    let arg_list = cx.list(args);
    let node_range = cx.range(node);

    // If this send is the call of a block node (e.g. Array.new([]) { ... }),
    // autocorrect would produce `[...] { ... }` which is invalid Ruby.
    if cx.block_node(node).get().is_some() {
        return;
    }

    match method_name {
        "new" => {
            // Array.new([...]) / ::Array.new([...])
            // Receiver must be `Array` constant with nil or cbase scope.
            let Some(recv_id) = receiver.get() else {
                return;
            };
            if !is_array_const(recv_id, cx) {
                return;
            }
            // Must have exactly one argument that is an array literal.
            if arg_list.len() != 1 {
                return;
            }
            let arg_id = arg_list[0];
            if !matches!(cx.kind(arg_id), NodeKind::Array { .. }) {
                return;
            }

            let arg_range = cx.range(arg_id);

            // Offense: entire send node.
            cx.emit_offense(node_range, MSG, None);

            // Autocorrect (two surgical edits):
            // Edit 1: delete `Array.new(` — from node start to arg start.
            cx.emit_edit(
                Range {
                    start: node_range.start,
                    end: arg_range.start,
                },
                "",
            );
            // Edit 2: delete the closing `)` — from arg end to node end.
            cx.emit_edit(
                Range {
                    start: arg_range.end,
                    end: node_range.end,
                },
                "",
            );
        }
        "[]" => {
            // Array['a', 'b'] / ::Array['a', 'b'] → ['a', 'b']
            // Receiver must be `Array` constant with nil or cbase scope.
            let Some(recv_id) = receiver.get() else {
                return;
            };
            if !is_array_const(recv_id, cx) {
                return;
            }
            // Guard against `Array.[]('a')` explicit dot form: autocorrecting
            // that would delete the `Array` prefix leaving `[](...)` which is
            // invalid Ruby. Only handle the implicit bracket form `Array[...]`.
            if cx.is_dot(node) {
                return;
            }
            // Any number of args (including zero) is valid for [].

            // Offense: entire send node.
            cx.emit_offense(node_range, MSG, None);

            // Autocorrect: delete the `Array` receiver prefix.
            // The `[]` selector starts right after the receiver in bracket form,
            // so we delete from the node start to just before the `[` selector.
            let selector_start = cx.selector(node).start;
            cx.emit_edit(
                Range {
                    start: node_range.start,
                    end: selector_start,
                },
                "",
            );
        }
        "Array" => {
            // Array([...]) — Kernel method, nil receiver.
            // Must have no explicit receiver (nil).
            if receiver.get().is_some() {
                return;
            }
            // Must have exactly one argument that is an array literal.
            if arg_list.len() != 1 {
                return;
            }
            let arg_id = arg_list[0];
            if !matches!(cx.kind(arg_id), NodeKind::Array { .. }) {
                return;
            }

            let arg_range = cx.range(arg_id);

            // Offense: entire send node.
            cx.emit_offense(node_range, MSG, None);

            // Autocorrect (two surgical edits):
            // Edit 1: delete `Array(` — from node start to arg start.
            cx.emit_edit(
                Range {
                    start: node_range.start,
                    end: arg_range.start,
                },
                "",
            );
            // Edit 2: delete the closing `)` — from arg end to node end.
            cx.emit_edit(
                Range {
                    start: arg_range.end,
                    end: node_range.end,
                },
                "",
            );
        }
        _ => {}
    }
}

/// Returns true if `node` is a `Const` with name `Array` and nil or cbase scope.
fn is_array_const(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return false;
    };
    if cx.symbol_str(name) != "Array" {
        return false;
    }
    // Allow nil scope (bare `Array`) and cbase scope (`::Array`),
    // but reject namespaced constants like `Foo::Array`.
    if let Some(scope_id) = scope.get() {
        if !matches!(cx.kind(scope_id), NodeKind::Cbase) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::RedundantArrayConstructor;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Array.new([...]) -----

    #[test]
    fn flags_array_new_empty() {
        test::<RedundantArrayConstructor>().expect_offense(indoc! {"
            Array.new([])
            ^^^^^^^^^^^^^ Remove the redundant `Array` constructor.
        "});
    }

    #[test]
    fn corrects_array_new_empty() {
        test::<RedundantArrayConstructor>().expect_correction(
            indoc! {"
                Array.new([])
                ^^^^^^^^^^^^^ Remove the redundant `Array` constructor.
            "},
            "[]\n",
        );
    }

    #[test]
    fn flags_array_new_with_elements() {
        test::<RedundantArrayConstructor>().expect_offense(indoc! {"
            Array.new(['foo', 'bar'])
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Remove the redundant `Array` constructor.
        "});
    }

    #[test]
    fn corrects_array_new_with_elements() {
        test::<RedundantArrayConstructor>().expect_correction(
            indoc! {"
                Array.new(['foo', 'bar'])
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Remove the redundant `Array` constructor.
            "},
            "['foo', 'bar']\n",
        );
    }

    // ----- Array[...] -----

    #[test]
    fn flags_array_bracket_empty() {
        test::<RedundantArrayConstructor>().expect_offense(indoc! {"
            Array[]
            ^^^^^^^ Remove the redundant `Array` constructor.
        "});
    }

    #[test]
    fn corrects_array_bracket_empty() {
        test::<RedundantArrayConstructor>().expect_correction(
            indoc! {"
                Array[]
                ^^^^^^^ Remove the redundant `Array` constructor.
            "},
            "[]\n",
        );
    }

    #[test]
    fn flags_array_bracket_with_elements() {
        test::<RedundantArrayConstructor>().expect_offense(indoc! {"
            Array['foo', 'bar']
            ^^^^^^^^^^^^^^^^^^^ Remove the redundant `Array` constructor.
        "});
    }

    #[test]
    fn corrects_array_bracket_with_elements() {
        test::<RedundantArrayConstructor>().expect_correction(
            indoc! {"
                Array['foo', 'bar']
                ^^^^^^^^^^^^^^^^^^^ Remove the redundant `Array` constructor.
            "},
            "['foo', 'bar']\n",
        );
    }

    // ----- Array([...]) -----

    #[test]
    fn flags_kernel_array_empty() {
        test::<RedundantArrayConstructor>().expect_offense(indoc! {"
            Array([])
            ^^^^^^^^^ Remove the redundant `Array` constructor.
        "});
    }

    #[test]
    fn corrects_kernel_array_empty() {
        test::<RedundantArrayConstructor>().expect_correction(
            indoc! {"
                Array([])
                ^^^^^^^^^ Remove the redundant `Array` constructor.
            "},
            "[]\n",
        );
    }

    #[test]
    fn flags_kernel_array_with_elements() {
        test::<RedundantArrayConstructor>().expect_offense(indoc! {"
            Array(['foo', 'bar'])
            ^^^^^^^^^^^^^^^^^^^^^ Remove the redundant `Array` constructor.
        "});
    }

    #[test]
    fn corrects_kernel_array_with_elements() {
        test::<RedundantArrayConstructor>().expect_correction(
            indoc! {"
                Array(['foo', 'bar'])
                ^^^^^^^^^^^^^^^^^^^^^ Remove the redundant `Array` constructor.
            "},
            "['foo', 'bar']\n",
        );
    }

    // ----- Negative cases -----

    #[test]
    fn accepts_array_new_size_default() {
        test::<RedundantArrayConstructor>().expect_no_offenses("Array.new(3, 'foo')\n");
    }

    #[test]
    fn accepts_array_new_size_only() {
        test::<RedundantArrayConstructor>().expect_no_offenses("Array.new(3)\n");
    }

    #[test]
    fn accepts_array_new_no_args() {
        // Array.new with no args: no array literal argument, not flagged.
        test::<RedundantArrayConstructor>().expect_no_offenses("Array.new\n");
    }

    #[test]
    fn accepts_kernel_array_with_non_array_arg() {
        // Array('foo') converts non-array, not redundant.
        test::<RedundantArrayConstructor>().expect_no_offenses("Array('foo')\n");
    }

    #[test]
    fn accepts_namespaced_array_new() {
        test::<RedundantArrayConstructor>().expect_no_offenses("Foo::Array.new([])\n");
    }

    #[test]
    fn accepts_plain_array_literal() {
        test::<RedundantArrayConstructor>().expect_no_offenses("['foo', 'bar']\n");
    }

    #[test]
    fn accepts_array_new_with_block() {
        // Array.new([1]) { |x| x } — send wrapped as block call: correction
        // would produce `[1] { |x| x }` which is invalid Ruby.
        test::<RedundantArrayConstructor>()
            .expect_no_offenses("Array.new([1]) { |x| x }\n");
    }

    #[test]
    fn accepts_array_bracket_explicit_dot_form() {
        // Array.[]('a') — explicit dot form: correction would produce []('a')
        // which is invalid Ruby.
        test::<RedundantArrayConstructor>().expect_no_offenses("Array.[]('a')\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantArrayConstructor);
