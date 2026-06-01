//! `Style/RequireOrder` — sort `require` and `require_relative` in alphabetical
//! order within contiguous sections.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RequireOrder
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Offense detection is fully parity with RuboCop.
//!   Autocorrect is not implemented (documented gap): RuboCop moves whole lines
//!   including surrounding comments, which requires a line-granular
//!   range_with_comments_and_lines helper not yet available in Murphy's plugin
//!   API. The cop is marked SafeAutoCorrect:false in RuboCop's default.yml.
//!   Offense-only is the safe conservative choice for v1.
//!
//!   Covered:
//!     - require / require_relative in top-level and nested scopes
//!     - Sections separated by blank lines sort independently
//!     - Modifier-form conditionals (require 'x' if cond) are unwrapped
//!     - Non-modifier (block) if between requires breaks the scan
//!     - Different methods (require vs require_relative) break the scan
//!     - Receiver-qualified calls (Bundler.require) break the scan
//!     - Non-Send siblings break the scan
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RequireOrder;

const MSG: &str = "Sort `%s` in alphabetical order.";

#[cop(
    name = "Style/RequireOrder",
    description = "Sort `require` and `require_relative` in alphabetical order.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RequireOrder {
    #[on_node(kind = "send", methods = ["require", "require_relative"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

enum SiblingKind {
    /// The node is (or wraps) a same-method no-receiver require with a str arg.
    Require(NodeId),
    /// The node breaks the contiguous run — stop scanning.
    Break,
}

/// Returns whether the sibling participates in the scan as a same-method require.
/// A modifier-if wrapping a matching require is unwrapped.
fn classify_sibling(sibling_id: NodeId, method_name: &str, cx: &Cx<'_>) -> SiblingKind {
    match cx.kind(sibling_id) {
        NodeKind::Send {
            receiver,
            method,
            args,
        } => {
            if receiver.get().is_some() {
                return SiblingKind::Break;
            }
            if cx.symbol_str(*method) != method_name {
                return SiblingKind::Break;
            }
            if cx.list(*args).is_empty() {
                return SiblingKind::Break;
            }
            SiblingKind::Require(sibling_id)
        }
        NodeKind::If { then_, else_, .. } => {
            if !cx.is_modifier_form(sibling_id) {
                return SiblingKind::Break;
            }
            // Modifier-if: check the body that contains the require.
            for opt_id in [*then_, *else_] {
                if let Some(body_id) = opt_id.get()
                    && let NodeKind::Send { receiver, method, args } = cx.kind(body_id)
                    && receiver.get().is_none()
                    && cx.symbol_str(*method) == method_name
                    && !cx.list(*args).is_empty()
                {
                    return SiblingKind::Require(body_id);
                }
            }
            SiblingKind::Break
        }
        _ => SiblingKind::Break,
    }
}

/// Returns the string value of the first argument if it's a plain `Str`.
fn str_arg<'a>(send_id: NodeId, cx: &'a Cx<'_>) -> Option<&'a str> {
    let NodeKind::Send { args, .. } = cx.kind(send_id) else {
        return None;
    };
    let list = cx.list(*args);
    let NodeKind::Str(sym) = cx.kind(list[0]) else {
        return None;
    };
    Some(cx.string_str(*sym))
}

/// Returns `true` if no blank line separates `range1` and `range2`.
fn in_same_section(range1: Range, range2: Range, cx: &Cx<'_>) -> bool {
    let start = range1.start.min(range2.start) as usize;
    let end = range1.end.max(range2.end) as usize;
    !cx.source()[start..end].contains("\n\n")
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send { receiver, method, args } = cx.kind(node) else {
        return;
    };
    if receiver.get().is_some() {
        return;
    }
    if cx.list(*args).is_empty() {
        return;
    }
    let method_name = cx.symbol_str(*method);

    // If the require's direct parent is a modifier-if, scan from the if node.
    let search_id = {
        let parent = cx.parent(node).get();
        match parent {
            Some(p) if matches!(cx.kind(p), NodeKind::If { .. }) && cx.is_modifier_form(p) => p,
            _ => node,
        }
    };

    let Some(current_value) = str_arg(node, cx) else {
        return;
    };
    let current_range = cx.range(search_id);

    // Walk left siblings from search_id, scanning the contiguous run.
    let mut scan = cx.left_sibling(search_id);
    while let Some(sib_id) = scan.get() {
        match classify_sibling(sib_id, method_name, cx) {
            SiblingKind::Break => break,
            SiblingKind::Require(req_id) => {
                let Some(sib_value) = str_arg(req_id, cx) else {
                    break;
                };
                if !in_same_section(cx.range(sib_id), current_range, cx) {
                    break;
                }
                if current_value < sib_value {
                    let msg = MSG.replace("%s", method_name);
                    cx.emit_offense(cx.range(node), &msg, None);
                    return;
                }
            }
        }
        scan = cx.left_sibling(sib_id);
    }
}

