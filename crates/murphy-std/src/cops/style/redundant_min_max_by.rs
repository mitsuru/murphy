//! `Style/RedundantMinMaxBy` — identifies places where `max_by`/`min_by`/
//! `minmax_by` with an identity block can be replaced by `max`/`min`/`minmax`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantMinMaxBy
//! upstream_version_checked: 1.87.0
//! version_added: "1.85"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags identity blocks on `max_by`/`min_by`/`minmax_by`:
//!     - block form `{ |x| x }` (and `do |x| x end`) where the sole body
//!       expression is the lvar of the single block arg
//!     - numblock form `{ _1 }`
//!     - itblock form `{ it }`
//!   Autocorrect replaces the selector-through-block-end range with the
//!   `_by`-stripped method name (`max_by` -> `max`, etc.).
//!
//!   Receiver is optional (mirrors RuboCop's `(call _ {...})` pattern):
//!   bare `max_by { |x| x }` on an implicit `self` receiver is flagged.
//!   Safe-navigation `array&.max_by { |x| x }` (csend) is flagged.
//!
//!   Parser note: under MRI < 3.4 the `it` block parameter is parsed as a
//!   method call, so standalone RuboCop on such a Ruby never reaches its
//!   `on_itblock` handler. Murphy's prism-based parser produces `Itblock`
//!   nodes regardless, so Murphy flags `{ it }` per the upstream cop's
//!   intent. This is a parser-version artifact, not a behavioral divergence.
//!
//!   Match precision (mirrors the NodePattern exactly):
//!     - block args list length must be exactly 1
//!     - body must be a bare `Lvar` whose name equals the arg name
//!     - `{ |x| x.foo }`, `{ |x, y| x }`, `{ |x| (x) }` (parenthesized body)
//!       are NOT matched
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! array.max_by { |x| x }
//! array.min_by { |x| x }
//! array.minmax_by { |x| x }
//! array.max_by { _1 }
//! array.min_by { it }
//!
//! # good
//! array.max
//! array.min
//! array.minmax
//! ```
//!
//! ## Why this shape
//!
//! We dispatch on the `Send`/`Csend` (the `max_by`/`min_by`/`minmax_by` call
//! node) and inspect the enclosing block node (its parent). The offense range
//! runs from the selector start through the end of the block, matching
//! RuboCop's `range_between(send.loc.selector.begin_pos, node.loc.end.end_pos)`.
//!
//! ## Autocorrect
//!
//! Replaces the entire offense range with the `_by`-stripped method name.
//! Whole-range replacement is appropriate because the identity block is
//! collapsed away entirely.

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantMinMaxBy;

#[cop(
    name = "Style/RedundantMinMaxBy",
    description = "Identifies places where `max_by`/`min_by` can be replaced by `max`/`min`.",
    default_severity = "warning",
    default_enabled = false,
    safe_autocorrect = true,
)]
impl RedundantMinMaxBy {
    /// Triggered on max_by/min_by/minmax_by sends.
    #[on_node(kind = "send", methods = ["max_by", "min_by", "minmax_by"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    /// Also handle csend (safe-navigation): `array&.max_by { |x| x }`.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if matches!(cx.method_name(node), Some("max_by" | "min_by" | "minmax_by")) {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let Some(method_name) = cx.method_name(node) else {
        return;
    };
    let Some(replacement) = replacement_for(method_name) else {
        return;
    };

    // RuboCop's `(call _ {...})` pattern has no arg matcher, so it matches only
    // zero-argument calls. `max_by(2) { |x| x }` (return the top 2 elements) is
    // valid and must NOT be flagged — and crucially must NOT be autocorrected,
    // since collapsing to `max` would silently drop the `(2)` and change the
    // semantics.
    if !cx.call_arguments(node).is_empty() {
        return;
    }

    // The send's parent must be the enclosing block, and the block must be an
    // identity block over the appropriate implicit/explicit parameter.
    let Some(parent) = cx.parent(node).get() else {
        return;
    };

    let message = match *cx.kind(parent) {
        NodeKind::Block { call, .. } if call == node => {
            let Some(var) = identity_block_var(parent, cx) else {
                return;
            };
            format!("Use `{replacement}` instead of `{method_name} {{ |{var}| {var} }}`.")
        }
        NodeKind::Numblock { send, max_n, .. } if send == node => {
            if max_n != 1 || !numblock_body_is(parent, "_1", cx) {
                return;
            }
            format!("Use `{replacement}` instead of `{method_name} {{ _1 }}`.")
        }
        NodeKind::Itblock { send, .. } if send == node => {
            if !itblock_body_is(parent, "it", cx) {
                return;
            }
            format!("Use `{replacement}` instead of `{method_name} {{ it }}`.")
        }
        _ => return,
    };

    let offense_range = Range {
        start: cx.selector(node).start,
        end: cx.range(parent).end,
    };

    cx.emit_offense(offense_range, &message, None);
    cx.emit_edit(offense_range, replacement);
}

