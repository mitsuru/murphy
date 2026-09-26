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

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop, def_node_matcher};

// RuboCop parity: `Style/RedundantArrayConstructor` `redundant_array_constructor`
// is `{(send (const {nil? cbase} :Array) :new $(array ...)) (send (const {nil? cbase} :Array) :[] ...) (send nil? :Array $(array ...))}`.
// Murphy splits the const-receiver arms (`Array.new`/`Array[]`) from the bare
// `Array(...)` Kernel arm, so the verbatim port covers the const part only:
// `(send (const nil? :Array) {:new :[]} ...)` (send-only, matching the
// `Send`-only dispatch; no `csend` handler).
// In Murphy `::Array` collapses to `Const{scope:None}`: `nil?` covers bare +
// `::` (both flag, pinned by `boundary_flags_cbase_array_new` +
// `boundary_flags_cbase_array_bracket`). Namespaced `Foo::Array` is not
// top-level, so silent (pinned by `accepts_namespaced_array_new` +
// `boundary_ignores_namespaced_array_bracket`). `send` covers `Send` only,
// matching the `#[on_node(kind = "send")]` dispatch (csend silent, pinned by
// `boundary_ignores_csend_array_new` + `boundary_ignores_csend_array_bracket`).
// The `$(array ...)` capture stays hand-rolled below (single array-literal arg
// for `new`; `is_dot` guard for `[]`); the bare `Array(...)` arm stays
// hand-rolled.
def_node_matcher!(
    array_new_or_index,
    "(send (const nil? :Array) {:new :[]} ...)"
);

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
            // `(send (const nil? :Array) {:new :[]} ...)`
            // (`Array.new` / `::Array.new`, top-level only, send-only).
            // The `$(array ...)` capture stays hand-rolled below.
            if !array_new_or_index(node, cx) {
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
            // `(send (const nil? :Array) {:new :[]} ...)`
            // (`Array[...]` / `::Array[...]`, top-level only, send-only).
            // The `is_dot` guard stays hand-rolled below.
            if !array_new_or_index(node, cx) {
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

    // --- Boundary characterization (murphy-ft88.16): pin the exact node set
    // the hand-rolled `is_array_const` (nil/cbase scope, reject namespaced) +
    // method `new`/`[]` matches, so the verbatim
    // `(send (const nil? :Array) {:new :[]} ...)` refactor can be proven
    // equivalent. `::Array` collapses to `Const{scope:None}`: `nil?` covers
    // bare + `::` (both flag). Namespaced `Foo::Array` is not top-level, so
    // silent (pre-existing `accepts_namespaced_array_new` pins `new`; this
    // pins `[]`). `send` covers `Send` only, matching the `Send`-only dispatch
    // (no `csend` handler, so `&.` is silent).

    #[test]
    fn boundary_flags_cbase_array_new() {
        test::<RedundantArrayConstructor>().expect_offense(indoc! {"
            ::Array.new([])
            ^^^^^^^^^^^^^^^ Remove the redundant `Array` constructor.
        "});
    }

    #[test]
    fn boundary_flags_cbase_array_bracket() {
        test::<RedundantArrayConstructor>().expect_offense(indoc! {"
            ::Array['foo', 'bar']
            ^^^^^^^^^^^^^^^^^^^^^ Remove the redundant `Array` constructor.
        "});
    }

    #[test]
    fn boundary_ignores_namespaced_array_bracket() {
        test::<RedundantArrayConstructor>().expect_no_offenses("Foo::Array['foo', 'bar']\n");
    }

    #[test]
    fn boundary_ignores_csend_array_new() {
        test::<RedundantArrayConstructor>().expect_no_offenses("Array&.new([])\n");
    }

    #[test]
    fn boundary_ignores_csend_array_bracket() {
        test::<RedundantArrayConstructor>().expect_no_offenses("Array&.[]('foo')\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantArrayConstructor);
