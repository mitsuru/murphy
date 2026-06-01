//! `RSpec/DescribeClass` — the first argument of a **top-level**
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rspec
//! upstream_cop: RSpec/DescribeClass
//! upstream_version_checked: 3.7.0
//! status: partial
//! gap_issues:
//!   - murphy-h8ke
//! notes: >
//!   Backfilled metadata; full upstream parity audit still needs to confirm remaining file-gating and behavior differences.
//! ```
//!
//! `RSpec.describe`/`describe` block should be the class or module
//! under test, not a string or symbol. Mirrors RuboCop-RSpec's cop of
//! the same name.
//!
//! ## Top-level gating
//!
//! Upstream's `TopLevelGroup` mixin walks from the root through any
//! `begin`/`class`/`module` it sees and treats every direct child of
//! that traversal as "top-level". Anything nested inside another
//! expression (a `describe`/`context` block, `shared_examples`,
//! `shared_context`, an iterator block, `if`/`def`/…) is **not**
//! top-level and is left to the outer group.
//!
//! In Murphy this is the same shape: a describe Send qualifies as
//! top-level when it is the `call` of a `Block`, and every ancestor of
//! that `Block` up to the root is one of `Begin`/`Class`/`Module`/
//! `Sclass`. Hitting any other node kind (a sibling `Block`, an `If`,
//! a `Def`, etc.) means the describe is nested and the cop bails.
//!
//! ## Receiver shapes
//!
//! - `OptNodeId::NONE` — bare `describe "x"` (RSpec's top-level
//!   monkey-patch).
//! - `Const { scope: None, name: "RSpec" }` — explicit
//!   `RSpec.describe "x"`. Murphy's translator collapses bare
//!   `RSpec` and cbase `::RSpec` to the same `Const { scope: None }`
//!   (see `translate_constant_path` + the
//!   `translates_toplevel_constant_path` test in
//!   `murphy-translate`), so `::RSpec.describe` is matched too.
//!
//! Any other receiver (e.g. `Other.describe "x"`) is intentionally
//! skipped — it belongs to some other DSL.
//!
//! ## First-argument classification
//!
//! - **`NodeKind::Str`** → emit, *unless* the string content looks
//!   like a constant path: `Thing`, `Some::Thing`, `VERSION`,
//!   `Some::VERSION`, `::Some::VERSION`. Mirrors RuboCop-RSpec's
//!   `string_constant?` (`/^(?:(?:::)?[A-Z]\w*)+$/`, ASCII-only via
//!   Ruby's no-`/u` `\w`). These string forms are used with
//!   `Object.const_get(self.class.description)` patterns so the
//!   subject genuinely *is* a constant — just not statically.
//! - **`NodeKind::Dstr`** (string interpolation) → emit. Interpolated
//!   strings can't be string constants — RuboCop's `string_constant?`
//!   short-circuits on `str_type?` and lets `Dstr` fall through.
//! - **`NodeKind::Sym`** → emit. Symbols are never class references.
//! - **`NodeKind::Const { .. }`** → OK (single-name `Foo` and scoped
//!   `Foo::Bar` both encode here; nested scope ids are walked
//!   transparently because `Const { scope }` chains).
//! - **Anything else** (variable read, method call, expression, …) →
//!   skip. Static analysis cannot tell whether the runtime value is a
//!   class, and a false-positive on a domain DSL is worse than a
//!   tolerated miss. (RuboCop's stricter `$[!const !#string_constant?]`
//!   would flag these too; Murphy keeps the conservative stance
//!   established when the cop was first ported.)
//!
//! ## `IgnoredMetadata`
//!
//! `describe "...", type: :request do ... end` is allowed by default —
//! Rails/Aruba/etc. spec types are integration descriptors, not class
//! descriptors. The default map mirrors upstream's `config/default.yml`
//! and lives in `DescribeClassOptions::default`. Users can extend or
//! replace it via `[cops.rules."RSpec/DescribeClass"]
//! IgnoredMetadata = { … }`.
//!
//! Murphy's `#[derive(CopOptions)]` macro doesn't currently express
//! nested `String -> [String]` maps, so `CopOptions` is hand-rolled
//! here. The schema entry is empty (the host's schema format doesn't
//! describe nested-map keys yet); the runtime decode and round-trip
//! still work because they go through `serde_json` directly.
//!
//! ## Offense range
//!
//! `cx.range(first_arg)` — only the offending first positional
//! argument, not the whole `describe` Send. Mirrors upstream's
//! `add_offense(described)` where `described` is the captured first
//! argument node.
//!
//! ## No autocorrect
//!
//! Synthesising a class identifier from a free-form string is unsafe
//! (the right class may not exist; the user may genuinely want to
//! describe a scenario rather than a class). The cop reports and lets
//! the user fix by hand.
//!
//! ## Known v1 limitation
//!
//! RuboCop only runs RSpec cops on `*_spec.rb` files (and excludes
//! `spec/features|requests|routing|system|views/` from this cop
//! specifically). Murphy has no per-cop file-pattern gating yet, so
//! this cop fires on bare `describe "foo"` outside spec files too.
//! Users on non-spec codebases can disable the cop via
//! `[cops.rules."RSpec/DescribeClass"] enabled = false`.

