//! murphy-rspec — RSpec cop pack (murphy-4n9).
//!
//! v1 cops (under [`cops::rspec`]):
//! - `RSpec/DescribeClass` — bootstrap (murphy-4n9.4).
//! - `RSpec/ExampleLength` — line cap on example bodies (murphy-6bv).
//! - `RSpec/MultipleExpectations` — `expect(...)` count cap per
//!   example (murphy-6tq).
//! - `RSpec/DescribeSymbol` — avoid describing symbols (murphy-4gd.3.1).
//! - `RSpec/Focus` — flags focused specs (murphy-4gd.3.1).
//! - `RSpec/EmptyHook` — flags empty hooks (murphy-4gd.3.1).
//! - `RSpec/MultipleDescribes` — multiple top-level groups (murphy-4gd.3.1).
//! - `RSpec/ContextMethod` — `context` should not describe methods (murphy-4gd.3.2).
//! - `RSpec/BeforeAfterAll` — avoid `before(:all)` / `after(:context)` (murphy-4gd.3.2).
//! - `RSpec/Be` — flag bare `be` without argument (murphy-4gd.3.2).
//! - `RSpec/BeEql` — prefer `be` over `eql` for identity literals (murphy-4gd.3.2).
//! - `RSpec/BeEmpty` — prefer `be_empty` for empty-array checks (murphy-4gd.3.3).
//! - `RSpec/BeEq` — prefer `be` over `eq` for booleans/nil (murphy-4gd.3.3).
//! - `RSpec/BeNil` — consistent `nil` matching style (murphy-4gd.3.3).
//! - `RSpec/Eq` — use `eq` instead of `be ==` (murphy-4gd.3.3).
//! - `RSpec/ClassCheck` — consistent `be_a` / `be_kind_of` style (murphy-4gd.3.4).
//! - `RSpec/ContainExactly` — prefer `match_array` for all-splat args (murphy-4gd.3.4).
//! - `RSpec/MatchArray` — prefer `contain_exactly` for array literals (murphy-4gd.3.4).
//! - `RSpec/NotToNot` — consistent `not_to` / `to_not` spelling (murphy-4gd.3.4).
//! - `RSpec/IdenticalEqualityAssertion` — flag identical equality sides (murphy-4gd.3.4).
//! - `RSpec/DescribeMethod` — second `describe` arg should name a method (murphy-4gd.3.6).
//! - `RSpec/ExpectInHook` — no `expect` in hooks (murphy-4gd.3.6).
//! - `RSpec/ExpectInLet` — no `expect` in `let` (murphy-4gd.3.6).
//! - `RSpec/HookArgument` — consistent hook scope style (murphy-4gd.3.6).
//! - `RSpec/VoidExpect` — flag `expect()` without `.to` / `.not_to` (murphy-4gd.3.6).
//! - `RSpec/AnyInstance` — avoid stubbing any instance globally (murphy-4gd.3.5).
//! - `RSpec/ItBehavesLike` — consistent shared-example inclusion style (murphy-4gd.3.5).
//! - `RSpec/VerifiedDoubles` — prefer verifying doubles (murphy-4gd.3.5).
//! - `RSpec/ReceiveNever` — prefer `not_to receive` over `never` (murphy-4gd.3.5).
//! - `RSpec/SubjectDeclaration` — define subject with the subject helper (murphy-4gd.3.5).
//! - `RSpec/AroundBlock` — `around` hooks must run the test (murphy-4gd.3.7).
//! - `RSpec/ChangeByZero` — prefer negated matchers over `change.by(0)` (murphy-4gd.3.7).
//! - `RSpec/ContextWording` — `context` docstring prefix style (murphy-4gd.3.7).
//! - `RSpec/DescribedClassModuleWrapping` — no specs inside `module` (murphy-4gd.3.7).
//! - `RSpec/ExampleWithoutDescription` — examples need descriptions (murphy-4gd.3.7).
//! - `RSpec/DescribedClass` — prefer `described_class` helper (murphy-4gd.3.8).
//! - `RSpec/Dialect` — custom RSpec dialect preferences (murphy-4gd.3.8).
//! - `RSpec/DuplicatedMetadata` — no duplicated metadata (murphy-4gd.3.8).
//! - `RSpec/EmptyExampleGroup` — no empty example groups (murphy-4gd.3.8).
//! - `RSpec/EmptyLineAfterExample` — blank line after examples (murphy-4gd.3.8).
//! - `RSpec/EmptyLineAfterExampleGroup` — blank line after groups (murphy-4gd.3.9).
//! - `RSpec/EmptyLineAfterFinalLet` — blank line after final `let` (murphy-4gd.3.9).
//! - `RSpec/EmptyLineAfterHook` — blank line after hooks (murphy-4gd.3.9).
//! - `RSpec/EmptyLineAfterSubject` — blank line after `subject` (murphy-4gd.3.9).
//! - `RSpec/EmptyMetadata` — no empty metadata hash (murphy-4gd.3.9).
//! - `RSpec/EmptyOutput` — no empty-string `output` matcher (murphy-4gd.3.10).
//! - `RSpec/ExampleWording` — example wording style (murphy-4gd.3.10).
//! - `RSpec/ExcessiveDocstringSpacing` — no excessive whitespace in descriptions (murphy-4gd.3.10).
//! - `RSpec/ExpectActual` — actual value in `expect(...)` (murphy-4gd.3.10).
//! - `RSpec/ExpectChange` — consistent `change` style (murphy-4gd.3.10).
//! - `RSpec/ExpectOutput` — `expect { ... }.to output` over `$stdout` mutation (murphy-4gd.3.11).
//! - `RSpec/HooksBeforeExamples` — hooks above examples (murphy-4gd.3.11).
//! - `RSpec/ImplicitBlockExpectation` — no implicit block expectations (murphy-4gd.3.11).
//! - `RSpec/ImplicitExpect` — consistent `is_expected` / `should` style (murphy-4gd.3.11).
//! - `RSpec/ImplicitSubject` — explicit vs implicit subject style (murphy-4gd.3.11).
//! - `RSpec/IncludeExamples` — prefer `it_behaves_like` over `include_examples` (murphy-4gd.3.12).
//! - `RSpec/IndexedLet` — no indexed `let` names like `item_1` (murphy-4gd.3.12).
//! - `RSpec/InstanceSpy` — use `instance_spy` with `have_received` (murphy-4gd.3.12).
//! - `RSpec/InstanceVariable` — avoid instance variables in specs (murphy-4gd.3.12).
//! - `RSpec/IsExpectedSpecify` — use `it` for one-line `is_expected` (murphy-4gd.3.12).
//! - `RSpec/IteratedExpectation` — use `all` instead of iterating (murphy-4gd.3.13).
//! - `RSpec/LeadingSubject` — `subject` first in the group (murphy-4gd.3.13).
//! - `RSpec/LeakyConstantDeclaration` — stub constants, don't declare them (murphy-4gd.3.13).
//! - `RSpec/LetBeforeExamples` — `let` before examples (murphy-4gd.3.13).
//! - `RSpec/LetSetup` — no unreferenced `let!` setup (murphy-4gd.3.13).
//! - `RSpec/AlignLeftLetBrace` — align `{` of adjacent lets (murphy-4gd.3.14).
//! - `RSpec/AlignRightLetBrace` — align `}` of adjacent lets (murphy-4gd.3.14).
//! - `RSpec/MessageChain` — no `receive_message_chain` / `stub_chain` (murphy-4gd.3.14).
//! - `RSpec/MessageExpectation` — consistent `allow` / `expect` style (murphy-4gd.3.14).
//! - `RSpec/MessageSpies` — set message expectations with spies (murphy-4gd.3.14).
//!
//! Source layout: each namespace lives under `src/cops/<namespace>/`
//! so the file path tells you the cop's id at a glance.
//!
//! Authored against `murphy-plugin-api` only (single-surface ABI, ADR
//! 0038); the runtime `murphy-` dep set is asserted by
//! `tests/dep_boundary.rs`.

