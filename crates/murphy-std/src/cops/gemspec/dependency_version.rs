//! `Gemspec/DependencyVersion` — require (or forbid) a version specification or
//! commit reference on every `add_dependency` / `add_runtime_dependency` /
//! `add_development_dependency` call whose receiver is the
//! `Gem::Specification.new` block variable. The cop runs only on `*.gemspec`
//! files; the host applies the per-cop `Include` from `config/default.yml`, so
//! this cop never inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Gemspec/DependencyVersion
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Reproduces RuboCop's `Gemspec/DependencyVersion` (core, 1.87.0). The cop
//!   `include`s `GemspecHelp` and `ConfigurableEnforcedStyle`; the dependency
//!   matcher is
//!   `(send (lvar #match_block_variable_name?) #add_dependency_method? ...)` with
//!   `ADD_DEPENDENCY_METHODS = %i[add_dependency add_runtime_dependency
//!   add_development_dependency]`.
//!
//!   Receiver gate: `match_block_variable_name?` searches the whole AST for the
//!   `GemspecHelp` pattern
//!   `(block (send (const (const {cbase nil?} :Gem) :Specification) :new)
//!   (args (arg $_)) ...)` and yields the block variable name. We mirror this
//!   with the shared `gem_specification_block_var` shape (same helper family as
//!   `Gemspec/AttributeAssignment`): the FIRST `Gem::Specification.new do |x|`
//!   (or `::Gem::Specification`) block with exactly one plain `arg`. A
//!   declaration is in scope iff its receiver is an `Lvar` whose name equals that
//!   block variable. RuboCop compares only the *name* (no lexical-scope check),
//!   so `spec.add_dependency 'x'` outside the block — but with a matching lvar
//!   name — is still flagged; verified against standalone rubocop 1.87.0. No
//!   gemspec block (or a zero-/multi-arg block) → no block variable → the cop is
//!   silent, matching RuboCop's empty `gem_specification` search yield.
//!
//!   EnforcedStyle (default `required`):
//!     - required:  offense iff the call has NO version specification AND NO
//!       commit reference.
//!     - forbidden: offense iff the call HAS a version specification OR a commit
//!       reference.
//!
//!   `includes_version_specification?`
//!   (`(send _ #add_dependency_method? <(str #version_specification?) ...>)`):
//!   ANY `str` argument (the `<...>` set matcher ranges over every argument,
//!   including the name) whose value matches RuboCop's
//!   `VERSION_SPECIFICATION_REGEX = /^\s*[~<>=]*\s*[0-9.]+/` (start-anchored:
//!   optional leading ASCII whitespace, optional run of `~<>=`, optional
//!   whitespace, then at least one `[0-9.]` char). Hand-rolled (murphy-std has no
//!   `regex` dep), identical to `Bundler/GemVersion`.
//!
//!   `includes_commit_reference?`
//!   (`(send _ #add_dependency_method? <(hash <(pair (sym {:branch :ref :tag})
//!   (str _)) ...>) ...>)`): any `hash` argument with a `pair` whose key is a
//!   `sym` in `{branch, ref, tag}` and whose value is a `str`. A non-string
//!   value such as `spec.add_dependency 'x', ref: SOME_CONST` is NOT a commit
//!   reference → flagged under `required` (verified against rubocop 1.87.0).
//!
//!   `AllowedGems` (default `[]`) exempts a declaration when its first-argument
//!   string value is in the list — RuboCop's
//!   `allowed_gems.include?(node.first_argument.str_content)`. For a non-`str`
//!   first argument (e.g. `spec.add_dependency SOME_CONST`), `str_content`
//!   returns `nil` and `[...].include?(nil)` is false, so the declaration is
//!   never exempt and (under `required`) is flagged — verified against rubocop
//!   1.87.0. We mirror this by returning `None` from `first_arg_str` (never
//!   exempt). DIVERGENCE: for an *absent* first argument
//!   (`spec.add_dependency` with no args), RuboCop calls
//!   `nil.str_content` and raises (cop errors out, emits no offense — verified
//!   against rubocop 1.87.0); Murphy is more robust, returning `None` and
//!   treating the call as having no version specification, so under `required`
//!   it is flagged. This near-impossible gemspec shape (a dependency with no
//!   name) is an accepted robustness improvement, not tracked as open work.
//!
//!   Offense range is the whole `... add_dependency ...` send node
//!   (`add_offense(node)`), so under `forbidden` the carets span the version
//!   argument too. Messages are the fixed
//!   `Dependency version specification is required.` /
//!   `... is forbidden.`. RuboCop's `correct_style_detected` /
//!   `opposite_style_detected` auto-detection bookkeeping is intentionally
//!   omitted (it does not change offense behaviour; the sibling
//!   `Bundler/GemVersion` omits the analogous machinery too).
//!
//!   Negligible accepted divergences (not tracked as open work): (1) only the
//!   FIRST `Gem::Specification.new` block's variable name is used when a file
//!   contains multiple spec blocks — RuboCop's `gem_specification` search also
//!   short-circuits on the first match via the `return` inside its block; (2)
//!   safe-navigation `spec&.add_dependency` parses as `CSend`, which RuboCop's
//!   `(send (lvar ...) ...)` pattern does not match either.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct DependencyVersion;

