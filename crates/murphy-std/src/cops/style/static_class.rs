//! `Style/StaticClass` — prefer modules to classes with only class methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/StaticClass
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection covers all four allowed body-element kinds: public `def self.x`,
//!   `class << self` containing public defs and assignments, constant/variable
//!   assignments, and `extend` calls. Autocorrect rewrites `def self.x` form and
//!   pure-assignment/extend classes to modules; `class << self` autocorrect is
//!   deferred (offense only) because the `sclass` body unwrap produces complex
//!   multi-line edits that risk invalid Ruby in edge cases.
//!   The cop is marked `Enabled: false` and `Safe: false` in the default config.
//!   Offense range covers `class Name` (keyword + name) rather than the whole
//!   class body, matching RuboCop's behavior of highlighting the declaration line.
//! ```
//!
//! ## Matched shapes
//!
//! `class` nodes (no superclass) whose body consists entirely of:
//!
//! - `def self.method_name` — public class method definitions
//! - `class << self` containing only public `def` and assignment nodes
//! - Constant/variable assignment nodes (`X = 1`, `@x = 1`, etc.)
//! - `extend SomeMod` calls
//!
//! ## Not matched
//!
//! - Empty class bodies
//! - Classes with superclass
//! - Classes containing any instance method definitions (`def foo`)
//! - Classes with `private`, `protected`, `private_class_method`, or other
//!   visibility-changing calls (these are `Send` nodes not matching the
//!   allowed set, so they block conversion)
//!
//! ## Autocorrect
//!
//! For classes without any `class << self` bodies:
//! 1. Replace `class` keyword with `module`
//! 2. Insert `\nmodule_function` after the class name
//! 3. For each `def self.method_name`: delete the `self.` prefix

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct StaticClass;

const MSG: &str = "Prefer modules to classes with only class methods.";

#[cop(
    name = "Style/StaticClass",
    description = "Prefer modules to classes with only class methods.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl StaticClass {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class {
            name,
            superclass,
            body,
        } = *cx.kind(node)
        else {
            return;
        };

        // Skip if class has a superclass.
        if superclass.get().is_some() {
            return;
        }

        // Skip empty class bodies.
        if body.get().is_none() {
            return;
        }

        // Skip if body is empty Begin.
        let body_is_empty = matches!(
            body.get().map(|id| cx.kind(id)),
            Some(NodeKind::Begin(list)) if cx.list(*list).is_empty()
        );
        if body_is_empty {
            return;
        }

        // Check if all elements can be converted to module members; at least one
        // must be present (non-empty body).
        let mut has_elements = false;
        let all_convertible = all_elements(body, cx, |el| {
            has_elements = true;
            is_convertible(el, cx)
        });
        if !has_elements || !all_convertible {
            return;
        }

        // Offense: flag `class Name` (keyword through end of name) — the declaration
        // line only, matching RuboCop's practice of highlighting the class keyword.
        let node_start = cx.range(node).start;
        let name_end = cx.range(name).end;
        let offense_range = Range {
            start: node_start,
            end: name_end,
        };
        cx.emit_offense(offense_range, MSG, None);

        // Autocorrect only if no sclass elements are present (avoid invalid Ruby).
        if any_element(body, cx, |el| matches!(cx.kind(el), NodeKind::Sclass { .. })) {
            return;
        }

        // 1. Replace `class` keyword with `module`.
        let keyword_range = cx.loc(node).keyword();
        if keyword_range == Range::ZERO {
            return;
        }
        cx.emit_edit(keyword_range, "module");

        // 2. Insert `\nmodule_function` after the class name.
        let name_end_range = Range {
            start: name_end,
            end: name_end,
        };
        cx.emit_edit(name_end_range, "\nmodule_function");

        // 3. For each `def self.method_name`: delete the `self.` prefix.
        for_each_element(body, cx, |el| {
            if let NodeKind::Def { receiver, .. } = *cx.kind(el)
                && let Some(recv_id) = receiver.get()
                    && matches!(cx.kind(recv_id), NodeKind::SelfExpr) {
                        // Delete from start of `self` through end of `.` dot.
                        // Use token_after(recv.end) to find the `.` dot token,
                        // then delete up to the end of the dot.
                        let recv_start = cx.range(recv_id).start;
                        let recv_end = cx.range(recv_id).end;
                        let method_name_start = cx
                            .token_after(recv_end)
                            .map(|dot| dot.range.end)
                            .unwrap_or(recv_end + 1);
                        let self_dot_range = Range {
                            start: recv_start,
                            end: method_name_start,
                        };
                        cx.emit_edit(self_dot_range, "");
                    }
        });
    }
}

