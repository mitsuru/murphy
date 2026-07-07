//! `Style/OneClassPerFile` — checks that each source file defines at most one
//! top-level class or module.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/OneClassPerFile
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags the 2nd and subsequent non-allowed top-level class/module
//!   definitions in a file. A definition is top-level when it is the arena
//!   root (single statement) or a direct child of the root statement list
//!   (a `Begin` node), mirroring RuboCop's `top_level_definition?`
//!   (`node.parent&.begin_type? ? node.parent.root? : node.root?`).
//!   Offense range is node-start..name-end, matching `loc.name.end_pos`.
//!   `AllowedClasses` matches the short (last-segment) const name and excludes
//!   matched definitions from the count, like RuboCop. The `Exclude:`
//!   spec/test default is config-driven (default.yml), not implemented in-cop.
//!   No autocorrect in RuboCop, so this is full parity.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const MSG: &str = "Do not define multiple classes/modules at the top level in a single file.";

#[derive(Default)]
pub struct OneClassPerFile;

#[derive(CopOptions)]
pub struct OneClassPerFileOptions {
    #[option(
        name = "AllowedClasses",
        default = [],
        description = "Class/module short names that are excluded from the per-file count."
    )]
    pub allowed_classes: Vec<String>,
}

#[cop(
    name = "Style/OneClassPerFile",
    description = "Checks that each source file defines at most one top-level class or module.",
    default_severity = "warning",
    default_enabled = false,
    options = OneClassPerFileOptions
)]
impl OneClassPerFile {
    // Dispatch once on the root statement list rather than per class/module
    // node — otherwise each dispatch re-walks the list from the beginning to
    // count preceding definitions, giving O(N^2) CPU cost on files with many
    // top-level definitions (Codex 2026-07-02 attacker-controlled-source DoS
    // finding, murphy-hup1).
    //
    // A single top-level definition makes the arena root the class/module
    // itself (not a `Begin`), so this callback never fires and no offense is
    // emitted — matching the "at most one" semantics. Only a root statement
    // list can hold multiple definitions, and the root statement list is a
    // `Begin` (not `Kwbegin`, which is `begin...end` and only appears as a
    // nested expression).
    #[on_node(kind = "begin")]
    fn check_root_statement_list(&self, node: NodeId, cx: &Cx<'_>) {
        if node != cx.root() {
            return;
        }
        let NodeKind::Begin(list) = cx.kind(node) else {
            return;
        };
        let opts = cx.options_or_default::<OneClassPerFileOptions>();

        // RuboCop fires once `@top_level_definitions.length > 1`, i.e. for
        // every non-allowed top-level definition after the first in source
        // order. `AllowedClasses` entries are neither counted as "the first"
        // nor flagged themselves, matching RuboCop's exclusion.
        let mut seen_top_level_definition = false;
        for &child in cx.list(*list) {
            if !is_class_or_module(child, cx) || is_allowed(child, cx, &opts) {
                continue;
            }
            if seen_top_level_definition {
                cx.emit_offense(offense_range(child, cx), MSG, None);
            }
            seen_top_level_definition = true;
        }
    }
}

fn is_class_or_module(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Class { .. } | NodeKind::Module { .. }
    )
}

/// Checks whether the definition's short (last-segment) const name is listed in
/// `AllowedClasses`, mirroring `allowed_classes.include?(node.identifier.short_name)`.
fn is_allowed(node: NodeId, cx: &Cx<'_>, opts: &OneClassPerFileOptions) -> bool {
    if opts.allowed_classes.is_empty() {
        return false;
    }
    let Some(short) = short_name(node, cx) else {
        return false;
    };
    opts.allowed_classes.iter().any(|c| c == short)
}

/// The short name of a class/module definition (the const's last segment).
/// For `class Foo::Bar` the const is `(const :Bar (const :Foo))`, so the
/// outermost const's `name` symbol is `Bar`.
fn short_name<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    let name_node = match cx.kind(node) {
        NodeKind::Class { name, .. } | NodeKind::Module { name, .. } => *name,
        _ => return None,
    };
    match cx.kind(name_node) {
        NodeKind::Const { name, .. } => Some(cx.symbol_str(*name)),
        _ => None,
    }
}