/// Enforced style: require vs forbid version/commit-reference specifications.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DependencyVersionStyle {
    #[default]
    #[option(value = "required")]
    Required,
    #[option(value = "forbidden")]
    Forbidden,
}

#[derive(CopOptions)]
pub struct DependencyVersionOptions {
    #[option(
        name = "EnforcedStyle",
        default = "required",
        description = "Whether to require or forbid dependency version specifications."
    )]
    pub enforced_style: DependencyVersionStyle,

    #[option(
        name = "AllowedGems",
        default = [],
        description = "Gems that are exempt from version-specification enforcement."
    )]
    pub allowed_gems: Vec<String>,
}

const REQUIRED_MSG: &str = "Dependency version specification is required.";
const FORBIDDEN_MSG: &str = "Dependency version specification is forbidden.";

/// `ADD_DEPENDENCY_METHODS` — the dependency-declaration method names.
const ADD_DEPENDENCY_METHODS: [&str; 3] = [
    "add_dependency",
    "add_runtime_dependency",
    "add_development_dependency",
];

#[cop(
    name = "Gemspec/DependencyVersion",
    description = "Requires or forbids specifying gem dependency versions.",
    default_severity = "warning",
    default_enabled = false,
    options = DependencyVersionOptions
)]
impl DependencyVersion {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        // RuboCop's `match_block_variable_name?` resolves the gemspec block
        // variable once; no block variable → no declaration can match.
        let Some(block_var) = gem_specification_block_var(cx) else {
            return;
        };

        let opts = cx.options_or_default::<DependencyVersionOptions>();
        let root = cx.root();
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            if !is_add_dependency_declaration(node, block_var, cx) {
                continue;
            }
            if let Some(name) = first_arg_str(node, cx)
                && opts.allowed_gems.iter().any(|g| g == name)
            {
                continue;
            }
            if offense(node, opts.enforced_style, cx) {
                let message = match opts.enforced_style {
                    DependencyVersionStyle::Required => REQUIRED_MSG,
                    DependencyVersionStyle::Forbidden => FORBIDDEN_MSG,
                };
                cx.emit_offense(cx.range(node), message, None);
            }
        }
    }
}

/// RuboCop's `add_dependency_method_declaration?`
/// (`(send (lvar #match_block_variable_name?) #add_dependency_method? ...)`):
/// a `Send` (not a block, not csend) whose method is an `ADD_DEPENDENCY_METHODS`
/// name and whose receiver is an `Lvar` named `block_var`.
fn is_add_dependency_declaration(node: NodeId, block_var: &str, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::Send { .. }) {
        return false;
    }
    if !cx
        .method_name(node)
        .is_some_and(|m| ADD_DEPENDENCY_METHODS.contains(&m))
    {
        return false;
    }
    let Some(receiver) = cx.call_receiver(node).get() else {
        return false;
    };
    matches!(*cx.kind(receiver), NodeKind::Lvar(sym) if cx.symbol_str(sym) == block_var)
}

/// First-argument string value (`node.first_argument.str_content`) if the first
/// argument is a `Str`; `None` if absent or non-`str`. RuboCop's
/// `allowed_gems.include?(str_content)` treats a `nil` str_content as never
/// matching, which a `None` here reproduces.
fn first_arg_str<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    let first = *cx.call_arguments(node).first()?;
    match *cx.kind(first) {
        NodeKind::Str(id) => Some(cx.string_str(id)),
        _ => None,
    }
}

/// Whether `node` is an offense under the active style.
fn offense(node: NodeId, style: DependencyVersionStyle, cx: &Cx<'_>) -> bool {
    let has_spec = includes_version_specification(node, cx) || includes_commit_reference(node, cx);
    match style {
        DependencyVersionStyle::Required => !has_spec,
        DependencyVersionStyle::Forbidden => has_spec,
    }
}

/// RuboCop's `includes_version_specification?`
/// (`(send _ #add_dependency_method? <(str #version_specification?) ...>)`): ANY
/// argument is a `Str` whose value matches the version regex. The `<...>` set
/// matcher includes the first (name) argument.
fn includes_version_specification(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.call_arguments(node).iter().any(|&arg| match *cx.kind(arg) {
        NodeKind::Str(id) => version_specification(cx.string_str(id)),
        _ => false,
    })
}

