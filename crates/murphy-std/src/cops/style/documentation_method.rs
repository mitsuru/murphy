//! `Style/DocumentationMethod` — flags missing documentation for public methods.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/DocumentationMethod
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `def`/`defs` that lack a preceding own-line comment block, with
//!   configurable `RequireForNonPublicMethods` (default `false`) and
//!   `AllowedMethods` list (default empty). `initialize` is always exempt.
//!
//!   Visibility detection:
//!   - `private def foo` / `protected def foo` — direct parent is a `Send`
//!     with method `private`/`protected` and the def as an argument.
//!   - `private; def foo` — a bare `send(:private)` or `send(:protected)`
//!     precedes the def as a sibling in an enclosing `begin` block.
//!   - `module_function` / `ruby2_keywords` modifier: RuboCop checks
//!     `modifier_node?` and redirects the offense/documentation check to the
//!     modifier send node rather than the def. Murphy mirrors this by checking
//!     the modifier's preceding comments.
//!
//!   Documentation comment: any contiguous block of own-line `#` comments
//!   immediately above the node (or above the modifier, if present).
//!   This uses `cx.range_with_comments(node)` — if it extends before the
//!   node's own start, comments are present.
//!
//!   Disabled by default (Enabled: false in default.yml).
//!   No autocorrect.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Missing method documentation comment.";

#[derive(Default)]
pub struct DocumentationMethod;

#[derive(CopOptions)]
pub struct DocumentationMethodOptions {
    #[option(
        name = "RequireForNonPublicMethods",
        default = false,
        description = "When true, also requires documentation for private and protected methods."
    )]
    pub require_for_non_public_methods: bool,

    #[option(
        name = "AllowedMethods",
        default = [],
        description = "Methods that are exempt from the documentation requirement."
    )]
    pub allowed_methods: Vec<String>,
}

#[cop(
    name = "Style/DocumentationMethod",
    description = "Checks for missing documentation comment for public methods.",
    default_severity = "warning",
    default_enabled = false,
    options = DocumentationMethodOptions,
)]
impl DocumentationMethod {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Skip `initialize` — always exempt.
    if cx.method_name(node) == Some("initialize") {
        return;
    }

    let opts = cx.options_or_default::<DocumentationMethodOptions>();

    // Check for `module_function` / `ruby2_keywords` modifier: if the def's
    // direct parent is a Send with one of these methods (and the def is an
    // argument), treat the parent as the node to check for documentation.
    let check_node = if let Some(parent) = cx.parent(node).get() {
        if is_modifier_node(parent, cx) {
            parent
        } else {
            node
        }
    } else {
        node
    };

    // Visibility check.
    if !opts.require_for_non_public_methods && is_non_public(node, cx) {
        return;
    }

    // Documentation comment check: `range_with_comments` extends the range
    // backwards to include preceding own-line comments. If extended, there
    // are comments.
    if has_documentation_comment(check_node, cx) {
        return;
    }

    // AllowedMethods check.
    let method_name = cx.method_name(node).unwrap_or("");
    if opts.allowed_methods.iter().any(|m| m == method_name) {
        return;
    }

    cx.emit_offense(first_line_range(node, cx), MSG, None);
}

/// Returns `true` if `parent` is a `module_function` or `ruby2_keywords` Send
/// that wraps the def as an argument.
fn is_modifier_node(parent: NodeId, cx: &Cx<'_>) -> bool {
    if let NodeKind::Send { method, .. } = cx.kind(parent) {
        let name = cx.symbol_str(*method);
        return name == "module_function" || name == "ruby2_keywords";
    }
    false
}

/// Returns `true` if the def node is non-public (private or protected).
///
/// Two shapes:
/// 1. `private def foo` / `protected def foo` — parent is `Send { method:
///    "private"|"protected", args: [node] }`.
/// 2. `private; def foo` — a bare `send(:private)` / `send(:protected)` with
///    no args precedes `node` in an enclosing `begin` block.
fn is_non_public(node: NodeId, cx: &Cx<'_>) -> bool {
    // Shape 1: parent is the modifier call (private/protected def foo).
    if let Some(parent) = cx.parent(node).get()
        && let NodeKind::Send { method, .. } = cx.kind(parent)
    {
        let name = cx.symbol_str(*method);
        if name == "private" || name == "protected" {
            return true;
        }
    }

    // Shape 2: scan preceding siblings in enclosing `begin`.
    find_visibility_from_siblings(node, cx)
}

