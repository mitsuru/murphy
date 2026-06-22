//! `Gemspec/DevelopmentDependencies` — checks that development dependencies are
//! specified in the location dictated by `EnforcedStyle` (`Gemfile`, `gems.rb`,
//! or `gemspec`) rather than wherever they currently appear. The cop runs only
//! on `*.gemspec` / `Gemfile` / `gems.rb` files; the host applies the per-cop
//! `Include` from `config/default.yml`, so this cop never inspects the filename
//! itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Gemspec/DevelopmentDependencies
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop v1.87.0 (`gemspec/development_dependencies.rb`) exactly.
//!   `RESTRICT_ON_SEND = %i[add_development_dependency gem]` and the style
//!   selects which call shape is an offense:
//!
//!     - `:Gemfile` / `:'gems.rb'` → flag
//!       `(send _ :add_development_dependency (str #forbidden_gem? ...) _? _?)`:
//!       any receiver (including none), method `add_development_dependency`,
//!       first arg a `str` whose value is a forbidden gem, plus 0–2 more args
//!       (so 1–3 total arguments — a 4-arg call does NOT match).
//!     - `:gemspec` → flag `(send _ :gem (str #forbidden_gem? ...))`: any
//!       receiver, method `gem`, EXACTLY one `str` arg (so `gem 'x', '~> 1'`
//!       does NOT match — this asymmetry with `add_development_dependency` is
//!       real and pinned by tests).
//!
//!   `_` for the receiver matches anything, so both bare
//!   `add_development_dependency 'x'` and `spec.add_development_dependency 'x'`
//!   match; receiver is not filtered. The first argument must be a plain `Str`
//!   (no paren-unwrap, matching the strict `str` pattern). `forbidden_gem?` is
//!   `!AllowedGems.include?(name)`, so the default empty `AllowedGems` forbids
//!   every gem; a name listed in `AllowedGems` is exempt.
//!
//!   Message is RuboCop's `MSG` (`'Specify development dependencies in
//!   %<preferred>s.'`) with `preferred: style`, i.e. the literal style string
//!   (`Gemfile` / `gems.rb` / `gemspec`, exact casing) is embedded:
//!   ``Specify development dependencies in Gemfile.`` etc. The offense range is
//!   the whole send node (`add_offense(node)`), so the caret spans the receiver
//!   (when present) through the last argument.
//!
//!   `Enabled: pending` in `config/default.yml` → `default_enabled = false`.
//!   `EnforcedStyle: Gemfile` is the default. Behaviour, per-style message
//!   text, the 1–3 arg `add_development_dependency` / exactly-1-arg `gem`
//!   asymmetry, the any-receiver match, and the `AllowedGems` exemption were all
//!   verified against standalone rubocop 1.87.0 on a sample gemspec.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct (ADR 0035).
#[derive(Default)]
pub struct DevelopmentDependencies;

/// Options for [`DevelopmentDependencies`].
///
/// `EnforcedStyle` mirrors RuboCop (`SupportedStyles: [Gemfile, gems.rb,
/// gemspec]`, default `Gemfile`); `AllowedGems` defaults to `[]`.
#[derive(CopOptions)]
pub struct DevelopmentDependenciesOptions {
    #[option(
        name = "EnforcedStyle",
        default = "Gemfile",
        description = "Where development dependencies should be specified."
    )]
    pub enforced_style: EnforcedStyle,

    #[option(
        name = "AllowedGems",
        default = [],
        description = "Gems that are exempt from the development-dependency location check."
    )]
    pub allowed_gems: Vec<String>,
}

/// `SupportedStyles: [Gemfile, gems.rb, gemspec]`. The string values are
/// embedded verbatim in the offense message, so casing is load-bearing.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnforcedStyle {
    #[option(value = "Gemfile")]
    Gemfile,
    #[option(value = "gems.rb")]
    GemsRb,
    #[option(value = "gemspec")]
    Gemspec,
}