// --------------------------------------------------------------------------
// Element iteration helpers — avoid heap-allocating a Vec per class node.
// --------------------------------------------------------------------------

/// Iterate over the direct body elements of a class/sclass body, calling `f`
/// for each. Handles the `Begin` wrapper transparently.
fn for_each_element<F>(body: OptNodeId, cx: &Cx<'_>, mut f: F)
where
    F: FnMut(NodeId),
{
    let Some(body_id) = body.get() else {
        return;
    };
    match *cx.kind(body_id) {
        NodeKind::Begin(list) => {
            for &el in cx.list(list) {
                f(el);
            }
        }
        _ => f(body_id),
    }
}

/// Returns `true` if ALL elements pass the predicate (empty body → `false`).
fn all_elements<F>(body: OptNodeId, cx: &Cx<'_>, mut pred: F) -> bool
where
    F: FnMut(NodeId) -> bool,
{
    let Some(body_id) = body.get() else {
        return false;
    };
    match *cx.kind(body_id) {
        NodeKind::Begin(list) => {
            let slice = cx.list(list);
            !slice.is_empty() && slice.iter().all(|&el| pred(el))
        }
        _ => pred(body_id),
    }
}

/// Returns `true` if ANY element passes the predicate.
fn any_element<F>(body: OptNodeId, cx: &Cx<'_>, mut pred: F) -> bool
where
    F: FnMut(NodeId) -> bool,
{
    let Some(body_id) = body.get() else {
        return false;
    };
    match *cx.kind(body_id) {
        NodeKind::Begin(list) => cx.list(list).iter().any(|&el| pred(el)),
        _ => pred(body_id),
    }
}

// --------------------------------------------------------------------------
// Convertibility checks
// --------------------------------------------------------------------------

/// Returns true if this body element is allowed in a convertible static class.
fn is_convertible(el: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(el) {
        // `def self.method_name` — public class method def.
        NodeKind::Def { receiver, .. } => receiver
            .get()
            .is_some_and(|r| matches!(cx.kind(r), NodeKind::SelfExpr)),
        // `class << self` — check that all children are convertible.
        NodeKind::Sclass { body, .. } => sclass_convertible(body, cx),
        // Constant/variable assignments.
        NodeKind::Casgn { .. }
        | NodeKind::Lvasgn { .. }
        | NodeKind::Ivasgn { .. }
        | NodeKind::Gvasgn { .. }
        | NodeKind::Cvasgn { .. } => true,
        // `extend SomeMod` calls (receiverless Send named `extend`).
        NodeKind::Send {
            receiver, method, ..
        } => receiver.get().is_none() && cx.symbol_str(method) == "extend",
        _ => false,
    }
}

/// Returns true if all elements inside a `class << self` body are convertible.
fn sclass_convertible(body: OptNodeId, cx: &Cx<'_>) -> bool {
    all_elements(body, cx, |el| {
        match *cx.kind(el) {
            // Plain `def foo` in sclass context = class method.
            NodeKind::Def { receiver, .. } => receiver.get().is_none(),
            // Assignments are allowed.
            NodeKind::Casgn { .. }
            | NodeKind::Lvasgn { .. }
            | NodeKind::Ivasgn { .. }
            | NodeKind::Gvasgn { .. }
            | NodeKind::Cvasgn { .. } => true,
            _ => false,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::StaticClass;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Flagged cases -----

    #[test]
    fn flags_class_with_only_class_methods() {
        test::<StaticClass>().expect_offense(indoc! {"
            class SomeClass
            ^^^^^^^^^^^^^^^ Prefer modules to classes with only class methods.
              def self.some_method
                # body omitted
              end
              def self.some_other_method
                # body omitted
              end
            end
        "});
    }

    #[test]
    fn corrects_class_with_only_class_methods() {
        test::<StaticClass>().expect_correction(
            indoc! {"
                class SomeClass
                ^^^^^^^^^^^^^^^ Prefer modules to classes with only class methods.
                  def self.some_method
                    # body omitted
                  end
                  def self.some_other_method
                    # body omitted
                  end
                end
            "},
            indoc! {"
                module SomeClass
                module_function
                  def some_method
                    # body omitted
                  end
                  def some_other_method
                    # body omitted
                  end
                end
            "},
        );
    }

    #[test]
    fn flags_class_with_single_class_method() {
        test::<StaticClass>().expect_correction(
            indoc! {"
                class Foo
                ^^^^^^^^^ Prefer modules to classes with only class methods.
                  def self.bar; end
                end
            "},
            indoc! {"
                module Foo
                module_function
                  def bar; end
                end
            "},
        );
    }

    #[test]
    fn flags_class_with_assignment_and_class_method() {
        test::<StaticClass>().expect_correction(
            indoc! {"
                class Foo
                ^^^^^^^^^ Prefer modules to classes with only class methods.
                  X = 1
                  def self.bar; end
                end
            "},
            indoc! {"
                module Foo
                module_function
                  X = 1
                  def bar; end
                end
            "},
        );
    }

    #[test]
    fn flags_class_with_extend_call() {
        test::<StaticClass>().expect_correction(
            indoc! {"
                class Foo
                ^^^^^^^^^ Prefer modules to classes with only class methods.
                  extend SomeModule
                  def self.bar; end
                end
            "},
            indoc! {"
                module Foo
                module_function
                  extend SomeModule
                  def bar; end
                end
            "},
        );
    }

    // sclass: offense only, no autocorrect (complex unwrap deferred)
    #[test]
    fn flags_class_with_sclass_body() {
        test::<StaticClass>().expect_offense(indoc! {"
            class SomeClass
            ^^^^^^^^^^^^^^^ Prefer modules to classes with only class methods.
              class << self
                def some_method; end
              end
            end
        "});
    }

    // ----- No-offense cases -----

    #[test]
    fn accepts_class_with_superclass() {
        test::<StaticClass>().expect_no_offenses(indoc! {"
            class SomeClass < Base
              def self.some_method; end
            end
        "});
    }

    #[test]
    fn accepts_class_with_instance_method() {
        test::<StaticClass>().expect_no_offenses(indoc! {"
            class SomeClass
              def instance_method; end
              def self.class_method; end
            end
        "});
    }

    #[test]
    fn accepts_empty_class() {
        test::<StaticClass>().expect_no_offenses(indoc! {"
            class SomeClass
            end
        "});
    }

    #[test]
    fn accepts_class_with_private_visibility() {
        // `private` is a receiverless Send node not matching allowed set — blocks
        // conversion.
        test::<StaticClass>().expect_no_offenses(indoc! {"
            class SomeClass
              private
              def self.some_method; end
            end
        "});
    }

    #[test]
    fn accepts_class_with_private_class_method() {
        // `private_class_method` is a receiverless Send node not matching allowed set.
        test::<StaticClass>().expect_no_offenses(indoc! {"
            class SomeClass
              private_class_method :some_method
              def self.some_method; end
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(StaticClass);
