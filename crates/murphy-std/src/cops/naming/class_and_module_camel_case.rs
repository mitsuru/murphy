//! `Naming/ClassAndModuleCamelCase` — flag class/module names that contain
//! underscores (snake_case) instead of CamelCase.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Naming/ClassAndModuleCamelCase
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Faithful port of RuboCop's `on_class` (aliased to `on_module`):
//!
//!     return unless node.loc.name.source.include?('_')
//!     allowed = /#{cop_config['AllowedNames'].join('|')}/
//!     name = node.loc.name.source.gsub(allowed, '')
//!     return unless name.include?('_')
//!     add_offense(node.loc.name)
//!
//!   Murphy reads `node.loc.name.source` as the raw source of the class/module
//!   name node (`Const`/`ConstantPath`), which spans the FULL qualified path
//!   (`Top::Sub_Name`), exactly like RuboCop's `loc.name`. The offense caret
//!   covers that full range — verified against rubocop 1.87.0:
//!     * `class Foo_Bar`        → col 7..13 (`Foo_Bar`)
//!     * `module Baz_Qux`       → col 8..14 (`Baz_Qux`)
//!     * `class Top::Sub_Name`  → col 7..19 (`Top::Sub_Name`, whole path)
//!     * `class ::Foo_Bar`      → col 7..15 (`::Foo_Bar`, cbase prefix included)
//!
//!   `AllowedNames` (default `["module_parent"]`) entries are joined with `|`
//!   into a single regex and removed from the name (RuboCop's `gsub`) before
//!   the residual underscore check. The offense range is always the original
//!   full name, never the stripped string. Custom-config behaviour verified:
//!     * `AllowedNames: ["Foo_Bar"]` on `class Foo_Bar` → no offense
//!     * `AllowedNames: ["Bar"]`     on `class Foo_Bar` → offense (`Foo_`)
//!
//!   This cop intentionally does NOT fire on plain constant assignment
//!   (`Foo_Bar = Class.new`) — RuboCop has no `on_casgn`; verified no offense.
//!
//!   Flavor caveat: the only divergence from RuboCop for *arbitrary* user
//!   `AllowedNames` is Rust-regex vs Ruby-regex syntax. The default value
//!   (`module_parent`) is plain text, so default-config parity is exact. An
//!   invalid user-supplied pattern is skipped (no removal) rather than
//!   panicking, matching murphy's `matches_any_pattern` convention.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, cop, regex::Regex};

const MSG: &str = "Use CamelCase for classes and modules.";

#[derive(Default)]
pub struct ClassAndModuleCamelCase;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AllowedNames",
        default = ["module_parent"],
        description = "Allowed class/module names (full or part of the name)."
    )]
    pub allowed_names: Vec<String>,
}

#[cop(
    name = "Naming/ClassAndModuleCamelCase",
    description = "Use CamelCase for classes and modules.",
    default_severity = "warning",
    default_enabled = true,
    options = Options
)]
impl ClassAndModuleCamelCase {
    #[on_node(kind = "class")]
    fn check_class(&self, node: NodeId, cx: &Cx<'_>) {
        self.check(node, cx);
    }

    #[on_node(kind = "module")]
    fn check_module(&self, node: NodeId, cx: &Cx<'_>) {
        self.check(node, cx);
    }
}

impl ClassAndModuleCamelCase {
    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        // The name node (`Const`/`ConstantPath`) — its range mirrors RuboCop's
        // `node.loc.name`, spanning the full qualified path.
        let Some(name_node) = class_or_module_name(node, cx) else {
            return;
        };
        let name_range = cx.range(name_node);
        let name_src = cx.raw_source(name_range);

        // Fast path: no underscore → never an offense.
        if !name_src.contains('_') {
            return;
        }

        let opts = cx.options_or_default::<Options>();

        // RuboCop: `name = source.gsub(/allowed.join('|')/, '')`. Remove every
        // match of the joined-allowed regex, then re-check for an underscore.
        let residual = strip_allowed(name_src, &opts.allowed_names);
        if !residual.contains('_') {
            return;
        }

        // Offense range is the original full name, not the stripped residual.
        cx.emit_offense(name_range, MSG, None);
    }
}

/// Resolve the name node of a `class`/`module` definition, matching RuboCop's
/// `node.loc.name`. Returns `None` if the kind is neither (defensive).
fn class_or_module_name(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    use murphy_plugin_api::NodeKind;
    match *cx.kind(node) {
        NodeKind::Class { name, .. } | NodeKind::Module { name, .. } => Some(name),
        _ => None,
    }
}

/// Port of RuboCop's `source.gsub(/#{AllowedNames.join('|')}/, '')`.
///
/// Joins the allowed names with `|` into a single regex and removes every
/// match from `name`. An empty list yields an empty regex (matches the empty
/// string everywhere → removes nothing, like Ruby). An invalid regex is
/// skipped (no removal) rather than panicking, mirroring murphy's
/// `matches_any_pattern` convention.
fn strip_allowed<'a>(name: &'a str, allowed: &[String]) -> std::borrow::Cow<'a, str> {
    if allowed.is_empty() {
        return std::borrow::Cow::Borrowed(name);
    }
    let joined = allowed.join("|");
    match Regex::new(&joined) {
        Ok(re) => re.replace_all(name, ""),
        Err(_) => std::borrow::Cow::Borrowed(name),
    }
}

