//! `Naming/RescuedExceptionsVariableName` — make sure rescued exception
//! variables are named as configured (`PreferredName`, default `e`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/RescuedExceptionsVariableName
//! upstream_version_checked: 1.87.0
//! version_added: "0.67"
//! safe: true
//! supports_autocorrect: false
//! status: partial
//! gap_issues:
//!   - murphy-mi7c
//! notes: >
//!   Detection is at full parity with RuboCop 1.87.0, verified against the
//!   standalone gem. Mirrors `on_resbody`: read the exception variable name
//!   from the `Resbody.var` binding (RuboCop's `node.exception_variable.name`);
//!   skip when the resbody has any ancestor resbody (`each_ancestor(:resbody)`,
//!   so nested rescues are left alone); compute the preferred name (prefixed
//!   with `_` when the offending name starts with `_`); skip when it already
//!   matches; and skip when the body shadows the *base* preferred name via a
//!   descendant `lvar` read (`shadowed_variable_name?`). The offense range is
//!   the variable's own source range (`variable.source_range`), so the sigil is
//!   included for `@e`/`$e`/`@@e` and the full `Foo::Bar` is covered for a
//!   constant target. All five binding kinds RuboCop accepts (`lvasgn`,
//!   `ivasgn`, `gvasgn`, `cvasgn`, and `casgn` — each responding to `.name`)
//!   are covered; each was checked against rubocop 1.87.0 and fires. For a
//!   scoped constant target the message names the *leaf* (`Bar`, RuboCop's
//!   `casgn.name`) while the range spans the whole `Foo::Bar`.
//!
//!   Gap (tracked in murphy-mi7c): RuboCop `extend`s `AutoCorrector` and rewrites
//!   the variable across the rescue body and the right-siblings of the enclosing
//!   `kwbegin` (with `value_omission?` and reassignment-stops-propagation
//!   handling). That corrector surface is intentionally not ported in this
//!   initial detection-only port, so `supports_autocorrect` is `false` while
//!   RuboCop's is `true`.
//! ```
//!
//! ## Offense shape
//!
//! A `Resbody` whose `var` binding is a value-less assignment node
//! (`Lvasgn`/`Ivasgn`/`Gvasgn`/`Cvasgn`/`Casgn`). The variable name (including
//! any sigil, or the leaf for a constant target) is compared against
//! `PreferredName`; a leading underscore on the offending name carries through
//! to the preferred name (`_foo` → `_e`).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct RescuedExceptionsVariableName;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "PreferredName",
        default = "e",
        description = "The required name of the rescued exception variable."
    )]
    pub preferred_name: String,
}

#[cop(
    name = "Naming/RescuedExceptionsVariableName",
    description = "Use consistent rescued exceptions variables naming.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl RescuedExceptionsVariableName {
    #[on_node(kind = "resbody")]
    fn check_resbody(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Resbody { var, .. } = *cx.kind(node) else {
            return;
        };

        // `return unless offending_name` — only resbodies that bind a named
        // variable. `rescue => Foo` (a `Casgn` constant target) has no name.
        let Some(var_id) = var.get() else {
            return;
        };
        let Some(offending_name) = binding_name(var_id, cx) else {
            return;
        };

        // `return if node.each_ancestor(:resbody).any?` — nested rescues are
        // left to the outer one so the inner variable does not shadow it.
        if cx
            .ancestors(node)
            .any(|a| matches!(cx.kind(a), NodeKind::Resbody { .. }))
        {
            return;
        }

        let opts = cx.options_or_default::<Options>();
        let base = opts.preferred_name.as_str();

        // `preferred_name`: prefix with `_` when the offending name is itself
        // underscore-prefixed (`_foo` → `_e`).
        let preferred: String = if offending_name.starts_with('_') {
            format!("_{base}")
        } else {
            base.to_owned()
        };

        // `return if preferred_name.to_sym == offending_name`.
        if preferred == offending_name {
            return;
        }

        // `return if shadowed_variable_name?(node)` — skip when a descendant
        // `lvar` read already uses the *base* preferred name.
        if shadows_base_name(node, base, cx) {
            return;
        }

        let message = format!("Use `{preferred}` instead of `{offending_name}`.");
        cx.emit_offense(cx.range(var_id), &message, None);
    }
}

/// Name of an exception-variable binding, matching the node kinds whose RuboCop
/// counterpart responds to `.name`. For sigil'd variables (`@e`/`$e`/`@@e`) the
/// sigil is part of the name; for a constant target (`rescue => Foo::Bar`) the
/// name is the *leaf* constant (`Bar`), matching RuboCop's `casgn.name` — while
/// the offense *range* (`cx.range(var_id)`) still covers the whole `Foo::Bar`,
/// matching `variable.source_range`. Returns `None` for any other kind.
fn binding_name<'a>(var_id: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(var_id) {
        NodeKind::Lvasgn { name, .. }
        | NodeKind::Ivasgn { name, .. }
        | NodeKind::Gvasgn { name, .. }
        | NodeKind::Cvasgn { name, .. }
        | NodeKind::Casgn { name, .. } => Some(cx.symbol_str(name)),
        _ => None,
    }
}

/// `shadowed_variable_name?`: true when any descendant `lvar` read inside the
/// resbody is named exactly `base` (the configured preferred name, without the
/// underscore prefix). RuboCop calls `preferred_name(n)` with an AST node whose
/// `to_s` never starts with `_`, so the comparison is always against the base
/// name.
fn shadows_base_name(resbody: NodeId, base: &str, cx: &Cx<'_>) -> bool {
    cx.descendants(resbody).into_iter().any(|d| {
        matches!(*cx.kind(d), NodeKind::Lvar(name) if cx.symbol_str(name) == base)
    })
}

