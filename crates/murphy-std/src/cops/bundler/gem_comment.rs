//! `Bundler/GemComment` — require a comment describing each `gem` declaration in
//! a Gemfile/gems.rb. The cop runs only on Gemfile/gems.rb files; the host
//! applies the per-cop `Include` from `config/default.yml`, so this cop never
//! inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Bundler/GemComment
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Reproduces RuboCop's `Bundler/GemComment`. Fires on bare `gem 'name'`
//!   declarations (RuboCop's `(send nil? :gem str ...)` via the
//!   `GemDeclaration` mixin) that lack a describing comment, citing the whole
//!   `gem` send node with the static message `Missing gem description comment.`
//!
//!   Comment detection mirrors RuboCop's `commented_any_descendant?`, which
//!   combines `Parser::Source::Comment.associate_locations` (`ast_with_comments`)
//!   with the `precede?` rule (`comment_line - node_line <= 1`) across the gem
//!   node *and every descendant*. Murphy collapses that per-node association
//!   into a flat line model that is behaviourally equivalent on Gemfile shapes:
//!   a comment "describes" the gem iff its line lies within the gem node's own
//!   source span (`[start_line, end_line]`) — catching trailing and
//!   between-arg comments that associate with the send or any argument — OR it
//!   is an *own-line* comment on the line immediately above the gem
//!   (`start_line - 1`). The own-line gate on the `start_line - 1` line is
//!   load-bearing: a *trailing* comment on the previous statement's line
//!   (`gem 'a' # x` / `gem 'b'`) associates with that previous statement in
//!   RuboCop, so it must NOT count as a leading comment for the next gem.
//!   Verified case-by-case against standalone rubocop 1.87.0 (leading own-line
//!   comment, blank-line gap, trailing same-line comment, trailing-on-previous,
//!   between-arg / above-descendant comments on multiline gems).
//!
//!   Options match default.yml: `IgnoredGems: []` (skip gems whose name is
//!   listed) and `OnlyFor: []` (when non-empty, only flag gems matching at
//!   least one selector). `OnlyFor` selectors mirror RuboCop exactly:
//!   `version_specifiers` (a second positional `Str` arg exists),
//!   `restrictive_version_specifiers` (a positional `Str` arg from index 1
//!   matching `/\A\s*(?:<|~>|\d|=)/` — note `=` and a bare leading digit count,
//!   `>=` / `!=` do not), and any other selector intersected against the keys
//!   of a trailing options hash.
//! ```

use murphy_plugin_api::{Comment, CommentKind, CopOptions, Cx, NodeId, NodeKind, cop};

const MSG: &str = "Missing gem description comment.";

const VERSION_SPECIFIERS_OPTION: &str = "version_specifiers";
const RESTRICTIVE_VERSION_SPECIFIERS_OPTION: &str = "restrictive_version_specifiers";

#[derive(Default)]
pub struct GemComment;

#[derive(CopOptions)]
pub struct GemCommentOptions {
    #[option(
        name = "IgnoredGems",
        default = [],
        description = "Gems to ignore (do not require a describing comment)."
    )]
    pub ignored_gems: Vec<String>,
    #[option(
        name = "OnlyFor",
        default = [],
        description = "When non-empty, only require a comment for gems matching these selectors."
    )]
    pub only_for: Vec<String>,
}

#[cop(
    name = "Bundler/GemComment",
    description = "Add a comment describing each gem.",
    default_severity = "warning",
    default_enabled = false,
    options = GemCommentOptions
)]
impl GemComment {
    #[on_node(kind = "send", methods = ["gem"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>, opts: &GemCommentOptions) {
        // RuboCop's `gem_declaration?`: `(send nil? :gem str ...)`.
        if cx.call_receiver(node).get().is_some() {
            return;
        }
        let args = cx.call_arguments(node);
        let Some(&first) = args.first() else {
            return;
        };
        let NodeKind::Str(name_id) = *cx.kind(first) else {
            return;
        };
        let gem_name = cx.string_str(name_id);

        // `ignored_gem?`: skip gems listed in `IgnoredGems`.
        if opts.ignored_gems.iter().any(|g| g == gem_name) {
            return;
        }

