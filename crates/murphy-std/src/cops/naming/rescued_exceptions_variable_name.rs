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
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Full parity with RuboCop 1.87.0, verified against the standalone gem.
//!   Mirrors `on_resbody`: read the exception variable name
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
//!   Autocorrect mirrors RuboCop's `AutoCorrector`: replace the exception
//!   variable binding, then rename matching `lvar` reads across the rescue
//!   body AND the right-siblings of the enclosing `kwbegin` (explicit
//!   `begin...end`, detected as `Kwbegin` or a `Begin` whose source starts
//!   with the `begin` keyword). Ruby 3.1 value-omitted hash pairs
//!   (`do_something(error:)`, whose value lowers to `Unknown`) are expanded
//!   to `error: e` via a zero-width insert, matching RuboCop's
//!   `insert_after(operator, " e")`. A matching `lvasgn`/`masgn` reassignment
//!   corrects only its RHS and then stops propagation within that subtree
//!   (`break`), so later reads keep the old name — verified against the
//!   `error = { error_message: error.message }` and
//!   `error, foo = 1, error` specs.
//! ```
//!
//! ## Offense shape
//!
//! A `Resbody` whose `var` binding is a value-less assignment node
//! (`Lvasgn`/`Ivasgn`/`Gvasgn`/`Cvasgn`/`Casgn`). The variable name (including
//! any sigil, or the leaf for a constant target) is compared against
//! `PreferredName`; a leading underscore on the offending name carries through
//! to the preferred name (`_foo` → `_e`).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

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
        let NodeKind::Resbody { var, body, .. } = *cx.kind(node) else {
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
        let var_range = cx.range(var_id);
        cx.emit_offense(var_range, &message, None);
        // RuboCop's `autocorrect`: replace the binding, rename uses in the
        // rescue body, then rename uses in the right-siblings of the enclosing
        // `kwbegin`.
        cx.emit_edit(var_range, &preferred);
        if let Some(body_id) = body.get() {
            correct_node(body_id, offending_name, &preferred, cx);
        }
        if let Some(kwbegin) = enclosing_kwbegin(node, cx) {
            for sibling in right_siblings(kwbegin, cx) {
                correct_node(sibling, offending_name, &preferred, cx);
            }
        }
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
    cx.descendants(resbody)
        .into_iter()
        .any(|d| matches!(*cx.kind(d), NodeKind::Lvar(name) if cx.symbol_str(name) == base))
}

/// First ancestor that is an explicit `begin...end` block — parser-gem's
/// `:kwbegin`. Murphy's translator lowers explicit `begin` to `Begin`, so
/// accept `Kwbegin` plus any `Begin` whose source starts with the `begin`
/// keyword (word boundary). Mirrors
/// `Layout/RescueEnsureAlignment::is_kwbegin`.
fn enclosing_kwbegin(resbody: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    cx.ancestors(resbody)
        .find(|&ancestor| is_kwbegin(ancestor, cx))
}

fn is_kwbegin(id: NodeId, cx: &Cx<'_>) -> bool {
    if matches!(*cx.kind(id), NodeKind::Kwbegin(_)) {
        return true;
    }
    if !matches!(*cx.kind(id), NodeKind::Begin(_)) {
        return false;
    }
    let src = cx.raw_source(cx.range(id));
    src.strip_prefix("begin").is_some_and(|rest| {
        rest.is_empty() || {
            let b = rest.as_bytes()[0];
            !(b.is_ascii_alphanumeric() || b == b'_')
        }
    })
}

/// Nodes after `kwbegin` within its parent — RuboCop's
/// `kwbegin_node.right_siblings`.
fn right_siblings(kwbegin: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    let Some(parent) = cx.parent(kwbegin).get() else {
        return Vec::new();
    };
    let kids = cx.children(parent);
    match kids.iter().position(|&c| c == kwbegin) {
        Some(idx) => kids[idx + 1..].to_vec(),
        None => Vec::new(),
    }
}

