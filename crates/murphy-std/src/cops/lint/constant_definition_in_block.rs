//! `Lint/ConstantDefinitionInBlock` — flag constant/class/module definitions
//! placed directly inside a block, which leaks the definition into the
//! enclosing namespace rather than scoping it to the block.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/ConstantDefinitionInBlock
//! upstream_version_checked: 1.86.2
//! version_added: "0.91"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's two matchers — `({^any_block [^begin ^^any_block]}
//!   nil? ...)` for `on_casgn` and `({^any_block [^begin ^^any_block]} ...)`
//!   for `on_class`/`on_module`. A definition fires when its parent is a block
//!   (block/numblock/itblock), or its parent is an implicit `begin` whose own
//!   parent is a block. The `casgn` variant additionally requires a `nil`
//!   scope (relative const assignment `BAR =`, not `Foo::BAR =`). Murphy's
//!   prism lowering drops a `cbase` scope, so `::BAR =` lowers to the same
//!   `(casgn :BAR nil ...)` shape as `BAR =`; RuboCop matches only
//!   `(casgn nil? ...)` and `::BAR` has a cbase (non-nil) scope, so the cop
//!   interim-excludes a casgn whose source starts with `::` until the lowering
//!   preserves cbase. The block's method name is the nearest block ancestor's
//!   selector; `AllowedMethods` (default `[enums]`) suppresses the offense and
//!   is user-configurable via a `Vec<String>` runtime option, matching
//!   RuboCop's `include AllowedMethods`.
//! ```
//!
//! ## Matched shapes
//!
//! `Casgn { scope: None, .. }`, `Class`, or `Module` nodes whose parent is a
//! block, or whose parent is an implicit `begin` directly inside a block.
//!
//! ## Why this shape
//!
//! `foo do BAR = 1 end` defines `BAR` in the enclosing namespace, not scoped
//! to the block, which is almost always a mistake (use a local, a method, or
//! `const_set` for meta-programming). RuboCop allows `enums` because some DSLs
//! (e.g. ActiveRecord `enum`) legitimately define constants inside their block.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct ConstantDefinitionInBlock;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowedMethods",
        default = ["enums"],
        description = "Block method names that may legitimately define constants."
    )]
    pub allowed_methods: Vec<String>,
}

#[cop(
    name = "Lint/ConstantDefinitionInBlock",
    description = "Do not define constants within a block.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl ConstantDefinitionInBlock {
    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        // `nil?` scope: relative const assignment (`BAR =`), not `Foo::BAR =`.
        let NodeKind::Casgn { scope, .. } = *cx.kind(node) else {
            return;
        };
        if scope.get().is_some() {
            return;
        }
        // Interim: murphy's prism lowering drops a `cbase` scope, so `::BAR = 1`
        // lowers to the same `(casgn :BAR nil ...)` shape as `BAR = 1`. RuboCop
        // matches only `(casgn nil? ...)`, and `::BAR` has a cbase (non-nil)
        // scope, so it is not matched. Distinguish by the leading `::` in source
        // until the lowering preserves cbase.
        if cx.raw_source(cx.range(node)).trim_start().starts_with("::") {
            return;
        }
        self.check_definition(node, cx);
    }

    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_definition(node, cx);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_definition(node, cx);
    }
}

impl ConstantDefinitionInBlock {
    fn check_definition(&self, node: NodeId, cx: &Cx<'_>) {
        if !defined_in_block(node, cx) {
            return;
        }
        if enclosing_block_method_allowed(node, cx) {
            return;
        }
        cx.emit_offense(
            cx.range(node),
            "Do not define constants this way within a block.",
            None,
        );
    }
}

/// RuboCop's `{^any_block [^begin ^^any_block]}`: the node's parent is a block,
/// or its parent is a `begin` whose own parent is a block.
fn defined_in_block(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if cx.is_any_block_type(parent) {
        return true;
    }
    if matches!(cx.kind(parent), NodeKind::Begin(_))
        && let Some(grandparent) = cx.parent(parent).get()
    {
        return cx.is_any_block_type(grandparent);
    }
    false
}

