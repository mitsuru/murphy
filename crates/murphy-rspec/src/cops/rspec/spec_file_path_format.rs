//! `RSpec/SpecFilePathFormat` — spec file paths must match the described class.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/SpecFilePathFormat
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_top_level_example_group` (`TopLevelGroup` with
//!   the `top_level_groups.one?` guard: files with zero or several
//!   top-level spec groups are unchecked) plus `example_group_arguments`
//!   (RSpec-or-bare `ExampleGroups.all` with a `Const` first arg),
//!   `ignore_metadata?` (a `type: :routing` pair anywhere in the
//!   trailing args, i.e. the default `IgnoreMetadata`), `expected_path`
//!   (enclosing `class` / `module` namespace plus the constant path,
//!   mapped through the default `CustomTransform` (`RuboCop` →
//!   `rubocop`, `RSpec` → `rspec`) else `camel_to_snake_case`), and the
//!   method-name suffix (`IgnoreMethods`, default false; whitespace to
//!   `_`, non-word chars stripped). The assembled regex must match the
//!   file path at the end (`filename_ends_with?`); otherwise the `Send`
//!   flags per `add_offense(send_node)` with `Spec path should end with
//!   `%<suffix>s`.` (glob-styled suffix). Detection is at parity for the
//!   default configuration (verified vs 3.7.0, including namespaced
//!   constants, `CustomTransform`, `IgnoreMethods`, routing metadata,
//!   multi-group and non-const clean cases); custom `CustomTransform` /
//!   `IgnoreMetadata` tables are not configurable in this batch — the
//!   `CopOptions` schema only carries scalars, so the upstream defaults
//!   are baked in (status: partial, config as gap). Upstream ships no
//!   autocorrect and none is added here.
//! ```
//!
//! ## Matched shapes
//!
//! File-scoped (`#[on_new_investigation]`): exactly one top-level spec
//! group in the file, describing a constant:
//!
//! - `describe MyClass` in `whatever_spec.rb` — flagged (`Send`
//!   range, `` Spec path should end with `my_class*_spec.rb`. ``).
//! - `describe MyClass` in `my_class_spec.rb` — clean.
//! - `describe MyClass, '#method'` in `my_class/method_spec.rb` —
//!   clean; in `my_class_spec.rb` — flagged
//!   (`` my_class*method*_spec.rb ``).
//! - `describe Foo::BarBaz` in `foo/bar_baz_spec.rb` — clean.
//! - `describe RuboCop` in `rubocop_spec.rb` — clean
//!   (`CustomTransform`).
//! - `describe MyClass, type: :routing` anywhere — clean.
//! - Two top-level groups, or `describe 'just a string'` — unchecked,
//!   clean.
//!
//! ## No autocorrect
//!
//! Upstream ships no autocorrect; renaming the file needs human
//! judgement.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop, regex::Regex};

use crate::cops::rspec_helpers::{
    is_example_group_call, is_spec_group_call, is_top_level_block, send_without_block_range,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpecFilePathFormat;

#[derive(CopOptions)]
pub struct SpecFilePathFormatOptions {
    #[option(
        name = "IgnoreMethods",
        default = false,
        description = "Whether method-name descriptions are ignored when checking the path."
    )]
    pub ignore_methods: bool,
}

#[cop(
    name = "RSpec/SpecFilePathFormat",
    description = "Checks that spec file paths are consistent and well-formed.",
    default_severity = "warning",
    default_enabled = true,
    options = SpecFilePathFormatOptions,
)]
impl SpecFilePathFormat {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        let root = cx.root();
        let mut groups: Vec<NodeId> = Vec::new();
        for node in core::iter::once(root).chain(cx.descendants(root)) {
            let NodeKind::Block { call, .. } = *cx.kind(node) else {
                continue;
            };
            // Upstream `top_level_groups` selects `spec_group?`
            // (example groups plus shared groups).
            if !is_spec_group_call(cx, call) {
                continue;
            }
            if !is_top_level_block(cx, node) {
                continue;
            }
            groups.push(node);
        }
        // `return unless top_level_groups.one?`
        if groups.len() != 1 {
            return;
        }
        let group = groups[0];
        let NodeKind::Block { call, .. } = *cx.kind(group) else {
            return;
        };
        // `on_top_level_example_group` fires only for example groups.
        if !is_example_group_call(cx, call) {
            return;
        }
        let NodeKind::Send { args, .. } = *cx.kind(call) else {
            return;
        };
        let arg_ids = cx.list(args);
        let Some(&class_name) = arg_ids.first() else {
            return;
        };
        // `next if !class_name.const_type?`
        if !matches!(*cx.kind(class_name), NodeKind::Const { .. }) {
            return;
        }
        let arguments = &arg_ids[1..];
        if has_routing_metadata(cx, arguments) {
            return;
        }
        let opts = cx.options_or_default::<SpecFilePathFormatOptions>();
        let pattern = correct_path_pattern(cx, group, class_name, arguments, opts.ignore_methods);
        let re = Regex::new(&format!("{pattern}$")).unwrap_or_else(|_| Regex::new("$^").unwrap());
        if re.is_match(cx.file_path()) {
            return;
        }
        cx.emit_offense(
            send_without_block_range(cx, call),
            &format!("Spec path should end with `{}`.", glob_suffix(&pattern)),
            None,
        );
    }
}