/// RuboCop's `version_specification?` against
/// `VERSION_SPECIFICATION_REGEX = /^\s*[~<>=]*\s*[0-9.]+/` — start-anchored:
/// optional leading ASCII whitespace, then an optional run of `~ < > =`, then
/// optional ASCII whitespace, then at least one `[0-9.]` character.
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
/// (`(send _ #add_dependency_method? <(hash <(pair (sym {:branch :ref :tag})
/// (str _)) ...>) ...>)`): ANY argument is a `Hash` containing a `Pair` whose
/// key is a `Sym` in `{branch, ref, tag}` and whose value is a `Str`.
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

/// The single block-variable name of the FIRST `Gem::Specification.new do |x|`
/// (or `::Gem::Specification`) block, mirroring `GemspecHelp`'s
/// `(block (send (const (const {cbase nil?} :Gem) :Specification) :new)
/// (args (arg $_)) ...)`. Requires exactly one plain `arg`.
fn gem_specification_block_var<'a>(cx: &Cx<'a>) -> Option<&'a str> {
    let root = cx.root();
    for node in std::iter::once(root).chain(cx.descendants(root)) {
        if !matches!(cx.kind(node), NodeKind::Block { .. }) {
            continue;
        }
        if !is_gem_specification_call(cx.block_call(node).get(), cx) {
            continue;
        }
        let args = cx.block_arguments(node).get()?;
        let NodeKind::Args(list) = *cx.kind(args) else {
            continue;
        };
        // RuboCop's `(args (arg $_))` requires exactly one plain arg.
        if let [only] = cx.list(list)
            && let NodeKind::Arg(sym) = *cx.kind(*only)
        {
            return Some(cx.symbol_str(sym));
        }
    }
    None
}

/// True when `call` is `Gem::Specification.new` (or `::Gem::Specification.new`).
fn is_gem_specification_call(call: Option<NodeId>, cx: &Cx<'_>) -> bool {
    let Some(call) = call else {
        return false;
    };
    if cx.method_name(call) != Some("new") {
        return false;
    }
    let Some(receiver) = cx.call_receiver(call).get() else {
        return false;
    };
    is_gem_specification_const(receiver, cx)
}

/// True when `node` is the const `Gem::Specification` or `::Gem::Specification`,
/// mirroring RuboCop's `(const (const {cbase nil?} :Gem) :Specification)`.
fn is_gem_specification_const(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return false;
    };
    if cx.symbol_str(name) != "Specification" {
        return false;
    }
    let Some(scope) = scope.get() else {
        return false;
    };
    cx.is_global_const(scope, "Gem")
}

murphy_plugin_api::submit_cop!(DependencyVersion);

