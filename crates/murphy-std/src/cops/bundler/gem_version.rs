//! `Bundler/GemVersion` — require (or forbid) a version specification or commit
//! reference on every `gem 'x'` declaration. The cop runs only on
//! Gemfile/gems.rb files; the host applies the per-cop `Include` from
//! `config/default.yml`, so this cop never inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Bundler/GemVersion
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Reproduces RuboCop's `GemDeclaration` mixin `gem_declaration?`
//!   (`(send nil? :gem str ...)`) by matching bare `gem` calls (send-only) whose
//!   first argument is a plain `Str` (parenthesized `gem ('x')` is excluded,
//!   matching the strict `str` pattern — same precedent as DuplicatedGem).
//!
//!   EnforcedStyle (default `required`):
//!     - required:  offense iff the call has NO version specification AND NO
//!       commit reference.
//!     - forbidden: offense iff the call HAS a version specification OR a commit
//!       reference.
//!
//!   `includes_version_specification?` mirrors the parser pattern
//!   `(send nil? :gem <(str #version_specification?) ...>)`: the `<...>` set
//!   matcher ranges over EVERY argument including the first (the gem name), so
//!   `gem '1.2.3'` (name itself matches the version regex) is NOT flagged under
//!   `required` — verified against standalone rubocop 1.87.0. We check whether
//!   ANY `str` argument's value matches RuboCop's
//!   `/^\s*[~<>=]*\s*[0-9.]+/` (start-anchored: optional leading ASCII
//!   whitespace, optional run of `~<>=`, optional whitespace, then at least one
//!   `[0-9.]` char). The regex is hand-rolled (murphy-std has no `regex` dep).
//!
//!   `includes_commit_reference?` mirrors
//!   `(send nil? :gem <(hash <(pair (sym {:branch :ref :tag}) (str _)) ...>) ...>)`:
//!   any `hash` argument containing a `pair` whose key is a `sym` in
//!   `{branch, ref, tag}` and whose value is a `str`. A non-string value such as
//!   `gem 'x', ref: SOME_CONST` is NOT a commit reference → flagged under
//!   `required` (verified against rubocop 1.87.0).
//!
//!   `AllowedGems` (default `[]`) exempts a declaration when its first-argument
//!   string value (the gem name) is in the list — RuboCop's
//!   `allowed_gems.include?(node.first_argument.value)`.
//!
//!   Offense range is the whole `gem ...` send node (`add_offense(node)`), so in
//!   `forbidden` style the carets span the version argument too. Messages are
//!   the fixed `Gem version specification is required.` /
//!   `... is forbidden.`. RuboCop's `correct_style_detected` /
//!   `opposite_style_detected` style auto-detection bookkeeping is intentionally
//!   omitted (it does not change offense behaviour; DuplicatedGem omits the
//!   analogous machinery too).
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct GemVersion;

/// Enforced style: require vs forbid version/commit-reference specifications.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GemVersionStyle {
    #[default]
    #[option(value = "required")]
    Required,
    #[option(value = "forbidden")]
    Forbidden,
}

#[derive(CopOptions)]
pub struct GemVersionOptions {
    #[option(
        name = "EnforcedStyle",
        default = "required",
        description = "Whether to require or forbid gem version specifications."
    )]
    pub enforced_style: GemVersionStyle,

    #[option(
        name = "AllowedGems",
        default = [],
        description = "Gems that are exempt from version-specification enforcement."
    )]
    pub allowed_gems: Vec<String>,
}

const REQUIRED_MSG: &str = "Gem version specification is required.";
const FORBIDDEN_MSG: &str = "Gem version specification is forbidden.";