pub mod cops;

/// rubocop-rspec-derived per-cop defaults embedded in the `.so` as a resource.
///
/// Carries the file-scope defaults that cannot be expressed through
/// `#[cop]` / `#[option]` metadata — currently `RSpec/DescribeClass: Exclude`
/// for the non-class spec directories and `RSpec/BeforeAfterAll: Exclude`
/// for helper/support files. The host merges this below user config
/// via `MurphyConfig::apply_pack_default_layers` in `murphy-core`.
pub const BUNDLED_DEFAULTS_YAML: &str = include_str!("../config/default.yml");

/// Pure data symbol the host reads after dlopen (not a behavior callback).
///
/// The `RawSlice` points at this `.so`'s `'static` rodata, valid only while
/// the `libloading::Library` is held. The host copies the bytes to an owned
/// value while the `Library` is alive (see
/// `murphy_core::plugin_loader::load_plugin_pack`).
#[unsafe(no_mangle)]
pub static MURPHY_PLUGIN_DEFAULT_CONFIG: murphy_plugin_api::RawSlice =
    murphy_plugin_api::RawSlice::from_str(BUNDLED_DEFAULTS_YAML);

// cop の登録は各 cop ファイルの submit_cop!(T) が担う。
murphy_plugin_api::register_cops!(mode = dynamic);

#[cfg(test)]
mod tests {
    /// Dummy smoke test: ensures `cargo test --workspace` materialises
    /// the cdylib build artifact (the e2e test in
    /// `crates/murphy-cli/tests/rspec_pack_e2e.rs` reads it via dlopen).
    /// The Cargo dep graph already guarantees this through `murphy-cli`'s
    /// `[dev-dependencies]`, but the explicit test keeps the invariant
    /// local to this crate.
    #[test]
    fn smoke_compiles() {}
}

#[cfg(test)]
mod option_key_guard {
    /// Every cop option's config key must be RuboCop-style PascalCase, or
    /// `.murphy.yml` config silently no-ops. See `murphy-pj12`.
    #[test]
    fn all_option_keys_are_pascal_case() {
        murphy_plugin_api::test_support::assert_pack_option_keys_pascal_case(&crate::PACK_COPS);
    }
}