/// Returns the `_by`-stripped method name when `method_name` is one of the
/// three redundant-by methods, otherwise `None`.
fn replacement_for(method_name: &str) -> Option<&'static str> {
    match method_name {
        "max_by" => Some("max"),
        "min_by" => Some("min"),
        "minmax_by" => Some("minmax"),
        _ => None,
    }
}

/// For a `Block` node, returns the block-arg name when the block is an identity
/// block — exactly one positional `Arg`, body is a bare `Lvar` with the same
/// name. Returns `None` otherwise.
fn identity_block_var<'a>(block_node: NodeId, cx: &'a Cx<'_>) -> Option<&'a str> {
    let NodeKind::Block { args, body, .. } = *cx.kind(block_node) else {
        return None;
    };
    let body_node = body.get()?;

    // args must be exactly one positional Arg.
    let NodeKind::Args(arg_list) = *cx.kind(args) else {
        return None;
    };
    let args = cx.list(arg_list);
    let [single] = args else {
        return None;
    };
    let NodeKind::Arg(arg_sym) = *cx.kind(*single) else {
        return None;
    };
    let arg_name = cx.symbol_str(arg_sym);

    // body must be exactly `Lvar(arg_name)` (no parenthesization unwrapping).
    if lvar_name(body_node, cx) == Some(arg_name) {
        Some(arg_name)
    } else {
        None
    }
}

/// Returns `true` when the `Numblock` body is exactly `Lvar(lvar_name)`.
fn numblock_body_is(block_node: NodeId, lvar_name_str: &str, cx: &Cx<'_>) -> bool {
    let NodeKind::Numblock { body, .. } = *cx.kind(block_node) else {
        return false;
    };
    body.get().is_some_and(|b| lvar_name(b, cx) == Some(lvar_name_str))
}

/// Returns `true` when the `Itblock` body is exactly `Lvar(lvar_name)`.
fn itblock_body_is(block_node: NodeId, lvar_name_str: &str, cx: &Cx<'_>) -> bool {
    let NodeKind::Itblock { body, .. } = *cx.kind(block_node) else {
        return false;
    };
    body.get().is_some_and(|b| lvar_name(b, cx) == Some(lvar_name_str))
}

