//! `Lint/DuplicateSetElement` — flag (and remove) a duplicate element in a
//! `Set` / `SortedSet` construction.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/DuplicateSetElement
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covers the three RuboCop construction shapes: `Set[a, b, a]` /
//!   `SortedSet[…]`, `Set.new([a, b, a])` / `SortedSet.new(…)`, and
//!   `[a, b, a].to_set`. The `{nil? cbase}` const scoping is matched via
//!   `cx.is_global_const`, so `::Set[…]` is also covered. Only elements that
//!   are literals, constants, or variables are compared (matching RuboCop's
//!   `literal? || const_type? || variable?` filter), so elements with possibly
//!   changing return values (method calls) are skipped. Duplicate identity is
//!   compared by trimmed `raw_source` (the `Style/IdenticalConditionalBranches`
//!   precedent), which is whitespace-sensitive — a documented divergence from
//!   RuboCop's structural AST node equality. Autocorrect removes the range from
//!   the immediately-preceding sibling's end to the duplicate's end, matching
//!   RuboCop's `register_offense`.
//! ```

use std::collections::HashSet;

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind, Range};

#[derive(Default)]
pub struct DuplicateSetElement;

#[cop(
    name = "Lint/DuplicateSetElement",
    description = "Checks for duplicate elements in Set and SortedSet.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicateSetElement {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let Some((elements, class_name)) = set_construction(node, cx) else {
            return;
        };
        check_elements(elements, &class_name, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        let Some((elements, class_name)) = set_construction(node, cx) else {
            return;
        };
        check_elements(elements, &class_name, cx);
    }
}

/// Resolve a node to `(set_elements, class_name)` if it is one of the three
/// recognized `Set` / `SortedSet` constructions, else `None`.
fn set_construction<'a>(node: NodeId, cx: &Cx<'a>) -> Option<(&'a [NodeId], String)> {
    let method = cx.method_name(node)?;
    match method {
        // `Set[a, b, a]` / `SortedSet[a, b, a]` — receiver is the const,
        // args are the elements.
        "[]" => {
            let receiver = cx.call_receiver(node).get()?;
            let class_name = set_class_name(receiver, cx)?;
            Some((cx.call_arguments(node), class_name))
        }
        // `Set.new([a, b, a])` / `SortedSet.new([…])` — receiver is the const,
        // the single argument is an array literal of elements.
        "new" => {
            let receiver = cx.call_receiver(node).get()?;
            let class_name = set_class_name(receiver, cx)?;
            let [arg] = cx.call_arguments(node) else {
                return None;
            };
            let NodeKind::Array(list) = *cx.kind(*arg) else {
                return None;
            };
            Some((cx.list(list), class_name))
        }
        // `[a, b, a].to_set` — receiver is the array literal; class name is
        // always `Set` (RuboCop's `node.receiver.const_type? ? … : 'Set'`).
        "to_set" => {
            let receiver = cx.call_receiver(node).get()?;
            let NodeKind::Array(list) = *cx.kind(receiver) else {
                return None;
            };
            Some((cx.list(list), "Set".to_string()))
        }
        _ => None,
    }
}

/// The `const_name` for the receiver of `Set[…]` / `Set.new(…)`, or `None`
/// if the receiver is not a `Set`/`SortedSet` constant.
fn set_class_name(receiver: NodeId, cx: &Cx<'_>) -> Option<String> {
    for name in ["Set", "SortedSet"] {
        if cx.is_global_const(receiver, name) {
            return Some(name.to_string());
        }
    }
    None
}

fn check_elements(elements: &[NodeId], class_name: &str, cx: &Cx<'_>) {
    let mut seen: HashSet<&str> = HashSet::new();
    for (index, &element) in elements.iter().enumerate() {
        // Only compare elements with statically stable values: literals,
        // constants, and variables. Skip everything else (e.g. method calls)
        // to avoid false positives.
        if !cx.is_literal(element)
            && !matches!(cx.kind(element), NodeKind::Const { .. })
            && !cx.is_variable(element)
        {
            continue;
        }
        let src = cx.raw_source(cx.range(element));
        if seen.insert(src) {
            continue;
        }
        // Duplicate. The previous sibling is the element immediately before
        // this one in source order (`set_elements[index - 1]`), not the
        // first-seen match — removing from there deletes only the duplicate
        // and its leading separator.
        let message = format!("Remove the duplicate element in {class_name}.");
        cx.emit_offense(cx.range(element), &message, None);
        if index > 0 {
            let prev = elements[index - 1];
            cx.emit_edit(
                Range {
                    start: cx.range(prev).end,
                    end: cx.range(element).end,
                },
                "",
            );
        }
    }
}

murphy_plugin_api::submit_cop!(DuplicateSetElement);

#[cfg(test)]
mod tests {
    use super::DuplicateSetElement;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_removes_duplicate_in_bracket_set() {
        test::<DuplicateSetElement>().expect_correction(
            indoc! {r#"
                Set[:foo, :bar, :foo]
                                ^^^^ Remove the duplicate element in Set.
            "#},
            "Set[:foo, :bar]\n",
        );
    }

    #[test]
    fn flags_and_removes_duplicate_in_sorted_set() {
        test::<DuplicateSetElement>().expect_correction(
            indoc! {r#"
                SortedSet[1, 2, 1]
                                ^ Remove the duplicate element in SortedSet.
            "#},
            "SortedSet[1, 2]\n",
        );
    }

    #[test]
    fn flags_and_removes_duplicate_in_set_new() {
        test::<DuplicateSetElement>().expect_correction(
            indoc! {r#"
                Set.new([:foo, :bar, :foo])
                                     ^^^^ Remove the duplicate element in Set.
            "#},
            "Set.new([:foo, :bar])\n",
        );
    }

    #[test]
    fn flags_and_removes_duplicate_in_to_set() {
        test::<DuplicateSetElement>().expect_correction(
            indoc! {r#"
                [1, 2, 1].to_set
                       ^ Remove the duplicate element in Set.
            "#},
            "[1, 2].to_set\n",
        );
    }

    #[test]
    fn flags_duplicate_const_element() {
        test::<DuplicateSetElement>().expect_offense(indoc! {r#"
            Set[FOO, BAR, FOO]
                          ^^^ Remove the duplicate element in Set.
        "#});
    }

    #[test]
    fn flags_duplicate_variable_element() {
        // Instance variables are always `variable?` (no assignment needed);
        // bare `foo` without a prior assignment parses as a method call and is
        // intentionally not eligible (matches RuboCop).
        test::<DuplicateSetElement>().expect_offense(indoc! {r#"
            Set[@foo, @bar, @foo]
                            ^^^^ Remove the duplicate element in Set.
        "#});
    }

    #[test]
    fn ignores_bare_identifier_elements() {
        // Bare `foo` parses as a receiverless send (possibly changing value),
        // so it is not eligible for de-duplication.
        test::<DuplicateSetElement>().expect_no_offenses("Set[foo, bar, foo]\n");
    }

    #[test]
    fn accepts_unique_elements() {
        test::<DuplicateSetElement>().expect_no_offenses("Set[:foo, :bar, :baz]\n");
    }

    #[test]
    fn ignores_method_call_elements() {
        // Method calls may return changing values; not eligible for comparison.
        test::<DuplicateSetElement>().expect_no_offenses("Set[foo.bar, foo.bar]\n");
    }

    #[test]
    fn ignores_unrelated_constant_bracket() {
        test::<DuplicateSetElement>().expect_no_offenses("Other[:foo, :foo]\n");
    }
}
