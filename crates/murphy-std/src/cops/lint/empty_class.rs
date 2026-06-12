//! `Lint/EmptyClass` — flag classes and metaclasses without a body.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EmptyClass
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues:
//!   - murphy-9cr.9
//! notes: >
//!   Detection mirrors RuboCop's on_class / on_sclass: a class with no body
//!   and no parent class is flagged with CLASS_MSG; a metaclass (`class <<
//!   self`) with no body is flagged with METACLASS_MSG. Offense highlight is
//!   clamped to the node's first line (Murphy convention, matching
//!   `Lint/MissingSuper`) vs RuboCop's whole-node range; the start position
//!   matches. `AllowComments` defaults to `false` (matching RuboCop) but
//!   the override is ABI-blocked until murphy-9cr.9 wires options through Cx;
//!   the default IS the live behavior. No autocorrect (RuboCop has none).
//! ```
//!
//! ## `AllowComments` default and the v1 option-wiring limitation
//!
//! RuboCop's default for `AllowComments` is `false`, so by default a
//! comment-only class body (`class Foo; # comment; end`) is still flagged.
//! The option is exported via `#[derive(CopOptions)]` so the host validates
//! `[cops.rules."Lint/EmptyClass"]` keys, but runtime reads still come from
//! `Options::default()` — setting `AllowComments = true` has no dispatch-time
//! effect until murphy-9cr.9. This mirrors `Lint/EmptyWhen`.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct EmptyClass;

/// Cop options for [`EmptyClass`]. v1: read from `Default` at dispatch
/// time (`murphy-9cr.9` will wire live overrides through `Cx`).
#[derive(CopOptions)]
pub struct Options {
    #[option(
        default = false,
        description = "When true, don't flag a class whose only body is a comment."
    )]
    pub allow_comments: bool,
}

#[cop(
    name = "Lint/EmptyClass",
    description = "Flag classes and metaclasses without a body.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl EmptyClass {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class {
            superclass, body, ..
        } = *cx.kind(node)
        else {
            return;
        };
        // RuboCop: `unless body_or_allowed_comment_lines?(node) || node.parent_class`.
        if superclass.get().is_some() {
            return;
        }
        if body_or_allowed_comment_lines(node, body, cx) {
            return;
        }
        cx.emit_offense(
            crate::cops::util::first_line_range(node, cx),
            "Empty class detected.",
            None,
        );
    }

    #[on_node(kind = "sclass")]
    fn check_sclass(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Sclass { body, .. } = *cx.kind(node) else {
            return;
        };
        if body_or_allowed_comment_lines(node, body, cx) {
            return;
        }
        cx.emit_offense(
            crate::cops::util::first_line_range(node, cx),
            "Empty metaclass detected.",
            None,
        );
    }
}

/// RuboCop's `body_or_allowed_comment_lines?`: true if the node has a body, or
/// `AllowComments` is enabled and the node's source range contains a comment.
fn body_or_allowed_comment_lines(
    node: NodeId,
    body: murphy_plugin_api::OptNodeId,
    cx: &Cx<'_>,
) -> bool {
    if body.get().is_some() {
        return true;
    }
    let opts = Options::default();
    opts.allow_comments && !cx.comments_in_range(cx.range(node)).is_empty()
}

murphy_plugin_api::submit_cop!(EmptyClass);

#[cfg(test)]
mod tests {
    use super::EmptyClass;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_class() {
        test::<EmptyClass>().expect_offense(indoc! {r#"
            class Foo
            ^^^^^^^^^ Empty class detected.
            end
        "#});
    }

    #[test]
    fn accepts_class_with_body() {
        test::<EmptyClass>().expect_no_offenses(indoc! {r#"
            class Foo
              def bar; end
            end
        "#});
    }

    #[test]
    fn accepts_empty_class_with_superclass() {
        // RuboCop: `node.parent_class` short-circuits the offense.
        test::<EmptyClass>().expect_no_offenses(indoc! {r#"
            class Foo < Bar
            end
        "#});
    }

    #[test]
    fn flags_empty_metaclass() {
        test::<EmptyClass>().expect_offense(indoc! {r#"
            class << self
            ^^^^^^^^^^^^^ Empty metaclass detected.
            end
        "#});
    }

    #[test]
    fn accepts_metaclass_with_body() {
        test::<EmptyClass>().expect_no_offenses(indoc! {r#"
            class << self
              def bar; end
            end
        "#});
    }

    #[test]
    fn flags_comment_only_class_by_default() {
        // AllowComments default is false, so a comment-only body is flagged.
        test::<EmptyClass>().expect_offense(indoc! {r#"
            class Foo
            ^^^^^^^^^ Empty class detected.
              # TODO: implement
            end
        "#});
    }

    #[test]
    fn does_not_flag_namespaced_module_nesting() {
        // A module is not a class; it must not be flagged by this cop.
        test::<EmptyClass>().expect_no_offenses(indoc! {r#"
            module Foo
            end
        "#});
    }

    #[test]
    fn offense_message_matches_rubocop_verbatim() {
        // Pins RuboCop's CLASS_MSG.
        test::<EmptyClass>().expect_offense(indoc! {r#"
            class A
            ^^^^^^^ Empty class detected.
            end
        "#});
    }
}