#[cfg(test)]
mod tests {
    use super::{Options, RescuedExceptionsVariableName};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- core: local-variable binding (carets from rubocop 1.87.0
    //     column/last_column). ---

    #[test]
    fn flags_local_variable_binding() {
        // rubocop: line 3, col 25..32 (`bad_name`).
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue StandardError => bad_name
                                    ^^^^^^^^ Use `e` instead of `bad_name`.
              y
            end
        "#});
    }

    #[test]
    fn flags_bare_rescue_binding() {
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue => foo
                      ^^^ Use `e` instead of `foo`.
              y
            end
        "#});
    }

    #[test]
    fn no_offense_when_already_preferred() {
        test::<RescuedExceptionsVariableName>().expect_no_offenses(indoc! {r#"
            begin
              x
            rescue => e
              y
            end
        "#});
    }

    // --- underscore prefix ---

    #[test]
    fn flags_underscore_prefixed_with_underscore_preferred() {
        // rubocop: `_foo` → preferred `_e`, col 11..14.
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue => _foo
                      ^^^^ Use `_e` instead of `_foo`.
              y
            end
        "#});
    }

    #[test]
    fn no_offense_for_underscore_preferred() {
        // `_e` matches the underscore-prefixed preferred name.
        test::<RescuedExceptionsVariableName>().expect_no_offenses(indoc! {r#"
            begin
              x
            rescue => _e
              y
            end
        "#});
    }

    // --- nested rescue: only the outer is flagged ---

    #[test]
    fn flags_only_outer_in_nested_rescue() {
        // rubocop flags `foo` at line 3 col 11..13; the inner `bar` is left.
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue => foo
                      ^^^ Use `e` instead of `foo`.
              begin
                y
              rescue => bar
                z
              end
            end
        "#});
    }

    // --- shadow skip: descendant lvar read of base name ---

    #[test]
    fn skips_when_body_reads_base_name() {
        // body reads `e` → shadowed → no offense (verified against rubocop).
        test::<RescuedExceptionsVariableName>().expect_no_offenses(indoc! {r#"
            begin
              x
            rescue => foo
              e = 1
              puts e
            end
        "#});
    }

    #[test]
    fn fires_when_body_only_assigns_base_name() {
        // `e = 1` with no read is an lvasgn, not an lvar — not a shadow.
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue => foo
                      ^^^ Use `e` instead of `foo`.
              e = 1
            end
        "#});
    }

    #[test]
    fn fires_when_body_reads_unrelated_name() {
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue => foo
                      ^^^ Use `e` instead of `foo`.
              y = 1
              puts y
            end
        "#});
    }

    #[test]
    fn underscore_shadow_uses_base_name() {
        // `_foo` (preferred `_e`) but body reads base `e` → shadowed → skip.
        test::<RescuedExceptionsVariableName>().expect_no_offenses(indoc! {r#"
            begin
              x
            rescue => _foo
              e = 1
              puts e
            end
        "#});
    }

    #[test]
    fn underscore_shadow_ignores_underscore_read() {
        // `_foo` (preferred `_e`); body reads `_e` (not base `e`) → still fires.
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue => _foo
                      ^^^^ Use `_e` instead of `_foo`.
              _e = 1
              puts _e
            end
        "#});
    }

    // --- all four binding kinds (rubocop fires on each) ---

    #[test]
    fn flags_instance_variable_binding() {
        // rubocop: col 11..12 (`@e`); message bad-name includes the sigil.
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue => @e
                      ^^ Use `e` instead of `@e`.
              y
            end
        "#});
    }

    #[test]
    fn flags_global_variable_binding() {
        // rubocop: col 11..12 (`$e`).
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue => $e
                      ^^ Use `e` instead of `$e`.
              y
            end
        "#});
    }

    #[test]
    fn flags_class_variable_binding() {
        // rubocop: col 11..13 (`@@e`).
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue => @@e
                      ^^^ Use `e` instead of `@@e`.
              y
            end
        "#});
    }

    #[test]
    fn flags_constant_target_binding() {
        // `rescue => Foo` binds the exception to the constant; rubocop fires
        // with the constant as the bad name, col 11..13.
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue => Foo
                      ^^^ Use `e` instead of `Foo`.
              y
            end
        "#});
    }

    #[test]
    fn flags_scoped_constant_target_binding() {
        // `rescue => Foo::Bar`: rubocop names the leaf (`Bar`) in the message
        // but the range spans the whole `Foo::Bar` (col 11..18).
        test::<RescuedExceptionsVariableName>().expect_offense(indoc! {r#"
            begin
              x
            rescue => Foo::Bar
                      ^^^^^^^^ Use `e` instead of `Bar`.
              y
            end
        "#});
    }

    // --- exclusions ---

    #[test]
    fn ignores_rescue_without_binding() {
        test::<RescuedExceptionsVariableName>().expect_no_offenses(indoc! {r#"
            begin
              x
            rescue StandardError
              y
            end
        "#});
    }

    // --- custom PreferredName ---

    #[test]
    fn flags_against_custom_preferred_name() {
        test::<RescuedExceptionsVariableName>()
            .with_options(&Options {
                preferred_name: "exc".to_owned(),
            })
            .expect_offense(indoc! {r#"
                begin
                  x
                rescue => e
                          ^ Use `exc` instead of `e`.
                  y
                end
            "#});
    }

    #[test]
    fn no_offense_for_custom_preferred_name_match() {
        test::<RescuedExceptionsVariableName>()
            .with_options(&Options {
                preferred_name: "exc".to_owned(),
            })
            .expect_no_offenses(indoc! {r#"
                begin
                  x
                rescue => exc
                  y
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(RescuedExceptionsVariableName);