#[cfg(test)]
mod tests {
    use super::{ClassAndModuleCamelCase, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offenses (carets derived from rubocop 1.87.0 column..last_column;
    //     leading spaces = column-1, carets = last_column-column+1). ---

    #[test]
    fn flags_snake_case_class_name() {
        // rubocop: line 1, col 7..13 (`Foo_Bar`)
        test::<ClassAndModuleCamelCase>().expect_offense(indoc! {r#"
            class Foo_Bar
                  ^^^^^^^ Use CamelCase for classes and modules.
            end
        "#});
    }

    #[test]
    fn flags_snake_case_module_name() {
        // rubocop: line 1, col 8..14 (`Baz_Qux`)
        test::<ClassAndModuleCamelCase>().expect_offense(indoc! {r#"
            module Baz_Qux
                   ^^^^^^^ Use CamelCase for classes and modules.
            end
        "#});
    }

    #[test]
    fn flags_scoped_name_full_path() {
        // rubocop: line 1, col 7..19 (`Top::Sub_Name`) — the WHOLE qualified
        // path, not just the snake_case leaf.
        test::<ClassAndModuleCamelCase>().expect_offense(indoc! {r#"
            class Top::Sub_Name
                  ^^^^^^^^^^^^^ Use CamelCase for classes and modules.
            end
        "#});
    }

    #[test]
    fn flags_cbase_qualified_name() {
        // rubocop: line 1, col 7..15 (`::Foo_Bar`) — `loc.name.source`
        // includes the leading `::`. Murphy's name-node range matches.
        test::<ClassAndModuleCamelCase>().expect_offense(indoc! {r#"
            class ::Foo_Bar
                  ^^^^^^^^^ Use CamelCase for classes and modules.
            end
        "#});
    }

    #[test]
    fn flags_class_with_superclass() {
        // The superclass clause does not affect the name range.
        test::<ClassAndModuleCamelCase>().expect_offense(indoc! {r#"
            class Foo_Bar < Base
                  ^^^^^^^ Use CamelCase for classes and modules.
            end
        "#});
    }

    // --- no offenses ---

    #[test]
    fn accepts_camel_case_class() {
        test::<ClassAndModuleCamelCase>().expect_no_offenses(indoc! {r#"
            class FooBar
            end
        "#});
    }

    #[test]
    fn accepts_camel_case_module() {
        test::<ClassAndModuleCamelCase>().expect_no_offenses(indoc! {r#"
            module Normal
            end
        "#});
    }

    #[test]
    fn accepts_scoped_camel_case() {
        test::<ClassAndModuleCamelCase>().expect_no_offenses(indoc! {r#"
            class Top::SubName
            end
        "#});
    }

    #[test]
    fn ignores_constant_assignment() {
        // RuboCop has no `on_casgn`; `Foo_Bar = Class.new` does NOT fire.
        test::<ClassAndModuleCamelCase>().expect_no_offenses("Foo_Bar = Class.new\n");
    }

    // --- AllowedNames option ---

    #[test]
    fn allowed_name_strips_whole_name() {
        // `AllowedNames: ["Foo_Bar"]` removes the whole name → no residual
        // underscore → no offense. Verified against rubocop 1.87.0.
        test::<ClassAndModuleCamelCase>()
            .with_options(&Options {
                allowed_names: vec!["Foo_Bar".to_string()],
            })
            .expect_no_offenses(indoc! {r#"
                class Foo_Bar
                end
            "#});
    }

    #[test]
    fn allowed_name_partial_strip_still_offends() {
        // `AllowedNames: ["Bar"]` strips only `Bar`, leaving `Foo_` with an
        // underscore → offense on the ORIGINAL full name (col 7..13).
        // Verified against rubocop 1.87.0.
        test::<ClassAndModuleCamelCase>()
            .with_options(&Options {
                allowed_names: vec!["Bar".to_string()],
            })
            .expect_offense(indoc! {r#"
                class Foo_Bar
                      ^^^^^^^ Use CamelCase for classes and modules.
                end
            "#});
    }

    #[test]
    fn default_allowed_names_does_not_suppress() {
        // Default `["module_parent"]` does not appear in `Foo_Bar`, so the
        // offense still fires under default config.
        test::<ClassAndModuleCamelCase>()
            .with_options(&Options::default())
            .expect_offense(indoc! {r#"
                class Foo_Bar
                      ^^^^^^^ Use CamelCase for classes and modules.
                end
            "#});
    }

    #[test]
    fn empty_allowed_names_strips_nothing() {
        // Empty list → no removal → snake_case name still offends.
        test::<ClassAndModuleCamelCase>()
            .with_options(&Options {
                allowed_names: vec![],
            })
            .expect_offense(indoc! {r#"
                class Foo_Bar
                      ^^^^^^^ Use CamelCase for classes and modules.
                end
            "#});
    }
}
murphy_plugin_api::submit_cop!(ClassAndModuleCamelCase);