/// Returns the lvar name when `node` is a bare `Lvar`, otherwise `None`.
fn lvar_name<'a>(node: NodeId, cx: &'a Cx<'_>) -> Option<&'a str> {
    match *cx.kind(node) {
        NodeKind::Lvar(sym) => Some(cx.symbol_str(sym)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::RedundantMinMaxBy;
    use murphy_plugin_api::test_support::{indoc, test};

    // block form: max_by/min_by/minmax_by { |x| x }

    #[test]
    fn flags_max_by_block() {
        test::<RedundantMinMaxBy>().expect_offense(indoc! {r#"
            array.max_by { |x| x }
                  ^^^^^^^^^^^^^^^^ Use `max` instead of `max_by { |x| x }`.
        "#});
    }

    #[test]
    fn corrects_max_by_block() {
        test::<RedundantMinMaxBy>().expect_correction(
            indoc! {r#"
                array.max_by { |x| x }
                      ^^^^^^^^^^^^^^^^ Use `max` instead of `max_by { |x| x }`.
            "#},
            "array.max\n",
        );
    }

    #[test]
    fn flags_min_by_block() {
        test::<RedundantMinMaxBy>().expect_offense(indoc! {r#"
            array.min_by { |x| x }
                  ^^^^^^^^^^^^^^^^ Use `min` instead of `min_by { |x| x }`.
        "#});
    }

    #[test]
    fn flags_minmax_by_block() {
        test::<RedundantMinMaxBy>().expect_offense(indoc! {r#"
            array.minmax_by { |x| x }
                  ^^^^^^^^^^^^^^^^^^^ Use `minmax` instead of `minmax_by { |x| x }`.
        "#});
    }

    // arbitrary var name is preserved in the message

    #[test]
    fn flags_block_with_custom_var_name() {
        test::<RedundantMinMaxBy>().expect_offense(indoc! {r#"
            array.min_by { |item| item }
                  ^^^^^^^^^^^^^^^^^^^^^^ Use `min` instead of `min_by { |item| item }`.
        "#});
    }

    // do...end block on a single line: keeps the whole offense range on one
    // line so strict carets apply. A multi-line do-end offense spans the
    // `end`, which the caret-based harness cannot annotate; that shape is
    // covered by the CLI firing check instead. The block-form logic is
    // identical (same `Block` AST node) regardless of delimiter.

    #[test]
    fn flags_single_line_do_end_block() {
        test::<RedundantMinMaxBy>().expect_offense(indoc! {r#"
            array.max_by do |x| x end
                  ^^^^^^^^^^^^^^^^^^^ Use `max` instead of `max_by { |x| x }`.
        "#});
    }

    #[test]
    fn corrects_single_line_do_end_block() {
        test::<RedundantMinMaxBy>().expect_correction(
            indoc! {r#"
                array.max_by do |x| x end
                      ^^^^^^^^^^^^^^^^^^^ Use `max` instead of `max_by { |x| x }`.
            "#},
            "array.max\n",
        );
    }

    // numblock: max_by { _1 }

    #[test]
    fn flags_numblock() {
        test::<RedundantMinMaxBy>().expect_offense(indoc! {r#"
            array.max_by { _1 }
                  ^^^^^^^^^^^^^ Use `max` instead of `max_by { _1 }`.
        "#});
    }

    #[test]
    fn corrects_numblock() {
        test::<RedundantMinMaxBy>().expect_correction(
            indoc! {r#"
                array.max_by { _1 }
                      ^^^^^^^^^^^^^ Use `max` instead of `max_by { _1 }`.
            "#},
            "array.max\n",
        );
    }

    // itblock: min_by { it }

    #[test]
    fn flags_itblock() {
        test::<RedundantMinMaxBy>().expect_offense(indoc! {r#"
            array.min_by { it }
                  ^^^^^^^^^^^^^ Use `min` instead of `min_by { it }`.
        "#});
    }

    #[test]
    fn corrects_itblock() {
        test::<RedundantMinMaxBy>().expect_correction(
            indoc! {r#"
                array.min_by { it }
                      ^^^^^^^^^^^^^ Use `min` instead of `min_by { it }`.
            "#},
            "array.min\n",
        );
    }

    // bare receiver (implicit self): flagged, no receiver guard

    #[test]
    fn flags_bare_receiver() {
        test::<RedundantMinMaxBy>().expect_offense(indoc! {r#"
            max_by { |x| x }
            ^^^^^^^^^^^^^^^^ Use `max` instead of `max_by { |x| x }`.
        "#});
    }

    // safe-navigation (csend): flagged

    #[test]
    fn flags_csend() {
        test::<RedundantMinMaxBy>().expect_offense(indoc! {r#"
            array&.max_by { |x| x }
                   ^^^^^^^^^^^^^^^^ Use `max` instead of `max_by { |x| x }`.
        "#});
    }

    #[test]
    fn corrects_csend() {
        test::<RedundantMinMaxBy>().expect_correction(
            indoc! {r#"
                array&.max_by { |x| x }
                       ^^^^^^^^^^^^^^^^ Use `max` instead of `max_by { |x| x }`.
            "#},
            "array&.max\n",
        );
    }

    // negative: body references a different name than the block arg

    #[test]
    fn accepts_block_with_method_call_body() {
        test::<RedundantMinMaxBy>().expect_no_offenses("array.max_by { |x| x.foo }\n");
    }

    // negative: multi-param block

    #[test]
    fn accepts_multi_param_block() {
        test::<RedundantMinMaxBy>().expect_no_offenses("array.max_by { |x, y| x }\n");
    }

    // negative: parenthesized body is not a bare lvar

    #[test]
    fn accepts_parenthesized_body() {
        test::<RedundantMinMaxBy>().expect_no_offenses("array.max_by { |x| (x) }\n");
    }

    // negative: body lvar name differs from the arg name

    #[test]
    fn accepts_block_with_wrong_lvar() {
        test::<RedundantMinMaxBy>().expect_no_offenses("array.max_by { |x| y }\n");
    }

    // negative: empty block

    #[test]
    fn accepts_empty_block() {
        test::<RedundantMinMaxBy>().expect_no_offenses("array.max_by { }\n");
    }

    // negative: non-by method

    #[test]
    fn accepts_plain_max() {
        test::<RedundantMinMaxBy>().expect_no_offenses("array.max\n");
    }

    // negative: block does more than return the element

    #[test]
    fn accepts_block_with_extra_statement() {
        test::<RedundantMinMaxBy>().expect_no_offenses("array.max_by { |x| puts x; x }\n");
    }

    // negative: numblock referencing _2 (more than one param)

    #[test]
    fn accepts_numblock_with_arithmetic() {
        test::<RedundantMinMaxBy>().expect_no_offenses("array.max_by { _1 + 1 }\n");
    }

    // negative: integer arg `max_by(2)` returns the top N — not redundant.
    // `(call _ {...})` matches only zero-arg calls in RuboCop.

    #[test]
    fn accepts_max_by_with_integer_arg_block() {
        test::<RedundantMinMaxBy>().expect_no_offenses("array.max_by(2) { |x| x }\n");
    }

    #[test]
    fn accepts_max_by_with_integer_arg_numblock() {
        test::<RedundantMinMaxBy>().expect_no_offenses("array.max_by(2) { _1 }\n");
    }
}

murphy_plugin_api::submit_cop!(RedundantMinMaxBy);