/// `ignore_metadata?` for the default `IgnoreMetadata: {type: routing}`:
/// a `(pair (sym :type) (sym :routing))` anywhere in the trailing args.
fn has_routing_metadata(cx: &Cx<'_>, arguments: &[NodeId]) -> bool {
    arguments.iter().any(|&arg| {
        core::iter::once(arg)
            .chain(cx.descendants(arg))
            .any(|id| {
                let NodeKind::Pair { key, value } = *cx.kind(id) else {
                    return false;
                };
                let NodeKind::Sym(key_sym) = *cx.kind(key) else {
                    return false;
                };
                let NodeKind::Sym(value_sym) = *cx.kind(value) else {
                    return false;
                };
                cx.symbol_str(key_sym) == "type" && cx.symbol_str(value_sym) == "routing"
            })
    })
}

/// `correct_path_pattern`: the expected snake-case path, an optional
/// `.*` plus sanitised method name, and the spec suffix — joined with
/// no separator, exactly like upstream's `path.join`.
fn correct_path_pattern(
    cx: &Cx<'_>,
    group: NodeId,
    class_name: NodeId,
    arguments: &[NodeId],
    ignore_methods: bool,
) -> String {
    let expected = expected_path(cx, group, class_name);
    let method_part = arguments
        .first()
        .and_then(|&first| method_name_pattern(cx, first, ignore_methods));
    match method_part {
        Some(name) => format!(r"{expected}.*{name}[^/]*_spec\.rb"),
        None => format!(r"{expected}[^/]*_spec\.rb"),
    }
}

/// `expected_path`: enclosing namespace plus the constant path, mapped
/// through the default `CustomTransform` else `camel_to_snake_case`,
/// joined with `/` (`File.join`).
fn expected_path(cx: &Cx<'_>, group: NodeId, class_name: NodeId) -> String {
    let mut parts = lexical_namespace(cx, group);
    let konst = cx.const_name(class_name).unwrap_or_default();
    for part in konst.split("::") {
        if !part.is_empty() {
            parts.push(part.to_owned());
        }
    }
    parts
        .iter()
        .map(|name| custom_transform(name).unwrap_or_else(|| camel_to_snake_case(name)))
        .collect::<Vec<_>>()
        .join("/")
}

/// Default `CustomTransform: {RuboCop: rubocop, RSpec: rspec}`.
fn custom_transform(name: &str) -> Option<String> {
    match name {
        "RuboCop" => Some("rubocop".to_owned()),
        "RSpec" => Some("rspec".to_owned()),
        _ => None,
    }
}

/// `name_pattern`: `nil` for non-strings or `IgnoreMethods`, else the
/// string with whitespace to `_` and non-word chars stripped.
fn method_name_pattern(cx: &Cx<'_>, method_name: NodeId, ignore_methods: bool) -> Option<String> {
    if ignore_methods {
        return None;
    }
    let NodeKind::Str(str_id) = *cx.kind(method_name) else {
        return None;
    };
    Some(
        cx.string_str(str_id)
            .chars()
            .map(|c| if c.is_whitespace() { '_' } else { c })
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect(),
    )
}

/// `camel_to_snake_case` per upstream's two substitutions, lowercased.
fn camel_to_snake_case(name: &str) -> String {
    let step1 = Regex::new(r"([^A-Z])([A-Z]+)").unwrap();
    let step2 = Regex::new(r"([A-Z])([A-Z][^A-Z\d]+)").unwrap();
    step2
        .replace_all(&step1.replace_all(name, "${1}_${2}"), "${1}_${2}")
        .to_lowercase()
}