/// Scan backwards through siblings in the nearest enclosing `begin` block to
/// find the most recent visibility modifier (`private`/`protected`/`public`)
/// with no args. Returns `true` if the last such modifier is `private` or
/// `protected`.
fn find_visibility_from_siblings(node: NodeId, cx: &Cx<'_>) -> bool {
    // Find the node to look for among siblings: if parent is a modifier
    // wrapper (module_function/ruby2_keywords), look for that.
    let target = if let Some(parent) = cx.parent(node).get() {
        if is_modifier_node(parent, cx) { parent } else { node }
    } else {
        node
    };

    let Some(parent) = cx.parent(target).get() else {
        return false;
    };

    let NodeKind::Begin(list) = cx.kind(parent) else {
        return false;
    };

    let siblings = cx.list(*list);
    let Some(pos) = siblings.iter().position(|&id| id == target) else {
        return false;
    };

    // Walk backwards to find the most recent visibility modifier.
    for &sibling in siblings[..pos].iter().rev() {
        if let NodeKind::Send { method, args, receiver } = cx.kind(sibling) {
            let name = cx.symbol_str(*method);
            // Bare call (no receiver, no args) — this is the visibility modifier.
            if receiver.get().is_none() && cx.list(*args).is_empty() {
                match name {
                    "private" | "protected" => return true,
                    "public" => return false,
                    _ => {}
                }
            }
        }
    }

    false
}

/// Returns `true` if there is at least one own-line comment immediately above
/// `node` (uses `range_with_comments` to detect them).
fn has_documentation_comment(node: NodeId, cx: &Cx<'_>) -> bool {
    let range_no_comments = cx.range(node);
    let range_with_comments = cx.range_with_comments(node);
    range_with_comments.start < range_no_comments.start
}

/// Returns the range of the first source line of the node (up to the first newline).
fn first_line_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);
    let source = cx.source().as_bytes();
    let node_start = node_range.start as usize;
    let first_line_end = source[node_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(node_range.end as usize, |pos| node_start + pos);
    Range {
        start: node_range.start,
        end: first_line_end as u32,
    }
}


#[cfg(test)]
mod tests {
    use super::DocumentationMethod;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_method_without_documentation() {
        test::<DocumentationMethod>().expect_offense(indoc! {r#"
            class Foo
              def bar
              ^^^^^^^ Missing method documentation comment.
                puts baz
              end
            end
        "#});
    }

    #[test]
    fn accepts_method_with_documentation() {
        test::<DocumentationMethod>().expect_no_offenses(indoc! {r#"
            class Foo
              # Documentation
              def bar
                puts baz
              end
            end
        "#});
    }

    #[test]
    fn flags_module_method_without_documentation() {
        test::<DocumentationMethod>().expect_offense(indoc! {r#"
            module Foo
              def bar
              ^^^^^^^ Missing method documentation comment.
                puts baz
              end
            end
        "#});
    }

    #[test]
    fn accepts_module_method_with_documentation() {
        test::<DocumentationMethod>().expect_no_offenses(indoc! {r#"
            module Foo
              # Documentation
              def bar
                puts baz
              end
            end
        "#});
    }

    #[test]
    fn flags_singleton_method_without_documentation() {
        test::<DocumentationMethod>().expect_offense(indoc! {r#"
            def foo.bar
            ^^^^^^^^^^^ Missing method documentation comment.
              puts baz
            end
        "#});
    }

    #[test]
    fn accepts_singleton_method_with_documentation() {
        test::<DocumentationMethod>().expect_no_offenses(indoc! {r#"
            # Documentation
            def foo.bar
              puts baz
            end
        "#});
    }

    #[test]
    fn accepts_initialize() {
        test::<DocumentationMethod>().expect_no_offenses(indoc! {r#"
            class Foo
              def initialize
              end
            end
        "#});
    }

    #[test]
    fn accepts_private_method_by_default() {
        test::<DocumentationMethod>().expect_no_offenses(indoc! {r#"
            class Foo
              private
              def do_something
              end
            end
        "#});
    }

    #[test]
    fn accepts_protected_method_by_default() {
        test::<DocumentationMethod>().expect_no_offenses(indoc! {r#"
            class Foo
              protected
              def do_something
              end
            end
        "#});
    }

    #[test]
    fn accepts_private_def_modifier() {
        test::<DocumentationMethod>().expect_no_offenses(indoc! {r#"
            class Foo
              private def do_something
              end
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(DocumentationMethod);