/// RuboCop's `allowed_method?(method_name(node))`: the nearest enclosing block
/// ancestor's selector is in the configured `AllowedMethods`.
fn enclosing_block_method_allowed(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(block) = cx.ancestors(node).find(|&a| cx.is_any_block_type(a)) else {
        return false;
    };
    let opts = cx.options_or_default::<Options>();
    cx.method_name(block)
        .is_some_and(|m| opts.allowed_methods.iter().any(|a| a == m))
}

#[cfg(test)]
mod tests {
    use super::{ConstantDefinitionInBlock, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_constant_in_block() {
        test::<ConstantDefinitionInBlock>().expect_offense(indoc! {r#"
            foo do
              BAR = 42
              ^^^^^^^^ Do not define constants this way within a block.
            end
        "#});
    }

    #[test]
    fn flags_constant_in_block_with_multiple_statements() {
        // The casgn's parent is a `begin` whose parent is the block.
        test::<ConstantDefinitionInBlock>().expect_offense(indoc! {r#"
            foo do
              baz
              BAR = 42
              ^^^^^^^^ Do not define constants this way within a block.
            end
        "#});
    }

    #[test]
    fn flags_class_in_block() {
        test::<ConstantDefinitionInBlock>().expect_offense(indoc! {r#"
            foo do
              class Bar; end
              ^^^^^^^^^^^^^^ Do not define constants this way within a block.
            end
        "#});
    }

    #[test]
    fn flags_module_in_block() {
        test::<ConstantDefinitionInBlock>().expect_offense(indoc! {r#"
            foo do
              module Bar; end
              ^^^^^^^^^^^^^^^ Do not define constants this way within a block.
            end
        "#});
    }

    #[test]
    fn does_not_flag_namespaced_constant_assignment() {
        // `Foo::BAR =` has a non-nil scope → not matched (RuboCop `nil?`).
        test::<ConstantDefinitionInBlock>().expect_no_offenses(indoc! {r#"
            foo do
              Foo::BAR = 42
            end
        "#});
    }

    #[test]
    fn does_not_flag_cbase_constant_assignment() {
        // Mastodon FP: `::BAR =` has a `cbase` scope in RuboCop, so the
        // `(casgn nil? ...)` matcher does not fire. Murphy's prism lowering
        // drops the cbase (scope becomes nil), so it is distinguished here by
        // the leading `::` in the source. Clean.
        test::<ConstantDefinitionInBlock>().expect_no_offenses(indoc! {r#"
            foo do
              ::BAR = 1
            end
        "#});
    }

    #[test]
    fn does_not_flag_constant_at_class_top_level() {
        test::<ConstantDefinitionInBlock>().expect_no_offenses(indoc! {r#"
            class A
              BAR = 42
            end
        "#});
    }

    #[test]
    fn does_not_flag_allowed_method_block() {
        // `enums` is on the default AllowedMethods list.
        test::<ConstantDefinitionInBlock>().expect_no_offenses(indoc! {r#"
            enums do
              BAR = 42
            end
        "#});
    }

    #[test]
    fn allowed_methods_option_suppresses_custom_method() {
        // A user-configured `AllowedMethods` entry suppresses the offense,
        // mirroring RuboCop's `include AllowedMethods`.
        let opts = Options {
            allowed_methods: vec!["my_dsl".to_string()],
        };
        test::<ConstantDefinitionInBlock>()
            .with_options(&opts)
            .expect_no_offenses(indoc! {r#"
                my_dsl do
                  BAR = 42
                end
            "#});
    }

    #[test]
    fn allowed_methods_option_replaces_default_enums() {
        // Setting `AllowedMethods` replaces the default, so `enums` is no
        // longer allowed once the user overrides the list.
        let opts = Options {
            allowed_methods: vec!["my_dsl".to_string()],
        };
        test::<ConstantDefinitionInBlock>()
            .with_options(&opts)
            .expect_offense(indoc! {r#"
                enums do
                  BAR = 42
                  ^^^^^^^^ Do not define constants this way within a block.
                end
            "#});
    }

    #[test]
    fn flags_constant_in_nested_block() {
        test::<ConstantDefinitionInBlock>().expect_offense(indoc! {r#"
            foo do
              bar do
                BAR = 42
                ^^^^^^^^ Do not define constants this way within a block.
              end
            end
        "#});
    }
}

murphy_plugin_api::submit_cop!(ConstantDefinitionInBlock);
