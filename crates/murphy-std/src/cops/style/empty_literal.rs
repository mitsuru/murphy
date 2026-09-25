//! `Style/EmptyLiteral` — prefer `[]`/`{}`/`""` over `Array.new`/`Hash.new`/`String.new`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/EmptyLiteral
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `Array.new`, `Hash.new`, and `String.new` with no arguments and no
//!   block, autocorrecting to `[]`, `{}`, and `""` respectively.
//!   `String.new` is not flagged when the `frozen_string_literal` magic comment
//!   is present (mutable vs frozen semantics difference).
//!   `Hash.new` as a method argument without parentheses is corrected to `({})` 
//!   to avoid ambiguity with a block.
//!   Calls with arguments or blocks (e.g. `Array.new(10)`, `Hash.new { }`) are
//!   not flagged.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop, def_node_matcher};

// RuboCop parity: `Style/EmptyLiteral` heads are
// `(send (const {nil? cbase} :Array) :new)`,
// `(send (const {nil? cbase} :Hash) :new)`,
// `(send (const {nil? cbase} :String) :new)` (zero args, send-only).
// In Murphy `::Array` collapses to `Const{scope:None}`: `nil?` covers bare +
// `::` (pinned by `boundary_flags_cbase_array_new`). Namespaced `Foo::Array`
// etc. still accept (pinned by `boundary_accepts_namespaced_*`). `send`
// covers `Send` only (not `Csend`, pinned by `boundary_accepts_csend_array_new`).
// No-arg (no `...`), no-block, frozen-string-literal, and unparenthesized-arg
// guards stay hand-rolled below.
def_node_matcher!(
    empty_literal_new,
    "(send (const nil? {:Array :Hash :String}) :new)"
);

const ARR_MSG: &str = "Use array literal `[]` instead of `Array.new`.";
const HASH_MSG: &str = "Use hash literal `{}` instead of `Hash.new`.";
const STR_MSG: &str = "Use string literal `\"\"` instead of `String.new`.";

#[derive(Default)]
pub struct EmptyLiteral;

#[cop(
    name = "Style/EmptyLiteral",
    description = "Prefer literals to Array.new/Hash.new/String.new.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl EmptyLiteral {
    #[on_node(kind = "send")]
    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        // `(send (const nil? {:Array :Hash :String}) :new)` (zero args,
        // `Send` only). `nil?` covers bare + `::`; `send` excludes `Csend`.
        if !empty_literal_new(node, cx) {
            return;
        }
        let Some(recv) = cx.call_receiver(node).get() else {
            return;
        };
        let NodeKind::Const { name, .. } = *cx.kind(recv) else {
            return;
        };
        let const_name = cx.symbol_str(name);

        // Skip if this call is the send-part of a block (Array.new { }, Hash.new { }).
        if parent_is_block_call(node, cx) {
            return;
        }

        match const_name {
            "Array" => {
                cx.emit_offense(cx.range(node), ARR_MSG, None);
                cx.emit_edit(cx.range(node), "[]");
            }
            "Hash" => {
                cx.emit_offense(cx.range(node), HASH_MSG, None);
                // If this call is used as an unparenthesized argument, wrap in
                // parens to avoid `{}` being parsed as a block.
                let replacement = if is_unparenthesized_arg(node, cx) {
                    "({})"
                } else {
                    "{}"
                };
                cx.emit_edit(cx.range(node), replacement);
            }
            "String" => {
                // Skip when frozen_string_literal comment is present — String.new
                // produces a mutable string while `""` would be frozen.
                if cx.frozen_string_literal_comment().is_some() {
                    return;
                }
                cx.emit_offense(cx.range(node), STR_MSG, None);
                cx.emit_edit(cx.range(node), "\"\"");
            }
            _ => {}
        }
    }
}

/// Returns `true` if `node` is the send part of a block (e.g. `Array.new { }`).
/// In that case, the block changes semantics and we must not flag it.
fn parent_is_block_call(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    match *cx.kind(parent) {
        NodeKind::Block { call, .. }
        | NodeKind::Numblock { send: call, .. }
        | NodeKind::Itblock { send: call, .. } => call == node,
        _ => false,
    }
}