/// RuboCop's `variable_name_matches?`: for `masgn`, true when any descendant
/// `lvasgn` matches; otherwise a direct name comparison. Only `Lvar`/`Lvasgn`
/// carry plain local names here — sigil'd (`@foo`) and constant (`Foo`)
/// bindings never match a local read, so those uses are left alone exactly
/// like RuboCop (verified: `rescue => @foo` leaves `puts @foo` untouched).
fn variable_name_matches(id: NodeId, name: &str, cx: &Cx<'_>) -> bool {
    match *cx.kind(id) {
        NodeKind::Masgn { .. } => cx.descendants(id).into_iter().any(
            |d| matches!(*cx.kind(d), NodeKind::Lvasgn { name: n, .. } if cx.symbol_str(n) == name),
        ),
        NodeKind::Lvar(n) => cx.symbol_str(n) == name,
        NodeKind::Lvasgn { name: n, .. } => cx.symbol_str(n) == name,
        _ => false,
    }
}

/// True when `pair` uses Ruby 3.1 value omission (`{foo:}`): the value lowers
/// to `Unknown` with a range starting before the key ends. Copied from
/// `Style/HashSyntax::is_value_omitted` — the only ABI signal for an omitted
/// value.
fn is_value_omitted(pair: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Pair { key, value } = *cx.kind(pair) else {
        return false;
    };
    if !matches!(*cx.kind(value), NodeKind::Unknown) {
        return false;
    }
    let key_range = cx.range(key);
    let value_range = cx.range(value);
    value_range.start < key_range.end
}

/// True when `pair` is a value-omitted pair whose key symbol equals `name`
/// (`do_something(error:)` with offending `error`). The key stays; the fix
/// expands the value.
fn omitted_pair_key_matches(pair: NodeId, name: &str, cx: &Cx<'_>) -> bool {
    if !is_value_omitted(pair, cx) {
        return false;
    }
    let NodeKind::Pair { key, .. } = *cx.kind(pair) else {
        return false;
    };
    match *cx.kind(key) {
        NodeKind::Sym(sym) => cx.symbol_str(sym) == name,
        _ => false,
    }
}

/// RHS of a reassignment for `correct_reassignment` (`lvasgn` value /
/// `masgn` rhs). `None` for value-less targets such as `foo += 1`'s inner
/// `Lvasgn`, where there is simply nothing to recurse into.
fn reassignment_rhs(id: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(id) {
        NodeKind::Lvasgn { value, .. } => value.get(),
        NodeKind::Masgn { rhs, .. } => Some(rhs),
        _ => None,
    }
}