#[cop(
    name = "Bundler/GemVersion",
    description = "Requires or forbids specifying gem versions.",
    default_severity = "warning",
    default_enabled = false,
    options = GemVersionOptions
)]
impl GemVersion {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<GemVersionOptions>();
        let root = cx.root();
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            let Some(name) = gem_declaration_name(node, cx) else {
                continue;
            };
            if opts.allowed_gems.iter().any(|g| g == name) {
                continue;
            }
            if offense(node, opts.enforced_style, cx) {
                let message = match opts.enforced_style {
                    GemVersionStyle::Required => REQUIRED_MSG,
                    GemVersionStyle::Forbidden => FORBIDDEN_MSG,
                };
                cx.emit_offense(cx.range(node), message, None);
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
    // RuboCop's pattern is send-only; restricting to `Send` also avoids
    // double-counting a block-form `gem 'x' do…end` (a `Block` whose
    // `method_name` delegates to `"gem"`) via both the block node and its
    // inner send during the whole-AST walk.
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

/// Whether `node` is an offense under the active style.
fn offense(node: NodeId, style: GemVersionStyle, cx: &Cx<'_>) -> bool {
    let has_spec = includes_version_specification(node, cx) || includes_commit_reference(node, cx);
    match style {
        GemVersionStyle::Required => !has_spec,
        GemVersionStyle::Forbidden => has_spec,
    }
}

/// RuboCop's `includes_version_specification?`
/// (`(send nil? :gem <(str #version_specification?) ...>)`): ANY argument is a
/// `Str` whose value matches the version regex. The `<...>` set matcher includes
/// the first (name) argument, so `gem '1.2.3'` qualifies.
fn includes_version_specification(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.call_arguments(node).iter().any(|&arg| match *cx.kind(arg) {
        NodeKind::Str(id) => version_specification(cx.string_str(id)),
        _ => false,
    })
}

/// RuboCop's `version_specification?` against `VERSION_SPECIFICATION_REGEX`
/// `/^\s*[~<>=]*\s*[0-9.]+/` — start-anchored: optional leading ASCII
/// whitespace, then an optional run of `~ < > =`, then optional ASCII
/// whitespace, then at least one `[0-9.]` character.
fn version_specification(expr: &str) -> bool {
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    while i < bytes.len() && matches!(bytes[i], b'~' | b'<' | b'>' | b'=') {
        i += 1;
    }
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i < bytes.len() && matches!(bytes[i], b'0'..=b'9' | b'.')
}

/// RuboCop's `includes_commit_reference?`
/// (`(send nil? :gem <(hash <(pair (sym {:branch :ref :tag}) (str _)) ...>) ...>)`):
/// ANY argument is a `Hash` containing a `Pair` whose key is a `Sym` in
/// `{branch, ref, tag}` and whose value is a `Str`.
fn includes_commit_reference(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.call_arguments(node).iter().any(|&arg| {
        cx.hash_pairs(arg).iter().any(|&pair| {
            let Some(key) = cx.pair_key(pair).get() else {
                return false;
            };
            let NodeKind::Sym(sym) = *cx.kind(key) else {
                return false;
            };
            if !matches!(cx.symbol_str(sym), "branch" | "ref" | "tag") {
                return false;
            }
            cx.pair_value(pair)
                .get()
                .is_some_and(|v| matches!(cx.kind(v), NodeKind::Str(_)))
        })
    })
}

murphy_plugin_api::submit_cop!(GemVersion);

#[cfg(test)]
mod tests {
    use super::{GemVersion, GemVersionOptions, GemVersionStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- EnforcedStyle: required (default) -----

    #[test]
    fn required_flags_gem_without_version() {
        test::<GemVersion>().expect_offense(indoc! {r#"
            gem 'rubocop'
            ^^^^^^^^^^^^^ Gem version specification is required.
        "#});
    }

    #[test]
    fn required_allows_tilde_version() {
        test::<GemVersion>().expect_no_offenses(indoc! {r#"
            gem 'rubocop', '~> 1.12'
        "#});
    }

    #[test]
    fn required_allows_gte_version() {
        test::<GemVersion>().expect_no_offenses(indoc! {r#"
            gem 'rubocop', '>= 1.10.0'
        "#});
    }

    #[test]
    fn required_allows_multiple_version_constraints() {
        test::<GemVersion>().expect_no_offenses(indoc! {r#"
            gem 'rubocop', '>= 1.5.0', '< 1.10.0'
        "#});
    }

    #[test]
    fn required_allows_branch_ref_tag_commit_references() {
        test::<GemVersion>().expect_no_offenses(indoc! {r#"
            gem 'a', branch: 'feature-branch'
            gem 'b', ref: '74b5bfbb2c4b6fd6cdbbc7254bd7084b36e0c85b'
            gem 'c', tag: 'v1.17.0'
        "#});
    }

    #[test]
    fn required_does_not_flag_when_name_itself_matches_version_regex() {
        // The `<...>` set matcher includes the first (name) argument, so a name
        // that matches the version regex satisfies the pattern. Verified against
        // standalone rubocop 1.87.0.
        test::<GemVersion>().expect_no_offenses(indoc! {r#"
            gem '1.2.3'
        "#});
    }

    #[test]
    fn required_flags_commit_reference_with_non_string_value() {
        // `ref:` with a non-`str` value is NOT a commit reference → flagged.
        test::<GemVersion>().expect_offense(indoc! {r#"
            gem 'x', ref: SOME_CONST
            ^^^^^^^^^^^^^^^^^^^^^^^^ Gem version specification is required.
        "#});
    }

    #[test]
    fn required_ignores_gem_with_receiver_and_non_gem_calls() {
        test::<GemVersion>().expect_no_offenses(indoc! {r#"
            source 'https://rubygems.org'
            obj.gem 'rubocop'
        "#});
    }

    #[test]
    fn required_ignores_gem_with_non_string_first_arg() {
        test::<GemVersion>().expect_no_offenses(indoc! {r#"
            gem name
        "#});
    }

    // ----- EnforcedStyle: forbidden -----

    #[test]
    fn forbidden_allows_gem_without_version() {
        test::<GemVersion>()
            .with_options(&GemVersionOptions {
                enforced_style: GemVersionStyle::Forbidden,
                allowed_gems: Vec::new(),
            })
            .expect_no_offenses(indoc! {r#"
                gem 'rubocop'
            "#});
    }

    #[test]
    fn forbidden_flags_version_spec_over_whole_node() {
        test::<GemVersion>()
            .with_options(&GemVersionOptions {
                enforced_style: GemVersionStyle::Forbidden,
                allowed_gems: Vec::new(),
            })
            .expect_offense(indoc! {r#"
                gem 'rubocop', '~> 1.12'
                ^^^^^^^^^^^^^^^^^^^^^^^^ Gem version specification is forbidden.
            "#});
    }

    #[test]
    fn forbidden_flags_commit_reference() {
        test::<GemVersion>()
            .with_options(&GemVersionOptions {
                enforced_style: GemVersionStyle::Forbidden,
                allowed_gems: Vec::new(),
            })
            .expect_offense(indoc! {r#"
                gem 'x', tag: 'v1.0'
                ^^^^^^^^^^^^^^^^^^^^ Gem version specification is forbidden.
            "#});
    }

    // ----- AllowedGems -----

    #[test]
    fn allowed_gem_is_exempt_under_required() {
        test::<GemVersion>()
            .with_options(&GemVersionOptions {
                enforced_style: GemVersionStyle::Required,
                allowed_gems: vec!["rubocop".to_string()],
            })
            .expect_no_offenses(indoc! {r#"
                gem 'rubocop'
            "#});
    }

    #[test]
    fn allowed_gem_is_exempt_under_forbidden() {
        test::<GemVersion>()
            .with_options(&GemVersionOptions {
                enforced_style: GemVersionStyle::Forbidden,
                allowed_gems: vec!["allowed_gem".to_string()],
            })
            .expect_no_offenses(indoc! {r#"
                gem 'allowed_gem', '~> 2.0'
            "#});
    }
}