/// Returns `true` if `node` is a direct send argument without parentheses,
/// i.e. the immediate parent is a `send` or `csend` and the argument list
/// is not parenthesised (checked via whether there are multiple or it's the
/// sole argument at the call site without parens).
///
/// For Murphy's purposes: if the parent is a Send/CSend and the node is one
/// of its arguments (not the receiver), and the call has no closing paren
/// token that wraps this argument, we treat it as unparenthesized.
fn is_unparenthesized_arg(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    match *cx.kind(parent) {
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {
            // Check if `node` is in the argument list (not the receiver).
            let recv = cx.call_receiver(parent);
            if recv.get() == Some(node) {
                return false;
            }
            // Check whether the argument list has closing paren.
            !cx.is_parenthesized(parent)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::EmptyLiteral;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Array.new ---

    #[test]
    fn flags_array_new() {
        test::<EmptyLiteral>().expect_offense(indoc! {"
            x = Array.new
                ^^^^^^^^^ Use array literal `[]` instead of `Array.new`.
        "});
    }

    #[test]
    fn autocorrects_array_new() {
        test::<EmptyLiteral>().expect_correction(
            indoc! {"
                x = Array.new
                    ^^^^^^^^^ Use array literal `[]` instead of `Array.new`.
            "},
            "x = []\n",
        );
    }

    #[test]
    fn accepts_array_new_with_size() {
        test::<EmptyLiteral>().expect_no_offenses("Array.new(10)\n");
    }

    #[test]
    fn accepts_array_new_with_block() {
        test::<EmptyLiteral>().expect_no_offenses("Array.new(3) { |i| i }\n");
    }

    // --- Hash.new ---

    #[test]
    fn flags_hash_new() {
        test::<EmptyLiteral>().expect_offense(indoc! {"
            x = Hash.new
                ^^^^^^^^ Use hash literal `{}` instead of `Hash.new`.
        "});
    }

    #[test]
    fn autocorrects_hash_new() {
        test::<EmptyLiteral>().expect_correction(
            indoc! {"
                x = Hash.new
                    ^^^^^^^^ Use hash literal `{}` instead of `Hash.new`.
            "},
            "x = {}\n",
        );
    }

    #[test]
    fn accepts_hash_new_with_block() {
        test::<EmptyLiteral>().expect_no_offenses("Hash.new { |h, k| h[k] = [] }\n");
    }

    // --- String.new ---

    #[test]
    fn flags_string_new_without_frozen_comment() {
        test::<EmptyLiteral>().expect_offense(indoc! {r#"
            x = String.new
                ^^^^^^^^^^ Use string literal `""` instead of `String.new`.
        "#});
    }

    #[test]
    fn autocorrects_string_new() {
        test::<EmptyLiteral>().expect_correction(
            indoc! {r#"
                x = String.new
                    ^^^^^^^^^^ Use string literal `""` instead of `String.new`.
            "#},
            "x = \"\"\n",
        );
    }

    #[test]
    fn accepts_string_new_with_frozen_string_literal() {
        test::<EmptyLiteral>()
            .expect_no_offenses("# frozen_string_literal: true\nString.new\n");
    }

    // --- accepts ---

    #[test]
    fn accepts_empty_array_literal() {
        test::<EmptyLiteral>().expect_no_offenses("[]\n");
    }

    #[test]
    fn accepts_empty_hash_literal() {
        test::<EmptyLiteral>().expect_no_offenses("{}\n");
    }

    // --- Boundary characterization (murphy-ft88.6): pin the exact node set
    // the hand-rolled `Array`/`Hash`/`String` guard matches, so the verbatim
    // `(send (const nil? {:Array :Hash :String}) :new)` refactor can be
    // proven equivalent. `::Array` collapses to `Const{scope:None}` in
    // Murphy: `nil?` covers bare + `::`. Namespaced `Foo::Array` still
    // accepts; `&.` is a `csend` node and `send` covers `Send` only.

    #[test]
    fn boundary_flags_cbase_array_new() {
        test::<EmptyLiteral>().expect_offense(indoc! {"
            x = ::Array.new
                ^^^^^^^^^^^ Use array literal `[]` instead of `Array.new`.
        "});
    }

    #[test]
    fn boundary_accepts_namespaced_array_new() {
        test::<EmptyLiteral>().expect_no_offenses("x = Foo::Array.new\n");
    }

    #[test]
    fn boundary_accepts_namespaced_hash_new() {
        test::<EmptyLiteral>().expect_no_offenses("x = Foo::Hash.new\n");
    }

    #[test]
    fn boundary_accepts_namespaced_string_new() {
        test::<EmptyLiteral>().expect_no_offenses("x = Foo::String.new\n");
    }

    #[test]
    fn boundary_accepts_csend_array_new() {
        // `&.` is a `csend` node; `send` covers `Send` only.
        test::<EmptyLiteral>().expect_no_offenses("x = Array&.new\n");
    }
}

murphy_plugin_api::submit_cop!(EmptyLiteral);