#[cfg(test)]
mod tests {
    use super::{DependencyVersion, DependencyVersionOptions, DependencyVersionStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- EnforcedStyle: required (default) -----

    #[test]
    fn required_flags_add_dependency_without_version() {
        test::<DependencyVersion>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.add_dependency 'rubocop'
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is required.
            end
        "#});
    }

    #[test]
    fn required_flags_all_three_dependency_methods() {
        test::<DependencyVersion>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.add_dependency 'a'
              ^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is required.
              spec.add_runtime_dependency 'b'
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is required.
              spec.add_development_dependency 'c'
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is required.
            end
        "#});
    }

    #[test]
    fn required_allows_tilde_version() {
        test::<DependencyVersion>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.add_dependency 'rubocop', '~> 1.12'
            end
        "#});
    }

    #[test]
    fn required_allows_gte_version() {
        test::<DependencyVersion>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.add_dependency 'rubocop', '>= 1.10.0'
            end
        "#});
    }

    #[test]
    fn required_allows_branch_ref_tag_commit_references() {
        test::<DependencyVersion>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.add_dependency 'a', branch: 'feature-branch'
              spec.add_dependency 'b', ref: '74b5bfbb2c4b6fd6cdbbc7254bd7084b36e0c85b'
              spec.add_dependency 'c', tag: 'v1.17.0'
            end
        "#});
    }

    #[test]
    fn required_flags_commit_reference_with_non_string_value() {
        // `ref:` with a non-`str` value is NOT a commit reference → flagged.
        test::<DependencyVersion>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.add_dependency 'x', ref: SOME_CONST
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is required.
            end
        "#});
    }

    #[test]
    fn required_flags_non_string_first_arg() {
        // Receiver is the block var → matched; non-str first arg → no version,
        // no AllowedGems exemption → flagged (verified against rubocop 1.87.0).
        test::<DependencyVersion>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.add_dependency SOME_NAME
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is required.
            end
        "#});
    }

    // ----- receiver / block-variable gate -----

    #[test]
    fn ignores_bare_add_dependency_without_receiver() {
        // No receiver (not an lvar) → does not match the declaration pattern.
        test::<DependencyVersion>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              add_dependency 'x'
            end
        "#});
    }

    #[test]
    fn ignores_receiver_not_matching_block_var() {
        // Block var is `spec`; `other.add_dependency` (different lvar via assign)
        // does not match.
        test::<DependencyVersion>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.name = 'x'
            end
            other = obj
            other.add_dependency 'y'
        "#});
    }

    #[test]
    fn flags_alternate_block_var_name() {
        test::<DependencyVersion>().expect_offense(indoc! {r#"
            Gem::Specification.new do |s|
              s.add_dependency 'rake'
              ^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is required.
            end
        "#});
    }

    #[test]
    fn flags_with_cbase_gem_specification() {
        test::<DependencyVersion>().expect_offense(indoc! {r#"
            ::Gem::Specification.new do |spec|
              spec.add_dependency 'rake'
              ^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is required.
            end
        "#});
    }

    #[test]
    fn flags_matching_lvar_outside_the_block() {
        // RuboCop's `match_block_variable_name?` compares only the name, with no
        // lexical-scope check, so a matching-name lvar outside the block is
        // flagged (verified against rubocop 1.87.0).
        test::<DependencyVersion>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.name = 'x'
            end
            spec = foo
            spec.add_dependency 'outside'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is required.
        "#});
    }

    #[test]
    fn silent_when_no_gem_specification_block() {
        // No `Gem::Specification.new` block → no block variable → no offense,
        // even though `spec` is a real lvar here.
        test::<DependencyVersion>().expect_no_offenses(indoc! {r#"
            spec = something
            spec.add_dependency 'x'
        "#});
    }

    #[test]
    fn flags_zero_arg_add_dependency_robustly() {
        // RuboCop raises (cop errors, no offense) on `nil.str_content` for a
        // zero-arg call; Murphy is more robust — `first_arg_str` is None, no
        // version spec → flagged under `required`. Documented divergence.
        test::<DependencyVersion>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.add_dependency
              ^^^^^^^^^^^^^^^^^^^ Dependency version specification is required.
            end
        "#});
    }

    #[test]
    fn silent_when_block_has_no_args() {
        // `(args (arg $_))` requires exactly one arg; a no-arg block yields none.
        test::<DependencyVersion>().expect_no_offenses(indoc! {r#"
            Gem::Specification.new do
              add_dependency 'x'
            end
        "#});
    }

    // ----- EnforcedStyle: forbidden -----

    #[test]
    fn forbidden_allows_add_dependency_without_version() {
        test::<DependencyVersion>()
            .with_options(&DependencyVersionOptions {
                enforced_style: DependencyVersionStyle::Forbidden,
                allowed_gems: Vec::new(),
            })
            .expect_no_offenses(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.add_dependency 'rubocop'
                end
            "#});
    }

    #[test]
    fn forbidden_flags_version_spec_over_whole_node() {
        test::<DependencyVersion>()
            .with_options(&DependencyVersionOptions {
                enforced_style: DependencyVersionStyle::Forbidden,
                allowed_gems: Vec::new(),
            })
            .expect_offense(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.add_dependency 'rubocop', '~> 1.12'
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is forbidden.
                end
            "#});
    }

    #[test]
    fn forbidden_flags_commit_reference() {
        test::<DependencyVersion>()
            .with_options(&DependencyVersionOptions {
                enforced_style: DependencyVersionStyle::Forbidden,
                allowed_gems: Vec::new(),
            })
            .expect_offense(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.add_dependency 'x', tag: 'v1.0'
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is forbidden.
                end
            "#});
    }

    // ----- AllowedGems -----

    #[test]
    fn allowed_gem_is_exempt_under_required() {
        test::<DependencyVersion>()
            .with_options(&DependencyVersionOptions {
                enforced_style: DependencyVersionStyle::Required,
                allowed_gems: vec!["rubocop".to_string()],
            })
            .expect_no_offenses(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.add_dependency 'rubocop'
                end
            "#});
    }

    #[test]
    fn allowed_gem_is_exempt_under_forbidden() {
        test::<DependencyVersion>()
            .with_options(&DependencyVersionOptions {
                enforced_style: DependencyVersionStyle::Forbidden,
                allowed_gems: vec!["allowed_gem".to_string()],
            })
            .expect_no_offenses(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.add_dependency 'allowed_gem', '~> 2.0'
                end
            "#});
    }

    #[test]
    fn non_allowed_gem_with_dynamic_name_still_flagged_under_required() {
        // Non-str first arg → first_arg_str None → not exempt → flagged.
        test::<DependencyVersion>()
            .with_options(&DependencyVersionOptions {
                enforced_style: DependencyVersionStyle::Required,
                allowed_gems: vec!["foo".to_string()],
            })
            .expect_offense(indoc! {r#"
                Gem::Specification.new do |spec|
                  spec.add_dependency DYNAMIC
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Dependency version specification is required.
                end
            "#});
    }
}
