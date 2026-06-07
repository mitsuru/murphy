//! `Style/RedundantInterpolation` — checks for strings that are just an interpolated expression.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantInterpolation
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Implements the full detection logic (single-interpolation dstr, variable
//!   interpolation, begin-wrapped expressions, implicit concatenation guard,
//!   %W() array guard, match-pattern `=>` skip). Autocorrect is implemented
//!   with three branches (variable `.to_s`, single-variable with parenthesization
//!   handling, and general `(expr).to_s`).
//!
//!   Known v1 limitation: Ruby version gating (`target_ruby_version <= 2.7`
//!   for match-pattern) is not implemented — Murphy does not expose the
//!   target Ruby version to plugin cops yet. The match-pattern skip only
//!   guards the `=>` form (`MatchPattern`), which is correct for Ruby 3.0+
//!   but misses the `in` → `match-pattern` alias in Ruby 2.7. In practice
//!   `"#{42 in var}"` is extremely rare on Ruby 2.7, so this should not
//!   cause real-world false positives.
//! ```
//!
//! ## Dispatch
//!
//! Subscribes to `NodeKind::Dstr` and checks for single-interpolation strings
//! like `"#{var}"`, `"#{expr}"`, `"#@var"`, `"#@@var"`, `"#$var"`, etc.
//!
//! ## Matched shapes
//!
//! - `"#{single_interpolation}"` — any `dstr` with exactly one child that is
//!   a variable (ivar, cvar, gvar, lvar, back_ref, nth_ref) or a `begin` node.
//! - Excludes implicitly concatenated strings (parent is also `dstr`).
//! - Excludes strings inside `%W(...)` arrays.
//! - Excludes strings containing `=>` pattern match expressions.
//!
//! ## Autocorrect
//!
//! Replaces the entire string literal with the expression + `.to_s`.
//! Three autocorrect branches:
//!
//! 1. **Variable interpolation** (`#@var`, `#@@var`, `#$var`, `#$1`, etc.):
//!    `"#@var"` → `@var.to_s`
//! 2. **Single-variable interpolation** (`"#{var}"`, `"#{method(args)}"`,
//!    `"#{method arg}"`): drops `#{`...`}`, appends `.to_s`, and inserts
//!    parentheses when the method call is not already parenthesized.
//! 3. **Other** (`"#{1 + 1}"`, `"#{1 + 1; 2 + 2}"`): wraps in parens:
//!    `(expr).to_s`.

use murphy_plugin_api::{Cx, NodeId, NodeKind, cop};

const MSG: &str = "Prefer `to_s` over string interpolation.";

const OPERATOR_METHODS: &[&str] = &[
    "==", "!=", "===", "=~", "!~", "<=>", "<", "<=", ">", ">=", "+", "-",
    "*", "/", "%", "**", "&", "|", "^", "~", "[]", "[]=", "!", "-@", "+@",
];

#[derive(Default)]
pub struct RedundantInterpolation;

#[cop(
    name = "Style/RedundantInterpolation",
    description = "Checks for strings that are just an interpolated expression.",
    default_severity = "warning",
    default_enabled = true,
    options = murphy_plugin_api::NoOptions,
)]
impl RedundantInterpolation {
    #[on_node(kind = "dstr")]
    fn check_dstr(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Dstr(children) = *cx.kind(node) else {
            return;
        };
        let children_list = cx.list(children);
        if children_list.len() != 1 {
            return;
        }
        let embedded = children_list[0];

        if let Some(parent) = cx.parent(node).get()
            && matches!(*cx.kind(parent), NodeKind::Dstr(_))
        {
            return;
        }

        if let Some(parent) = cx.parent(node).get()
            && matches!(*cx.kind(parent), NodeKind::Array(_))
            && is_percent_array(parent, cx)
        {
            return;
        }

        if !is_interpolation(embedded, cx) {
            return;
        }

        if contains_match_pattern(embedded, cx) {
            return;
        }

        cx.emit_offense(cx.range(node), MSG, None);

        autocorrect(node, embedded, cx);
    }
}

