//! `Bundler/DuplicatedGroup` — flag a `group :x do … end` declaration whose
//! group set (and surrounding source/git/platforms/path context) duplicates an
//! earlier `group` declaration in the Gemfile. The cop runs only on
//! Gemfile/gems.rb files; the host applies the per-cop `Include` from
//! `config/default.yml`, so this cop never inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Bundler/DuplicatedGroup
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Reproduces RuboCop's `def_node_search :group_declarations,
//!   '(send nil? :group ...)'` by walking the whole AST in source order and
//!   collecting bare `group` calls (send-only) regardless of argument shape.
//!   Matching only `Send` (not the enclosing `Block`) means a block-form
//!   `group :x do…end` is counted once via its inner send, matching RuboCop's
//!   send-only search and avoiding double counting.
//!
//!   Grouping key mirrors RuboCop's `"#{source_key}#{group_attributes.sort.join}"`:
//!     - `group_attributes` maps each argument to a normalized token — a symbol
//!       uses its name without the colon (`cx.symbol_str`, == RuboCop's
//!       `value.to_s`); a string uses its value (`cx.string_str`); a hash uses
//!       its pairs' raw sources, each pair sorted and joined by `, ` (==
//!       `pairs.map(&:source).sort.join(', ')`); anything else uses raw source
//!       (== the `respond_to?(:value) ? … : source` fallback). The per-argument
//!       tokens are then sorted and joined with the EMPTY separator, exactly as
//!       RuboCop's `.sort.join`, so `group :development, :test` and
//!       `group :test, :development` collide.
//!     - `source_key` is the nearest ancestor `Block` whose method is one of
//!       `source`/`git`/`platforms`/`path`, rendered as
//!       `method_name + first_argument_source` (empty when that send has no
//!       first argument). The group's own enclosing block has method `group`
//!       (not in the set) and is correctly skipped. Differing source contexts
//!       (e.g. `platforms :ruby` vs `platforms :jruby`) yield distinct keys, so
//!       identical inner groups are NOT flagged — RuboCop's documented carve-out.
//!
//!   Each duplicate after the first is flagged. The offense range is the `group`
//!   *send* node range (== RuboCop's `node.loc.column...node.loc.last_column` on
//!   the first line), so the carets span `group :development` and exclude the
//!   ` do` block opener and body. The message cites the first occurrence's
//!   1-based line and uses `node.arguments.map(&:source).join(', ')` for the
//!   group name — RAW argument source in SOURCE order (colon included), NOT the
//!   normalized key. The message is hardcoded `… of the Gemfile.` even for
//!   `gems.rb`, as in RuboCop. Behaviour verified case-by-case against
//!   standalone rubocop 1.87.0.
//!
//!   Two negligible offense-range divergences (accepted, not tracked as open
//!   work), both near-impossible Gemfile shapes:
//!   (1) a multi-line group declaration (`group :a,\n  :b do`) — RuboCop's
//!   `source_range(buffer, first_line, column...last_column)` reconstructs a
//!   range from only the first line's columns, which truncates oddly; Murphy's
//!   range spans through the last argument across lines. (2) a parenthesized
//!   call `group(:test) do` — Murphy ends the range at the last argument
//!   (`group(:test`), dropping the closing paren that RuboCop's `last_column`
//!   includes. Both are caret-width-only differences; cop firing is identical.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct DuplicatedGroup;

#[cop(
    name = "Bundler/DuplicatedGroup",
    description = "Checks for duplicate group entries in Gemfile.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicatedGroup {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let root = cx.root();

        // Whole-AST source-order walk → groups of `group` declarations keyed by
        // source-context + normalized sorted attributes. Insertion order (==
        // source order) preserved so `nodes.first` is the first occurrence.
        let mut groups: Vec<(String, Vec<NodeId>)> = Vec::new();
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            let Some(key) = group_declaration_key(node, cx) else {
                continue;
            };
            if let Some(entry) = groups.iter_mut().find(|(k, _)| *k == key) {
                entry.1.push(node);
            } else {
                groups.push((key, vec![node]));
            }
        }

        for (_, nodes) in &groups {
            if nodes.len() < 2 {
                continue;
            }
            let first_line = line_of(cx, group_offense_range(nodes[0], cx).start);
            for &node in &nodes[1..] {
                let group_name = group_name(node, cx);
                let message = format!(
                    "Gem group `{group_name}` already defined on line {first_line} of the Gemfile."
                );
                cx.emit_offense(group_offense_range(node, cx), &message, None);
            }
        }
    }
}

