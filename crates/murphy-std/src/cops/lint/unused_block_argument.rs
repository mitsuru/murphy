//! `Lint/UnusedBlockArgument` — flag block parameters that are never read
//! inside the block body.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UnusedBlockArgument
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   IgnoreEmptyBlocks (RuboCop default true) is implemented: empty-bodied
//!   blocks do not flag unused args. Shadow args (`|x; y|`) are intentionally
//!   excluded from reporting (they are the domain of
//!   Lint/ShadowingOuterLocalVariable).
//! ```
//!
//! ## Autocorrect
//!
//! Prefix the unused argument name with `_` (e.g. `x` → `_x`).
//! Arguments already prefixed with `_` are skipped.

use std::collections::HashSet;

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

#[derive(Default)]
pub struct UnusedBlockArgument;

#[derive(CopOptions)]
pub struct UnusedBlockArgumentOptions {
    #[option(
        name = "IgnoreEmptyBlocks",
        default = true,
        description = "Do not flag unused arguments of empty-bodied blocks."
    )]
    pub ignore_empty_blocks: bool,
}

#[cop(
    name = "Lint/UnusedBlockArgument",
    description = "Flag unused block arguments.",
    default_severity = "warning",
    default_enabled = true,
    options = UnusedBlockArgumentOptions
)]
impl UnusedBlockArgument {
    #[on_node(kind = "block")]
    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { args: _, body, .. } = *cx.kind(node) else {
            return;
        };

        // RuboCop's `IgnoreEmptyBlocks` (default true): an empty-bodied block
        // (`->(error) {}`, `foo { |x| }`) keeps its argument names for the API
        // it satisfies, so its unused args are not flagged.
        if cx
            .options_or_default::<UnusedBlockArgumentOptions>()
            .ignore_empty_blocks
            && body.get().is_none()
        {
            return;
        }

        let Some(model) = cx.var_model() else { return };
        let Some(scope) = model.scope(node) else {
            return;
        };

        // Lazily built fallback: all Lvar names reachable in this block body
        // (including nested scopes). Handles cross-scope reads of block args,
        // e.g. `|x| [1].each { puts x }` where `x` is read in a nested block.
        let mut lvar_fallback: Option<HashSet<&str>> = None;

        for var in scope.variables().iter().filter(|v| v.is_argument) {
            let name_str = cx.symbol_str(var.name);

            // Skip `_`-prefixed args — intentionally unused.
            if name_str.starts_with('_') {
                continue;
            }

            // Skip shadow args (`|x; y|`). They are declared as `is_argument`
            // in the model for the ShadowingOuterLocalVariable cop's use, but
            // UnusedBlockArgument should not report on them.
            if matches!(*cx.kind(var.declaration_node), NodeKind::Shadowarg(_)) {
                continue;
            }

            // Primary: check model references in this scope.
            let model_used = !var.references.is_empty();

            // Fallback: scan all body descendants for Lvar reads. This catches
            // cross-scope reads (e.g. the arg used only inside a nested block),
            // which the model won't see in this block scope.
            let is_used = model_used || {
                if let Some(body_id) = body.get() {
                    let reads = lvar_fallback.get_or_insert_with(|| lvar_reads(cx, body_id));
                    reads.contains(name_str)
                } else {
                    false
                }
            };

            if is_used {
                continue;
            }

            let range = cx.node(var.declaration_node).loc.name;
            cx.emit_offense(
                range,
                &format!(
                    "Unused block argument - `{name_str}`. If it's necessary, use `_` or \
                     `_{name_str}` as an argument name to indicate that it won't be used."
                ),
                None,
            );
            // Autocorrect: prefix name with `_`.
            cx.emit_edit(
                Range {
                    start: range.start,
                    end: range.start,
                },
                "_",
            );
        }
    }
}

/// Collect all `Lvar` name strings reachable under `body`, including those
/// inside nested scopes. This handles cross-scope reads of block arguments,
/// e.g. an arg that is only referenced inside a nested block.
fn lvar_reads<'a>(cx: &Cx<'a>, body: NodeId) -> HashSet<&'a str> {
    std::iter::once(body)
        .chain(cx.descendants(body))
        .filter_map(|id| match *cx.kind(id) {
            NodeKind::Lvar(s) => Some(cx.symbol_str(s)),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::UnusedBlockArgument;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_unused_block_arg() {
        test::<UnusedBlockArgument>().expect_offense(indoc! {r#"
            [1].each do |x|
                         ^ Unused block argument - `x`. If it's necessary, use `_` or `_x` as an argument name to indicate that it won't be used.
              puts 1
            end
        "#});
    }

    #[test]
    fn ignores_unused_arg_in_empty_block() {
        // IgnoreEmptyBlocks (default): the empty-bodied lambda/block keeps its
        // argument for the callback API it satisfies.
        test::<UnusedBlockArgument>().expect_no_offenses("x = ->(error) {}\n");
        test::<UnusedBlockArgument>().expect_no_offenses("[1].each { |y| }\n");
    }

    #[test]
    fn no_offense_for_used_block_arg() {
        test::<UnusedBlockArgument>().expect_no_offenses(indoc! {r#"
            [1].each do |x|
              puts x
            end
        "#});
    }

    #[test]
    fn no_offense_for_underscore_prefixed_arg() {
        test::<UnusedBlockArgument>().expect_no_offenses(indoc! {r#"
            [1].each do |_x|
              puts 1
            end
        "#});
    }

    #[test]
    fn autocorrects_by_prefixing_underscore() {
        test::<UnusedBlockArgument>()
            .expect_correction(
                indoc! {r#"
                    [1].each do |x|
                                 ^ Unused block argument - `x`. If it's necessary, use `_` or `_x` as an argument name to indicate that it won't be used.
                      puts 1
                    end
                "#},
                indoc! {r#"
                    [1].each do |_x|
                      puts 1
                    end
                "#},
            );
    }

    #[test]
    fn no_offense_when_arg_used_in_nested_block() {
        // Cross-scope: `x` is not referenced in the block's own scope, but the
        // lvar-scan fallback detects its use inside the nested block.
        test::<UnusedBlockArgument>().expect_no_offenses(indoc! {r#"
            [1].each do |x|
              [2].each { puts x }
            end
        "#});
    }

    #[test]
    fn no_offense_for_shadow_arg() {
        // Shadow args (`|x; y|`) declare `y` in the block scope so that
        // ShadowingOuterLocalVariable can detect conflicts, but
        // UnusedBlockArgument must not report on them.
        test::<UnusedBlockArgument>().expect_no_offenses(indoc! {r#"
            x = 1
            [1].each do |n; x|
              puts n
            end
            puts x
        "#});
    }

    #[test]
    fn flags_multiple_unused_args() {
        test::<UnusedBlockArgument>().expect_offense(indoc! {r#"
            {a: 1}.each do |k, v|
                            ^ Unused block argument - `k`. If it's necessary, use `_` or `_k` as an argument name to indicate that it won't be used.
              puts v
            end
        "#});
    }

    #[test]
    fn no_offense_for_plain_underscore_arg() {
        test::<UnusedBlockArgument>().expect_no_offenses(indoc! {r#"
            [1].each do |_|
              puts 1
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(UnusedBlockArgument);
