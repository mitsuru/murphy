//! `Style/ArrayIntersectWithSingleElement` — use `include?(element)` instead
//! of `intersect?([element])`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ArrayIntersectWithSingleElement
//! upstream_version_checked: 1.81.7
//! version_added: "1.81"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Ports the core pattern: `intersect?([single_element])` ->
//!   `include?(element)`, for both regular array literals and `%i[sym]`.
//!   Percent-literal autocorrect (for `%i[sym]` / `%w[str]`) is only emitted
//!   when the bare element text forms a valid simple Ruby identifier, method
//!   name, or numeric literal; complex cases (containing spaces, hyphens, or
//!   other special chars) emit the offense but skip the edit to avoid
//!   producing invalid Ruby (a v1 limitation). RuboCop uses `.inspect` which
//!   handles arbitrary Ruby values; Murphy does not yet have an equivalent.
//!   Both `send` and `csend` are handled, mirroring RuboCop's
//!   `alias on_csend on_send`. `Safe: false` matches upstream.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! array.intersect?([element])
//! array.intersect?(%i[element])
//! array&.intersect?([element])
//!
//! # good
//! array.include?(element)
//! ```
//!
//! ## Why this shape
//!
//! Calling `intersect?([x])` builds a temporary single-element array just to
//! check membership; `include?(x)` expresses the same intent more directly and
//! avoids the allocation.
//!
//! ## Autocorrect
//!
//! Two surgical edits (per `.claude/rules/autocorrect-pattern.md`):
//! 1. Rename the selector from `intersect?` to `include?`.
//! 2. Replace the single-element array argument with just the element itself.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "Use `include?(element)` instead of `intersect?([element])`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct ArrayIntersectWithSingleElement;

#[cop(
    name = "Style/ArrayIntersectWithSingleElement",
    description = "Use `include?(element)` instead of `intersect?([element])`.",
    default_severity = "warning",
    default_enabled = true,
    safe = false,
    options = NoOptions,
)]
impl ArrayIntersectWithSingleElement {
    #[on_node(kind = "send", methods = ["intersect?"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Csend { method, .. } = *cx.kind(node) else {
            return;
        };
        if cx.symbol_str(method) == "intersect?" {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Extract the argument list from either Send or Csend.
    let args = match *cx.kind(node) {
        NodeKind::Send { args, .. } | NodeKind::Csend { args, .. } => args,
        _ => return,
    };

    let args_list = cx.list(args);

    // Must have exactly one argument.
    if args_list.len() != 1 {
        return;
    }

    let arg = args_list[0];

    // That argument must be an Array literal.
    let NodeKind::Array(elem_list) = *cx.kind(arg) else {
        return;
    };

    let elems = cx.list(elem_list);

    // The array must have exactly one element.
    if elems.len() != 1 {
        return;
    }

    let elem = elems[0];

    // Offense range: from selector start to end of the call node.
    let selector = cx.selector(node);
    let offense_range = Range {
        start: selector.start,
        end: cx.range(node).end,
    };

    cx.emit_offense(offense_range, MSG, None);

    // Autocorrect -- two surgical edits:

    // Edit 1: rename the selector from `intersect?` to `include?`.
    cx.emit_edit(selector, "include?");

    // Edit 2: replace the array argument with the unwrapped element.
    let array_src = cx.raw_source(cx.range(arg));
    if array_src.starts_with('%') {
        // Percent literal: `%i[sym]` or `%w[str]`.
        // The element AST node carries the interpreted value; raw_source gives
        // the bare word inside the percent literal (no `:` or quotes).
        match *cx.kind(elem) {
            NodeKind::Sym(sym) => {
                let name = cx.symbol_str(sym);
                // Only emit the edit when the symbol name forms a valid simple
                // Ruby symbol literal (word chars, `?`, `!`, no spaces or
                // hyphens). For complex names, skip the edit.
                if is_simple_symbol_name(name) {
                    cx.emit_edit(cx.range(arg), &format!(":{name}"));
                }
            }
            NodeKind::Str(id) => {
                let val = cx.string_str(id);
                // Only emit when the string value needs no escaping (no
                // double-quotes, backslashes, or control characters).
                if is_simple_string_value(val) {
                    cx.emit_edit(cx.range(arg), &format!("\"{val}\""));
                }
            }
            _ => {
                // Unknown percent-literal element type; skip the edit.
            }
        }
    } else {
        // Regular array literal `[element]` -- use the element's raw source.
        let replacement = cx.raw_source(cx.range(elem)).to_owned();
        cx.emit_edit(cx.range(arg), &replacement);
    }
}

