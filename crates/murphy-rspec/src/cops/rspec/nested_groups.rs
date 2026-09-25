//! `RSpec/NestedGroups` — cap example-group nesting depth.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/NestedGroups
//! upstream_version_checked: 3.7.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_top_level_group` (`TopLevelGroup`: each
//!   top-level `spec_group?` starts a walk at nesting 1):
//!   `find_nested_example_groups` flags every `example_group?` deeper
//!   than `Max` (default 3) per `add_offense(send)` (trimmed via
//!   `send_without_block_range` since Murphy's `Send` covers the block)
//!   with `Maximum example group nesting exceeded [N/M]`. Nesting only
//!   increments for `Block` groups not listed in `AllowedGroups`
//!   (default empty); traversal only descends through `Block` / `Begin`
//!   children, so groups hidden inside `if` / `def` do not count —
//!   exactly like upstream's `each_child_node(:block, :begin)` (verified
//!   vs 3.7.0, including `Max: 2` and `AllowedGroups: [path]`). No
//!   autocorrect upstream, none here.
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on top-level spec-group `Block`s:
//!
//! - Four nested groups with default `Max: 3` — the innermost flagged.
//! - Three nested groups — at the limit, clean.
//! - `Max: 2` — the third and fourth levels flag.
//! - `AllowedGroups: [path]` — the listed group does not increment.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; flattening nested groups needs human
//! judgement about shared setup.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

use crate::cops::rspec_helpers::{
    is_example_group_call, is_spec_group_call, is_top_level_block, send_without_block_range,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct NestedGroups;

#[derive(CopOptions)]
pub struct NestedGroupsOptions {
    #[option(
        name = "Max",
        default = 3,
        description = "Maximum example group nesting depth."
    )]
    pub max: i64,
    #[option(
        name = "AllowedGroups",
        default = [],
        description = "Group methods that do not count toward nesting depth."
    )]
    pub allowed_groups: Vec<String>,
}

#[cop(
    name = "RSpec/NestedGroups",
    description = "Checks for nested example groups.",
    default_severity = "warning",
    default_enabled = true,
    options = NestedGroupsOptions
)]
impl NestedGroups {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        // Upstream `TopLevelGroup`: only top-level spec groups start a
        // walk (`top_level_nodes` through `begin` / `class` / `module`).
        if !is_spec_group_call(cx, call) {
            return;
        }
        if !is_top_level_block(cx, node) {
            return;
        }
        let opts = cx.options_or_default::<NestedGroupsOptions>();
        walk(cx, node, 1, &opts);
    }
}

/// Recursive walk mirroring `find_nested_example_groups`.
///
/// `nesting` is the current depth (outermost group is 1). Only `Block`
/// / `Begin` children are visited, matching upstream's
/// `each_child_node(:block, :begin)`.
fn walk(cx: &Cx<'_>, node: NodeId, nesting: i64, opts: &NestedGroupsOptions) {
    let is_group = matches!(*cx.kind(node), NodeKind::Block { call, .. } if is_example_group_call(cx, call));
    if is_group && nesting > opts.max {
        let NodeKind::Block { call, .. } = *cx.kind(node) else {
            return;
        };
        cx.emit_offense(
            send_without_block_range(cx, call),
            &format!(
                "Maximum example group nesting exceeded [{nesting}/{max}]",
                max = opts.max
            ),
            None,
        );
    }
    let next = if counts_toward_nesting(cx, node, is_group, opts) {
        nesting + 1
    } else {
        nesting
    };
    for child in direct_block_begin_children(cx, node) {
        walk(cx, child, next, opts);
    }
}

