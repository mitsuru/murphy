//! `RSpec/VariableName` — memoized helper names use the configured style.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/VariableName
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Mirrors upstream `on_send` gated by `InsideExampleGroup` with
//!   `variable_definition?` (`(send nil? {subject, subject!, let, let!}
//!   $({sym str dsym dstr} ...) ...)`): bare-receiver memoized helpers.
//!   `Dstr` / `Dsym` first args return early, then
//!   `matches_allowed_pattern?(variable.value)` (unanchored regexes from
//!   `AllowedPatterns`, plus the deprecated `IgnoredPatterns` key which
//!   upstream still merges) skips, then `check_name` validates against
//!   `ConfigurableNaming::FORMATS` (`snake_case` default, `camelCase`
//!   alternate) per `add_offense(variable.source_range)` with
//!   `Use %<style>s for variable names.` The style regexes are byte-level
//!   ports of RuboCop's `FORMATS` hash — the same pair
//!   `Naming/VariableName` documents (`snake_case`:
//!   `/^@{0,2}[\d[[:lower:]]_]+[!?=]?$/`, `camelCase`:
//!   `/^@{0,2}(?:_|_?[[:lower:]][\d[[:lower:]][[:upper:]]]*)[!?=]?$/`),
//!   ASCII-only like that cop. `inside_example_group?` is simplified to
//!   "any ancestor `Block` is a spec group (example or shared)" — the
//!   same simplification `RSpec/EmptyLineAfterSubject` carries, and it
//!   agrees with upstream on the documented shapes (including
//!   `shared_examples`). Detection is at parity (verified vs 3.7.0,
//!   including `let!` / `subject!` / proc-block-pass and the
//!   `AllowedPatterns` / `IgnoredPatterns` gates); autocorrect (replace
//!   with the preferred spelling) is not ported in this batch — same
//!   convention as `RSpec/BeNil` (status: partial, autocorrect as gap).
//! ```
//!
//! ## Matched shapes
//!
//! Dispatched on `Send` with `methods = ["let", "let!", "subject",
//! "subject!"]` (bare receiver only) inside an example group. Default
//! style `snake_case`:
//!
//! - `let(:userName) { }` — flagged (the name arg).
//! - `let('user-name') { }` — kebab string, flagged.
//! - `let(:user_name) { }` — snake, clean.
//! - `let(:"user#{name}") { }` — interpolated symbol, clean (early return).
//! - `let("user#{name}") { }` — interpolated string, clean.
//! - `let(:userName, &create_user) { }` — proc form, flagged.
//! - Top-level `let(:userName) { }` — no group ancestor, clean.
//! - `AllowedPatterns: ['^userFood$']` — `let(:userFood)` clean.
//!
//! Alternate style `camelCase` (`EnforcedStyle: camelCase`) mirrors with
//! the opposite spelling flagged.
//!
//! ## No autocorrect
//!
//! Upstream replaces with the preferred spelling. This batch reports only.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct VariableName;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Default)]
pub enum VariableNameStyle {
    #[default]
    #[option(value = "snake_case")]
    SnakeCase,
    #[option(value = "camelCase")]
    CamelCase,
}

impl VariableNameStyle {
    fn as_str(self) -> &'static str {
        match self {
            VariableNameStyle::SnakeCase => "snake_case",
            VariableNameStyle::CamelCase => "camelCase",
        }
    }

    fn matches(self, name: &str) -> bool {
        match self {
            VariableNameStyle::SnakeCase => is_snake_case(name),
            VariableNameStyle::CamelCase => is_camel_case(name),
        }
    }
}

#[derive(CopOptions)]
pub struct VariableNameOptions {
    #[option(
        name = "EnforcedStyle",
        default = "snake_case",
        description = "Required memoized-helper name style: snake_case or camelCase."
    )]
    pub enforced_style: VariableNameStyle,
    #[option(
        name = "AllowedPatterns",
        default = [],
        description = "Regexes; a helper name matching any is allowed."
    )]
    pub allowed_patterns: Vec<String>,
    #[option(
        name = "IgnoredPatterns",
        default = [],
        description = "Deprecated alias of AllowedPatterns."
    )]
    pub ignored_patterns: Vec<String>,
}

