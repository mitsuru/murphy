//! `Bundler/OrderedGems` — gems within a section of the Gemfile should be
//! listed in alphabetical order. RuboCop compares each pair of *consecutive*
//! `gem` declarations (in source order) that sit on adjacent lines; an offense
//! is registered on the second gem of a pair when it sorts before the first.
//! The cop runs only on Gemfile/gems.rb files; the host applies the per-cop
//! `Include` from `config/default.yml`, so this cop never inspects the filename
//! itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Bundler/OrderedGems
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection is at full RuboCop parity; the only gap is autocorrect (see
//!   below), which is intentionally not ported (issue murphy-e7bz.7 scopes this
//!   cop as "safe / no-autocorrect"). The same convention as
//!   `Style/RedundantBegin` (status: partial, autocorrect documented as a gap).
//!
//!   Reproduces RuboCop's `def_node_search :gem_declarations,
//!   '(send nil? :gem str ...)'` by walking the whole AST in source order and
//!   collecting bare `gem` calls (send-only) whose first argument is a plain
//!   `Str` (parenthesized `gem ('x')` is excluded, matching the strict `str`
//!   pattern). RuboCop's `find_gem_name` recurses into the receiver when the
//!   first argument is not a string, but the `gem_declarations` pattern already
//!   constrains the first argument to `str`, so the name is always that string.
//!
//!   Declarations are walked with `each_cons(2)`. For each consecutive pair
//!   (previous, current):
//!   - `consecutive_lines?`: `previous.last_line == current_first_line - 1`,
//!     where `previous.last_line` is the source line of `cx.range(previous).end`
//!     (so a multi-line previous declaration is handled correctly) and
//!     `current_first_line` is computed via `get_source_range`.
//!   - When out of order (`canonical(current) < canonical(previous)`), the
//!     offense is registered on the *current* (second) node, spanning its whole
//!     range, with the message swapping the names:
//!     "... Gem `{current}` should appear before `{previous}`."
//!
//!   `get_source_range` reproduces RuboCop's inverted `unless
//!   comments_as_separators` logic. With `TreatCommentsAsGroupSeparators: true`
//!   (default), the current node's own start line is used, so a comment between
//!   two gems breaks the line-adjacency and the gems are treated as separate
//!   sections (no offense). With `TreatCommentsAsGroupSeparators: false`, the
//!   first line of the contiguous block of own-line comments directly above the
//!   current node is used instead (RuboCop's `ast_with_comments[node].first`),
//!   so a comment no longer separates the gems. This own-line comment scan
//!   approximates Parser's `associate_locations`; it matches RuboCop on every
//!   verified case (single/multiple preceding comments, blank-line-then-comment,
//!   trailing inline comment on the previous gem's line), and any residual
//!   divergence is the same accepted family as other comment-association
//!   approximations in this repo.
//!
//!   `gem_canonical_name`: with `ConsiderPunctuation: false` (default), `-` and
//!   `_` are stripped before the case-insensitive comparison; with `true` they
//!   are kept. Behaviour verified case-by-case against standalone rubocop
//!   1.87.0 (simple out-of-order, in-order, comment separator default/flipped,
//!   blank-line separator, group-block boundary, multi-line previous, both
//!   options).
//!
//!   GAP — autocorrect: RuboCop is `[Correctable]` (reorders the gem lines via
//!   `OrderedGemCorrector`). Murphy emits offense-only; reordering whole source
//!   lines (and carrying their attached comments) is a line-rewriting
//!   correction that the offense already guides the user to perform.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Options for [`OrderedGems`].
#[derive(CopOptions)]
pub struct OrderedGemsOptions {
    #[option(
        name = "TreatCommentsAsGroupSeparators",
        default = true,
        description = "Treat comments between gems as separating distinct sections."
    )]
    pub treat_comments_as_group_separators: bool,
    #[option(
        name = "ConsiderPunctuation",
        default = false,
        description = "Consider `-` and `_` when comparing gem names for ordering."
    )]
    pub consider_punctuation: bool,
}

#[derive(Default)]
pub struct OrderedGems;