/// Returns `true` when `name` can be written as a plain `:name` symbol literal
/// without quoting -- i.e. it matches `/\A[a-zA-Z_][a-zA-Z0-9_]*[?!]?\z/`.
fn is_simple_symbol_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bytes = name.as_bytes();
    // Allow simple identifier: starts with letter or `_`, rest is word chars
    // optionally ending in `?` or `!`.
    let base = if bytes.last().copied() == Some(b'?') || bytes.last().copied() == Some(b'!') {
        &bytes[..bytes.len() - 1]
    } else {
        bytes
    };
    if base.is_empty() {
        return false;
    }
    let first = base[0];
    if !first.is_ascii_alphabetic() && first != b'_' {
        return false;
    }
    base[1..].iter().all(|&b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Returns `true` when `val` can be embedded in a double-quoted string without
/// any escape sequences -- i.e. it contains no `"`, `\`, or control characters.
fn is_simple_string_value(val: &str) -> bool {
    val.bytes()
        .all(|b| b >= 0x20 && b != b'"' && b != b'\\')
}

#[cfg(test)]
mod tests {
    use super::ArrayIntersectWithSingleElement;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No offense: good usage ---

    #[test]
    fn no_offense_for_include() {
        test::<ArrayIntersectWithSingleElement>().expect_no_offenses("array.include?(element)\n");
    }

    // --- Offense + autocorrect: regular array literal ---

    #[test]
    fn flags_and_corrects_single_element_array() {
        test::<ArrayIntersectWithSingleElement>().expect_correction(
            indoc! {r#"
                array.intersect?([element])
                      ^^^^^^^^^^^^^^^^^^^^^ Use `include?(element)` instead of `intersect?([element])`.
            "#},
            "array.include?(element)\n",
        );
    }

    // --- Offense + autocorrect: percent symbol array ---

    #[test]
    fn flags_and_corrects_percent_i_literal() {
        test::<ArrayIntersectWithSingleElement>().expect_correction(
            indoc! {r#"
                array.intersect?(%i[element])
                      ^^^^^^^^^^^^^^^^^^^^^^^ Use `include?(element)` instead of `intersect?([element])`.
            "#},
            "array.include?(:element)\n",
        );
    }

    // --- No offense: multi-element arrays ---

    #[test]
    fn no_offense_for_two_element_array() {
        test::<ArrayIntersectWithSingleElement>()
            .expect_no_offenses("array.intersect?([a, b])\n");
    }

    #[test]
    fn no_offense_for_empty_array() {
        test::<ArrayIntersectWithSingleElement>()
            .expect_no_offenses("array.intersect?([])\n");
    }

    // --- No offense: non-array argument ---

    #[test]
    fn no_offense_for_non_array_arg() {
        test::<ArrayIntersectWithSingleElement>()
            .expect_no_offenses("array.intersect?(other)\n");
    }

    // --- No offense: two arguments ---

    #[test]
    fn no_offense_for_two_args() {
        test::<ArrayIntersectWithSingleElement>()
            .expect_no_offenses("array.intersect?([a], [b])\n");
    }

    // --- csend ---

    #[test]
    fn flags_csend_single_element_array() {
        test::<ArrayIntersectWithSingleElement>().expect_correction(
            indoc! {r#"
                array&.intersect?([element])
                       ^^^^^^^^^^^^^^^^^^^^^ Use `include?(element)` instead of `intersect?([element])`.
            "#},
            "array&.include?(element)\n",
        );
    }

    // --- Helper unit tests ---

    #[test]
    fn simple_symbol_name_helpers() {
        use super::is_simple_symbol_name;
        assert!(is_simple_symbol_name("foo"));
        assert!(is_simple_symbol_name("foo?"));
        assert!(is_simple_symbol_name("foo!"));
        assert!(is_simple_symbol_name("_bar"));
        assert!(!is_simple_symbol_name("foo-bar"));
        assert!(!is_simple_symbol_name("foo bar"));
        assert!(!is_simple_symbol_name(""));
    }
}

murphy_plugin_api::submit_cop!(ArrayIntersectWithSingleElement);