/// Glob-styled suffix for the offense message: `.*` → `*`, `[^/]*` →
/// `*`, `\.` → `.` (first occurrences, mirroring upstream `sub`).
fn glob_suffix(pattern: &str) -> String {
    // `[^/]*` contains no `.*` substring, so order is safe either way.
    let suffix = pattern.replacen(".*", "*", 1);
    let suffix = suffix.replacen("[^/]*", "*", 1);
    suffix.replacen(r"\.", ".", 1)
}

/// Enclosing `class` / `module` names split on `::`, outermost first
/// (mirrors `Namespace#namespace`).
fn lexical_namespace(cx: &Cx<'_>, id: NodeId) -> Vec<String> {
    let mut stack: Vec<NodeId> = cx.ancestors(id).collect();
    stack.reverse();
    let mut out = Vec::new();
    for anc in stack {
        let name_id = match *cx.kind(anc) {
            NodeKind::Class { name, .. } => Some(name),
            NodeKind::Module { name, .. } => Some(name),
            _ => None,
        };
        if let Some(nid) = name_id
            && let Some(full) = cx.const_name(nid)
        {
            for part in full.split("::") {
                if !part.is_empty() {
                    out.push(part.to_owned());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::SpecFilePathFormat;
    use super::SpecFilePathFormatOptions;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_wrong_path_for_class() {
        test::<SpecFilePathFormat>()
            .with_file_path("whatever_spec.rb")
            .expect_offense(indoc! {r#"
                describe MyClass do
                ^^^^^^^^^^^^^^^^ Spec path should end with `my_class*_spec.rb`.
                end
            "#});
    }

    #[test]
    fn ignores_correct_path_for_class() {
        test::<SpecFilePathFormat>()
            .with_file_path("my_class_spec.rb")
            .expect_no_offenses(indoc! {r#"
                describe MyClass do
                end
            "#});
    }

    #[test]
    fn ignores_method_in_nested_path() {
        test::<SpecFilePathFormat>()
            .with_file_path("my_class/method_spec.rb")
            .expect_no_offenses(indoc! {r#"
                describe MyClass, '#method' do
                end
            "#});
    }

    #[test]
    fn flags_method_without_method_in_path() {
        test::<SpecFilePathFormat>()
            .with_file_path("my_class_spec.rb")
            .expect_offense(indoc! {r#"
                describe MyClass, '#method' do
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Spec path should end with `my_class*method*_spec.rb`.
                end
            "#});
    }

    #[test]
    fn ignores_namespaced_path() {
        test::<SpecFilePathFormat>()
            .with_file_path("foo/bar_baz_spec.rb")
            .expect_no_offenses(indoc! {r#"
                describe Foo::BarBaz do
                end
            "#});
    }

    #[test]
    fn ignores_custom_transform() {
        test::<SpecFilePathFormat>()
            .with_file_path("rubocop_spec.rb")
            .expect_no_offenses(indoc! {r#"
                describe RuboCop do
                end
            "#});
    }

    #[test]
    fn ignores_routing_metadata() {
        test::<SpecFilePathFormat>()
            .with_file_path("whatever_spec.rb")
            .expect_no_offenses(indoc! {r#"
                describe MyClass, type: :routing do
                end
            "#});
    }

    #[test]
    fn ignores_multiple_top_level_groups() {
        test::<SpecFilePathFormat>()
            .with_file_path("whatever_spec.rb")
            .expect_no_offenses(indoc! {r#"
                describe MyClass do
                end
                describe Other do
                end
            "#});
    }

    #[test]
    fn ignores_non_const_description() {
        test::<SpecFilePathFormat>()
            .with_file_path("whatever_spec.rb")
            .expect_no_offenses(indoc! {r#"
                describe 'just a string' do
                end
            "#});
    }

    #[test]
    fn ignores_enclosing_module_namespace() {
        test::<SpecFilePathFormat>()
            .with_file_path("foo/bar_spec.rb")
            .expect_no_offenses(indoc! {r#"
                module Foo
                  describe Bar do
                  end
                end
            "#});
    }

    #[test]
    fn ignores_method_when_ignoring_methods() {
        let opts = SpecFilePathFormatOptions {
            ignore_methods: true,
        };
        test::<SpecFilePathFormat>()
            .with_options(&opts)
            .with_file_path("my_class_spec.rb")
            .expect_no_offenses(indoc! {r#"
                describe MyClass, '#method' do
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(SpecFilePathFormat);