#[cop(
    name = "RSpec/VariableName",
    description = "Checks that memoized helper names use the configured style.",
    default_severity = "warning",
    default_enabled = true,
    options = VariableNameOptions,
)]
impl VariableName {
    #[on_node(kind = "send", methods = ["let", "let!", "subject", "subject!"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send {
            receiver, args, ..
        } = *cx.kind(node)
        else {
            return;
        };
        if receiver != OptNodeId::NONE {
            return;
        }
        if !is_inside_example_group(cx, node) {
            return;
        }
        let Some(&first) = cx.list(args).first() else {
            return;
        };
        // `return if variable.type?(:dstr, :dsym)`
        if matches!(
            *cx.kind(first),
            NodeKind::Dstr(_) | NodeKind::Dsym(_)
        ) {
            return;
        }
        let name = match *cx.kind(first) {
            NodeKind::Sym(sym) => cx.symbol_str(sym).to_owned(),
            NodeKind::Str(id) => cx.string_str(id).to_owned(),
            _ => return,
        };
        let opts = cx.options_or_default::<VariableNameOptions>();
        // Upstream merges `AllowedPatterns` + deprecated `IgnoredPatterns`.
        if cx.matches_any_pattern(&name, &opts.allowed_patterns)
            || cx.matches_any_pattern(&name, &opts.ignored_patterns)
        {
            return;
        }
        if opts.enforced_style.matches(&name) {
            return;
        }
        cx.emit_offense(
            cx.range(first),
            &format!("Use {} for variable names.", opts.enforced_style.as_str()),
            None,
        );
    }
}

/// Simplified `inside_example_group?`: `true` when any ancestor `Block`
/// is a spec group (example or shared) with an RSpec-or-bare receiver.
fn is_inside_example_group(cx: &Cx<'_>, node: NodeId) -> bool {
    use crate::cops::rspec_helpers::is_spec_group_call;
    for anc in cx.ancestors(node) {
        if let NodeKind::Block { call, .. } = *cx.kind(anc)
            && is_spec_group_call(cx, call)
        {
            return true;
        }
    }
    false
}

