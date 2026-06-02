//! `Style/Documentation` — enforce documentation on top-level classes and modules.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Documentation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Checks that classes and non-namespace modules have a leading documentation
//!   comment. Exemptions:
//!     - Classes with no body (empty body).
//!     - Namespace-only containers: modules/classes whose body contains only
//!       classes, modules, constant assignments (casgn), or
//!       `public_constant`/`private_constant` calls.
//!     - Classes/modules with a `#:nodoc:` end-of-line comment on the same
//!       line, or whose ancestor has `:nodoc: all`.
//!     - Modules consisting entirely of `include`/`extend`/`prepend` statements.
//!     - Constants named in `AllowedConstants`.
//!   No autocorrect (RuboCop does not implement one either).
//!   Gap vs RuboCop: Murphy does not replicate RuboCop's `ast_with_comments`
//!   exact comment-association algorithm; we use `range_with_comments` which
//!   covers the common contiguous-own-line-comments-above-node case.
//! ```
//!
//! ## Matched shapes
//!
//! - `class` nodes with a non-nil body that require documentation
//! - `module` nodes with a non-nil body that require documentation

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Missing top-level documentation comment for `%type %name`.";

#[derive(Default)]
pub struct Documentation;

#[derive(Default, Debug)]
pub struct DocumentationOptions {
    pub allowed_constants: Vec<String>,
}

impl CopOptions for DocumentationOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, murphy_plugin_api::ConfigError> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(murphy_plugin_api::ConfigError::parse)?;
        let obj = value
            .as_object()
            .ok_or_else(murphy_plugin_api::ConfigError::not_an_object)?;

        let allowed_constants = if let Some(v) = obj.get("AllowedConstants") {
            let arr = v
                .as_array()
                .ok_or_else(|| {
                    murphy_plugin_api::ConfigError::type_mismatch("AllowedConstants", "array")
                })?;
            let mut result = Vec::with_capacity(arr.len());
            for (i, item) in arr.iter().enumerate() {
                let s = item.as_str().ok_or_else(|| {
                    murphy_plugin_api::ConfigError::type_mismatch(
                        format!("AllowedConstants[{i}]"),
                        "string",
                    )
                })?;
                result.push(s.to_owned());
            }
            result
        } else {
            Vec::new()
        };

        Ok(DocumentationOptions { allowed_constants })
    }

    fn to_config_json(&self) -> String {
        let items: Vec<String> = self
            .allowed_constants
            .iter()
            .map(|s| format!("{s:?}"))
            .collect();
        format!("{{\"AllowedConstants\":[{}]}}", items.join(","))
    }
}

#[cop(
    name = "Style/Documentation",
    description = "Document classes and non-namespace modules.",
    default_severity = "warning",
    default_enabled = true,
    options = DocumentationOptions,
)]
impl Documentation {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Class { name, body, .. } = *cx.kind(node) else {
            return;
        };
        // Skip empty-body classes.
        let Some(body_id) = body.get() else { return };
        check(node, name, "class", body_id, cx);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Module { name, body } = *cx.kind(node) else {
            return;
        };
        // Unlike classes, empty modules are NOT exempt — they still need docs.
        // When body is nil, pass None so namespace?/include_statement_only? see
        // an empty body and return false, letting the offense through.
        check_module_inner(node, name, body.get(), cx);
    }
}