fn autocorrect(node: NodeId, embedded: NodeId, cx: &Cx<'_>) {
    if is_variable_interpolation(embedded, cx) {
        let var_src = cx.raw_source(cx.range(embedded));
        cx.emit_edit(cx.range(node), &format!("{}.to_s", var_src));
        return;
    }

    let begin_children = match *cx.kind(embedded) {
        NodeKind::Begin(cl) | NodeKind::Kwbegin(cl) => cx.list(cl),
        _ => return,
    };

    let expr_src = |id: NodeId| -> String { cx.raw_source(cx.range(id)).to_string() };

    // The Begin node's source includes `#{` and `}` markers. Derive the body text
    // from the children's sources rather than the Begin itself.
    let body_src = || -> String {
        let parts: Vec<String> = begin_children
            .iter()
            .map(|child| expr_src(*child))
            .collect();
        parts.join("; ")
    };

    if begin_children.len() == 1 {
        let only_child = begin_children[0];

        if is_variable_interpolation(only_child, cx) {
            let var_src = expr_src(only_child);
            cx.emit_edit(cx.range(node), &format!("{}.to_s", var_src));
        } else if let NodeKind::Send { receiver, method, args } = *cx.kind(only_child) {
            if is_operator_method_name(method, cx) {
                let b = body_src();
                cx.emit_edit(cx.range(node), &format!("({}).to_s", b));
            } else if needs_parenthesization(only_child, cx) {
                let receiver_src = receiver
                    .get()
                    .map(&expr_src)
                    .unwrap_or_default();
                let method_name = cx.symbol_str(method);
                let dot = if receiver.get().is_some() { "." } else { "" };
                let args_src: Vec<String> =
                    cx.list(args).iter().map(|a| expr_src(*a)).collect();
                cx.emit_edit(
                    cx.range(node),
                    &format!(
                        "{}{}{}({}).to_s",
                        receiver_src,
                        dot,
                        method_name,
                        args_src.join(", ")
                    ),
                );
            } else {
                let send_src = expr_src(only_child);
                cx.emit_edit(cx.range(node), &format!("{}.to_s", send_src));
            }
        } else {
            let b = body_src();
            cx.emit_edit(cx.range(node), &format!("({}).to_s", b));
        }
    } else {
        let b = body_src();
        cx.emit_edit(cx.range(node), &format!("({}).to_s", b));
    }
}

fn is_percent_array(node: NodeId, cx: &Cx<'_>) -> bool {
    let src = cx.raw_source(cx.range(node));
    src.trim_start().starts_with("%W")
}

fn needs_parenthesization(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { method, args, .. } = *cx.kind(node) else {
        return false;
    };
    let args_list = cx.list(args);
    if args_list.is_empty() {
        return false;
    }
    let method_name = cx.symbol_str(method);
    let range = cx.range(node);
    let src = cx.raw_source(range);
    let Some(method_end) = find_method_name_end(src, method_name) else {
        return false;
    };
    let after_method = src[method_end..].trim_start();
    !after_method.starts_with('(')
}

fn find_method_name_end(src: &str, method_name: &str) -> Option<usize> {
    let idx = src.rfind(method_name)?;
    Some(idx + method_name.len())
}

fn is_variable_interpolation(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Ivar(_)
            | NodeKind::Cvar(_)
            | NodeKind::Gvar(_)
            | NodeKind::Lvar(_)
            | NodeKind::BackRef(_)
            | NodeKind::NthRef(_)
    )
}

fn is_interpolation(node: NodeId, cx: &Cx<'_>) -> bool {
    is_variable_interpolation(node, cx)
        || matches!(*cx.kind(node), NodeKind::Begin(_) | NodeKind::Kwbegin(_))
}