/// `snake_case`: `/^@{0,2}[\d[[:lower:]]_]+[!?=]?$/` (ASCII-only port,
/// same as `Naming/VariableName`).
fn is_snake_case(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b'@' && i < 2 {
        i += 1;
    }
    let mut end = bytes.len();
    if end > i && matches!(bytes[end - 1], b'!' | b'?' | b'=') {
        end -= 1;
    }
    if end <= i {
        return false;
    }
    bytes[i..end]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// `camelCase`: `/^@{0,2}(?:_|_?[[:lower:]][\d[[:lower:]][[:upper:]]]*)[!?=]?$/`
/// (ASCII-only port, same as `Naming/VariableName`).
fn is_camel_case(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b'@' && i < 2 {
        i += 1;
    }
    let mut end = bytes.len();
    if end > i && matches!(bytes[end - 1], b'!' | b'?' | b'=') {
        end -= 1;
    }
    let body = &bytes[i..end];
    if body == b"_" {
        return true;
    }
    let mut j = 0;
    if j < body.len() && body[j] == b'_' {
        j += 1;
    }
    if j >= body.len() || !body[j].is_ascii_lowercase() {
        return false;
    }
    j += 1;
    body[j..]
        .iter()
        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_uppercase() || b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::{VariableName, VariableNameOptions, VariableNameStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn camel_case() -> VariableNameOptions {
        VariableNameOptions {
            enforced_style: VariableNameStyle::CamelCase,
            allowed_patterns: vec![],
            ignored_patterns: vec![],
        }
    }

    fn snake_with_allowed() -> VariableNameOptions {
        VariableNameOptions {
            enforced_style: VariableNameStyle::SnakeCase,
            allowed_patterns: vec!["^userFood$".to_owned(), "^userPet$".to_owned()],
            ignored_patterns: vec![],
        }
    }

    fn snake_with_ignored() -> VariableNameOptions {
        VariableNameOptions {
            enforced_style: VariableNameStyle::SnakeCase,
            allowed_patterns: vec![],
            ignored_patterns: vec!["^userFood$".to_owned(), "^userPet$".to_owned()],
        }
    }

    #[test]
    fn flags_camel_let() {
        test::<VariableName>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  let(:userName) { 'Adam' }
                      ^^^^^^^^^ Use snake_case for variable names.
                end
            "#});
    }

    #[test]
    fn flags_pascal_let() {
        test::<VariableName>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  let(:UserName) { 'Adam' }
                      ^^^^^^^^^ Use snake_case for variable names.
                end
            "#});
    }

    #[test]
    fn ignores_snake_let() {
        test::<VariableName>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  let(:user_name) { 'Adam' }
                end
            "#});
    }

    #[test]
    fn ignores_dsym() {
        test::<VariableName>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  let(:"user#{name}") { 'Adam' }
                end
            "#});
    }

    #[test]
    fn flags_camel_string() {
        test::<VariableName>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  let('userName') { 'Adam' }
                      ^^^^^^^^^^ Use snake_case for variable names.
                end
            "#});
    }

    #[test]
    fn flags_kebab_string() {
        test::<VariableName>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  let('user-name') { 'Adam' }
                      ^^^^^^^^^^^ Use snake_case for variable names.
                end
            "#});
    }

    #[test]
    fn ignores_dstr() {
        test::<VariableName>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  let("user#{name}") { 'Adam' }
                end
            "#});
    }

    #[test]
    fn flags_proc_form() {
        test::<VariableName>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  let(:userName, &create_user)
                      ^^^^^^^^^ Use snake_case for variable names.
                end
            "#});
    }

    #[test]
    fn flags_let_bang() {
        test::<VariableName>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  let!(:userName) { 'Adam' }
                       ^^^^^^^^^ Use snake_case for variable names.
                end
            "#});
    }

    #[test]
    fn flags_subject() {
        test::<VariableName>().expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  subject(:userName) { 'Adam' }
                          ^^^^^^^^^ Use snake_case for variable names.
                end
            "#});
    }

    #[test]
    fn flags_shared_examples_group() {
        test::<VariableName>().expect_offense(indoc! {r#"
                RSpec.shared_examples 'foo example' do
                  let(:userName) { 'Adam' }
                      ^^^^^^^^^ Use snake_case for variable names.
                end
            "#});
    }

    #[test]
    fn ignores_top_level() {
        test::<VariableName>().expect_no_offenses(indoc! {r#"
                let(:userName) { 'Adam' }
            "#});
    }

    #[test]
    fn flags_snake_in_camel_style() {
        test::<VariableName>()
            .with_options(&camel_case())
            .expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  let(:user_name) { 'Adam' }
                      ^^^^^^^^^^ Use camelCase for variable names.
                end
            "#});
    }

    #[test]
    fn ignores_camel_in_camel_style() {
        test::<VariableName>()
            .with_options(&camel_case())
            .expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  let(:userName) { 'Adam' }
                end
            "#});
    }

    #[test]
    fn allows_allowed_patterns() {
        test::<VariableName>()
            .with_options(&snake_with_allowed())
            .expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  let(:userFood) { 'Adam' }
                end
            "#});
    }

    #[test]
    fn flags_non_allowed_with_allowed_config() {
        test::<VariableName>()
            .with_options(&snake_with_allowed())
            .expect_offense(indoc! {r#"
                RSpec.describe Foo do
                  let(:userName) { 'Adam' }
                      ^^^^^^^^^ Use snake_case for variable names.
                end
            "#});
    }

    #[test]
    fn allows_ignored_patterns_deprecated() {
        test::<VariableName>()
            .with_options(&snake_with_ignored())
            .expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                  let(:userFood) { 'Adam' }
                end
            "#});
    }
}

murphy_plugin_api::submit_cop!(VariableName);