#[cfg(test)]
mod tests {
    use super::RequireOrder;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn no_offense_sorted_require() {
        test::<RequireOrder>().expect_no_offenses(indoc! {r#"
            require 'a'
            require 'b'
        "#});
    }

    #[test]
    fn no_offense_single_require() {
        test::<RequireOrder>().expect_no_offenses(indoc! {r#"
            require 'a'
        "#});
    }

    #[test]
    fn no_offense_different_sections() {
        test::<RequireOrder>().expect_no_offenses(indoc! {r#"
            require 'b'
            require 'd'

            require 'a'
            require 'c'
        "#});
    }

    #[test]
    fn no_offense_mixed_methods_break_scan() {
        test::<RequireOrder>().expect_no_offenses(indoc! {r#"
            require 'b'
            require_relative 'a'
        "#});
    }

    #[test]
    fn no_offense_require_relative_between_unsorted_require() {
        test::<RequireOrder>().expect_no_offenses(indoc! {r#"
            require 'c'
            require_relative 'b'
            require 'a'
        "#});
    }

    #[test]
    fn no_offense_bundler_require_breaks_scan() {
        test::<RequireOrder>().expect_no_offenses(indoc! {r#"
            require 'e'
            Bundler.require(:default)
            require 'c'
        "#});
    }

    #[test]
    fn no_offense_block_if_breaks_scan() {
        test::<RequireOrder>().expect_no_offenses(indoc! {r#"
            require 'c'
            if foo
              require 'a'
            end
            require 'b'
        "#});
    }

    #[test]
    fn no_offense_non_send_between_requires() {
        test::<RequireOrder>().expect_no_offenses(indoc! {r#"
            require 'a'
            begin
            end
            require 'b'
        "#});
    }

    #[test]
    fn no_offense_mixed_quote_styles_sorted() {
        test::<RequireOrder>().expect_no_offenses(indoc! {r#"
            require 'a'
            require "b"
        "#});
    }

    #[test]
    fn no_offense_sorted_require_relative() {
        test::<RequireOrder>().expect_no_offenses(indoc! {r#"
            require_relative 'a'
            require_relative 'b'
        "#});
    }

    #[test]
    fn flags_unsorted_require() {
        test::<RequireOrder>().expect_offense(indoc! {r#"
            require 'b'
            require 'a'
            ^^^^^^^^^^^ Sort `require` in alphabetical order.
        "#});
    }

    #[test]
    fn flags_unsorted_require_relative() {
        test::<RequireOrder>().expect_offense(indoc! {r#"
            require_relative 'b'
            require_relative 'a'
            ^^^^^^^^^^^^^^^^^^^^ Sort `require_relative` in alphabetical order.
        "#});
    }

    #[test]
    fn flags_multiple_unsorted_requires() {
        test::<RequireOrder>().expect_offense(indoc! {r#"
            require 'd'
            require 'a'
            ^^^^^^^^^^^ Sort `require` in alphabetical order.
            require 'b'
            ^^^^^^^^^^^ Sort `require` in alphabetical order.
            require 'c'
            ^^^^^^^^^^^ Sort `require` in alphabetical order.
        "#});
    }

    #[test]
    fn flags_modifier_if_out_of_order() {
        test::<RequireOrder>().expect_offense(indoc! {r#"
            require 'c'
            require 'a' if foo
            ^^^^^^^^^^^ Sort `require` in alphabetical order.
            require 'b'
            ^^^^^^^^^^^ Sort `require` in alphabetical order.
        "#});
    }

    #[test]
    fn flags_modifier_unless_out_of_order() {
        test::<RequireOrder>().expect_offense(indoc! {r#"
            require 'c'
            require 'a' unless foo
            ^^^^^^^^^^^ Sort `require` in alphabetical order.
            require 'b'
            ^^^^^^^^^^^ Sort `require` in alphabetical order.
        "#});
    }

    #[test]
    fn flags_unsorted_inside_rescue_block() {
        test::<RequireOrder>().expect_offense(indoc! {r#"
            begin
              do_something
            rescue
              require 'b'
              require 'a'
              ^^^^^^^^^^^ Sort `require` in alphabetical order.
            end
        "#});
    }

    #[test]
    fn flags_unsorted_inside_block_if() {
        test::<RequireOrder>().expect_offense(indoc! {r#"
            require 'd'
            if foo
              require 'b'
              require 'a'
              ^^^^^^^^^^^ Sort `require` in alphabetical order.
            end
            require 'c'
        "#});
    }
}
murphy_plugin_api::submit_cop!(RequireOrder);
