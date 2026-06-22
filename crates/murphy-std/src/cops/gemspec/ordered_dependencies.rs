//! `Gemspec/OrderedDependencies` — dependency declarations in a gemspec
//! (`spec.add_dependency`, `spec.add_runtime_dependency`,
//! `spec.add_development_dependency`) should be listed in alphabetical order
//! within their section. RuboCop compares each pair of *consecutive* dependency
//! declarations (in source order) that sit on adjacent lines and use the *same*
//! `add_*` method; an offense is registered on the second declaration of a pair
//! when its name sorts before the first. The cop runs only on `*.gemspec` files;
//! the host applies the per-cop `Include` from `config/default.yml`, so this cop
//! never inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Gemspec/OrderedDependencies
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection is at full RuboCop parity; the only gap is autocorrect (see
//!   below), which is intentionally not ported (the issue scopes this cop as
//!   "safe / no-autocorrect"), mirroring the sibling `Bundler/OrderedGems`
//!   (status: partial, autocorrect documented as a gap).
//!
//!   Reproduces RuboCop's `def_node_search :dependency_declarations,
//!   '(send (lvar _) {:add_dependency :add_runtime_dependency
//!   :add_development_dependency} (str _) ...)'` by walking the whole AST in
//!   source order and collecting calls whose receiver is a plain local variable
//!   (`Lvar`), whose method is one of the three `add_*` dependency methods, and
//!   whose first argument is a plain `Str`. The receiver and argument are NOT
//!   unwrapped: RuboCop's `(lvar _)` / `(str _)` are strict, so a non-lvar
//!   receiver (`self.add_dependency`, `Foo.add_dependency`, or a bare
//!   `add_dependency` whose implicit receiver is `(send :spec nil)`) and a
//!   parenthesized / non-string first argument are all excluded — same precedent
//!   as `Bundler/DuplicatedGem`. RuboCop's `find_gem_name` recurses into the
//!   receiver when the first argument is not a string, but the
//!   `dependency_declarations` pattern already constrains the first argument to
//!   `str`, so the name is always that string.
//!
//!   Declarations are walked with `each_cons(2)`. For each consecutive pair
//!   (previous, current), three filters (all must hold to flag):
//!   - `consecutive_lines?`: `previous.last_line == current_first_line - 1`,
//!     where `previous.last_line` is the source line of `cx.range(previous).end`
//!     (so a multi-line previous declaration is handled) and `current_first_line`
//!     comes from `get_source_range`.
//!   - `case_insensitive_out_of_order?(current, previous)`:
//!     `canonical(current) < canonical(previous)`.
//!   - same method: `get_dependency_name(previous) == get_dependency_name(current)`
//!     — i.e. both use the same `add_*` selector. Different methods (or
//!     interleaved declarations of different methods) are never compared, which
//!     is why the flat-list `windows(2)` walk — not a per-method pre-grouping —
//!     is the faithful port: each_cons pairs only adjacent declarations.
//!
//!   When all three hold, the offense is registered on the *current* (second)
//!   node, spanning its whole range, with the message swapping the names
//!   (`previous: gem_name(current)`, `current: gem_name(previous)`):
//!   "... Dependency `{current_name}` should appear before `{previous_name}`."
//!
//!   `get_source_range` reproduces RuboCop's inverted `unless
//!   comments_as_separators` logic. With `TreatCommentsAsGroupSeparators: true`
//!   (default), the current node's own start line is used, so a comment between
//!   two declarations breaks line-adjacency and they are treated as separate
//!   sections (no offense). With `TreatCommentsAsGroupSeparators: false`, the
//!   first line of the contiguous block of own-line comments directly above the
//!   current node is used instead (RuboCop's `ast_with_comments[node].first`),
//!   so a comment no longer separates them. This own-line comment scan
//!   approximates Parser's `associate_locations`, the same accepted approximation
//!   as `Bundler/OrderedGems`.
//!
//!   `gem_canonical_name`: with `ConsiderPunctuation: false` (default), `-` and
//!   `_` are stripped before the case-insensitive comparison; with `true` they
//!   are kept. Behaviour verified case-by-case against standalone rubocop 1.87.0
//!   (simple out-of-order, in-order, comment separator default/flipped, blank
//!   line, multi-line previous, different-method non-comparison, interleaved
//!   methods, lvar-only receiver, bare/self receiver exclusion, both options).
//!
//!   Severity: RuboCop reports `convention` for this cop (no `Severity:` line in
//!   default.yml → framework default). Murphy's `Severity` enum is binary
//!   (`Warning`/`Error`) — there is no `convention` variant — so the JSON
//!   contract reports `severity: warning`, while the human-readable format still
//!   prints the `C` department letter. `default_severity = "warning"` is the
//!   correct and only valid macro value. This convention→`warning` JSON mapping
//!   is framework-wide (every convention/refactor-level RuboCop cop behaves the
//!   same way), not a cop-specific gap.
//!
//!   GAP — autocorrect: RuboCop is `[Correctable]` (reorders the declaration
//!   lines via `OrderedGemCorrector`). Murphy emits offense-only; reordering
//!   whole source lines (and carrying their attached comments) is a line-
//!   rewriting correction that the offense already guides the user to perform.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Options for [`OrderedDependencies`].
#[derive(CopOptions)]
pub struct OrderedDependenciesOptions {
    #[option(
        name = "TreatCommentsAsGroupSeparators",
        default = true,
        description = "Treat comments between dependencies as separating distinct sections."
    )]
    pub treat_comments_as_group_separators: bool,
    #[option(
        name = "ConsiderPunctuation",
        default = false,
        description = "Consider `-` and `_` when comparing dependency names for ordering."
    )]
    pub consider_punctuation: bool,
}