        // `commented_any_descendant?`: skip if a describing comment is present.
        if has_describing_comment(node, cx) {
            return;
        }

        // `OnlyFor`: when non-empty, only flag gems matching a selector.
        if !opts.only_for.is_empty() && !checked_options_present(node, args, cx, opts) {
            return;
        }

        cx.emit_offense(cx.range(node), MSG, None);
    }
}

/// RuboCop's `commented_any_descendant?` collapsed to a flat line model.
///
/// A comment describes the gem iff its line is within the gem node's source
/// span (`[start_line, end_line]`) — covering trailing and between-arg comments
/// that `associate_locations` would attach to the send or any argument — OR it
/// is an *own-line* comment on the line immediately above the gem
/// (`start_line - 1`). The own-line gate is what keeps a trailing comment on
/// the *previous* statement's line from counting as this gem's leading comment.
fn has_describing_comment(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    let start_line = line_of(cx, range.start);
    // `range.end` is exclusive; step back one byte so a node ending exactly at a
    // newline boundary does not overshoot into the following line.
    let end_line = line_of(cx, range.end.saturating_sub(1));

    cx.comments().iter().any(|comment| {
        let comment_line = line_of(cx, comment.range.start);
        if comment_line >= start_line && comment_line <= end_line {
            return true;
        }
        comment_line + 1 == start_line && is_own_line_comment(*comment, cx)
    })
}

/// RuboCop's `checked_options_present?`.
fn checked_options_present(
    node: NodeId,
    args: &[NodeId],
    cx: &Cx<'_>,
    opts: &GemCommentOptions,
) -> bool {
    if opts
        .only_for
        .iter()
        .any(|o| o == VERSION_SPECIFIERS_OPTION)
        && version_specified_gem(args, cx)
    {
        return true;
    }
    if opts
        .only_for
        .iter()
        .any(|o| o == RESTRICTIVE_VERSION_SPECIFIERS_OPTION)
        && restrictive_version_specified_gem(args, cx)
    {
        return true;
    }
    contains_checked_options(node, args, cx, opts)
}

/// RuboCop's `version_specified_gem?`: `arguments[1]` is a `Str` (the second
/// positional argument; the first is the gem name). All other positional
/// arguments to `gem` are version specifiers.
fn version_specified_gem(args: &[NodeId], cx: &Cx<'_>) -> bool {
    args.get(1)
        .is_some_and(|&arg| matches!(*cx.kind(arg), NodeKind::Str(_)))
}

/// RuboCop's `restrictive_version_specified_gem?`: a positional `Str` argument
/// from index 1 onward whose value matches `/\A\s*(?:<|~>|\d|=)/`.
fn restrictive_version_specified_gem(args: &[NodeId], cx: &Cx<'_>) -> bool {
    if !version_specified_gem(args, cx) {
        return false;
    }
    args.iter().skip(1).any(|&arg| {
        let NodeKind::Str(id) = *cx.kind(arg) else {
            return false;
        };
        is_restrictive_version(cx.string_str(id))
    })
}

/// `/\A\s*(?:<|~>|\d|=)/`: after leading whitespace, the first significant
/// token is `<`, `~>`, a digit, or `=`. `>=` / `!=` are *not* restrictive.
fn is_restrictive_version(value: &str) -> bool {
    let trimmed = value.trim_start();
    let bytes = trimmed.as_bytes();
    match bytes.first() {
        Some(b'<') | Some(b'=') => true,
        Some(b) if b.is_ascii_digit() => true,
        Some(b'~') => bytes.get(1) == Some(&b'>'),
        _ => false,
    }
}

/// RuboCop's `contains_checked_options?`: any `OnlyFor` selector equals a key of
/// the gem's trailing options hash.
fn contains_checked_options(
    _node: NodeId,
    args: &[NodeId],
    cx: &Cx<'_>,
    opts: &GemCommentOptions,
) -> bool {
    let Some(&last) = args.last() else {
        return false;
    };
    let NodeKind::Hash(pairs) = *cx.kind(last) else {
        return false;
    };
    cx.list(pairs).iter().any(|&pair| {
        let Some(key) = hash_pair_symbol_key(pair, cx) else {
            return false;
        };
        opts.only_for.iter().any(|o| o == key)
    })
}

