//! `Style/Attr` — checks for uses of `Module#attr`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Attr
//! upstream_version_checked: 1.86.2
//! version_added: "0.9"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full parity with RuboCop 1.86.2.
//!   allowed_context? logic: finds nearest Class or Block ancestor; fires unless
//!   that ancestor is a non-class_eval/module_eval block, or contains a `def attr`
//!   descendant. Top-level and module-body attr calls are flagged (no Class/Block
//!   ancestor found).
//!   Offense range: the selector (loc.name in Murphy), matching RuboCop's
//!   add_offense(node.loc.selector, ...).
//!   Autocorrect: two surgical edits — (1) rename selector to replacement method,
//!   (2) remove `, <boolean>` trailing arg when the second argument is a boolean.
//!   Edge: attr :foo, :bar, true — last arg is boolean but second arg is not,
//!   so no removal occurs (matches RuboCop's setter = args[1] + boolean_type? check).
//!   Numblock/Itblock as nearest block ancestor: RuboCop's :block pattern does not
//!   match numblock/itblock; this edge case is cosmetically absent in Murphy too.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! attr :something, true   # → attr_accessor :something
//! attr :one, :two, :three # → attr_reader :one, :two, :three
//!
//! # good
//! attr_accessor :something
//! attr_reader :one, :two, :three
//! ```
//!
//! ## Why this shape
//!
//! `Module#attr` with a single argument acts like `attr_reader`, but with a second
//! boolean argument it created an accessor (deprecated in Ruby 1.9). The intent is
//! ambiguous, so the cop requires the explicit `attr_reader` / `attr_accessor` form.
//!
//! The cop fires only when `attr` is called as a command (no explicit receiver) with
//! at least one argument, and only in class bodies or `class_eval`/`module_eval`
//! blocks — the contexts where `attr` actually behaves as a method definition.
//!
//! ## Autocorrect
//!
//! Two surgical `emit_edit` calls:
//! 1. Rename the `attr` selector to `attr_reader` or `attr_accessor`.
//! 2. When the second argument is a boolean literal, remove `, <boolean>` from the
//!    argument list (i.e. from the end of the first argument to the end of the call).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct Attr;

const MSG: &str = "Do not use `attr`. Use `%s` instead.";

#[cop(
    name = "Style/Attr",
    description = "Checks for uses of Module#attr.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl Attr {
    #[on_node(kind = "send", methods = ["attr"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Must be a command call (no receiver) with at least one argument.
        if !cx.is_command(node, "attr") {
            return;
        }
        if !cx.has_call_arguments(node) {
            return;
        }

        // Check context: only fire in class bodies or class_eval/module_eval blocks.
        if is_allowed_context(node, cx) {
            return;
        }

        let replacement = replacement_method(node, cx);
        let msg = MSG.replacen("%s", replacement, 1);
        let selector = cx.selector(node);
        cx.emit_offense(selector, &msg, None);

        // Autocorrect edit 1: rename selector.
        cx.emit_edit(selector, replacement);

        // Autocorrect edit 2: remove `, <boolean>` when second argument is a boolean.
        let NodeKind::Send { args, .. } = *cx.kind(node) else {
            return;
        };
        let arg_list = cx.list(args);
        if arg_list.len() >= 2 {
            let second_arg = arg_list[1];
            if matches!(cx.kind(second_arg), NodeKind::True_ | NodeKind::False_) {
                // Remove from end of first arg to end of whole call node.
                let remove_start = cx.range(arg_list[0]).end;
                let remove_end = cx.range(node).end;
                cx.emit_edit(
                    Range {
                        start: remove_start,
                        end: remove_end,
                    },
                    "",
                );
            }
        }
    }
}

/// Returns `true` if the `attr` call is in a context where it should NOT be flagged.
///
/// Mirrors RuboCop's `allowed_context?`:
/// - Find the nearest ancestor that is a `Class` or `Block`.
/// - If no such ancestor: top-level or module body — FLAG (return false).
/// - If nearest is a `Block`: only flag when its call is `class_eval` or `module_eval`
///   AND the block does not contain a `def attr` descendant.
/// - If nearest is a `Class`: flag unless the class contains a `def attr` descendant.
fn is_allowed_context(node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        match cx.kind(ancestor) {
            NodeKind::Class { .. } => {
                // In a class body: flag unless this class defines its own `attr` method.
                return defines_attr_method(ancestor, cx);
            }
            NodeKind::Block { call, .. } => {
                // In a block: only flag if it's a class_eval/module_eval block.
                if !is_class_eval_block(*call, cx) {
                    // Not a class_eval/module_eval block — do not flag.
                    return true;
                }
                // class_eval/module_eval block: flag unless it defines its own `attr`.
                return defines_attr_method(ancestor, cx);
            }
            _ => {
                // Continue searching ancestors (skip Sclass, Module, Def, etc.).
            }
        }
    }
    // No Class or Block ancestor found (top-level, module body, sclass) → flag.
    false
}

/// Returns `true` if the container node has any descendant `def attr` (instance method).
///
/// Mirrors RuboCop's `define_attr_method?`.
fn defines_attr_method(container: NodeId, cx: &Cx<'_>) -> bool {
    cx.descendants(container).iter().any(|&d| {
        if let NodeKind::Def { name, receiver, .. } = cx.kind(d) {
            receiver.get().is_none() && cx.symbol_str(*name) == "attr"
        } else {
            false
        }
    })
}

/// Returns `true` if `call` is a `class_eval` or `module_eval` send.
///
/// Mirrors RuboCop's `class_eval?` node matcher:
/// `(block (send _ {:class_eval :module_eval}) ...)`
fn is_class_eval_block(call: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.method_name(call),
        Some("class_eval") | Some("module_eval")
    )
}