use std::collections::{BTreeMap, BTreeSet};

use murphy_plugin_api::{ConfigError, CopOptions, Cx, NodeId, NodeKind, OptNodeId, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct DescribeClass;

/// `IgnoredMetadata: { key => [values...] }`. Murphy's
/// `#[derive(CopOptions)]` doesn't model nested maps, so the impl is
/// hand-rolled — defaults mirror upstream's `config/default.yml`.
#[derive(Clone, Debug)]
pub struct DescribeClassOptions {
    pub ignored_metadata: BTreeMap<String, BTreeSet<String>>,
}

impl Default for DescribeClassOptions {
    fn default() -> Self {
        let type_values: BTreeSet<String> = [
            "channel",
            "controller",
            "helper",
            "job",
            "mailer",
            "model",
            "request",
            "routing",
            "view",
            "feature",
            "system",
            "mailbox",
            "aruba",
            "task",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        let mut map = BTreeMap::new();
        map.insert("type".to_string(), type_values);
        Self {
            ignored_metadata: map,
        }
    }
}

impl CopOptions for DescribeClassOptions {
    fn from_config_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        // Error surface mirrors `#[derive(CopOptions)]`: non-object
        // root → `not_an_object`; per-field shape mismatches →
        // `type_mismatch` with a path-qualified field name. Silently
        // falling back to `Default` would let typos go unnoticed.
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(ConfigError::parse)?;
        let obj = value.as_object().ok_or_else(ConfigError::not_an_object)?;

        // Missing `IgnoredMetadata` → defaults, consistent with how the
        // derive treats absent fields.
        let Some(metadata_value) = obj.get("IgnoredMetadata") else {
            return Ok(Self::default());
        };
        let metadata_obj = metadata_value
            .as_object()
            .ok_or_else(|| ConfigError::type_mismatch("IgnoredMetadata", "object"))?;

        let mut ignored: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (key, values) in metadata_obj {
            let array = values.as_array().ok_or_else(|| {
                ConfigError::type_mismatch(format!("IgnoredMetadata.{key}"), "array of strings")
            })?;
            let mut set = BTreeSet::new();
            for (i, elem) in array.iter().enumerate() {
                let s = elem.as_str().ok_or_else(|| {
                    ConfigError::type_mismatch(format!("IgnoredMetadata.{key}[{i}]"), "string")
                })?;
                set.insert(s.to_string());
            }
            ignored.insert(key.clone(), set);
        }
        Ok(Self {
            ignored_metadata: ignored,
        })
    }

    fn to_config_json(&self) -> String {
        let metadata: serde_json::Map<String, serde_json::Value> = self
            .ignored_metadata
            .iter()
            .map(|(k, vs)| {
                let arr: Vec<serde_json::Value> = vs
                    .iter()
                    .map(|v| serde_json::Value::String(v.clone()))
                    .collect();
                (k.clone(), serde_json::Value::Array(arr))
            })
            .collect();
        let mut top = serde_json::Map::new();
        top.insert(
            "IgnoredMetadata".to_string(),
            serde_json::Value::Object(metadata),
        );
        serde_json::Value::Object(top).to_string()
    }
}

#[cop(
    name = "RSpec/DescribeClass",
    description = "Check that the first argument to the top-level describe is a constant.",
    default_severity = "warning",
    default_enabled = true,
    options = DescribeClassOptions
)]
impl DescribeClass {
    #[on_node(kind = "send", methods = ["describe"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Send { receiver, args, .. } = *cx.kind(node) else {
            return;
        };
        if !receiver_is_rspec_or_bare(cx, receiver) {
            return;
        }
        if !is_top_level_describe(cx, node) {
            return;
        }
        let arg_ids = cx.list(args);
        let Some(&first) = arg_ids.first() else {
            return;
        };
        if !flagable_first_arg(cx, first) {
            return;
        }
        let opts = cx.options_or_default::<DescribeClassOptions>();
        if has_ignored_metadata(cx, arg_ids, &opts) {
            return;
        }
        cx.emit_offense(
            cx.range(first),
            "The first argument to describe should be the class or module being tested.",
            None,
        );
    }
}