#[derive(Default)]
pub struct OrderedDependencies;

#[cop(
    name = "Gemspec/OrderedDependencies",
    description = "Dependencies in the gemspec should be alphabetically sorted.",
    default_severity = "warning",
    default_enabled = true,
    options = OrderedDependenciesOptions
)]
impl OrderedDependencies {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<OrderedDependenciesOptions>();
        let root = cx.root();

        // Whole-AST source-order walk → `spec.add_*dependency 'name'` calls.
        // Sorted by range.start so `each_cons(2)` sees true source order even if
        // the descendant walk yields a parent-ish node ahead of a sibling.
        let mut declarations: Vec<Declaration<'_>> = std::iter::once(root)
            .chain(cx.descendants(root))
            .filter_map(|node| dependency_declaration(node, cx))
            .collect();
        declarations.sort_by_key(|decl| cx.range(decl.node).start);

        // 1-based source lines of own-line comments, used to walk the contiguous
        // comment block above a node when comments are not separators.
        let comment_lines: Vec<usize> = own_line_comment_lines(cx);

        for pair in declarations.windows(2) {
            let previous = &pair[0];
            let current = &pair[1];

            let previous_last_line = line_of(cx, cx.range(previous.node).end);
            let current_first_line =
                source_range_first_line(current.node, &comment_lines, &opts, cx);
            if previous_last_line != current_first_line.saturating_sub(1) {
                continue;
            }

            if !case_insensitive_out_of_order(current.name, previous.name, &opts) {
                continue;
            }

            // RuboCop: `get_dependency_name(previous) == get_dependency_name(current)`
            // — only compare declarations that use the same `add_*` method.
            if previous.method != current.method {
                continue;
            }

            let cur_name = current.name;
            let prev_name = previous.name;
            let message = format!(
                "Dependencies should be sorted in an alphabetical order within \
                 their section of the gemspec. Dependency `{cur_name}` should \
                 appear before `{prev_name}`."
            );
            cx.emit_offense(cx.range(current.node), &message, None);
        }
    }
}

/// A matched dependency declaration: the call node, the `add_*` selector, and
/// the first-argument string value.
struct Declaration<'a> {
    node: NodeId,
    method: &'a str,
    name: &'a str,
}

/// `Some(Declaration)` if `node` is RuboCop's
/// `(send (lvar _) {add_dependency add_runtime_dependency
/// add_development_dependency} (str _) ...)`; `None` otherwise.
///
/// Deliberately does *not* unwrap parentheses on the receiver or the argument:
/// RuboCop's `(lvar _)` / `(str _)` patterns are strict.
fn dependency_declaration<'a>(node: NodeId, cx: &Cx<'a>) -> Option<Declaration<'a>> {
    // Restrict to `Send` (RuboCop's pattern is send-only). A block-form
    // `spec.add_dependency 'x' do…end` would be a `Block` whose `method_name`
    // delegates; matching only `Send` avoids double-counting via the block node.
    if !matches!(cx.kind(node), NodeKind::Send { .. }) {
        return None;
    }
    let method = cx.method_name(node)?;
    if !matches!(
        method,
        "add_dependency" | "add_runtime_dependency" | "add_development_dependency"
    ) {
        return None;
    }
    // Receiver must be a plain local variable (`(lvar _)`).
    let receiver = cx.call_receiver(node).get()?;
    if !matches!(cx.kind(receiver), NodeKind::Lvar(_)) {
        return None;
    }
    let first_arg = *cx.call_arguments(node).first()?;
    match *cx.kind(first_arg) {
        NodeKind::Str(id) => Some(Declaration {
            node,
            method,
            name: cx.string_str(id),
        }),
        _ => None,
    }
}

