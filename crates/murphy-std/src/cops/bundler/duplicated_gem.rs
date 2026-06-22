//! `Bundler/DuplicatedGem` — flag a `gem 'x'` whose name duplicates an earlier
//! `gem 'x'` entry anywhere in the Gemfile, unless every duplicate occurrence is
//! a branch of one and the same conditional (RuboCop's "conditional
//! declaration" carve-out). The cop runs only on Gemfile/gems.rb files; the host
//! applies the per-cop `Include` from `config/default.yml`, so this cop never
//! inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Bundler/DuplicatedGem
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Reproduces RuboCop's `def_node_search :gem_declarations,
//!   '(send nil? :gem str ...)'` by walking the whole AST in source order and
//!   collecting bare `gem` calls (send-only) whose first argument is a plain
//!   `Str` (parenthesized `gem ('x')` is excluded, matching the strict `str`
//!   pattern). Declarations are grouped by the *string value* of the first
//!   argument (`cx.string_str`), so `gem 'x'` and `gem "x"` collide and are
//!   flagged — matching RuboCop, which groups by structural str-node equality.
//!   Each duplicate after the first is flagged, citing the first occurrence's
//!   1-based line; the message is hardcoded `... of the Gemfile.` even for
//!   `gems.rb`, as in RuboCop. Behaviour verified case-by-case against
//!   standalone rubocop 1.87.0 (simple/triple dup, group blocks, if/elsif/else,
//!   case/when, separate-conditional exemptions, top-level + conditional).
//!
//!   Conditional carve-out: a group is exempt iff `nodes[0]`'s nearest
//!   non-`Begin` ancestor is an `if`/`when`, and every node is "within" that
//!   conditional. We mirror RuboCop's `within_conditional?` (`branch == node ||
//!   branch.child_nodes.include?(node)`) with a *source-text* comparison
//!   (`raw_source`) rather than structural AST equality — the same accepted
//!   approximation `Lint/DuplicateBranch` documents (raw_source is stricter
//!   than `Node#==`, so it can only over-flag, never wrongly-exempt).
//!
//!   Two negligible edge-case divergences (accepted, not tracked as open work):
//!   (1) same-value/different-style duplicates (e.g. `gem 'x'` vs `gem "x"`)
//!   split across *separate* conditionals — RuboCop's structural `==` exempts,
//!   raw_source flags; the same family as the `DuplicateBranch` approximation.
//!   (2) an explicit `begin...end` wrapper between a gem and its enclosing `if`
//!   — Murphy's translate layer collapses `kwbegin` into `Begin`, so the
//!   non-`Begin` ancestor walk skips it and exempts where RuboCop (treating
//!   `kwbegin` as a stopping ancestor) flags. Both are near-impossible Gemfile
//!   shapes.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct DuplicatedGem;

#[cop(
    name = "Bundler/DuplicatedGem",
    description = "Checks for duplicate gem entries in Gemfile.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DuplicatedGem {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let root = cx.root();

        // Whole-AST source-order walk → groups of `gem` declarations keyed by
        // first-argument string value. Insertion order (== source order)
        // preserved so `nodes.first` is the first occurrence.
        let mut groups: Vec<(&str, Vec<NodeId>)> = Vec::new();
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            let Some(name) = gem_declaration_name(node, cx) else {
                continue;
            };
            if let Some(entry) = groups.iter_mut().find(|(key, _)| *key == name) {
                entry.1.push(node);
            } else {
                groups.push((name, vec![node]));
            }
        }

        for (name, nodes) in &groups {
            if nodes.len() < 2 || conditional_declaration(nodes, cx) {
                continue;
            }
            let first_line = line_of(cx, cx.range(nodes[0]).start);
            for &node in &nodes[1..] {
                let message = format!(
                    "Gem `{name}` requirements already given on line {first_line} of the Gemfile."
                );
                cx.emit_offense(cx.range(node), &message, None);
            }
        }
    }
}

/// The first-argument string value of `node` if it is a bare `gem 'name'`
/// declaration — RuboCop's `(send nil? :gem str ...)`. `None` otherwise.
///
/// Deliberately does *not* unwrap parentheses on the argument: RuboCop's `str`
/// pattern does not match `gem ('x')`, so neither do we.
fn gem_declaration_name<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    // Restrict to `Send` (RuboCop's pattern is send-only). A block-form
    // `gem 'x' do…end` is a `Block` whose `method_name` delegates to `"gem"`;
    // matching only `Send` avoids double-counting the call via both the block
    // node and its inner send during the whole-AST walk.
    if !matches!(cx.kind(node), NodeKind::Send { .. }) {
        return None;
    }
    if cx.method_name(node)? != "gem" || cx.call_receiver(node).get().is_some() {
        return None;
    }
    let first_arg = *cx.call_arguments(node).first()?;
    match *cx.kind(first_arg) {
        NodeKind::Str(id) => Some(cx.string_str(id)),
        _ => None,
    }
}