#[cop(
    name = "Gemspec/DevelopmentDependencies",
    description = "Checks that development dependencies are specified in Gemfile rather than gemspec.",
    default_severity = "warning",
    default_enabled = false,
    options = DevelopmentDependenciesOptions,
)]
impl DevelopmentDependencies {
    // `methods` mirrors upstream `RESTRICT_ON_SEND = %i[add_development_dependency gem]`.
    // Dispatching on `kind = "send"` excludes the safe-navigation `&.` form (a
    // `CSend` node), matching RuboCop's `on_send`, which does not fire on csend.
    #[on_node(kind = "send", methods = ["add_development_dependency", "gem"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<DevelopmentDependenciesOptions>();
        let Some(method) = cx.method_name(node) else {
            return;
        };

        // The style selects which call shape is an offense. Only one method is
        // relevant per run, matching RuboCop's `case style` dispatch.
        let matches = match opts.enforced_style {
            EnforcedStyle::Gemfile | EnforcedStyle::GemsRb => {
                method == "add_development_dependency"
                    && is_forbidden_dependency_call(node, /* exact_one_arg = */ false, &opts, cx)
            }
            EnforcedStyle::Gemspec => {
                method == "gem"
                    && is_forbidden_dependency_call(node, /* exact_one_arg = */ true, &opts, cx)
            }
        };

        if matches {
            let message = format!(
                "Specify development dependencies in {}.",
                opts.enforced_style.as_str()
            );
            cx.emit_offense(cx.range(node), &message, None);
        }
    }
}

/// Whether `node`'s arguments match the RuboCop pattern for a forbidden
/// development-dependency declaration.
///
/// Both patterns require the first argument to be a plain `Str` whose value is
/// a forbidden gem (not in `AllowedGems`). The argument-count constraint
/// differs by call:
///   - `add_development_dependency`: `(str ...) _? _?` → 1–3 total args
///     (`exact_one_arg = false`).
///   - `gem`: `(str ...)` → EXACTLY 1 arg (`exact_one_arg = true`).
fn is_forbidden_dependency_call(
    node: NodeId,
    exact_one_arg: bool,
    opts: &DevelopmentDependenciesOptions,
    cx: &Cx<'_>,
) -> bool {
    let args = cx.call_arguments(node);
    let arg_count = args.len();
    let count_ok = if exact_one_arg {
        arg_count == 1
    } else {
        (1..=3).contains(&arg_count)
    };
    if !count_ok {
        return false;
    }

    // First argument must be a plain `Str` (matching the strict `str` pattern —
    // deliberately no paren-unwrap). `gem ('x')` / `gem name` do not match.
    let NodeKind::Str(id) = *cx.kind(args[0]) else {
        return false;
    };
    let name = cx.string_str(id);

    // `forbidden_gem?` = name NOT in AllowedGems.
    !opts.allowed_gems.iter().any(|allowed| allowed == name)
}

murphy_plugin_api::submit_cop!(DevelopmentDependencies);

#[cfg(test)]
mod tests {
    use super::{
        DevelopmentDependencies as Cop, DevelopmentDependenciesOptions, EnforcedStyle,
    };
    use murphy_plugin_api::test_support::{indoc, test};

    fn gemspec_style() -> DevelopmentDependenciesOptions {
        DevelopmentDependenciesOptions {
            enforced_style: EnforcedStyle::Gemspec,
            allowed_gems: Vec::new(),
        }
    }

    fn gems_rb_style() -> DevelopmentDependenciesOptions {
        DevelopmentDependenciesOptions {
            enforced_style: EnforcedStyle::GemsRb,
            allowed_gems: Vec::new(),
        }
    }

    fn allowed(gems: &[&str]) -> DevelopmentDependenciesOptions {
        DevelopmentDependenciesOptions {
            enforced_style: EnforcedStyle::Gemfile,
            allowed_gems: gems.iter().map(|s| s.to_string()).collect(),
        }
    }

    // === default (Gemfile) style: flags add_development_dependency ===