/// Entry for module nodes — body may be nil (empty module).
fn check_module_inner(node: NodeId, name_node: NodeId, body: Option<NodeId>, cx: &Cx<'_>) {
    // Skip namespace-only containers.
    if is_namespace(body, cx) {
        return;
    }

    // Skip if there's a documentation comment immediately before the node.
    if has_documentation_comment(node, cx) {
        return;
    }

    // Skip if the constant name is in AllowedConstants.
    let options = cx.options_or_default::<DocumentationOptions>();
    let short_name = const_short_name(name_node, cx);
    if options
        .allowed_constants
        .iter()
        .any(|s| s.as_str() == short_name)
    {
        return;
    }

    // Skip if this is a nodoc node.
    if has_nodoc_comment(node, cx) {
        return;
    }

    // Skip if body consists only of include/extend/prepend statements.
    if body.is_some_and(|body_id| is_include_statement_only(body_id, cx)) {
        return;
    }

    // Build fully-qualified name for the message.
    let identifier = build_identifier(node, cx);

    let msg = MSG
        .replace("%type", "module")
        .replace("%name", &identifier);

    // Offense range: from node start to end of constant name.
    let node_range = cx.range(node);
    let name_range = cx.range(name_node);
    let offense_range = Range {
        start: node_range.start,
        end: name_range.end,
    };

    cx.emit_offense(offense_range, &msg, None);
}

fn check(node: NodeId, name_node: NodeId, type_str: &str, body_id: NodeId, cx: &Cx<'_>) {
    // Skip namespace-only containers.
    if is_namespace(Some(body_id), cx) {
        return;
    }

    // Skip if there's a documentation comment immediately before the node.
    if has_documentation_comment(node, cx) {
        return;
    }

    // Skip if the constant name is in AllowedConstants.
    let options = cx.options_or_default::<DocumentationOptions>();
    let short_name = const_short_name(name_node, cx);
    if options
        .allowed_constants
        .iter()
        .any(|s| s.as_str() == short_name)
    {
        return;
    }

    // Skip if this is a nodoc node.
    if has_nodoc_comment(node, cx) {
        return;
    }

    // Skip if body consists only of include/extend/prepend statements.
    if is_include_statement_only(body_id, cx) {
        return;
    }

    // Build fully-qualified name for the message.
    let identifier = build_identifier(node, cx);

    let msg = MSG
        .replace("%type", type_str)
        .replace("%name", &identifier);

    // Offense range: from node start to end of constant name.
    let node_range = cx.range(node);
    let name_range = cx.range(name_node);
    let offense_range = Range {
        start: node_range.start,
        end: name_range.end,
    };

    cx.emit_offense(offense_range, &msg, None);
}

/// Returns `true` if `body` is a namespace-only body: all children are
/// class/module/casgn or visibility declarations.
fn is_namespace(body: Option<NodeId>, cx: &Cx<'_>) -> bool {
    let Some(id) = body else { return false };
    match cx.kind(id) {
        NodeKind::Begin(list) => cx
            .list(*list)
            .iter()
            .all(|&child| is_constant_declaration(child, cx)),
        _ => is_constant_declaration(id, cx),
    }
}

fn is_constant_declaration(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Class { .. } | NodeKind::Module { .. } | NodeKind::Casgn { .. } => true,
        NodeKind::Send {
            receiver,
            method: sym,
            args,
        } => {
            // `public_constant :Name` / `private_constant :Name`
            let method_name = cx.symbol_str(*sym);
            if !matches!(method_name, "public_constant" | "private_constant") {
                return false;
            }
            if receiver.get().is_some() {
                return false;
            }
            let arg_list = cx.list(*args);
            arg_list.len() == 1
                && matches!(cx.kind(arg_list[0]), NodeKind::Sym(..) | NodeKind::Str(..))
        }
        _ => false,
    }
}

/// Returns `true` if the body consists only of include/extend/prepend statements.
fn is_include_statement_only(body_id: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(body_id) {
        NodeKind::Send {
            receiver,
            method: sym,
            args,
        } => {
            let method = cx.symbol_str(*sym);
            if !matches!(method, "include" | "extend" | "prepend") {
                return false;
            }
            if receiver.get().is_some() {
                return false;
            }
            let arg_list = cx.list(*args);
            arg_list.len() == 1 && matches!(cx.kind(arg_list[0]), NodeKind::Const { .. })
        }
        NodeKind::Begin(list) => cx
            .list(*list)
            .iter()
            .all(|&child| is_include_statement_only(child, cx)),
        _ => false,
    }
}