/// The grouping key for a bare `group(...)` send — RuboCop's
/// `"#{source_key}#{group_attributes.sort.join}"`. `None` if `node` is not a
/// bare `group` send (RuboCop's `(send nil? :group ...)`).
fn group_declaration_key(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    // Restrict to `Send` (RuboCop's pattern is send-only). A block-form
    // `group :x do…end` is a `Block` whose `method_name` delegates to `"group"`;
    // matching only `Send` avoids double-counting the call via both the block
    // node and its inner send during the whole-AST walk.
    if !matches!(cx.kind(node), NodeKind::Send { .. }) {
        return None;
    }
    if cx.method_name(node)? != "group" || cx.call_receiver(node).get().is_some() {
        return None;
    }

    let source_key = find_source_key(node, cx);
    let mut attributes = group_attributes(node, cx);
    attributes.sort();
    // RuboCop's `.sort.join` uses the empty separator.
    Some(format!("{source_key}{}", attributes.concat()))
}

/// RuboCop's `group_attributes`: map each argument to a normalized token.
fn group_attributes(node: NodeId, cx: &Cx<'_>) -> Vec<String> {
    cx.call_arguments(node)
        .iter()
        .map(|&arg| match *cx.kind(arg) {
            // Hash arg → its pairs' raw sources, each sorted and joined by `, `
            // (RuboCop: `argument.pairs.map(&:source).sort.join(', ')`).
            NodeKind::Hash(_) => {
                let mut pairs: Vec<&str> = cx
                    .hash_pairs(arg)
                    .iter()
                    .map(|&pair| cx.raw_source(cx.range(pair)))
                    .collect();
                pairs.sort_unstable();
                pairs.join(", ")
            }
            // Symbol → name without the colon (RuboCop: `value.to_s`).
            NodeKind::Sym(sym) => cx.symbol_str(sym).to_owned(),
            // String → value (RuboCop: `value.to_s`).
            NodeKind::Str(id) => cx.string_str(id).to_owned(),
            // Anything else without a literal value → raw source.
            _ => cx.raw_source(cx.range(arg)).to_owned(),
        })
        .collect()
}

/// RuboCop's `find_source_key`: the nearest ancestor `Block` whose method is
/// one of `source`/`git`/`platforms`/`path`, rendered as
/// `method_name + first_argument_source`. Empty string if there is no such
/// ancestor (RuboCop returns `nil`, which interpolates to `""`).
fn find_source_key(node: NodeId, cx: &Cx<'_>) -> String {
    const SOURCE_BLOCK_NAMES: [&str; 4] = ["source", "git", "platforms", "path"];

    let Some(block) = cx.ancestors(node).find(|&a| {
        matches!(cx.kind(a), NodeKind::Block { .. })
            && cx
                .method_name(a)
                .is_some_and(|m| SOURCE_BLOCK_NAMES.contains(&m))
    }) else {
        return String::new();
    };

    let method = cx.method_name(block).unwrap_or("");
    let first_arg_src = cx
        .block_call(block)
        .get()
        .and_then(|send| cx.call_arguments(send).first().copied())
        .map(|arg| cx.raw_source(cx.range(arg)))
        .unwrap_or("");
    format!("{method}{first_arg_src}")
}