/// Offense range: node start to the end of the const name node, matching
/// RuboCop's `range_between(node.source_range.begin_pos, node.loc.name.end_pos)`.
/// This excludes any superclass (`< Bar`) while including a namespaced name.
fn offense_range(node: NodeId, cx: &Cx<'_>) -> murphy_plugin_api::Range {
    let name_node = match cx.kind(node) {
        NodeKind::Class { name, .. } | NodeKind::Module { name, .. } => *name,
        _ => return cx.range(node),
    };
    murphy_plugin_api::Range {
        start: cx.range(node).start,
        end: cx.range(name_node).end,
    }
}

#[cfg(test)]
mod tests {
    use super::{OneClassPerFile, OneClassPerFileOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn accepts_single_top_level_class() {
        test::<OneClassPerFile>().expect_no_offenses("class Foo\nend\n");
    }

    #[test]
    fn accepts_single_top_level_module() {
        test::<OneClassPerFile>().expect_no_offenses("module Foo\nend\n");
    }

    #[test]
    fn flags_second_and_third_top_level_class() {
        test::<OneClassPerFile>().expect_offense(indoc! {"
            class Foo
            end

            class Bar
            ^^^^^^^^^ Do not define multiple classes/modules at the top level in a single file.
            end

            class Baz
            ^^^^^^^^^ Do not define multiple classes/modules at the top level in a single file.
            end
        "});
    }

    #[test]
    fn flags_mixed_class_and_module() {
        test::<OneClassPerFile>().expect_offense(indoc! {"
            class Foo
            end

            module Bar
            ^^^^^^^^^^ Do not define multiple classes/modules at the top level in a single file.
            end
        "});
    }

    #[test]
    fn excludes_superclass_from_offense_range() {
        test::<OneClassPerFile>().expect_offense(indoc! {"
            class Foo
            end

            class Bar < StandardError
            ^^^^^^^^^ Do not define multiple classes/modules at the top level in a single file.
            end
        "});
    }

    #[test]
    fn includes_namespace_in_offense_range() {
        test::<OneClassPerFile>().expect_offense(indoc! {"
            class Foo
            end

            class Baz::Qux
            ^^^^^^^^^^^^^^ Do not define multiple classes/modules at the top level in a single file.
            end
        "});
    }

    #[test]
    fn accepts_nested_classes_within_single_top_level() {
        test::<OneClassPerFile>().expect_no_offenses(indoc! {"
            class Foo
              class Inner
              end

              class Other
              end
            end
        "});
    }

    #[test]
    fn accepts_multiple_classes_within_single_module() {
        test::<OneClassPerFile>().expect_no_offenses(indoc! {"
            module Foo
              class Bar
              end

              class Baz
              end
            end
        "});
    }

    #[test]
    fn ignores_class_inside_begin_end_block() {
        // `begin...end` is Kwbegin, not the root statement list; the class
        // inside is not a top-level definition. Matches RuboCop (no offense).
        test::<OneClassPerFile>().expect_no_offenses(indoc! {"
            class Foo
            end

            begin
              class Bar
              end
            end
        "});
    }

    #[test]
    fn ignores_struct_new_assignment() {
        // `Foo = Struct.new` is a Casgn, not a class/module definition.
        test::<OneClassPerFile>().expect_no_offenses(indoc! {"
            class Foo
            end

            Bar = Struct.new(:a, :b)
        "});
    }

    #[test]
    fn ignores_singleton_class() {
        // `class << self` is Sclass, not a class definition with a name.
        test::<OneClassPerFile>().expect_no_offenses(indoc! {"
            class Foo
              class << self
                def bar; end
              end
            end
        "});
    }

    #[test]
    fn allowed_classes_excluded_from_count() {
        // With Bar allowed: Foo (0 preceding) silent, Bar allowed/skipped,
        // Baz has 1 preceding non-allowed def (Foo) → fires.
        test::<OneClassPerFile>()
            .with_options(&OneClassPerFileOptions {
                allowed_classes: vec!["Bar".to_string()],
            })
            .expect_offense(indoc! {"
                class Foo
                end

                class Bar
                end

                class Baz
                ^^^^^^^^^ Do not define multiple classes/modules at the top level in a single file.
                end
            "});
    }

    #[test]
    fn allowed_classes_matches_short_name_of_namespaced() {
        // AllowedClasses matches the short name `Bar` even for `Foo::Bar`.
        test::<OneClassPerFile>()
            .with_options(&OneClassPerFileOptions {
                allowed_classes: vec!["Bar".to_string()],
            })
            .expect_no_offenses(indoc! {"
                class Foo
                end

                class Baz::Bar
                end
            "});
    }
}
murphy_plugin_api::submit_cop!(OneClassPerFile);