/// Returns `true` if there is at least one non-nodoc own-line comment
/// immediately preceding the node (using `range_with_comments`).
fn has_documentation_comment(node: NodeId, cx: &Cx<'_>) -> bool {
    let with_comments = cx.range_with_comments(node);
    let node_range = cx.range(node);
    if with_comments.start < node_range.start {
        let comments = cx.comments_in_range(Range {
            start: with_comments.start,
            end: node_range.start,
        });
        return comments
            .iter()
            .any(|c| !is_nodoc_text(cx.raw_source(c.range)));
    }
    false
}

/// Returns `true` if there is a `:nodoc:` comment on the node's keyword line
/// or if any ancestor has `:nodoc: all`.
fn has_nodoc_comment(node: NodeId, cx: &Cx<'_>) -> bool {
    if node_has_nodoc(node, false, cx) {
        return true;
    }
    for ancestor in cx.ancestors(node) {
        if matches!(cx.kind(ancestor), NodeKind::Class { .. } | NodeKind::Module { .. })
            && node_has_nodoc(ancestor, true, cx)
        {
            return true;
        }
    }
    false
}

/// Returns `true` if the node itself has a `:nodoc:` (or `:nodoc: all` if
/// `require_all`) comment on the same line as its keyword.
fn node_has_nodoc(node: NodeId, require_all: bool, cx: &Cx<'_>) -> bool {
    let node_range = cx.range(node);
    let source = cx.source();
    let bytes = source.as_bytes();

    let node_start = node_range.start as usize;
    let line_end = bytes[node_start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |pos| node_start + pos);

    let line_range = Range {
        start: node_range.start,
        end: line_end as u32,
    };
    let comments_on_line = cx.comments_in_range(line_range);

    for comment in &comments_on_line {
        let text = cx.raw_source(comment.range);
        if require_all {
            if is_nodoc_all_text(text) {
                return true;
            }
        } else if is_nodoc_text(text) {
            return true;
        }
    }
    false
}

fn is_nodoc_text(text: &str) -> bool {
    let stripped = text.trim_start_matches('#').trim_start();
    stripped.starts_with(":nodoc:")
}

fn is_nodoc_all_text(text: &str) -> bool {
    let stripped = text.trim_start_matches('#').trim_start();
    if let Some(rest) = stripped.strip_prefix(":nodoc:") {
        rest.trim_start().starts_with("all")
    } else {
        false
    }
}

/// Returns the short name of a constant node (the last component).
fn const_short_name<'a>(node: NodeId, cx: &Cx<'a>) -> &'a str {
    match cx.kind(node) {
        NodeKind::Const { name: sym, .. } => cx.symbol_str(*sym),
        _ => cx.raw_source(cx.range(node)),
    }
}

/// Build a fully-qualified identifier string for the class/module.
fn build_identifier(node: NodeId, cx: &Cx<'_>) -> String {
    let name_node = match cx.kind(node) {
        NodeKind::Class { name, .. } | NodeKind::Module { name, .. } => *name,
        _ => return String::new(),
    };

    let local_name = qualify_const(name_node, cx);

    let outer_parts: Vec<String> = cx
        .ancestors(node)
        .filter_map(|anc| match cx.kind(anc) {
            NodeKind::Class { name, .. } | NodeKind::Module { name, .. } => {
                Some(qualify_const(*name, cx))
            }
            _ => None,
        })
        .collect();

    let mut parts: Vec<String> = outer_parts.into_iter().rev().collect();
    parts.push(local_name);
    let result = parts.join("::");
    result.replace("::::", "::")
}