/// RuboCop's `conditional_declaration?`: the duplicate group is exempt when
/// `nodes[0]`'s nearest non-`Begin` ancestor is an `if`/`when`, and every node
/// is "within" that same root conditional.
fn conditional_declaration(nodes: &[NodeId], cx: &Cx<'_>) -> bool {
    let Some(parent) = cx
        .ancestors(nodes[0])
        .find(|&a| !matches!(cx.kind(a), NodeKind::Begin(_)))
    else {
        return false;
    };

    let root_conditional = match *cx.kind(parent) {
        NodeKind::If { .. } => parent,
        NodeKind::When { .. } => match cx.parent(parent).get() {
            Some(case_node) => case_node,
            None => return false,
        },
        _ => return false,
    };

    let branches = conditional_branches(root_conditional, cx);
    nodes
        .iter()
        .all(|&node| within_conditional(node, &branches, cx))
}

/// Branch *bodies* of a root conditional, mirroring RuboCop's
/// `IfNode#branches` / `CaseNode#branches` with `.compact`:
///   - `if`: the then-body, every `elsif` body (flattened down the else-chain),
///     and the final `else` body if present (never bailing when `else` is
///     absent).
///   - `case`: each `when` body and the `else` body if present.
fn conditional_branches(root: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let mut branches = Vec::new();
    match *cx.kind(root) {
        NodeKind::If { .. } => {
            if let Some(then_body) = cx.if_branch(root).get() {
                branches.push(then_body);
            }
            let mut current = cx.if_else_branch(root).get();
            while let Some(id) = current {
                if matches!(cx.kind(id), NodeKind::If { .. }) && cx.is_elsif(id) {
                    if let Some(body) = cx.if_branch(id).get() {
                        branches.push(body);
                    }
                    current = cx.if_else_branch(id).get();
                } else {
                    branches.push(id);
                    break;
                }
            }
        }
        NodeKind::Case { .. } => {
            for &when_node in cx.case_when_branches(root) {
                if let Some(body) = cx.when_body(when_node).get() {
                    branches.push(body);
                }
            }
            if let Some(else_body) = cx.case_else_branch(root).get() {
                branches.push(else_body);
            }
        }
        _ => {}
    }
    branches
}

/// RuboCop's `within_conditional?`: `node` is within the conditional if any
/// branch body equals `node` or has `node` among its direct children. RuboCop
/// compares structurally (`Node#==`); we approximate with source text, the
/// accepted `Lint/DuplicateBranch` precedent.
fn within_conditional(node: NodeId, branches: &[NodeId], cx: &Cx<'_>) -> bool {
    let node_src = cx.raw_source(cx.range(node));
    branches.iter().any(|&branch| {
        cx.raw_source(cx.range(branch)) == node_src
            || cx
                .children(branch)
                .iter()
                .any(|&child| cx.raw_source(cx.range(child)) == node_src)
    })
}

/// 1-based source line number of the byte `offset`.
fn line_of(cx: &Cx<'_>, offset: u32) -> usize {
    cx.source()[..offset as usize].matches('\n').count() + 1
}

murphy_plugin_api::submit_cop!(DuplicatedGem);

