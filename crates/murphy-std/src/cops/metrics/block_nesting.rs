//! `Metrics/BlockNesting` — flag conditional/looping constructs nested more
//! deeply than `Max` levels.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Metrics/BlockNesting
//! upstream_version_checked: 1.87.0
//! version_added: "0.25"
//! version_changed: "1.65"
//! safe: true
//! supports_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Faithful port of RuboCop's recursive `check_nesting_level` walk, driven
//!   from the AST root via `on_new_investigation` (RuboCop's hook of the same
//!   name). The walk threads `current_level` and an `ignored` flag as value
//!   parameters down each branch.
//!
//!   `consider_node?` counts a node when it is one of RuboCop's
//!   `NESTING_BLOCKS` (`case`, `case_match`, `if`, `while`, `while_post`,
//!   `until`, `until_post`, `for`, `resbody`) — Murphy folds `while_post`/
//!   `until_post` into `While`/`Until` with `post: true`, both of which still
//!   count — or, when `CountBlocks` is `true`, any block type
//!   (`block`/`numblock`/`itblock`).
//!
//!   `count_if_block?` decides whether a considered node increments the level:
//!   non-`if`-type nodes always increment; an `if`-type node (RuboCop's `if`
//!   covers `if`/`unless`/`elsif`/ternary) increments unless it is an `elsif`
//!   (chained `else if`, which shares its parent's level), and a modifier-form
//!   `if`/`unless` increments only when `CountModifierForms` is `true`.
//!   Modifier-form loops are *not* if-types, so they always increment
//!   regardless of `CountModifierForms`, matching RuboCop.
//!
//!   When `current_level` first exceeds `Max`, the offense fires on the whole
//!   offending node (RuboCop's `add_offense(node)`), and that subtree is then
//!   marked ignored so deeper exceeding descendants are suppressed (RuboCop's
//!   `ignore_node` / `part_of_ignored_node?`). Sibling branches that each
//!   exceed `Max` independently are each flagged.
//!
//!   Message: "Avoid more than 3 levels of block nesting." (interpolates the
//!   configured `Max`, never the actual depth — verbatim with RuboCop, no
//!   `[n/max]` suffix).
//!
//!   No autocorrect: RuboCop does not autocorrect this cop.
//!
//!   Note: the recursive walk uses `cx.children`, which allocates a `Vec` per
//!   node (the children are assembled from disparate `NodeKind` fields, so
//!   there is no borrowable slice). This is accepted for a faithful port.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (Max: 3)
//! if a
//!   if b
//!     if c
//!       if d            # 4th level → offense
//!         do_something
//!       end
//!     end
//!   end
//! end
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct BlockNesting;

/// Options for [`BlockNesting`]. All three keys match RuboCop's defaults.
#[derive(CopOptions)]
pub struct BlockNestingOptions {
    #[option(
        name = "Max",
        default = 3,
        description = "Maximum level of nesting allowed for conditional/looping constructs."
    )]
    pub max: i64,
    #[option(
        name = "CountBlocks",
        default = false,
        description = "Count `{}`/`do` blocks toward the nesting level."
    )]
    pub count_blocks: bool,
    #[option(
        name = "CountModifierForms",
        default = false,
        description = "Count modifier-form `if`/`unless` toward the nesting level."
    )]
    pub count_modifier_forms: bool,
}

#[cop(
    name = "Metrics/BlockNesting",
    description = "Avoid excessive block nesting.",
    default_severity = "warning",
    default_enabled = true,
    options = BlockNestingOptions,
)]
impl BlockNesting {
    /// RuboCop `on_new_investigation`: walk the AST from the root.
    #[on_new_investigation]
    fn investigate(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<BlockNestingOptions>();
        check_nesting_level(cx.root(), opts.max, 0, false, &opts, cx);
    }
}