/// The symbol/string key of a hash `Pair` as `&str`, if the key is a `Sym` or
/// `Str` literal. RuboCop maps option keys via `keys.map(&:value)`, which on a
/// `(pair (sym :git) ...)` yields `:git` → `"git"`.
fn hash_pair_symbol_key<'a>(pair: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
        return None;
    };
    match *cx.kind(key) {
        NodeKind::Sym(id) => Some(cx.symbol_str(id)),
        NodeKind::Str(id) => Some(cx.string_str(id)),
        _ => None,
    }
}

/// A comment is "own-line" if everything before it on its line is whitespace
/// (mirrors `Cx::is_own_line_comment`, which is not part of the public surface).
fn is_own_line_comment(comment: Comment, cx: &Cx<'_>) -> bool {
    if comment.kind != CommentKind::Inline {
        return false;
    }
    let source = cx.source().as_bytes();
    let start = comment.range.start as usize;
    let line_start = source[..start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    source[line_start..start]
        .iter()
        .all(|byte| byte.is_ascii_whitespace())
}

/// 1-based source line number of the byte `offset`.
fn line_of(cx: &Cx<'_>, offset: u32) -> usize {
    cx.source()[..offset as usize].matches('\n').count() + 1
}

murphy_plugin_api::submit_cop!(GemComment);

#[cfg(test)]
mod tests {
    use super::{GemComment, GemCommentOptions};
    use murphy_plugin_api::test_support::{indoc, run_cop, test};

    // --- Default OnlyFor: [] — every uncommented gem is flagged. ------------

    #[test]
    fn flags_uncommented_gem() {
        test::<GemComment>().expect_offense(indoc! {r#"
            gem 'foo'
            ^^^^^^^^^ Missing gem description comment.
        "#});
    }

    #[test]
    fn accepts_gem_with_leading_own_line_comment() {
        test::<GemComment>().expect_no_offenses(indoc! {r#"
            # Helpers for the foo things.
            gem 'foo'
        "#});
    }

    #[test]
    fn flags_gem_with_comment_separated_by_blank_line() {
        // Blank line between the comment and the gem → comment is 2 lines up.
        test::<GemComment>().expect_offense(indoc! {r#"
            # far above

            gem 'foo'
            ^^^^^^^^^ Missing gem description comment.
        "#});
    }

    #[test]
    fn accepts_gem_with_trailing_same_line_comment() {
        test::<GemComment>().expect_no_offenses(indoc! {r#"
            gem 'foo' # describes foo
        "#});
    }

    #[test]
    fn flags_gem_after_trailing_comment_on_previous_gem() {
        // The trailing comment on line 1 associates with `gem 'a'`, not `gem 'b'`.
        test::<GemComment>().expect_offense(indoc! {r#"
            gem 'a' # describes a
            gem 'b'
            ^^^^^^^ Missing gem description comment.
        "#});
    }

    #[test]
    fn flags_gem_after_trailing_comment_on_previous_statement() {
        test::<GemComment>().expect_offense(indoc! {r#"
            source 'https://rubygems.org' # the source
            gem 'a'
            ^^^^^^^ Missing gem description comment.
        "#});
    }

    #[test]
    fn accepts_multiline_gem_with_comment_between_args() {
        // A comment on a descendant arg's line counts (commented_any_descendant?).
        test::<GemComment>().expect_no_offenses(indoc! {r#"
            gem 'foo',
              # version 2.1 introduces a breaking change
              '< 2.1'
        "#});
    }

    #[test]
    fn accepts_multiline_gem_with_trailing_comment_on_last_arg() {
        test::<GemComment>().expect_no_offenses(indoc! {r#"
            gem 'foo',
              '>= 1.0' # trailing on last arg
        "#});
    }

    #[test]
    fn flags_uncommented_multiline_gem() {
        // Whole-node offense range spans both lines, so use a count-based check
        // rather than a (single-line) caret annotation.
        let offenses = run_cop::<GemComment>("gem 'foo',\n  require: false\n");
        assert_eq!(offenses.len(), 1);
        assert_eq!(offenses[0].message, "Missing gem description comment.");
        assert_eq!(offenses[0].range.start, 0);
    }

    #[test]
    fn ignores_gem_with_receiver() {
        test::<GemComment>().expect_no_offenses(indoc! {r#"
            obj.gem 'foo'
        "#});
    }

    #[test]
    fn ignores_gem_with_non_string_first_arg() {
        test::<GemComment>().expect_no_offenses(indoc! {r#"
            gem name
        "#});
    }

    #[test]
    fn ignores_non_gem_calls() {
        test::<GemComment>().expect_no_offenses(indoc! {r#"
            source 'https://rubygems.org'
        "#});
    }

    // --- IgnoredGems --------------------------------------------------------

    #[test]
    fn accepts_ignored_gem() {
        test::<GemComment>()
            .with_options(&GemCommentOptions {
                ignored_gems: vec!["foo".to_string()],
                only_for: vec![],
            })
            .expect_no_offenses(indoc! {r#"
                gem 'foo'
            "#});
    }

    #[test]
    fn flags_non_ignored_gem_when_ignored_list_set() {
        test::<GemComment>()
            .with_options(&GemCommentOptions {
                ignored_gems: vec!["foo".to_string()],
                only_for: vec![],
            })
            .expect_offense(indoc! {r#"
                gem 'foo'
                gem 'bar'
                ^^^^^^^^^ Missing gem description comment.
            "#});
    }

    // --- OnlyFor: ['version_specifiers'] ------------------------------------

    #[test]
    fn only_for_version_specifiers_flags_gem_with_version() {
        test::<GemComment>()
            .with_options(&GemCommentOptions {
                ignored_gems: vec![],
                only_for: vec!["version_specifiers".to_string()],
            })
            .expect_offense(indoc! {r#"
                gem 'foo', '< 2.1'
                ^^^^^^^^^^^^^^^^^^ Missing gem description comment.
            "#});
    }

    #[test]
    fn only_for_version_specifiers_accepts_gem_without_version() {
        test::<GemComment>()
            .with_options(&GemCommentOptions {
                ignored_gems: vec![],
                only_for: vec!["version_specifiers".to_string()],
            })
            .expect_no_offenses(indoc! {r#"
                gem 'foo'
            "#});
    }

    #[test]
    fn only_for_version_specifiers_accepts_gem_with_only_options() {
        test::<GemComment>()
            .with_options(&GemCommentOptions {
                ignored_gems: vec![],
                only_for: vec!["version_specifiers".to_string()],
            })
            .expect_no_offenses(indoc! {r#"
                gem 'foo', github: 'a/b'
            "#});
    }

    // --- OnlyFor: ['restrictive_version_specifiers'] ------------------------

    #[test]
    fn only_for_restrictive_flags_upper_bound() {
        test::<GemComment>()
            .with_options(&GemCommentOptions {
                ignored_gems: vec![],
                only_for: vec!["restrictive_version_specifiers".to_string()],
            })
            .expect_offense(indoc! {r#"
                gem 'foo', '< 2.1'
                ^^^^^^^^^^^^^^^^^^ Missing gem description comment.
            "#});
    }

    #[test]
    fn only_for_restrictive_accepts_lower_bound() {
        // `>= 1.0` is not restrictive → no offense.
        test::<GemComment>()
            .with_options(&GemCommentOptions {
                ignored_gems: vec![],
                only_for: vec!["restrictive_version_specifiers".to_string()],
            })
            .expect_no_offenses(indoc! {r#"
                gem 'foo', '>= 1.0'
            "#});
    }

    // --- OnlyFor: [<source option>] -----------------------------------------

    #[test]
    fn only_for_github_flags_gem_with_github_option() {
        test::<GemComment>()
            .with_options(&GemCommentOptions {
                ignored_gems: vec![],
                only_for: vec!["github".to_string()],
            })
            .expect_offense(indoc! {r#"
                gem 'foo', github: 'some_account/some_fork'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Missing gem description comment.
            "#});
    }

    #[test]
    fn only_for_github_accepts_gem_without_github_option() {
        test::<GemComment>()
            .with_options(&GemCommentOptions {
                ignored_gems: vec![],
                only_for: vec!["github".to_string()],
            })
            .expect_no_offenses(indoc! {r#"
                gem 'foo', '< 2.1'
            "#});
    }
}