#[cop(
    name = "Bundler/OrderedGems",
    description = "Gems within groups in the Gemfile should be alphabetically sorted.",
    default_severity = "warning",
    default_enabled = true,
    options = OrderedGemsOptions
)]
impl OrderedGems {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<OrderedGemsOptions>();
        let root = cx.root();

        // Whole-AST source-order walk → bare `gem 'name'` declarations.
        // Sorted by range.start so `each_cons(2)` sees true source order even if
        // the descendant walk yields a parent-ish node ahead of a sibling.
        let mut declarations: Vec<(NodeId, &str)> = std::iter::once(root)
            .chain(cx.descendants(root))
            .filter_map(|node| gem_declaration_name(node, cx).map(|name| (node, name)))
            .collect();
        declarations.sort_by_key(|(node, _)| cx.range(*node).start);

        // 1-based source lines of own-line comments, used to walk the contiguous
        // comment block above a node when comments are not separators.
        let comment_lines: Vec<usize> = own_line_comment_lines(cx);

        for pair in declarations.windows(2) {
            let (previous, prev_name) = pair[0];
            let (current, cur_name) = pair[1];

            let previous_last_line = line_of(cx, cx.range(previous).end);
            let current_first_line = source_range_first_line(current, &comment_lines, &opts, cx);
            if previous_last_line != current_first_line.saturating_sub(1) {
                continue;
            }

            if !case_insensitive_out_of_order(cur_name, prev_name, &opts) {
                continue;
            }

            let message = format!(
                "Gems should be sorted in an alphabetical order within their \
                 section of the Gemfile. Gem `{cur_name}` should appear before `{prev_name}`."
            );
            cx.emit_offense(cx.range(current), &message, None);
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
    // matching only `Send` avoids double-counting the call via the block node.
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

/// RuboCop's `case_insensitive_out_of_order?(current, previous)` =
/// `canonical(current) < canonical(previous)`.
fn case_insensitive_out_of_order(
    current: &str,
    previous: &str,
    opts: &OrderedGemsOptions,
) -> bool {
    canonical_name(current, opts) < canonical_name(previous, opts)
}

/// RuboCop's `gem_canonical_name`: lowercase, with `-`/`_` stripped unless
/// `ConsiderPunctuation` is set.
fn canonical_name(name: &str, opts: &OrderedGemsOptions) -> String {
    let filtered: String = if opts.consider_punctuation {
        name.to_owned()
    } else {
        name.chars().filter(|&c| c != '-' && c != '_').collect()
    };
    filtered.to_lowercase()
}

/// RuboCop's `get_source_range(node, comments_as_separators).first_line`.
///
/// `unless comments_as_separators` — when comments are *not* separators, the
/// first line of the contiguous block of own-line comments directly above the
/// node is used (RuboCop's `ast_with_comments[node].first`); otherwise the
/// node's own start line.
fn source_range_first_line(
    node: NodeId,
    comment_lines: &[usize],
    opts: &OrderedGemsOptions,
    cx: &Cx<'_>,
) -> usize {
    let node_line = line_of(cx, cx.range(node).start);
    if opts.treat_comments_as_group_separators {
        return node_line;
    }
    // Walk upward over the contiguous block of own-line comments directly above.
    let mut first_line = node_line;
    while first_line > 1 && comment_lines.contains(&(first_line - 1)) {
        first_line -= 1;
    }
    first_line
}

/// 1-based source lines of own-line comments (the first non-whitespace
/// character on the line is `#`). These are the lines RuboCop's
/// `ast_with_comments` can attach to a following node.
fn own_line_comment_lines(cx: &Cx<'_>) -> Vec<usize> {
    let source = cx.source();
    cx.comments()
        .iter()
        .filter(|comment| {
            let start = comment.range.start as usize;
            let line_start = source[..start].rfind('\n').map_or(0, |pos| pos + 1);
            source[line_start..start].chars().all(char::is_whitespace)
        })
        .map(|comment| line_of(cx, comment.range.start))
        .collect()
}

/// 1-based source line number of the byte `offset`.
fn line_of(cx: &Cx<'_>, offset: u32) -> usize {
    cx.source()[..offset as usize].matches('\n').count() + 1
}

murphy_plugin_api::submit_cop!(OrderedGems);

#[cfg(test)]
mod tests {
    use super::{OrderedGems, OrderedGemsOptions};
    use murphy_plugin_api::CopOptions;
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(treat_comments: bool, consider_punctuation: bool) -> OrderedGemsOptions {
        OrderedGemsOptions {
            treat_comments_as_group_separators: treat_comments,
            consider_punctuation,
        }
    }

    #[test]
    fn flags_simple_out_of_order_pair() {
        // Offense on the SECOND gem; message swaps the names.
        test::<OrderedGems>().expect_offense(indoc! {r#"
            gem 'rspec'
            gem 'rake'
            ^^^^^^^^^^ Gems should be sorted in an alphabetical order within their section of the Gemfile. Gem `rake` should appear before `rspec`.
        "#});
    }

    #[test]
    fn allows_alphabetically_sorted_gems() {
        test::<OrderedGems>().expect_no_offenses(indoc! {r#"
            gem 'rake'
            gem 'rspec'
        "#});
    }

    #[test]
    fn flags_each_consecutive_out_of_order_pair() {
        // rspec, rake, pry → (rake<rspec) and (pry<rake) both flagged.
        test::<OrderedGems>().expect_offense(indoc! {r#"
            gem 'rspec'
            gem 'rake'
            ^^^^^^^^^^ Gems should be sorted in an alphabetical order within their section of the Gemfile. Gem `rake` should appear before `rspec`.
            gem 'pry'
            ^^^^^^^^^ Gems should be sorted in an alphabetical order within their section of the Gemfile. Gem `pry` should appear before `rake`.
        "#});
    }

    #[test]
    fn comment_separates_sections_by_default() {
        // TreatCommentsAsGroupSeparators=true (default): a comment between the
        // two gems breaks line-adjacency → separate sections → no offense.
        test::<OrderedGems>().expect_no_offenses(indoc! {r#"
            gem 'rspec'
            # a comment
            gem 'rake'
        "#});
    }

    #[test]
    fn comment_does_not_separate_when_option_disabled() {
        // TreatCommentsAsGroupSeparators=false: the comment is attached to the
        // current gem, restoring adjacency → out of order → offense.
        test::<OrderedGems>()
            .with_options(&opts(false, false))
            .expect_offense(indoc! {r#"
                gem 'rspec'
                # a comment
                gem 'rake'
                ^^^^^^^^^^ Gems should be sorted in an alphabetical order within their section of the Gemfile. Gem `rake` should appear before `rspec`.
            "#});
    }

    #[test]
    fn multiple_preceding_comments_attach_to_current_when_option_disabled() {
        // Two contiguous comments above `rake`; the topmost is line 2, prev is
        // line 1 → 1 == 2 - 1 → adjacent → offense.
        test::<OrderedGems>()
            .with_options(&opts(false, false))
            .expect_offense(indoc! {r#"
                gem 'rspec'
                # c1
                # c2
                gem 'rake'
                ^^^^^^^^^^ Gems should be sorted in an alphabetical order within their section of the Gemfile. Gem `rake` should appear before `rspec`.
            "#});
    }

    #[test]
    fn blank_line_before_comment_breaks_adjacency_when_option_disabled() {
        // Blank line (line 2) between previous gem and the comment (line 3):
        // first_line=3, prev.last=1, 1 != 3-1 → no offense.
        test::<OrderedGems>()
            .with_options(&opts(false, false))
            .expect_no_offenses(indoc! {r#"
                gem 'rspec'

                # c
                gem 'rake'
            "#});
    }

    #[test]
    fn trailing_inline_comment_on_previous_does_not_pull_current_up() {
        // Inline comment on the previous gem's line is not an own-line comment,
        // so it never attaches to current; the gems stay adjacent and flagged
        // even with the option disabled.
        test::<OrderedGems>()
            .with_options(&opts(false, false))
            .expect_offense(indoc! {r#"
                gem 'rspec' # inline
                gem 'rake'
                ^^^^^^^^^^ Gems should be sorted in an alphabetical order within their section of the Gemfile. Gem `rake` should appear before `rspec`.
            "#});
    }

    #[test]
    fn blank_line_separates_sections() {
        // A blank line breaks line-adjacency → separate sections → no offense.
        test::<OrderedGems>().expect_no_offenses(indoc! {r#"
            gem 'rspec'

            gem 'rake'
        "#});
    }

    #[test]
    fn gems_in_separate_group_blocks_are_not_compared() {
        // The `end`/`group` lines break line-adjacency, so cross-group gems are
        // never compared.
        test::<OrderedGems>().expect_no_offenses(indoc! {r#"
            group :a do
              gem 'rspec'
            end
            group :b do
              gem 'rake'
            end
        "#});
    }

    #[test]
    fn flags_out_of_order_within_same_group() {
        test::<OrderedGems>().expect_offense(indoc! {r#"
            group :a do
              gem 'rspec'
              gem 'rake'
              ^^^^^^^^^^ Gems should be sorted in an alphabetical order within their section of the Gemfile. Gem `rake` should appear before `rspec`.
            end
        "#});
    }

    #[test]
    fn multi_line_previous_declaration_uses_its_last_line() {
        // `rspec` spans lines 1-2; `rake` on line 3. prev.last=2, cur.first=3,
        // 2 == 3-1 → adjacent → offense.
        test::<OrderedGems>().expect_offense(indoc! {r#"
            gem 'rspec',
                require: false
            gem 'rake'
            ^^^^^^^^^^ Gems should be sorted in an alphabetical order within their section of the Gemfile. Gem `rake` should appear before `rspec`.
        "#});
    }

    #[test]
    fn punctuation_ignored_by_default() {
        // ConsiderPunctuation=false: `a-b` → `ab`, equal to `ab`; `'ab' < 'ab'`
        // is false → no offense.
        test::<OrderedGems>().expect_no_offenses(indoc! {r#"
            gem 'a-b'
            gem 'ab'
        "#});
    }

    #[test]
    fn punctuation_considered_when_option_enabled() {
        // ConsiderPunctuation=true: `'ab' < 'a-b'`? position 1 `b` > `-` → false
        // → no offense. Mirror RuboCop (verified: no offense).
        test::<OrderedGems>()
            .with_options(&opts(true, true))
            .expect_no_offenses(indoc! {r#"
                gem 'a-b'
                gem 'ab'
            "#});
    }

    #[test]
    fn punctuation_considered_flags_when_truly_out_of_order() {
        // ConsiderPunctuation=true: `gem 'a-c'` then `gem 'ab'`. canonical:
        // 'ab' < 'a-c'? pos1 'b'(0x62) vs '-'(0x2d): 'b' > '-' → false. Use a
        // clearer case: 'b' then 'a-z' → 'a-z' < 'b' → offense.
        test::<OrderedGems>()
            .with_options(&opts(true, true))
            .expect_offense(indoc! {r#"
                gem 'b'
                gem 'a-z'
                ^^^^^^^^^ Gems should be sorted in an alphabetical order within their section of the Gemfile. Gem `a-z` should appear before `b`.
            "#});
    }

    #[test]
    fn case_insensitive_comparison() {
        // `Rake` vs `rspec`: canonical 'rake' < 'rspec' → in order → no offense.
        test::<OrderedGems>().expect_no_offenses(indoc! {r#"
            gem 'Rake'
            gem 'rspec'
        "#});
    }

    #[test]
    fn ignores_non_gem_calls_and_gem_with_receiver() {
        test::<OrderedGems>().expect_no_offenses(indoc! {r#"
            source 'https://rubygems.org'
            obj.gem 'rspec'
            obj.gem 'rake'
        "#});
    }

    #[test]
    fn ignores_gem_with_non_string_first_arg() {
        test::<OrderedGems>().expect_no_offenses(indoc! {r#"
            gem b
            gem a
        "#});
    }

    #[test]
    fn options_roundtrip_defaults() {
        let json = br#"{"TreatCommentsAsGroupSeparators":true,"ConsiderPunctuation":false}"#;
        let o = <OrderedGemsOptions as CopOptions>::from_config_json(json)
            .expect("defaults must decode");
        assert!(o.treat_comments_as_group_separators);
        assert!(!o.consider_punctuation);
    }
}