/// RuboCop's `case_insensitive_out_of_order?(current, previous)` =
/// `canonical(current) < canonical(previous)`.
fn case_insensitive_out_of_order(
    current: &str,
    previous: &str,
    opts: &OrderedDependenciesOptions,
) -> bool {
    canonical_name(current, opts) < canonical_name(previous, opts)
}

/// RuboCop's `gem_canonical_name`: lowercase, with `-`/`_` stripped unless
/// `ConsiderPunctuation` is set.
fn canonical_name(name: &str, opts: &OrderedDependenciesOptions) -> String {
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
    opts: &OrderedDependenciesOptions,
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

murphy_plugin_api::submit_cop!(OrderedDependencies);

#[cfg(test)]
mod tests {
    use super::{OrderedDependencies, OrderedDependenciesOptions};
    use murphy_plugin_api::CopOptions;
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(treat_comments: bool, consider_punctuation: bool) -> OrderedDependenciesOptions {
        OrderedDependenciesOptions {
            treat_comments_as_group_separators: treat_comments,
            consider_punctuation,
        }
    }

    #[test]
    fn flags_simple_out_of_order_pair() {
        // Offense on the SECOND declaration; message swaps the names.
        // `spec` is given lvar status via the leading assignment so the receiver
        // is `(lvar spec)`, matching RuboCop's `(lvar _)` pattern.
        test::<OrderedDependencies>().expect_offense(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_dependency 'rubocop'
            spec.add_dependency 'rspec'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependencies should be sorted in an alphabetical order within their section of the gemspec. Dependency `rspec` should appear before `rubocop`.
        "#});
    }

    #[test]
    fn allows_alphabetically_sorted_dependencies() {
        test::<OrderedDependencies>().expect_no_offenses(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_dependency 'rspec'
            spec.add_dependency 'rubocop'
        "#});
    }

    #[test]
    fn flags_each_consecutive_out_of_order_pair() {
        // rubocop, rspec... no — use clearly out-of-order: z, m, a.
        test::<OrderedDependencies>().expect_offense(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_dependency 'z'
            spec.add_dependency 'm'
            ^^^^^^^^^^^^^^^^^^^^^^^ Dependencies should be sorted in an alphabetical order within their section of the gemspec. Dependency `m` should appear before `z`.
            spec.add_dependency 'a'
            ^^^^^^^^^^^^^^^^^^^^^^^ Dependencies should be sorted in an alphabetical order within their section of the gemspec. Dependency `a` should appear before `m`.
        "#});
    }

    #[test]
    fn runtime_dependency_method_is_matched() {
        test::<OrderedDependencies>().expect_offense(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_runtime_dependency 'z'
            spec.add_runtime_dependency 'a'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependencies should be sorted in an alphabetical order within their section of the gemspec. Dependency `a` should appear before `z`.
        "#});
    }

    #[test]
    fn development_dependency_method_is_matched() {
        test::<OrderedDependencies>().expect_offense(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_development_dependency 'z'
            spec.add_development_dependency 'a'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependencies should be sorted in an alphabetical order within their section of the gemspec. Dependency `a` should appear before `z`.
        "#});
    }

    #[test]
    fn different_methods_are_not_compared() {
        // `get_dependency_name(previous) == get_dependency_name(current)` filter:
        // different `add_*` methods are never compared even if out of order.
        test::<OrderedDependencies>().expect_no_offenses(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_dependency 'z'
            spec.add_development_dependency 'a'
        "#});
    }

    #[test]
    fn interleaved_methods_skip_both_cross_method_pairs() {
        // each_cons pairs: (z dep, a dev) cross-method skip; (a dev, b dep)
        // cross-method skip. The non-adjacent dep z / dep b are never compared.
        test::<OrderedDependencies>().expect_no_offenses(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_dependency 'z'
            spec.add_development_dependency 'a'
            spec.add_dependency 'b'
        "#});
    }

    #[test]
    fn comment_separates_sections_by_default() {
        // TreatCommentsAsGroupSeparators=true (default): a comment between the
        // two declarations breaks line-adjacency → separate sections → no offense.
        test::<OrderedDependencies>().expect_no_offenses(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_dependency 'rubocop'
            # a comment
            spec.add_dependency 'rspec'
        "#});
    }

    #[test]
    fn comment_does_not_separate_when_option_disabled() {
        // TreatCommentsAsGroupSeparators=false: the comment is attached to the
        // current declaration, restoring adjacency → out of order → offense.
        test::<OrderedDependencies>()
            .with_options(&opts(false, false))
            .expect_offense(indoc! {r#"
                spec = Gem::Specification.new
                spec.add_dependency 'rubocop'
                # a comment
                spec.add_dependency 'rspec'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependencies should be sorted in an alphabetical order within their section of the gemspec. Dependency `rspec` should appear before `rubocop`.
            "#});
    }

    #[test]
    fn blank_line_separates_sections() {
        // A blank line breaks line-adjacency → separate sections → no offense.
        test::<OrderedDependencies>().expect_no_offenses(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_dependency 'rubocop'

            spec.add_dependency 'rspec'
        "#});
    }

    #[test]
    fn multi_line_previous_declaration_uses_its_last_line() {
        // `rubocop` spans two lines; `rspec` on the next. prev.last == cur.first-1
        // → adjacent → offense.
        test::<OrderedDependencies>().expect_offense(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_dependency 'rubocop',
                                '>= 1.0'
            spec.add_dependency 'rspec'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependencies should be sorted in an alphabetical order within their section of the gemspec. Dependency `rspec` should appear before `rubocop`.
        "#});
    }

    #[test]
    fn punctuation_ignored_by_default() {
        // ConsiderPunctuation=false: `a-b` → `ab`, equal to `ab`; `'ab' < 'ab'`
        // is false → no offense.
        test::<OrderedDependencies>().expect_no_offenses(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_dependency 'a-b'
            spec.add_dependency 'ab'
        "#});
    }

    #[test]
    fn punctuation_considered_flags_when_out_of_order() {
        // ConsiderPunctuation=true: `b` then `a-z` → 'a-z' < 'b' → offense.
        test::<OrderedDependencies>()
            .with_options(&opts(true, true))
            .expect_offense(indoc! {r#"
                spec = Gem::Specification.new
                spec.add_dependency 'b'
                spec.add_dependency 'a-z'
                ^^^^^^^^^^^^^^^^^^^^^^^^^ Dependencies should be sorted in an alphabetical order within their section of the gemspec. Dependency `a-z` should appear before `b`.
            "#});
    }

    #[test]
    fn case_insensitive_comparison() {
        // `Rspec` vs `rubocop`: canonical 'rspec' < 'rubocop' → in order → none.
        test::<OrderedDependencies>().expect_no_offenses(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_dependency 'Rspec'
            spec.add_dependency 'rubocop'
        "#});
    }

    #[test]
    fn bare_call_without_lvar_receiver_is_ignored() {
        // A bare `add_dependency` (no prior assignment) parses with an implicit
        // `(send :spec nil)` receiver, NOT `(lvar _)` → not matched.
        test::<OrderedDependencies>().expect_no_offenses(indoc! {r#"
            spec.add_dependency 'z'
            spec.add_dependency 'a'
        "#});
    }

    #[test]
    fn self_receiver_is_ignored() {
        // `self.add_dependency` → receiver is `self`, not an lvar → not matched.
        test::<OrderedDependencies>().expect_no_offenses(indoc! {r#"
            self.add_dependency 'z'
            self.add_dependency 'a'
        "#});
    }

    #[test]
    fn non_dependency_methods_are_ignored() {
        test::<OrderedDependencies>().expect_no_offenses(indoc! {r#"
            spec = Gem::Specification.new
            spec.name = 'z'
            spec.summary = 'a'
        "#});
    }

    #[test]
    fn non_string_first_arg_is_ignored() {
        test::<OrderedDependencies>().expect_no_offenses(indoc! {r#"
            spec = Gem::Specification.new
            spec.add_dependency z
            spec.add_dependency a
        "#});
    }

    #[test]
    fn options_roundtrip_defaults() {
        let json = br#"{"TreatCommentsAsGroupSeparators":true,"ConsiderPunctuation":false}"#;
        let o = <OrderedDependenciesOptions as CopOptions>::from_config_json(json)
            .expect("defaults must decode");
        assert!(o.treat_comments_as_group_separators);
        assert!(!o.consider_punctuation);
    }
}