/// RuboCop's message `group_name`: `node.arguments.map(&:source).join(', ')` —
/// raw argument source in source order (colon included), NOT the normalized key.
fn group_name(node: NodeId, cx: &Cx<'_>) -> String {
    cx.call_arguments(node)
        .iter()
        .map(|&arg| cx.raw_source(cx.range(arg)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The offense range for a `group(...)` send — RuboCop's
/// `node.loc.column...node.loc.last_column` on the send's first line: the
/// `group` selector through the last argument, EXCLUDING the ` do…end` block.
///
/// Murphy's `cx.range` for a block's call-send spans the whole block, so we
/// reconstruct the send-only span from the selector start (`loc.name`) through
/// the end of the last argument (falling back to the selector end when the call
/// is argument-less).
fn group_offense_range(node: NodeId, cx: &Cx<'_>) -> murphy_plugin_api::Range {
    let name = cx.node(node).loc.name;
    let end = cx
        .call_arguments(node)
        .last()
        .map(|&arg| cx.range(arg).end)
        .unwrap_or(name.end);
    murphy_plugin_api::Range { start: name.start, end }
}

/// 1-based source line number of the byte `offset`.
fn line_of(cx: &Cx<'_>, offset: u32) -> usize {
    cx.source()[..offset as usize].matches('\n').count() + 1
}

murphy_plugin_api::submit_cop!(DuplicatedGroup);

#[cfg(test)]
mod tests {
    use super::DuplicatedGroup;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_simple_duplicate_group() {
        test::<DuplicatedGroup>().expect_offense(indoc! {r#"
            group :development do
              gem 'rubocop'
            end

            group :development do
            ^^^^^^^^^^^^^^^^^^ Gem group `:development` already defined on line 1 of the Gemfile.
              gem 'rubocop-rails'
            end
        "#});
    }

    #[test]
    fn flags_same_group_set_declared_twice_regardless_of_order() {
        // RuboCop normalizes group sets: `:development, :test` == `:test, :development`.
        test::<DuplicatedGroup>().expect_offense(indoc! {r#"
            group :development, :test do
              gem 'rubocop'
            end

            group :test, :development do
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Gem group `:test, :development` already defined on line 1 of the Gemfile.
              gem 'rspec'
            end
        "#});
    }

    #[test]
    fn allows_distinct_groups() {
        test::<DuplicatedGroup>().expect_no_offenses(indoc! {r#"
            group :development do
              gem 'rubocop'
            end

            group :development, :test do
              gem 'rspec'
            end
        "#});
    }

    #[test]
    fn allows_hash_option_in_one_group_only() {
        // `group :test, optional: true` vs `group :test` → different attributes
        // (hash present in one only) → distinct keys → no offense.
        test::<DuplicatedGroup>().expect_no_offenses(indoc! {r#"
            group :test, optional: true do
              gem 'a'
            end

            group :test do
              gem 'b'
            end
        "#});
    }

    #[test]
    fn flags_identical_groups_with_matching_hash_options() {
        test::<DuplicatedGroup>().expect_offense(indoc! {r#"
            group :test, optional: true do
              gem 'a'
            end

            group :test, optional: true do
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Gem group `:test, optional: true` already defined on line 1 of the Gemfile.
              gem 'b'
            end
        "#});
    }

    #[test]
    fn allows_identical_groups_under_different_source_contexts() {
        // The docstring example: same inner group under `platforms :ruby` vs
        // `platforms :jruby` → distinct source_key → no offense.
        test::<DuplicatedGroup>().expect_no_offenses(indoc! {r#"
            platforms :ruby do
              group :default do
                gem 'openssl'
              end
            end

            platforms :jruby do
              group :default do
                gem 'jruby-openssl'
              end
            end
        "#});
    }

    #[test]
    fn flags_identical_groups_under_same_source_context() {
        test::<DuplicatedGroup>().expect_offense(indoc! {r#"
            platforms :ruby do
              group :default do
                gem 'openssl'
              end

              group :default do
              ^^^^^^^^^^^^^^ Gem group `:default` already defined on line 2 of the Gemfile.
                gem 'other'
              end
            end
        "#});
    }

    #[test]
    fn triple_duplicate_flags_each_after_first_citing_first_line() {
        test::<DuplicatedGroup>().expect_offense(indoc! {r#"
            group :test do
              gem 'a'
            end
            group :test do
            ^^^^^^^^^^^ Gem group `:test` already defined on line 1 of the Gemfile.
              gem 'b'
            end
            group :test do
            ^^^^^^^^^^^ Gem group `:test` already defined on line 1 of the Gemfile.
              gem 'c'
            end
        "#});
    }

    #[test]
    fn block_form_group_is_not_self_duplicated() {
        // A single block-form `group :x do…end` must not be counted twice
        // (once as the Block node, once as the inner Send).
        test::<DuplicatedGroup>().expect_no_offenses(indoc! {r#"
            group :test do
              gem 'a'
            end
        "#});
    }

    #[test]
    fn ignores_group_with_receiver() {
        test::<DuplicatedGroup>().expect_no_offenses(indoc! {r#"
            obj.group :test do
              gem 'a'
            end
            obj.group :test do
              gem 'b'
            end
        "#});
    }
}
