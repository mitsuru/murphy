//! `Gemspec/RubyVersionGlobalsUsage` — flag any use of the `RUBY_VERSION`
//! (or `Ruby::VERSION`) constant inside a gemspec. `rake release` runs under
//! whatever Ruby the maintainer happens to have active, so a gemspec that reads
//! `RUBY_VERSION` bakes the *releaser's* environment into the published gem
//! rather than the user's. The cop runs only on `*.gemspec` files; the host
//! applies the per-cop `Include` from `config/default.yml`, so this cop never
//! inspects the filename itself.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Gemspec/RubyVersionGlobalsUsage
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop v1.87.0 (`gemspec/ruby_version_globals_usage.rb`). One
//!   check, one message, no autocorrect. RuboCop's `on_const` fires
//!   `add_offense(node, message: format(MSG, ruby_version: node.source))` when
//!   `gem_spec_with_ruby_version?(node)` holds, where that guard is
//!   `gem_specification(processed_source.ast) && ruby_version?(node)`.
//!
//!   GATE IS A NO-OP (verified against standalone rubocop 1.87.0): RuboCop's
//!   `gem_specification` is a `def_node_search` (no `?`) called WITHOUT a block,
//!   which returns an `Enumerator` — always truthy — so the
//!   `gem_specification(...) &&` conjunct never short-circuits. Empirically a
//!   gemspec containing `puts RUBY_VERSION` and NO `Gem::Specification.new`
//!   block still fires. Murphy therefore does NOT gate on a spec block; it
//!   walks every `Const` node, exactly mirroring observed behaviour and the
//!   sibling `Gemspec/RequiredRubyVersion`'s gate-free whole-AST walk.
//!
//!   `ruby_version?` matches RuboCop's
//!   `{ (const {cbase nil?} :RUBY_VERSION)
//!      (const (const {cbase nil?} :Ruby) :VERSION) }`:
//!   (a) a global const named `RUBY_VERSION` (`cx.is_global_const`, which is
//!       exactly `(const {nil? cbase} :RUBY_VERSION)`), OR
//!   (b) a const named `VERSION` whose scope is a global const named `Ruby`.
//!   The inner `Ruby` const never self-matches (its name is `Ruby`, not
//!   `VERSION`/`RUBY_VERSION`), so `Ruby::VERSION` is flagged once, not twice
//!   (locked in by a dedicated test).
//!
//!   Message uses `cx.raw_source(cx.range(node))` to reproduce RuboCop's
//!   `node.source` byte-for-byte, so `RUBY_VERSION`, `::RUBY_VERSION`, and
//!   `Ruby::VERSION` each render their literal spelling (all three verified
//!   against standalone rubocop 1.87.0, including the `::`-rooted spelling).
//!   Offense range is the whole const node (`cx.range(node)`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct RubyVersionGlobalsUsage;

#[cop(
    name = "Gemspec/RubyVersionGlobalsUsage",
    description = "Checks usage of RUBY_VERSION in gemspec.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl RubyVersionGlobalsUsage {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let root = cx.root();
        for node in std::iter::once(root).chain(cx.descendants(root)) {
            if !is_ruby_version_const(node, cx) {
                continue;
            }
            let source = cx.raw_source(cx.range(node));
            let message = format!("Do not use `{source}` in gemspec file.");
            cx.emit_offense(cx.range(node), &message, None);
        }
    }
}

/// RuboCop's `ruby_version?` matcher:
/// `{ (const {cbase nil?} :RUBY_VERSION)
///    (const (const {cbase nil?} :Ruby) :VERSION) }`.
fn is_ruby_version_const(node: NodeId, cx: &Cx<'_>) -> bool {
    // (const {cbase nil?} :RUBY_VERSION)
    if cx.is_global_const(node, "RUBY_VERSION") {
        return true;
    }
    // (const (const {cbase nil?} :Ruby) :VERSION)
    let NodeKind::Const { scope, name } = *cx.kind(node) else {
        return false;
    };
    if cx.symbol_str(name) != "VERSION" {
        return false;
    }
    scope
        .get()
        .is_some_and(|inner| cx.is_global_const(inner, "Ruby"))
}

murphy_plugin_api::submit_cop!(RubyVersionGlobalsUsage);

#[cfg(test)]
mod tests {
    use super::RubyVersionGlobalsUsage;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_ruby_version_constant() {
        test::<RubyVersionGlobalsUsage>().expect_offense(indoc! {r#"
            spec.required_ruby_version = RUBY_VERSION
                                         ^^^^^^^^^^^^ Do not use `RUBY_VERSION` in gemspec file.
        "#});
    }

    #[test]
    fn flags_bare_ruby_version_reference() {
        // Gate is a no-op: fires even with no `Gem::Specification.new` block.
        test::<RubyVersionGlobalsUsage>().expect_offense(indoc! {r#"
            puts RUBY_VERSION
                 ^^^^^^^^^^^^ Do not use `RUBY_VERSION` in gemspec file.
        "#});
    }

    #[test]
    fn flags_cbase_rooted_ruby_version() {
        // `::RUBY_VERSION` — message renders the literal `::`-rooted source.
        test::<RubyVersionGlobalsUsage>().expect_offense(indoc! {r#"
            puts ::RUBY_VERSION
                 ^^^^^^^^^^^^^^ Do not use `::RUBY_VERSION` in gemspec file.
        "#});
    }

    #[test]
    fn flags_ruby_version_scoped_const() {
        // `Ruby::VERSION` — flagged ONCE (the inner `Ruby` const must not match).
        test::<RubyVersionGlobalsUsage>().expect_offense(indoc! {r#"
            puts Ruby::VERSION
                 ^^^^^^^^^^^^^ Do not use `Ruby::VERSION` in gemspec file.
        "#});
    }

    #[test]
    fn flags_cbase_rooted_ruby_scoped_const() {
        // `::Ruby::VERSION` — scope is a cbase-rooted global `Ruby`.
        test::<RubyVersionGlobalsUsage>().expect_offense(indoc! {r#"
            puts ::Ruby::VERSION
                 ^^^^^^^^^^^^^^^ Do not use `::Ruby::VERSION` in gemspec file.
        "#});
    }

    #[test]
    fn flags_inside_gem_specification_block() {
        test::<RubyVersionGlobalsUsage>().expect_offense(indoc! {r#"
            Gem::Specification.new do |spec|
              spec.required_ruby_version = RUBY_VERSION
                                           ^^^^^^^^^^^^ Do not use `RUBY_VERSION` in gemspec file.
            end
        "#});
    }

    #[test]
    fn ignores_unrelated_constants() {
        test::<RubyVersionGlobalsUsage>().expect_no_offenses(indoc! {r#"
            spec.required_ruby_version = '>= 2.5.0'
            puts VERSION
            puts Other::VERSION
            puts Ruby::PATCHLEVEL
        "#});
    }

    #[test]
    fn ignores_ruby_version_local_variable() {
        test::<RubyVersionGlobalsUsage>().expect_no_offenses(indoc! {r#"
            ruby_version = '2.5'
            spec.required_ruby_version = ruby_version
        "#});
    }

    #[test]
    fn ignores_nested_ruby_const_other_member() {
        // A const named `Ruby` that is NOT followed by `::VERSION`.
        test::<RubyVersionGlobalsUsage>().expect_no_offenses(indoc! {r#"
            puts Foo::Ruby
        "#});
    }
}