/// `true` when `node` increments the depth: an example-group `Block`
/// whose method is not listed in `AllowedGroups`.
///
/// Mirrors upstream `count_up_nesting?` (`example_group && block_type?
/// && !allowed_groups.include?(method_name)`).
fn counts_toward_nesting(
    cx: &Cx<'_>,
    node: NodeId,
    is_group: bool,
    opts: &NestedGroupsOptions,
) -> bool {
    if !is_group {
        return false;
    }
    // `is_group` already implies a `Block`; silence the unused import
    // parity with sibling cops.
    let _ = OptNodeId::NONE;
    let NodeKind::Block { call, .. } = *cx.kind(node) else {
        return false;
    };
    let NodeKind::Send { method, .. } = *cx.kind(call) else {
        return false;
    };
    !opts
        .allowed_groups
        .iter()
        .any(|g| g == cx.symbol_str(method))
}

/// Direct `Block` / `Begin` children of `node`.
///
/// For a `Block`, this is the body when it is a `Block` / `Begin` (with
/// `Begin` members expanded one level); for a `Begin`, the members that
/// are `Block` / `Begin`. Anything else (sends, `if`s, `def`s, plain
/// code) stops the walk, exactly like upstream.
fn direct_block_begin_children(cx: &Cx<'_>, node: NodeId) -> Vec<NodeId> {
    match *cx.kind(node) {
        NodeKind::Block { body, .. } => {
            let Some(body_id) = body.get() else {
                return Vec::new();
            };
            match *cx.kind(body_id) {
                NodeKind::Block { .. } => vec![body_id],
                NodeKind::Begin(members) => cx
                    .list(members)
                    .iter()
                    .copied()
                    .filter(|&id| {
                        matches!(
                            *cx.kind(id),
                            NodeKind::Block { .. } | NodeKind::Begin(_)
                        )
                    })
                    .collect(),
                _ => Vec::new(),
            }
        }
        NodeKind::Begin(members) => cx
            .list(members)
            .iter()
            .copied()
            .filter(|&id| {
                matches!(
                    *cx.kind(id),
                    NodeKind::Block { .. } | NodeKind::Begin(_)
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{NestedGroups, NestedGroupsOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn max_two() -> NestedGroupsOptions {
        NestedGroupsOptions {
            max: 2,
            allowed_groups: Vec::new(),
        }
    }

    fn allow_path() -> NestedGroupsOptions {
        NestedGroupsOptions {
            max: 3,
            allowed_groups: vec!["path".to_owned()],
        }
    }

    #[test]
    fn flags_fourth_level_with_default_max() {
        test::<NestedGroups>().expect_offense(indoc! {r#"
                describe Foo do
                  context 'foo' do
                    context 'bar' do
                      context 'baz' do
                      ^^^^^^^^^^^^^ Maximum example group nesting exceeded [4/3]
                      end
                    end
                  end
                end
            "#});
    }

    #[test]
    fn ignores_three_levels_with_default_max() {
        test::<NestedGroups>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  context 'foo' do
                    context 'bar' do
                    end
                  end
                end
            "#});
    }

    #[test]
    fn flags_two_levels_with_max_two() {
        // `Max: 2` flags both the third and fourth levels (verified vs
        // 3.7.0).
        test::<NestedGroups>()
            .with_options(&max_two())
            .expect_offense(indoc! {r#"
                describe Foo do
                  context 'foo' do
                    context 'bar' do
                    ^^^^^^^^^^^^^ Maximum example group nesting exceeded [3/2]
                      context 'baz' do
                      ^^^^^^^^^^^^^ Maximum example group nesting exceeded [4/2]
                      end
                    end
                  end
                end
            "#});
    }

    #[test]
    fn ignores_allowed_group() {
        // `path` does not increment the depth (verified vs 3.7.0).
        test::<NestedGroups>()
            .with_options(&allow_path())
            .expect_no_offenses(indoc! {r#"
                describe Foo do
                  path '/foo' do
                    context 'bar' do
                    end
                  end
                end
            "#});
    }

    #[test]
    fn ignores_sibling_groups() {
        test::<NestedGroups>().expect_no_offenses(indoc! {r#"
                describe Foo do
                  context 'a' do
                  end
                  context 'b' do
                  end
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(NestedGroups);