    #[test]
    fn gemfile_flags_add_development_dependency_with_receiver() {
        test::<Cop>().expect_offense(indoc! {r#"
            spec.add_development_dependency "rspec"
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Specify development dependencies in Gemfile.
        "#});
    }

    #[test]
    fn gemfile_flags_bare_add_development_dependency() {
        // `_` receiver matches none, so a bare call is flagged too.
        test::<Cop>().expect_offense(indoc! {r#"
            add_development_dependency "rspec"
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Specify development dependencies in Gemfile.
        "#});
    }

    #[test]
    fn gemfile_flags_two_and_three_args() {
        // `(str ...) _? _?` → 2 and 3 total args both match.
        test::<Cop>().expect_offense(indoc! {r#"
            spec.add_development_dependency "rake", "~> 13.0"
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Specify development dependencies in Gemfile.
            spec.add_development_dependency "a", "1", "2"
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Specify development dependencies in Gemfile.
        "#});
    }

    #[test]
    fn gemfile_ignores_four_args() {
        // `(str ...) _? _?` caps at 3 args; a 4-arg call does NOT match.
        test::<Cop>().expect_no_offenses("spec.add_development_dependency \"b\", \"1\", \"2\", \"3\"\n");
    }

    #[test]
    fn gemfile_ignores_gem_calls() {
        // In Gemfile style only `add_development_dependency` is checked.
        test::<Cop>().expect_no_offenses("spec.gem \"rspec\"\n");
    }

    #[test]
    fn gemfile_ignores_non_string_first_arg() {
        // The strict `str` pattern: a non-literal first arg does not match.
        test::<Cop>().expect_no_offenses("spec.add_development_dependency name\n");
    }

    #[test]
    fn gemfile_ignores_zero_args() {
        test::<Cop>().expect_no_offenses("spec.add_development_dependency\n");
    }

    // === gems.rb style: same as Gemfile but different message ===

    #[test]
    fn gems_rb_flags_add_development_dependency_with_message() {
        test::<Cop>()
            .with_options(&gems_rb_style())
            .expect_offense(indoc! {r#"
                spec.add_development_dependency "rspec"
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Specify development dependencies in gems.rb.
            "#});
    }

    // === gemspec style: flags gem (exactly 1 arg) ===

    #[test]
    fn gemspec_flags_single_arg_gem() {
        test::<Cop>()
            .with_options(&gemspec_style())
            .expect_offense(indoc! {r#"
                spec.gem "rspec"
                ^^^^^^^^^^^^^^^^ Specify development dependencies in gemspec.
            "#});
    }

    #[test]
    fn gemspec_flags_bare_gem() {
        test::<Cop>()
            .with_options(&gemspec_style())
            .expect_offense(indoc! {r#"
                gem "rspec"
                ^^^^^^^^^^^ Specify development dependencies in gemspec.
            "#});
    }

    #[test]
    fn gemspec_ignores_two_arg_gem() {
        // `(send _ :gem (str ...))` → EXACTLY one arg; `gem 'x', '~> 1'` does
        // NOT match (asymmetry with add_development_dependency).
        test::<Cop>()
            .with_options(&gemspec_style())
            .expect_no_offenses("spec.gem \"rspec\", \"~> 3.0\"\n");
    }

    #[test]
    fn gemspec_ignores_add_development_dependency() {
        // In gemspec style only `gem` is checked.
        test::<Cop>()
            .with_options(&gemspec_style())
            .expect_no_offenses("spec.add_development_dependency \"rspec\"\n");
    }

    // === AllowedGems ===

    #[test]
    fn allowed_gem_is_exempt() {
        // `rspec` listed in AllowedGems → not forbidden → no offense.
        test::<Cop>()
            .with_options(&allowed(&["rspec"]))
            .expect_no_offenses("spec.add_development_dependency \"rspec\"\n");
    }

    #[test]
    fn non_allowed_gem_still_flagged_with_allowlist() {
        test::<Cop>()
            .with_options(&allowed(&["rspec"]))
            .expect_offense(indoc! {r#"
                spec.add_development_dependency "rake"
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Specify development dependencies in Gemfile.
            "#});
    }
}