/// RuboCop `check_nesting_level(node, max, current_level)`, with an extra
/// `ignored` flag replacing RuboCop's mutable `ignore_node` set (an ancestor
/// already exceeded `Max`, so this subtree's offenses are suppressed —
/// `part_of_ignored_node?`).
fn check_nesting_level(
    node: NodeId,
    max: i64,
    mut current_level: i64,
    mut ignored: bool,
    opts: &BlockNestingOptions,
    cx: &Cx<'_>,
) {
    if consider_node(node, opts, cx) {
        if count_if_block(node, opts, cx) {
            current_level += 1;
        }
        if current_level > max && !ignored {
            let message = format!("Avoid more than {max} levels of block nesting.");
            cx.emit_offense(cx.range(node), &message, None);
            // RuboCop `ignore_node(node)`: suppress deeper offenses in this
            // subtree.
            ignored = true;
        }
    }

    for child in cx.children(node) {
        check_nesting_level(child, max, current_level, ignored, opts, cx);
    }
}

/// RuboCop `consider_node?`: `NESTING_BLOCKS.include?(node.type)` or, when
/// `CountBlocks` is set, `node.any_block_type?`. `while_post`/`until_post`
/// fold into `While`/`Until` with `post: true` in Murphy and still count.
fn consider_node(node: NodeId, opts: &BlockNestingOptions, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::If { .. }
            | NodeKind::While { .. }
            | NodeKind::Until { .. }
            | NodeKind::For { .. }
            | NodeKind::Case { .. }
            | NodeKind::CaseMatch { .. }
            | NodeKind::Resbody { .. }
    ) || (opts.count_blocks && cx.is_any_block_type(node))
}

/// RuboCop `count_if_block?`:
/// - non-`if`-type nodes always count;
/// - an `elsif` never counts (shares the parent chain's level);
/// - a modifier-form `if`/`unless` counts only when `CountModifierForms`;
/// - any other `if`-type counts.
///
/// RuboCop's `if_type?` is the `If` `NodeKind` in Murphy — it folds `if`,
/// `unless`, `elsif`, and the ternary `?:` into one variant — so the gate keys
/// on the `If` kind, not `cx.is_if` (which would miss `unless`/ternary).
fn count_if_block(node: NodeId, opts: &BlockNestingOptions, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::If { .. }) {
        return true;
    }
    if cx.is_elsif(node) {
        return false;
    }
    if cx.is_modifier_form(node) {
        return opts.count_modifier_forms;
    }
    true
}

murphy_plugin_api::submit_cop!(BlockNesting);