/// `true` when `receiver` is the bare-`describe` form or a top-level
/// `RSpec` constant (including the cbase form `::RSpec`, which the
/// translator collapses to `Const { scope: None }`).
fn receiver_is_rspec_or_bare(cx: &Cx<'_>, receiver: OptNodeId) -> bool {
    let Some(rid) = receiver.get() else {
        return true;
    };
    matches!(
        *cx.kind(rid),
        NodeKind::Const { scope, name }
            if scope == OptNodeId::NONE && cx.symbol_str(name) == "RSpec"
    )
}

/// `true` when `send` is the call of a `Block` and every ancestor of
/// that `Block` is one of `Begin`/`Class`/`Module`/`Sclass` up to the
/// root. Mirrors upstream's `TopLevelGroup#top_level_nodes` traversal.
fn is_top_level_describe(cx: &Cx<'_>, send: NodeId) -> bool {
    let Some(block_id) = cx.parent(send).get() else {
        return false;
    };
    if !matches!(*cx.kind(block_id), NodeKind::Block { call, .. } if call == send) {
        return false;
    }
    let mut cur = block_id;
    while let Some(p) = cx.parent(cur).get() {
        match *cx.kind(p) {
            NodeKind::Begin(_)
            | NodeKind::Class { .. }
            | NodeKind::Module { .. }
            | NodeKind::Sclass { .. } => {
                cur = p;
            }
            _ => return false,
        }
    }
    true
}

/// `true` when the first positional argument is the literal shape the
/// cop wants to flag — a non-constant-looking string, an interpolated
/// string, or a symbol.
fn flagable_first_arg(cx: &Cx<'_>, arg: NodeId) -> bool {
    match *cx.kind(arg) {
        NodeKind::Str(string_id) => !looks_like_constant_path(cx.string_str(string_id)),
        NodeKind::Dstr(_) | NodeKind::Sym(_) => true,
        _ => false,
    }
}