fn qualify_const(node: NodeId, cx: &Cx<'_>) -> String {
    match cx.kind(node) {
        NodeKind::Const { scope, name: sym } => {
            let name = cx.symbol_str(*sym).to_owned();
            match scope.get() {
                None => name,
                Some(scope_node) => match cx.kind(scope_node) {
                    NodeKind::Cbase => format!("::{name}"),
                    _ => {
                        let prefix = qualify_const(scope_node, cx);
                        format!("{prefix}::{name}")
                    }
                },
            }
        }
        _ => cx.raw_source(cx.range(node)).to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Documentation, DocumentationOptions};
    use murphy_plugin_api::{CopOptions, test_support::{indoc, test}};

    #[test]
    fn flags_undocumented_class() {
        test::<Documentation>().expect_offense(indoc! {r#"
            class Person
            ^^^^^^^^^^^^ Missing top-level documentation comment for `class Person`.
              def greet; end
            end
        "#});
    }

    #[test]
    fn flags_undocumented_module() {
        test::<Documentation>().expect_offense(indoc! {r#"
            module Math
            ^^^^^^^^^^^ Missing top-level documentation comment for `module Math`.
              def foo; end
            end
        "#});
    }

    #[test]
    fn accepts_documented_class() {
        test::<Documentation>().expect_no_offenses(indoc! {r#"
            # Description of Person
            class Person
              def greet; end
            end
        "#});
    }

    #[test]
    fn accepts_documented_module() {
        test::<Documentation>().expect_no_offenses(indoc! {r#"
            # Utilities
            module Math
              def foo; end
            end
        "#});
    }

    #[test]
    fn accepts_class_with_empty_body() {
        test::<Documentation>().expect_no_offenses("class Person\nend\n");
    }

    #[test]
    fn flags_empty_module_without_doc() {
        // Unlike empty classes, empty modules still require documentation.
        test::<Documentation>().expect_offense(indoc! {r#"
            module Foo
            ^^^^^^^^^^ Missing top-level documentation comment for `module Foo`.
            end
        "#});
    }

    #[test]
    fn accepts_namespace_module_with_inner_class() {
        test::<Documentation>().expect_no_offenses(indoc! {r#"
            module Namespace
              # Documented
              class Inner
                def foo; end
              end
            end
        "#});
    }

    #[test]
    fn accepts_namespace_module_with_casgn() {
        test::<Documentation>().expect_no_offenses(indoc! {r#"
            module Namespace
              Public = Class.new
            end
        "#});
    }

    #[test]
    fn accepts_namespace_module_with_private_constant() {
        test::<Documentation>().expect_no_offenses(indoc! {r#"
            module Namespace
              class Private
              end
              private_constant :Private
            end
        "#});
    }

    #[test]
    fn accepts_include_only_module() {
        test::<Documentation>().expect_no_offenses(indoc! {r#"
            module Foo
              extend Bar
            end
        "#});
    }

    #[test]
    fn accepts_nodoc_class() {
        test::<Documentation>()
            .expect_no_offenses("class Foo # :nodoc:\n  def bar; end\nend\n");
    }

    #[test]
    fn accepts_allowed_constant() {
        test::<Documentation>()
            .with_options(&DocumentationOptions {
                allowed_constants: vec!["ClassMethods".to_owned()],
            })
            .expect_no_offenses(indoc! {r#"
                module A
                  module ClassMethods
                    def foo; end
                  end
                end
            "#});
    }

    #[test]
    fn flags_non_allowed_constant() {
        test::<Documentation>()
            .with_options(&DocumentationOptions {
                allowed_constants: vec!["ClassMethods".to_owned()],
            })
            .expect_offense(indoc! {r#"
                module OtherModule
                ^^^^^^^^^^^^^^^^^^ Missing top-level documentation comment for `module OtherModule`.
                  def foo; end
                end
            "#});
    }

    #[test]
    fn config_error_allowed_constants_not_array() {
        let err = <DocumentationOptions as CopOptions>::from_config_json(
            br#"{"AllowedConstants": "ClassMethods"}"#,
        )
        .expect_err("wrong shape is invalid");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "AllowedConstants");
        assert_eq!(expected, &"array");
    }

    #[test]
    fn config_error_allowed_constants_element_not_string() {
        let err = <DocumentationOptions as CopOptions>::from_config_json(
            br#"{"AllowedConstants": [1]}"#,
        )
        .expect_err("wrong shape is invalid");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "AllowedConstants[0]");
        assert_eq!(expected, &"string");
    }
}

murphy_plugin_api::submit_cop!(Documentation);