/// Determine the replacement method name.
///
/// Mirrors RuboCop's `replacement_method`:
/// - Last argument is `true`  → `attr_accessor`
/// - Last argument is `false` or non-boolean → `attr_reader`
fn replacement_method<'cx>(node: NodeId, cx: &Cx<'cx>) -> &'cx str {
    let NodeKind::Send { args, .. } = *cx.kind(node) else {
        return "attr_reader";
    };
    let arg_list = cx.list(args);
    if arg_list.last().is_some_and(|&last| matches!(cx.kind(last), NodeKind::True_)) {
        return "attr_accessor";
    }
    "attr_reader"
}

#[cfg(test)]
mod tests {
    use super::Attr;
    use murphy_plugin_api::test_support::{indoc, test};

    // ── Positive: flag `attr :foo, true` (accessor) ──────────────────────────

    #[test]
    fn flags_attr_with_true_in_class() {
        test::<Attr>().expect_offense(indoc! {r#"
            class Foo
              attr :something, true
              ^^^^ Do not use `attr`. Use `attr_accessor` instead.
            end
        "#});
    }

    // ── Positive: flag multi-symbol `attr` (reader) ──────────────────────────

    #[test]
    fn flags_attr_multi_symbol_in_class() {
        test::<Attr>().expect_offense(indoc! {r#"
            class Foo
              attr :one, :two, :three
              ^^^^ Do not use `attr`. Use `attr_reader` instead.
            end
        "#});
    }

    // ── Positive: flag `attr :foo, false` (reader) ───────────────────────────

    #[test]
    fn flags_attr_with_false_in_class() {
        test::<Attr>().expect_offense(indoc! {r#"
            class Foo
              attr :something, false
              ^^^^ Do not use `attr`. Use `attr_reader` instead.
            end
        "#});
    }

    // ── Positive: top-level `attr` is flagged ────────────────────────────────

    #[test]
    fn flags_attr_at_top_level() {
        test::<Attr>().expect_offense(indoc! {r#"
            attr :something, true
            ^^^^ Do not use `attr`. Use `attr_accessor` instead.
        "#});
    }

    // ── Positive: module body `attr` is flagged ──────────────────────────────

    #[test]
    fn flags_attr_in_module_body() {
        test::<Attr>().expect_offense(indoc! {r#"
            module Foo
              attr :something, true
              ^^^^ Do not use `attr`. Use `attr_accessor` instead.
            end
        "#});
    }

    // ── Positive: class_eval block ───────────────────────────────────────────

    #[test]
    fn flags_attr_in_class_eval_block() {
        test::<Attr>().expect_offense(indoc! {r#"
            Foo.class_eval do
              attr :bar, true
              ^^^^ Do not use `attr`. Use `attr_accessor` instead.
            end
        "#});
    }

    // ── Positive: module_eval block ──────────────────────────────────────────

    #[test]
    fn flags_attr_in_module_eval_block() {
        test::<Attr>().expect_offense(indoc! {r#"
            Foo.module_eval do
              attr :bar
              ^^^^ Do not use `attr`. Use `attr_reader` instead.
            end
        "#});
    }

    // ── Negative: no arguments ───────────────────────────────────────────────

    #[test]
    fn no_offense_attr_no_args() {
        test::<Attr>().expect_no_offenses(indoc! {r#"
            class Foo
              attr
            end
        "#});
    }

    // ── Negative: has receiver (not a command) ───────────────────────────────

    #[test]
    fn no_offense_attr_with_receiver() {
        test::<Attr>().expect_no_offenses(indoc! {r#"
            class Foo
              foo.attr(:something, true)
            end
        "#});
    }

    // ── Negative: inside regular block (not class_eval) ──────────────────────

    #[test]
    fn no_offense_attr_inside_regular_block() {
        test::<Attr>().expect_no_offenses(indoc! {r#"
            class Foo
              [1, 2].each do
                attr :something, true
              end
            end
        "#});
    }

    // ── Negative: class defines its own `def attr` ───────────────────────────

    #[test]
    fn no_offense_when_class_defines_attr_method() {
        test::<Attr>().expect_no_offenses(indoc! {r#"
            class Foo
              def attr(name)
                # custom attr
              end
              attr :something, true
            end
        "#});
    }

    // ── Autocorrect: `attr :foo, true` → `attr_accessor :foo` ───────────────

    #[test]
    fn corrects_attr_true_to_accessor() {
        test::<Attr>().expect_correction(
            indoc! {r#"
                class Foo
                  attr :something, true
                  ^^^^ Do not use `attr`. Use `attr_accessor` instead.
                end
            "#},
            indoc! {r#"
                class Foo
                  attr_accessor :something
                end
            "#},
        );
    }

    // ── Autocorrect: `attr :foo, false` → `attr_reader :foo` ────────────────

    #[test]
    fn corrects_attr_false_to_reader() {
        test::<Attr>().expect_correction(
            indoc! {r#"
                class Foo
                  attr :something, false
                  ^^^^ Do not use `attr`. Use `attr_reader` instead.
                end
            "#},
            indoc! {r#"
                class Foo
                  attr_reader :something
                end
            "#},
        );
    }

    // ── Autocorrect: `attr :one, :two` → `attr_reader :one, :two` ───────────

    #[test]
    fn corrects_attr_multi_symbol_to_reader() {
        test::<Attr>().expect_correction(
            indoc! {r#"
                class Foo
                  attr :one, :two, :three
                  ^^^^ Do not use `attr`. Use `attr_reader` instead.
                end
            "#},
            indoc! {r#"
                class Foo
                  attr_reader :one, :two, :three
                end
            "#},
        );
    }
}

murphy_plugin_api::submit_cop!(Attr);