fn is_operator_method_name(method: murphy_plugin_api::Symbol, cx: &Cx<'_>) -> bool {
    OPERATOR_METHODS.contains(&cx.symbol_str(method))
}

fn contains_match_pattern(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.descendants(node)
        .iter()
        .any(|child| matches!(*cx.kind(*child), NodeKind::MatchPattern { .. }))
}

murphy_plugin_api::submit_cop!(RedundantInterpolation);

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_expression() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#{1 + 1}"
                ^^^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "(1 + 1).to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_percent_pipe() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                %|#{1 + 1}|
                ^^^^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "(1 + 1).to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_percent_q() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                %Q(#{1 + 1})
                ^^^^^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "(1 + 1).to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_compound() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#{1 + 1; 2 + 2}"
                ^^^^^^^^^^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "(1 + 1; 2 + 2).to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_instance_variable() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#{@var}"
                ^^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "@var.to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_shorthand_ivar() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#@var"
                ^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "@var.to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_class_variable() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#{@@var}"
                ^^^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "@@var.to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_shorthand_cvar() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#@@var"
                ^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "@@var.to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_global_variable() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#{$var}"
                ^^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "$var.to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_shorthand_gvar() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#$var"
                ^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "$var.to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_nth_ref() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#{$1}"
                ^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "$1.to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_shorthand_nth_ref() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#$1"
                ^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "$1.to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_last_match_ref() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#{$+}"
                ^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "$+.to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_shorthand_last_match_ref() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#$+"
                ^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "$+.to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_local_variable() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                var = 1; "#{var}"
                         ^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "var = 1; var.to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_method_with_parens() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#{do_something(42)}"
                ^^^^^^^^^^^^^^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "do_something(42).to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_method_without_parens() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#{do_something 42}"
                ^^^^^^^^^^^^^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "do_something(42).to_s\n",
        );
    }

    #[test]
    fn flags_and_corrects_method_with_receiver_no_parens() {
        test::<RedundantInterpolation>().expect_correction(
            indoc! {r##"
                "#{foo.do_something 42}"
                ^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `to_s` over string interpolation.
            "##},
            "foo.do_something(42).to_s\n",
        );
    }

    #[test]
    fn flags_in_array_literal() {
        test::<RedundantInterpolation>().expect_offense(indoc! {r##"
            ["#{@var}", 'foo']
             ^^^^^^^^^ Prefer `to_s` over string interpolation.
        "##});
    }

    // --- Pattern-matching cases ---

    #[test]
    fn flags_in_pattern_matching() {
        test::<RedundantInterpolation>().expect_offense(indoc! {r##"
            "#{42 in var}"
            ^^^^^^^^^^^^^^ Prefer `to_s` over string interpolation.
        "##});
    }

    #[test]
    fn accepts_hashrocket_pattern_matching() {
        test::<RedundantInterpolation>().expect_no_offenses(
            r##""#{42 => var}"
"##,
        );
    }

    #[test]
    fn accepts_string_with_prefix_text() {
        test::<RedundantInterpolation>().expect_no_offenses(
            r##""this is #{@sparta}"
"##,
        );
    }

    #[test]
    fn accepts_string_with_suffix_text() {
        test::<RedundantInterpolation>().expect_no_offenses(
            r##""#{@sparta} this is"
"##,
        );
    }

    #[test]
    fn accepts_implicit_concat_with_later() {
        test::<RedundantInterpolation>().expect_no_offenses(
            r##""#{sparta}" ' this is'
"##,
        );
    }

    #[test]
    fn accepts_implicit_concat_with_earlier() {
        test::<RedundantInterpolation>().expect_no_offenses(
            r##"'this is ' "#{sparta}"
"##,
        );
    }

    #[test]
    fn accepts_percent_w_array() {
        test::<RedundantInterpolation>().expect_no_offenses(
            r##"%W(#{@var} foo)
"##,
        );
    }
}
