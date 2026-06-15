//! `Style/ArrayJoin` — use `Array#join` instead of `Array#*`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ArrayJoin
//! upstream_version_checked: 1.86.2
//! version_added: "0.20"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Detects `array_literal * string_literal` and autocorrects to
//!   `array_literal.join(string_literal)`.
//!   Only fires when the receiver is an array literal (NodeKind::Array) and the
//!   single argument is a string literal (NodeKind::Str), matching RuboCop's
//!   (send $array :* $str) pattern.
//!   Safe-navigation (a&.*(sep)) is not handled -- RuboCop does not handle it either.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! %w(foo bar baz) * ","
//! %w(foo bar baz)*", "
//!
//! # good
//! %w(foo bar baz).join(",")
//! %w(one two three) * 4      # integer arg -- not flagged
//! %w(one two three) * test   # variable arg -- not flagged
//! ```
//!
//! ## Why this shape
//!
//! RuboCop restricts detection to receiver being an array literal and arg being
//! a string literal to avoid false positives from Ruby's dynamic type system.
//! foo * "," where foo might not be an Array is not flagged.
//!
//! ## Autocorrect
//!
//! Replaces the whole send node with array.join(arg) by interpolating the raw
//! source of both sub-nodes. Whole-node replacement is used because the rewrite
//! rearranges the structure: a * b becomes a.join(b).

use murphy_plugin_api::{Cx, NoOptions, NodeId, cop, def_node_matcher};

const MSG: &str = "Favor `Array#join` over `Array#*`.";

// RuboCop parity: RuboCop's `Style/ArrayJoin` matcher is `(send $array :* $str)`.
// Murphy's macro differs in two ways from RuboCop node-pattern:
//   1. A bare `$array` is a capture *named* "array" with a `Wildcard` body
//      (matches anything), NOT a typed capture; the typed form is `$(array ...)`.
//   2. Atom kinds (`str`, `int`, `sym`, ...) cannot be captured generically —
//      `$(str ...)` is rejected; only a specific literal (`$"x"`) or the bare
//      kind matcher `str` is allowed. So the string arg is type-constrained with
//      a bare `str` and its node is read back via `cx.call_arguments`.
// The `str` arg slot (no `...`) means exactly one string argument, matching the
// prior `arg_nodes.len() != 1` + `NodeKind::Str` check; `$(array ...)` enforces
// the array-literal receiver and captures it for the `array.join(str)` rewrite.
def_node_matcher!(join_candidate, "(send $(array ...) :* str)");

/// Stateless unit struct.
#[derive(Default)]
pub struct ArrayJoin;

#[cop(
    name = "Style/ArrayJoin",
    description = "Use Array#join instead of Array#*.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ArrayJoin {
    #[on_node(kind = "send", methods = ["*"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Array-literal receiver (captured) `*` single string arg.
        let Some((recv_id,)) = join_candidate(node, cx) else {
            return;
        };
        // The match guarantees exactly one `str` argument; read its node back.
        let Some(&arg_id) = cx.call_arguments(node).first() else {
            return;
        };

        // Offense on the `*` selector (loc.selector / loc.name in RuboCop).
        let selector_range = cx.selector(node);
        cx.emit_offense(selector_range, MSG, None);

        // Autocorrect: `array * str` -> `array.join(str)`.
        let array_src = cx.raw_source(cx.range(recv_id));
        let arg_src = cx.raw_source(cx.range(arg_id));
        let replacement = format!("{array_src}.join({arg_src})");
        cx.emit_edit(cx.range(node), &replacement);
    }
}

murphy_plugin_api::submit_cop!(ArrayJoin);

#[cfg(test)]
mod tests {
    use super::ArrayJoin;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_array_literal_multiplied_by_string() {
        test::<ArrayJoin>()
            .expect_correction(
                indoc! {r#"
                    %w(one two three) * ", "
                                      ^ Favor `Array#join` over `Array#*`.
                "#},
                "%w(one two three).join(\", \")\n",
            );
    }

    #[test]
    fn flags_without_spaces_around_operator() {
        test::<ArrayJoin>()
            .expect_correction(
                indoc! {r#"
                    %w(one two three)*", "
                                     ^ Favor `Array#join` over `Array#*`.
                "#},
                "%w(one two three).join(\", \")\n",
            );
    }

    #[test]
    fn flags_when_assigned_to_variable() {
        test::<ArrayJoin>()
            .expect_correction(
                indoc! {r#"
                    foo = %w(one two three)*", "
                                           ^ Favor `Array#join` over `Array#*`.
                "#},
                "foo = %w(one two three).join(\", \")\n",
            );
    }

    #[test]
    fn does_not_flag_integer_argument() {
        test::<ArrayJoin>().expect_no_offenses("%w(one two three) * 4\n");
    }

    #[test]
    fn does_not_flag_variable_argument() {
        test::<ArrayJoin>().expect_no_offenses("%w(one two three) * test\n");
    }

    #[test]
    fn does_not_flag_variable_receiver() {
        test::<ArrayJoin>().expect_no_offenses("foo * \",\"\n");
    }

    // --- Boundary characterization (murphy-vn3o): pin the exact node set the
    // hand-rolled `NodeKind::Array` / `NodeKind::Str` destructure matches, so the
    // `(send $array :* $str)` refactor can be proven equivalent.

    #[test]
    fn boundary_bracket_array_literal_flagged() {
        // `[..]` is also NodeKind::Array, like `%w(..)`.
        test::<ArrayJoin>().expect_correction(
            indoc! {r#"
                [1, 2, 3] * ", "
                          ^ Favor `Array#join` over `Array#*`.
            "#},
            "[1, 2, 3].join(\", \")\n",
        );
    }

    #[test]
    fn boundary_explicit_dot_star_flagged() {
        // Explicit `.*( )` dot form is the same `send :*` node; the matcher must
        // treat it identically to the infix form.
        test::<ArrayJoin>().expect_correction(
            indoc! {r#"
                [1, 2].*(", ")
                       ^ Favor `Array#join` over `Array#*`.
            "#},
            "[1, 2].join(\", \")\n",
        );
    }

    #[test]
    fn boundary_two_string_args_not_flagged() {
        // `*` with two arguments is not the `Array#*` shape (exactly one str arg).
        test::<ArrayJoin>().expect_no_offenses("[1, 2].*(\",\", \"x\")\n");
    }
}