#[cfg(test)]
mod tests {
    use super::{BlockNesting, BlockNestingOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(max: i64, count_blocks: bool, count_modifier_forms: bool) -> BlockNestingOptions {
        BlockNestingOptions {
            max,
            count_blocks,
            count_modifier_forms,
        }
    }

    #[test]
    fn flags_fourth_if_level() {
        // `if d` is the 4th nesting level; offense on the whole single-line
        // block-form node (`if d then bar end`, cols 7..24). Matches RuboCop.
        test::<BlockNesting>().expect_offense(indoc! {"
            if a
              if b
                if c
                  if d then bar end
                  ^^^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                end
              end
            end
        "});
    }

    #[test]
    fn flags_ternary_as_if_level() {
        // RuboCop folds ternary into `if`; the ternary is the 4th level.
        test::<BlockNesting>().expect_offense(indoc! {"
            if a
              if b
                if c
                  d ? e : f
                  ^^^^^^^^^ Avoid more than 3 levels of block nesting.
                end
              end
            end
        "});
    }

    #[test]
    fn flags_unless_level() {
        // `unless` is an `if`-type and counts like `if`.
        test::<BlockNesting>().expect_offense(indoc! {"
            if a
              if b
                if c
                  unless d then bar end
                  ^^^^^^^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                end
              end
            end
        "});
    }

    #[test]
    fn flags_while_level() {
        test::<BlockNesting>().expect_offense(indoc! {"
            if a
              if b
                if c
                  while d; bar; end
                  ^^^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                end
              end
            end
        "});
    }

    #[test]
    fn flags_until_level() {
        test::<BlockNesting>().expect_offense(indoc! {"
            if a
              if b
                if c
                  until d; bar; end
                  ^^^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                end
              end
            end
        "});
    }

    #[test]
    fn flags_for_level() {
        test::<BlockNesting>().expect_offense(indoc! {"
            if a
              if b
                if c
                  for x in y; z; end
                  ^^^^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                end
              end
            end
        "});
    }

    #[test]
    fn flags_case_level() {
        test::<BlockNesting>().expect_offense(indoc! {"
            if a
              if b
                if c
                  case d; when 1 then e; end
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                end
              end
            end
        "});
    }

    #[test]
    fn flags_case_match_level() {
        // `case/in` pattern match (RuboCop `case_match`) counts like `case`.
        test::<BlockNesting>().expect_offense(indoc! {"
            if a
              if b
                if c
                  case d; in 1 then e; end
                  ^^^^^^^^^^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                end
              end
            end
        "});
    }

    #[test]
    fn resbody_counts_toward_nesting() {
        // `if a`(1) + `if b`(2) + resbody(3) + `if z`(4) → offense on `if z`.
        test::<BlockNesting>().expect_offense(indoc! {"
            if a
              if b
                begin
                  x
                rescue
                  if z then w end
                  ^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                end
              end
            end
        "});
    }

    #[test]
    fn accepts_third_level() {
        test::<BlockNesting>().expect_no_offenses(indoc! {"
            if a
              if b
                if c
                  do_something
                end
              end
            end
        "});
    }

    #[test]
    fn elsif_does_not_increment() {
        // The `elsif` chain shares the level of its `if`; nothing exceeds.
        test::<BlockNesting>().expect_no_offenses(indoc! {"
            if a
              if b
                if c
                  x
                elsif d
                  y
                end
              end
            end
        "});
    }

    #[test]
    fn modifier_form_not_counted_by_default() {
        // `bar if d` at would-be level 4 does not count (CountModifierForms
        // defaults to false).
        test::<BlockNesting>().expect_no_offenses(indoc! {"
            if a
              if b
                if c
                  bar if d
                end
              end
            end
        "});
    }

    #[test]
    fn modifier_form_counted_when_configured() {
        test::<BlockNesting>()
            .with_options(&opts(3, false, true))
            .expect_offense(indoc! {"
                if a
                  if b
                    if c
                      bar if d
                      ^^^^^^^^ Avoid more than 3 levels of block nesting.
                    end
                  end
                end
            "});
    }

    #[test]
    fn blocks_not_counted_by_default() {
        // `each do` blocks do not count (CountBlocks defaults to false).
        test::<BlockNesting>().expect_no_offenses(indoc! {"
            [1].each do |a|
              [2].each do |b|
                [3].each do |c|
                  [4].each do |d|
                    x
                  end
                end
              end
            end
        "});
    }

    #[test]
    fn blocks_counted_when_configured() {
        // With CountBlocks: true the 4th `each do` block exceeds Max.
        test::<BlockNesting>()
            .with_options(&opts(3, true, false))
            .expect_offense(indoc! {"
                [1].each do |a|
                  [2].each do |b|
                    [3].each do |c|
                      [4].each { |d| x }
                      ^^^^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                    end
                  end
                end
            "});
    }

    #[test]
    fn deeper_descendants_are_suppressed() {
        // Only the shallowest exceeder (`if d`, level 4) is flagged; the inner
        // `if e` (level 5) is suppressed (RuboCop `ignore_node` /
        // `part_of_ignored_node?`). Single-line so the caret stays on one line.
        test::<BlockNesting>().expect_offense(indoc! {"
            if a
              if b
                if c
                  if d then (if e then x end) end
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                end
              end
            end
        "});
    }

    #[test]
    fn sibling_branches_each_flagged() {
        // Two separate branches at level 4 each get their own offense.
        test::<BlockNesting>().expect_offense(indoc! {"
            if a
              if b
                if c
                  if d1 then x end
                  ^^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                  if d2 then y end
                  ^^^^^^^^^^^^^^^^ Avoid more than 3 levels of block nesting.
                end
              end
            end
        "});
    }

    #[test]
    fn custom_max_is_honored() {
        test::<BlockNesting>()
            .with_options(&opts(1, false, false))
            .expect_offense(indoc! {"
                if a
                  if b then c end
                  ^^^^^^^^^^^^^^^ Avoid more than 1 levels of block nesting.
                end
            "});
    }
}