/// Mirrors upstream `string_constant?`'s regex
/// `^(?:(?:::)?[A-Z]\w*)+$` — Ruby's `\w` without `/u` is
/// ASCII-only, so we use `[A-Za-z0-9_]` and reject anything else.
/// Avoids pulling in `regex` for one cop.
fn looks_like_constant_path(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut i = 0;
    if bytes.len() >= 2 && bytes[0] == b':' && bytes[1] == b':' {
        i = 2;
    }
    loop {
        if i >= bytes.len() || !bytes[i].is_ascii_uppercase() {
            return false;
        }
        i += 1;
        while i < bytes.len() && is_ident_continue(bytes[i]) {
            i += 1;
        }
        if i == bytes.len() {
            return true;
        }
        if i + 1 < bytes.len() && bytes[i] == b':' && bytes[i + 1] == b':' {
            i += 2;
            continue;
        }
        return false;
    }
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `true` when the trailing arg is a Hash whose pairs include any
/// `sym => sym` entry the user (or default) has marked ignorable.
/// Mirrors upstream's `(hash <#ignored_metadata? ...>)` matcher.
fn has_ignored_metadata(cx: &Cx<'_>, args: &[NodeId], opts: &DescribeClassOptions) -> bool {
    let Some(&last) = args.last() else {
        return false;
    };
    let NodeKind::Hash(pairs) = *cx.kind(last) else {
        return false;
    };
    for pair_id in cx.list(pairs).iter().copied() {
        let NodeKind::Pair { key, value } = *cx.kind(pair_id) else {
            continue;
        };
        let NodeKind::Sym(k_sym) = *cx.kind(key) else {
            continue;
        };
        let NodeKind::Sym(v_sym) = *cx.kind(value) else {
            continue;
        };
        let key_str = cx.symbol_str(k_sym);
        let val_str = cx.symbol_str(v_sym);
        if let Some(allowed) = opts.ignored_metadata.get(key_str)
            && allowed.contains(val_str)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{DescribeClass, DescribeClassOptions};
    use murphy_plugin_api::test_support::{indoc, test};
    use std::collections::{BTreeMap, BTreeSet};

    // === RuboCop-RSpec spec parity (one test per upstream `it`/`example`) ===

    #[test]
    fn flags_first_line_describe_with_string() {
        test::<DescribeClass>().expect_offense(indoc! {r#"
                describe "bad describe" do
                         ^^^^^^^^^^^^^^ The first argument to describe should be the class or module being tested.
                end
            "#});
    }

    #[test]
    fn allows_rspec_describe_const() {
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                RSpec.describe Foo do
                end
            "#});
    }

    #[test]
    fn allows_cbase_rspec_describe_const() {
        // `::RSpec` and bare `RSpec` collapse to `Const { scope: None }`
        // (see `translates_toplevel_constant_path` in murphy-translate).
        // Pinning this so a future AST that distinguishes the shapes
        // can't silently regress the cop.
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                ::RSpec.describe Foo do
                end
            "#});
    }

    #[test]
    fn flags_after_a_require() {
        // Multi-statement root → Begin wrapping. `is_top_level_describe`
        // walks through Begin.
        test::<DescribeClass>().expect_offense(indoc! {r#"
                require 'spec_helper'
                describe "bad describe" do
                         ^^^^^^^^^^^^^^ The first argument to describe should be the class or module being tested.
                end
            "#});
    }

    #[test]
    fn offense_range_highlights_only_the_first_arg() {
        // Even with multiple positional args, only the first is
        // ranged — mirrors upstream's `add_offense(described)`.
        test::<DescribeClass>().expect_offense(indoc! {r#"
                describe "bad describe", "blah blah" do
                         ^^^^^^^^^^^^^^ The first argument to describe should be the class or module being tested.
                end
            "#});
    }

    #[test]
    fn ignores_nested_describe_inside_const_described_outer() {
        // The outer describe is fine (const arg). The inner string-arg
        // describe is nested — `is_top_level_describe` returns false
        // because the inner Block's parent chain hits the outer Block.
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                describe Some::Class do
                  describe "bad describe" do
                  end
                end
            "#});
    }

    #[test]
    fn ignores_string_constant_without_namespace() {
        // `"Thing"` looks like a constant — RuboCop's `string_constant?`
        // accepts it; so do we.
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                describe 'Thing' do
                  subject { Object.const_get(self.class.description) }
                end
            "#});
    }

    #[test]
    fn ignores_string_constant_with_namespace() {
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                describe 'Some::Thing' do
                  subject { Object.const_get(self.class.description) }
                end
            "#});
    }

    #[test]
    fn ignores_value_constants() {
        // `VERSION` looks like an all-caps constant.
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                describe 'VERSION' do
                  subject { Object.const_get(self.class.description) }
                end
            "#});
    }

    #[test]
    fn ignores_value_constants_with_namespace() {
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                describe 'Some::VERSION' do
                  subject { Object.const_get(self.class.description) }
                end
            "#});
    }

    #[test]
    fn ignores_top_level_constants_with_double_colon_prefix() {
        // `::Some::VERSION` — the regex permits the leading `::`.
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                describe '::Some::VERSION' do
                  subject { Object.const_get(self.class.description) }
                end
            "#});
    }

    #[test]
    fn flags_camel_case_string() {
        // Starts with a lowercase letter — not a constant.
        test::<DescribeClass>().expect_offense(indoc! {r#"
                describe 'activeRecord' do
                         ^^^^^^^^^^^^^^ The first argument to describe should be the class or module being tested.
                  subject { Object.const_get(self.class.description) }
                end
            "#});
    }

    #[test]
    fn flags_string_starting_with_number() {
        test::<DescribeClass>().expect_offense(indoc! {r#"
                describe '2Thing' do
                         ^^^^^^^^ The first argument to describe should be the class or module being tested.
                  subject { Object.const_get(self.class.description) }
                end
            "#});
    }

    #[test]
    fn flags_empty_string() {
        // Empty string doesn't satisfy the constant-path regex (no
        // uppercase char).
        test::<DescribeClass>().expect_offense(indoc! {r#"
                describe '' do
                         ^^ The first argument to describe should be the class or module being tested.
                  subject { Object.const_get(self.class.description) }
                end
            "#});
    }

    #[test]
    fn flags_string_with_non_ascii_letter() {
        // Ruby's `\w` is ASCII-only without `/u` — `Foô` does NOT
        // match upstream's `string_constant?` regex, so the cop should
        // FLAG it. We do too.
        test::<DescribeClass>().expect_offense(indoc! {"
                describe 'Foô' do
                         ^^^^^ The first argument to describe should be the class or module being tested.
                end
            "});
    }

    #[test]
    fn ignores_empty_describe_block() {
        // `describe` with no positional arg — arg list is empty,
        // nothing to classify; not a target.
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                RSpec.describe do
                end

                describe do
                end
            "#});
    }

    #[test]
    fn ignores_describe_inside_shared_examples() {
        // `shared_examples` outer block — inner describe is nested.
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                shared_examples 'Common::Interface' do
                  describe '#public_interface' do
                    it 'conforms to interface' do
                      # ...
                    end
                  end
                end
            "#});
    }

    #[test]
    fn ignores_describe_inside_rspec_shared_context() {
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                RSpec.shared_context 'Common::Interface' do
                  describe '#public_interface' do
                    it 'conforms to interface' do
                      # ...
                    end
                  end
                end
            "#});
    }

    #[test]
    fn ignores_describe_inside_unnamed_shared_context() {
        // `shared_context` with no string arg.
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                shared_context do
                  describe '#public_interface' do
                    it 'conforms to interface' do
                      # ...
                    end
                  end
                end
            "#});
    }

    #[test]
    fn ignores_type_metadata_view() {
        // Default `IgnoredMetadata.type` includes `view`.
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                describe 'widgets/index', type: :view do
                end
            "#});
    }

    #[test]
    fn flags_describe_with_non_type_metadata() {
        // `foo: :bar` is not in default `IgnoredMetadata`.
        test::<DescribeClass>().expect_offense(indoc! {r#"
                describe 'foo bar', foo: :bar do
                         ^^^^^^^^^ The first argument to describe should be the class or module being tested.
                end
            "#});
    }

    #[test]
    fn ignores_feature_spec_with_mixed_metadata() {
        // Even with extra positional symbols and unrelated kwargs,
        // a single matching `type: :feature` pair triggers the
        // ignore. (`:test` is a positional argument, not a hash pair.)
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                describe 'my new feature', :test, foo: :bar, type: :feature do
                end
            "#});
    }

    #[test]
    fn flags_describe_with_positional_symbol_metadata_only() {
        // `describe "x", :feature` — RSpec treats positional symbol
        // metadata as `feature: true`, NOT as `type: :feature`. Upstream
        // RuboCop-RSpec's matcher pattern is `... (hash <#ignored_metadata? ...>)`
        // which requires a trailing Hash; bare symbol args do not qualify
        // for the IgnoredMetadata exception. Pinning this so a future
        // change can't silently start accepting `:feature` shorthand as
        // equivalent to `type: :feature` (a divergence from upstream
        // and from RSpec's own semantics).
        test::<DescribeClass>().expect_offense(indoc! {r#"
                describe 'my new feature', :feature do
                         ^^^^^^^^^^^^^^^^ The first argument to describe should be the class or module being tested.
                end
            "#});
    }

    #[test]
    fn flags_non_ignored_type_metadata_value() {
        // `type: :wow` — `wow` is not in the default `type` set.
        test::<DescribeClass>().expect_offense(indoc! {r#"
                describe 'wow', blah, type: :wow do
                         ^^^^^ The first argument to describe should be the class or module being tested.
                end
            "#});
    }

    // === IgnoredMetadata user config ===

    fn user_ignored_metadata() -> DescribeClassOptions {
        let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        map.insert(
            "foo".to_string(),
            ["bar"].iter().map(|s| s.to_string()).collect(),
        );
        map.insert(
            "type".to_string(),
            ["wow"].iter().map(|s| s.to_string()).collect(),
        );
        DescribeClassOptions {
            ignored_metadata: map,
        }
    }

    #[test]
    fn user_config_ignores_configured_metadata_key() {
        test::<DescribeClass>()
            .with_options(&user_ignored_metadata())
            .expect_no_offenses(indoc! {r#"
                describe 'foo bar', foo: :bar do
                end
            "#});
    }

    #[test]
    fn user_config_ignores_configured_type_value() {
        // Replaces the default `type` set — `:view` would still match
        // the default set but the user config only allows `:wow`.
        test::<DescribeClass>()
            .with_options(&user_ignored_metadata())
            .expect_no_offenses(indoc! {r#"
                describe 'my new system test', type: :wow do
                end
            "#});
    }

    // === Top-level gating: extra Murphy pins beyond the RuboCop spec ===

    #[test]
    fn flags_describe_at_root_inside_class_body() {
        // `class Foo; RSpec.describe Bar do; end; end` — the describe
        // Block's parent chain is Class → root (no other Block). The
        // outer is a const-arg describe so it doesn't itself emit, but
        // a string-arg inside a class body should fire.
        test::<DescribeClass>().expect_offense(indoc! {r#"
                class Foo
                  RSpec.describe "scenario" do
                                 ^^^^^^^^^^ The first argument to describe should be the class or module being tested.
                  end
                end
            "#});
    }

    #[test]
    fn ignores_describe_inside_iterator_block() {
        // `[1,2,3].each do; describe "…"; end` — the describe Block's
        // parent is the outer `each` Block (NOT a class/module/begin),
        // so the strict top-level walk rejects it.
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                [1, 2, 3].each do |i|
                  describe "Item #{i}" do
                  end
                end
            "#});
    }

    // === Conservative-flagging stance pins (NOT widened to non-literal args) ===

    #[test]
    fn ignores_non_literal_first_arg_method_call() {
        // RuboCop would flag (`!const !#string_constant?` captures
        // any non-const). Murphy keeps the conservative stance from
        // the original port — a method call's runtime value can't be
        // statically known.
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                describe foo_helper.thing do
                end
            "#});
    }

    #[test]
    fn ignores_non_literal_first_arg_variable() {
        test::<DescribeClass>().expect_no_offenses(indoc! {r#"
                klass = SomeClass
                describe klass do
                end
            "#});
    }

    #[test]
    fn flags_symbol_first_arg() {
        // Symbols are never class refs — flag.
        test::<DescribeClass>().expect_offense(indoc! {r#"
                describe :something do
                         ^^^^^^^^^^ The first argument to describe should be the class or module being tested.
                end
            "#});
    }

    #[test]
    fn flags_dstr_first_arg() {
        // Interpolated strings can't be string constants — flag.
        test::<DescribeClass>().expect_offense(indoc! {r#"
                describe "User #{id}" do
                         ^^^^^^^^^^^^ The first argument to describe should be the class or module being tested.
                end
            "#});
    }

    // === DescribeClassOptions roundtrip (CopOptions impl) ===

    #[test]
    fn options_default_includes_upstream_type_set() {
        let opts = DescribeClassOptions::default();
        let type_set = opts.ignored_metadata.get("type").expect("type key present");
        assert!(type_set.contains("view"));
        assert!(type_set.contains("request"));
        assert!(type_set.contains("feature"));
        // Sanity: defaults don't accidentally include an unrelated key.
        assert!(!opts.ignored_metadata.contains_key("foo"));
    }

    #[test]
    fn options_from_config_json_overrides_defaults() {
        let json = br#"{"IgnoredMetadata":{"flag":["a","b"]}}"#;
        let opts: DescribeClassOptions =
            <DescribeClassOptions as murphy_plugin_api::CopOptions>::from_config_json(json)
                .expect("parses");
        let flag_set = opts.ignored_metadata.get("flag").expect("flag key present");
        assert!(flag_set.contains("a"));
        assert!(flag_set.contains("b"));
        // User config REPLACES defaults — `type` is no longer present.
        assert!(!opts.ignored_metadata.contains_key("type"));
    }

    #[test]
    fn options_root_not_an_object_returns_not_an_object_error() {
        // Mirrors `#[derive(CopOptions)]` — non-object root is a
        // shape error, not a "use defaults" condition.
        let err = <DescribeClassOptions as murphy_plugin_api::CopOptions>::from_config_json(b"[]")
            .expect_err("array root is not an object");
        assert!(matches!(
            err.kind(),
            murphy_plugin_api::ConfigErrorKind::NotAnObject
        ));
    }

    #[test]
    fn options_missing_ignored_metadata_uses_defaults() {
        // Absent field falls back to defaults, the same way the derive
        // treats omitted fields.
        let opts: DescribeClassOptions =
            <DescribeClassOptions as murphy_plugin_api::CopOptions>::from_config_json(b"{}")
                .expect("empty object decodes");
        assert!(
            opts.ignored_metadata
                .get("type")
                .expect("default type set present")
                .contains("feature")
        );
    }

    #[test]
    fn options_ignored_metadata_not_an_object_errors() {
        // `IgnoredMetadata` must be a map. A bare string is invalid.
        let json = br#"{"IgnoredMetadata": "wrong"}"#;
        let err = <DescribeClassOptions as murphy_plugin_api::CopOptions>::from_config_json(json)
            .expect_err("string-valued IgnoredMetadata is invalid");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "IgnoredMetadata");
        assert_eq!(*expected, "object");
    }

    #[test]
    fn options_ignored_metadata_value_not_array_errors() {
        // Inner value must be an array; the upstream YAML shape is
        // `{ key => [values...] }`, not `{ key => value }`.
        let json = br#"{"IgnoredMetadata": {"type": "request"}}"#;
        let err = <DescribeClassOptions as murphy_plugin_api::CopOptions>::from_config_json(json)
            .expect_err("non-array IgnoredMetadata value is invalid");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "IgnoredMetadata.type");
        assert_eq!(*expected, "array of strings");
    }

    #[test]
    fn options_ignored_metadata_array_element_not_string_errors() {
        // Each element must be a string. A non-string element is a
        // shape error tagged with the index for actionable diagnostics.
        let json = br#"{"IgnoredMetadata": {"type": ["request", 42]}}"#;
        let err = <DescribeClassOptions as murphy_plugin_api::CopOptions>::from_config_json(json)
            .expect_err("non-string element is invalid");
        let murphy_plugin_api::ConfigErrorKind::TypeMismatch { field, expected } = err.kind()
        else {
            panic!("expected TypeMismatch, got {:?}", err.kind());
        };
        assert_eq!(field, "IgnoredMetadata.type[1]");
        assert_eq!(*expected, "string");
    }

    #[test]
    fn options_roundtrip_via_to_config_json() {
        let opts = user_ignored_metadata();
        let serialized =
            <DescribeClassOptions as murphy_plugin_api::CopOptions>::to_config_json(&opts);
        let decoded = <DescribeClassOptions as murphy_plugin_api::CopOptions>::from_config_json(
            serialized.as_bytes(),
        )
        .expect("roundtrip");
        assert_eq!(decoded.ignored_metadata, opts.ignored_metadata);
    }
}

murphy_plugin_api::submit_cop!(DescribeClass);