/// RuboCop's `correct_node`: walk `root` (inclusive) in DFS pre-order,
/// renaming matching `lvar` reads and expanding matching value-omitted pairs.
/// A matching `lvasgn`/`masgn` corrects only its RHS (`correct_reassignment`)
/// and then stops (`break`), so later reads keep the old name.
fn correct_node(root: NodeId, offending: &str, preferred: &str, cx: &Cx<'_>) {
    let mut ordered = Vec::with_capacity(32);
    ordered.push(root);
    ordered.extend(cx.descendants(root));
    for id in ordered {
        match *cx.kind(id) {
            NodeKind::Lvar(_) | NodeKind::Lvasgn { .. } | NodeKind::Masgn { .. } => {
                if !variable_name_matches(id, offending, cx) {
                    continue;
                }
                if matches!(*cx.kind(id), NodeKind::Lvar(_)) {
                    cx.emit_edit(cx.range(id), preferred);
                } else {
                    if let Some(rhs) = reassignment_rhs(id, cx) {
                        correct_node(rhs, offending, preferred, cx);
                    }
                    break;
                }
            }
            NodeKind::Pair { .. } if omitted_pair_key_matches(id, offending, cx) => {
                // Match RuboCop's `insert_after(operator, " e")`: the pair
                // expression ends at the `:` so a zero-width insert of
                // `" e"` at the end expands `error:` to `error: e`.
                let pair_range = cx.range(id);
                cx.emit_edit(
                    Range {
                        start: pair_range.end,
                        end: pair_range.end,
                    },
                    &format!(" {preferred}"),
                );
            }
            _ => {}
        }
    }
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

    // --- autocorrect (mirrors rubocop 1.87.0 expect_correction specs) ---

    #[test]
    fn corrects_binding_only() {
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                  something
                rescue MyException => exc
                                      ^^^ Use `e` instead of `exc`.
                  # do something
                end
            "#},
            indoc! {r#"
                begin
                  something
                rescue MyException => e
                  # do something
                end
            "#},
        );
    }

    #[test]
    fn corrects_underscore_binding() {
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                  something
                rescue MyException => _exc
                                      ^^^^ Use `_e` instead of `_exc`.
                  # do something
                end
            "#},
            indoc! {r#"
                begin
                  something
                rescue MyException => _e
                  # do something
                end
            "#},
        );
    }

    #[test]
    fn renames_variable_references() {
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                  something
                rescue MyException => exc
                                      ^^^ Use `e` instead of `exc`.
                  exc
                end
            "#},
            indoc! {r#"
                begin
                  something
                rescue MyException => e
                  e
                end
            "#},
        );
    }

    #[test]
    fn renames_underscore_references() {
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                  x
                rescue => _exc
                          ^^^^ Use `_e` instead of `_exc`.
                  puts _exc
                end
            "#},
            indoc! {r#"
                begin
                  x
                rescue => _e
                  puts _e
                end
            "#},
        );
    }

    #[test]
    fn corrects_explicit_hash_value() {
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                rescue => error
                          ^^^^^ Use `e` instead of `error`.
                  do_something(error: error)
                end
            "#},
            indoc! {r#"
                begin
                rescue => e
                  do_something(error: e)
                end
            "#},
        );
    }

    #[test]
    fn corrects_value_omitted_hash() {
        // `error:` omission lowers its value to `Unknown`; the fix expands to
        // `error: e`, matching RuboCop's `insert_after(operator, " e")`.
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                rescue => error
                          ^^^^^ Use `e` instead of `error`.
                  do_something(error:)
                end
            "#},
            indoc! {r#"
                begin
                rescue => e
                  do_something(error: e)
                end
            "#},
        );
    }

    #[test]
    fn corrects_method_call_receiver_reference() {
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                  get something
                rescue ActiveResource::Redirection => redirection
                                                      ^^^^^^^^^^^ Use `e` instead of `redirection`.
                  redirect_to redirection.response['Location']
                end
            "#},
            indoc! {r#"
                begin
                  get something
                rescue ActiveResource::Redirection => e
                  redirect_to e.response['Location']
                end
            "#},
        );
    }

    #[test]
    fn stops_at_lvasgn_reassignment() {
        // `error = {...}` keeps its LHS, corrects only the RHS, then stops:
        // trailing `puts error` stays.
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                def main
                  raise
                rescue StandardError => error
                                        ^^^^^ Use `e` instead of `error`.
                  error = {
                    error_message: error.message
                  }
                  puts error
                end
            "#},
            indoc! {r#"
                def main
                  raise
                rescue StandardError => e
                  error = {
                    error_message: e.message
                  }
                  puts error
                end
            "#},
        );
    }

    #[test]
    fn corrects_before_lvasgn_reassignment() {
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                def main
                  raise
                rescue StandardError => error
                                        ^^^^^ Use `e` instead of `error`.
                  message = error.message
                  puts message
                end
            "#},
            indoc! {r#"
                def main
                  raise
                rescue StandardError => e
                  message = e.message
                  puts message
                end
            "#},
        );
    }

    #[test]
    fn stops_at_masgn_reassignment() {
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                def main
                  raise
                rescue StandardError => error
                                        ^^^^^ Use `e` instead of `error`.
                  error, foo = 1, error
                  puts error
                end
            "#},
            indoc! {r#"
                def main
                  raise
                rescue StandardError => e
                  error, foo = 1, e
                  puts error
                end
            "#},
        );
    }

    #[test]
    fn corrects_non_matching_lvasgn_value() {
        // `x = error` does not reassign `error`, so later reads are renamed.
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                def main
                  raise
                rescue StandardError => error
                                        ^^^^^ Use `e` instead of `error`.
                  x = error
                  puts error
                  puts x
                end
            "#},
            indoc! {r#"
                def main
                  raise
                rescue StandardError => e
                  x = e
                  puts e
                  puts x
                end
            "#},
        );
    }

    #[test]
    fn corrects_right_siblings_of_kwbegin() {
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                  something
                rescue StandardError => e1
                                        ^^ Use `e` instead of `e1`.
                end
                foo(e1)
            "#},
            indoc! {r#"
                begin
                  something
                rescue StandardError => e
                end
                foo(e)
            "#},
        );
    }

    #[test]
    fn ignores_right_siblings_without_kwbegin() {
        // `def` with implicit rescue has no `kwbegin`, so trailing `foo(e1)`
        // keeps the old name (verified against rubocop 1.87.0).
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                def main
                  raise
                rescue StandardError => e1
                                        ^^ Use `e` instead of `e1`.
                end
                foo(e1)
            "#},
            indoc! {r#"
                def main
                  raise
                rescue StandardError => e
                end
                foo(e1)
            "#},
        );
    }

    #[test]
    fn corrects_outer_nested_rescue_reads() {
        // Only the outer resbody fires; its fix renames `e1` even inside the
        // inner rescue body, leaving `e2` alone.
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                rescue StandardError => e1
                                        ^^ Use `e` instead of `e1`.
                  begin
                    log(e1)
                  rescue StandardError => e2
                    log(e1, e2)
                  end
                end
            "#},
            indoc! {r#"
                begin
                rescue StandardError => e
                  begin
                    log(e)
                  rescue StandardError => e2
                    log(e, e2)
                  end
                end
            "#},
        );
    }

    #[test]
    fn corrects_constant_target_without_body_rename() {
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                  x
                rescue => Foo
                          ^^^ Use `e` instead of `Foo`.
                  puts Foo
                end
            "#},
            indoc! {r#"
                begin
                  x
                rescue => e
                  puts Foo
                end
            "#},
        );
    }

    #[test]
    fn corrects_scoped_constant_target() {
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                  x
                rescue => Foo::Bar
                          ^^^^^^^^ Use `e` instead of `Bar`.
                  y
                end
            "#},
            indoc! {r#"
                begin
                  x
                rescue => e
                  y
                end
            "#},
        );
    }

    #[test]
    fn corrects_sigil_binding_without_body_rename() {
        // `rescue => @foo` becomes `=> e`; the ivar read stays (RuboCop only
        // renames `lvar`/`lvasgn`/`masgn`, verified with the standalone gem).
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                  x
                rescue => @foo
                          ^^^^ Use `e` instead of `@foo`.
                  puts @foo
                end
            "#},
            indoc! {r#"
                begin
                  x
                rescue => e
                  puts @foo
                end
            "#},
        );
    }

    #[test]
    fn leaves_op_asgn_target_and_later_reads() {
        // `foo += 1` holds a matching `Lvasgn` target with no RHS: nothing in
        // the RHS to fix, then `break` stops later `puts foo` (rubocop 1.87.0).
        test::<RescuedExceptionsVariableName>().expect_correction(
            indoc! {r#"
                begin
                  x
                rescue => foo
                          ^^^ Use `e` instead of `foo`.
                  foo += 1
                  puts foo
                end
            "#},
            indoc! {r#"
                begin
                  x
                rescue => e
                  foo += 1
                  puts foo
                end
            "#},
        );
    }
}

murphy_plugin_api::submit_cop!(RescuedExceptionsVariableName);