#[cfg(test)]
mod tests {
    use super::DuplicatedGem;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_simple_duplicate() {
        test::<DuplicatedGem>().expect_offense(indoc! {r#"
            gem 'rake'
            gem 'rake'
            ^^^^^^^^^^ Gem `rake` requirements already given on line 1 of the Gemfile.
        "#});
    }

    #[test]
    fn flags_quote_style_mismatch_as_duplicate() {
        // RuboCop groups by string VALUE, so `'rake'` and `"rake"` collide.
        test::<DuplicatedGem>().expect_offense(indoc! {r#"
            gem 'rake'
            gem "rake"
            ^^^^^^^^^^ Gem `rake` requirements already given on line 1 of the Gemfile.
        "#});
    }

    #[test]
    fn triple_duplicate_flags_each_after_first_citing_first_line() {
        test::<DuplicatedGem>().expect_offense(indoc! {r#"
            gem 'x'
            gem 'x'
            ^^^^^^^ Gem `x` requirements already given on line 1 of the Gemfile.
            gem 'x'
            ^^^^^^^ Gem `x` requirements already given on line 1 of the Gemfile.
        "#});
    }

    #[test]
    fn allows_distinct_gems() {
        test::<DuplicatedGem>().expect_no_offenses(indoc! {r#"
            gem 'rake'
            gem 'rspec'
        "#});
    }

    #[test]
    fn flags_duplicate_across_group_blocks() {
        test::<DuplicatedGem>().expect_offense(indoc! {r#"
            group :test do
              gem 'x'
            end
            group :dev do
              gem 'x'
              ^^^^^^^ Gem `x` requirements already given on line 2 of the Gemfile.
            end
        "#});
    }

    #[test]
    fn allows_conditional_declaration_if_elsif_else() {
        test::<DuplicatedGem>().expect_no_offenses(indoc! {r#"
            if a
              gem 'x', path: l
            elsif b
              gem 'x', git: g
            else
              gem 'x', '~> 1'
            end
        "#});
    }

    #[test]
    fn allows_conditional_declaration_then_and_else_only() {
        // elsif present but gem-free; gems only in then + else.
        test::<DuplicatedGem>().expect_no_offenses(indoc! {r#"
            if a
              gem 'x'
            elsif b
              foo
            else
              gem 'x'
            end
        "#});
    }

    #[test]
    fn allows_conditional_declaration_elsif_and_else() {
        test::<DuplicatedGem>().expect_no_offenses(indoc! {r#"
            if a
              foo
            elsif b
              gem 'x'
            else
              gem 'x'
            end
        "#});
    }

    #[test]
    fn allows_identical_gems_in_separate_conditionals() {
        // Structurally identical gems in separate `if` blocks are exempt:
        // the second is structurally equal to the first conditional's branch.
        test::<DuplicatedGem>().expect_no_offenses(indoc! {r#"
            if a
              gem 'x'
            end
            if b
              gem 'x'
            end
        "#});
    }

    #[test]
    fn allows_identical_gems_separate_ifs_multistmt_branch() {
        // First branch is multi-statement; the second gem matches the first
        // branch's child structurally (clause-2 structural exemption).
        test::<DuplicatedGem>().expect_no_offenses(indoc! {r#"
            if a
              gem 'x'
              bar
            end
            if b
              gem 'x'
            end
        "#});
    }

    #[test]
    fn flags_differing_gems_in_separate_conditionals() {
        // Different options → not structurally equal → not exempt.
        test::<DuplicatedGem>().expect_offense(indoc! {r#"
            if a
              gem 'x', path: l
            end
            if b
              gem 'x', git: g
              ^^^^^^^^^^^^^^^ Gem `x` requirements already given on line 2 of the Gemfile.
            end
        "#});
    }

    #[test]
    fn flags_top_level_gem_duplicated_inside_conditional() {
        // nodes[0] is top-level → nearest non-Begin ancestor is the root
        // begin / none → not a conditional declaration → flag.
        test::<DuplicatedGem>().expect_offense(indoc! {r#"
            gem 'x'
            if b
              gem 'x'
              ^^^^^^^ Gem `x` requirements already given on line 1 of the Gemfile.
            end
        "#});
    }

    #[test]
    fn allows_conditional_declaration_case_when() {
        test::<DuplicatedGem>().expect_no_offenses(indoc! {r#"
            case x
            when 1
              gem 'x'
            when 2
              gem 'x'
            end
        "#});
    }

    #[test]
    fn allows_same_gem_in_same_conditional_branch() {
        // Two `gem 'x'` in one `if` branch: both within that conditional → exempt
        // (matches RuboCop, which treats a single-branch conditional as a carve-out).
        test::<DuplicatedGem>().expect_no_offenses(indoc! {r#"
            if a
              gem 'x'
              gem 'x'
            end
        "#});
    }

    #[test]
    fn ignores_non_gem_calls_and_gem_with_receiver() {
        test::<DuplicatedGem>().expect_no_offenses(indoc! {r#"
            source 'https://rubygems.org'
            source 'https://rubygems.org'
            obj.gem 'x'
            obj.gem 'x'
        "#});
    }

    #[test]
    fn ignores_gem_with_non_string_first_arg() {
        test::<DuplicatedGem>().expect_no_offenses(indoc! {r#"
            gem name
            gem name
        "#});
    }

    #[test]
    fn block_form_gem_is_not_self_duplicated() {
        // A single block-form `gem 'x' do…end` must not be counted twice
        // (once as the Block node, once as the inner Send).
        test::<DuplicatedGem>().expect_no_offenses(indoc! {r#"
            gem 'x' do
              foo
            end
        "#});
    }
}
