//! murphy-rails — Rails-focused dynamic plugin pack (cdylib).
//!
//! 138 RuboCop-rails cops registered as **arena-migration stubs**.
//! Each stub uses the standard `#[cop]` / `#[on_new_investigation]`
//! authorship pattern (same as `murphy-rspec` and `murphy-example-pack`)
//! with a no-op `investigate` body and `default_enabled = false`, so
//! the cop is inert at runtime but is enumerable by `murphy cops list`
//! and accepts `[cops.rules."Rails/..."]` config sections without
//! error (`§14a` of `docs/plans/2026-05-22-plugin-reboot-design.md`).
//!
//! Individual cops are migrated to the real arena AST by `murphy-au8`
//! subtasks — for each migrated cop the corresponding stub here is
//! replaced by a full `#[cop(...)] impl` with real `#[on_node]`
//! dispatch, and the cop name is removed from the
//! `is_cop_disabled_by_default` hardcode list in
//! `crates/murphy-core/src/config.rs` (cleanup tracked by
//! `murphy-bnd`).
//!
//! Cop names mirror RuboCop-rails 2.35.0 (rebuilt from the
//! pre-`murphy-9cr.22` rails crate; see `git show
//! 46a1de6^:crates/murphy-rails/src/cops/rails/`).

use murphy_plugin_api::{Cx, NoOptions, cop, register_cops, submit_cop};

/// default.yml embedded in the .so as a resource.
pub const BUNDLED_DEFAULTS_YAML: &str = include_str!("../config/default.yml");

/// Pure data symbol the host reads after dlopen (not a behavior callback).
///
/// The `RawSlice` points at this `.so`'s `'static` rodata, valid only while
/// the `libloading::Library` is held. The host must copy the bytes to an
/// owned value while the `Library` is alive (see
/// `murphy_core::plugin_loader::load_plugin_pack`).
#[unsafe(no_mangle)]
pub static MURPHY_PLUGIN_DEFAULT_CONFIG: murphy_plugin_api::RawSlice =
    murphy_plugin_api::RawSlice::from_str(BUNDLED_DEFAULTS_YAML);

// Cops in separate files register themselves via submit_cop! in their own modules.
pub mod cops;

// cop の登録は各 cop ファイル (inline stub / cops::rails::* 両方) の submit_cop!(T) が担う。
register_cops!(mode = dynamic);

#[cfg(test)]
mod option_key_guard {
    /// Every cop option's config key must be RuboCop-style PascalCase, or
    /// `.murphy.yml` config silently no-ops. See `murphy-pj12`.
    #[test]
    fn all_option_keys_are_pascal_case() {
        murphy_plugin_api::test_support::assert_pack_option_keys_pascal_case(&crate::PACK_COPS);
    }
}

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ActionControllerFlashBeforeRender
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ActionControllerFlashBeforeRender;

#[cop(
    name = "Rails/ActionControllerFlashBeforeRender",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ActionControllerFlashBeforeRender {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ActionControllerFlashBeforeRender);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ActionFilter
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ActionFilter;

#[cop(
    name = "Rails/ActionFilter",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ActionFilter {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ActionFilter);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ActionOrder
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ActionOrder;

#[cop(
    name = "Rails/ActionOrder",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ActionOrder {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ActionOrder);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ActiveRecordCallbacksOrder
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ActiveRecordCallbacksOrder;

#[cop(
    name = "Rails/ActiveRecordCallbacksOrder",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ActiveRecordCallbacksOrder {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ActiveRecordCallbacksOrder);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ActiveRecordOverride
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ActiveRecordOverride;

#[cop(
    name = "Rails/ActiveRecordOverride",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ActiveRecordOverride {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ActiveRecordOverride);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ActiveSupportAliases
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ActiveSupportAliases;

#[cop(
    name = "Rails/ActiveSupportAliases",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ActiveSupportAliases {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ActiveSupportAliases);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ActiveSupportOnLoad
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ActiveSupportOnLoad;

#[cop(
    name = "Rails/ActiveSupportOnLoad",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ActiveSupportOnLoad {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ActiveSupportOnLoad);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/AddColumnIndex
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct AddColumnIndex;

#[cop(
    name = "Rails/AddColumnIndex",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl AddColumnIndex {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(AddColumnIndex);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/AfterCommitOverride
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct AfterCommitOverride;

#[cop(
    name = "Rails/AfterCommitOverride",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl AfterCommitOverride {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(AfterCommitOverride);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ApplicationJob
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ApplicationJob;

#[cop(
    name = "Rails/ApplicationJob",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ApplicationJob {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ApplicationJob);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ApplicationMailer
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ApplicationMailer;

#[cop(
    name = "Rails/ApplicationMailer",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ApplicationMailer {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ApplicationMailer);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/BulkChangeTable
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct BulkChangeTable;

#[cop(
    name = "Rails/BulkChangeTable",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl BulkChangeTable {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(BulkChangeTable);

// `EnvironmentVariableAccess` promoted to real cop in
// `cops::rails::environment_variable_access`.

// `FilePath` promoted to real cop in
// `cops::rails::file_path`.

// `FindBy` promoted to real cop in
// `cops::rails::find_by`.

// `FindById` promoted to real cop in
// `cops::rails::find_by_id`.

// `FindByOrAssignmentMemoization` promoted to real cop in
// `cops::rails::find_by_or_assignment_memoization`.

// `FindEach` promoted to real cop in
// `cops::rails::find_each`.

// `FreezeTime` promoted to real cop in
// `cops::rails::freeze_time`.

// `HasAndBelongsToMany` promoted to real cop in
// `cops::rails::has_and_belongs_to_many`.

// `HasManyOrHasOneDependent` promoted to real cop in
// `cops::rails::has_many_or_has_one_dependent`.

// `HelperInstanceVariable` promoted to real cop in
// `cops::rails::helper_instance_variable`.

// `HttpPositionalArguments` promoted to real cop in
// `cops::rails::http_positional_arguments`.

// `HttpStatusNameConsistency` promoted to real cop in
// `cops::rails::http_status_name_consistency`.

// `I18nLazyLookup` promoted to real cop in
// `cops::rails::i18n_lazy_lookup`.

// `I18nLocaleAssignment` promoted to real cop in
// `cops::rails::i18n_locale_assignment`.

// `I18nLocaleTexts` promoted to real cop in
// `cops::rails::i18n_locale_texts`.

// `IgnoredColumnsAssignment` promoted to real cop in
// `cops::rails::ignored_columns_assignment`.

// `IgnoredSkipActionFilterOption` promoted to real cop in
// `cops::rails::ignored_skip_action_filter_option`.

// `IndexBy` promoted to real cop in
// `cops::rails::index_by`.

// `IndexWith` promoted to real cop in
// `cops::rails::index_with`.

// `Inquiry` promoted to real cop in
// `cops::rails::inquiry`.

// `InverseOf` promoted to real cop in
// `cops::rails::inverse_of`.

// `LexicallyScopedActionFilter` promoted to real cop in
// `cops::rails::lexically_scoped_action_filter`.

// `LinkToBlank` promoted to real cop in
// `cops::rails::link_to_blank`.

// `MailerName` promoted to real cop in
// `cops::rails::mailer_name`.

// `MatchRoute` promoted to real cop in
// `cops::rails::match_route`.

// `MigrationClassName` promoted to real cop in
// `cops::rails::migration_class_name`.

// `MultipleRoutePaths` promoted to real cop in
// `cops::rails::multiple_route_paths`.

// `NegateInclude` promoted to real cop in
// `cops::rails::negate_include`.

// `NotNullColumn` promoted to real cop in
// `cops::rails::not_null_column`.

// `OrderArguments` promoted to real cop in
// `cops::rails::order_arguments`.

// `OrderById` promoted to real cop in
// `cops::rails::order_by_id`.

// `Output` is now a real cop in `cops::rails::output` — `pub use`d at
// the crate root via the `use cops::rails::Output;` above so the
// `register_cops!` ident below resolves unchanged.

// `OutputSafety` promoted to real cop in
// `cops::rails::output_safety`.

// `Pick` is now a real cop in `cops::rails::pick` — `pub use`d at the
// crate root via the `use cops::rails::{AssertNot, Output, Pick,
// RequestReferer};` above so the `register_cops!` ident below resolves
// unchanged.

// `Pluck` promoted to real cop in
// `cops::rails::pluck`.

// `PluckId` promoted to real cop in
// `cops::rails::pluck_id`.

// `PluckInWhere` promoted to real cop in
// `cops::rails::pluck_in_where`.

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/PluralizationGrammar
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct PluralizationGrammar;

#[cop(
    name = "Rails/PluralizationGrammar",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl PluralizationGrammar {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(PluralizationGrammar);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/Presence
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct Presence;

#[cop(
    name = "Rails/Presence",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl Presence {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(Presence);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/Present
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct Present;

#[cop(
    name = "Rails/Present",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl Present {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(Present);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RakeEnvironment
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RakeEnvironment;

#[cop(
    name = "Rails/RakeEnvironment",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RakeEnvironment {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RakeEnvironment);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ReadWriteAttribute
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ReadWriteAttribute;

#[cop(
    name = "Rails/ReadWriteAttribute",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ReadWriteAttribute {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ReadWriteAttribute);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RedirectBackOrTo
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RedirectBackOrTo;

#[cop(
    name = "Rails/RedirectBackOrTo",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RedirectBackOrTo {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RedirectBackOrTo);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RedundantActiveRecordAllMethod
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RedundantActiveRecordAllMethod;

#[cop(
    name = "Rails/RedundantActiveRecordAllMethod",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantActiveRecordAllMethod {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RedundantActiveRecordAllMethod);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RedundantAllowNil
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RedundantAllowNil;

#[cop(
    name = "Rails/RedundantAllowNil",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantAllowNil {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RedundantAllowNil);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RedundantForeignKey
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RedundantForeignKey;

#[cop(
    name = "Rails/RedundantForeignKey",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantForeignKey {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RedundantForeignKey);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RedundantPresenceValidationOnBelongsTo
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RedundantPresenceValidationOnBelongsTo;

#[cop(
    name = "Rails/RedundantPresenceValidationOnBelongsTo",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantPresenceValidationOnBelongsTo {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RedundantPresenceValidationOnBelongsTo);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RedundantReceiverInWithOptions
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RedundantReceiverInWithOptions;

#[cop(
    name = "Rails/RedundantReceiverInWithOptions",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantReceiverInWithOptions {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RedundantReceiverInWithOptions);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RedundantTravelBack
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RedundantTravelBack;

#[cop(
    name = "Rails/RedundantTravelBack",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantTravelBack {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RedundantTravelBack);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ReflectionClassName
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ReflectionClassName;

#[cop(
    name = "Rails/ReflectionClassName",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ReflectionClassName {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ReflectionClassName);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RelativeDateConstant
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RelativeDateConstant;

#[cop(
    name = "Rails/RelativeDateConstant",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RelativeDateConstant {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RelativeDateConstant);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RenderInline
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RenderInline;

#[cop(
    name = "Rails/RenderInline",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RenderInline {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RenderInline);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RenderPlainText
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RenderPlainText;

#[cop(
    name = "Rails/RenderPlainText",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RenderPlainText {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RenderPlainText);

// `RequestReferer` is now a real cop in `cops::rails::request_referer`
// — `pub use`d at the crate root via the `use cops::rails::{Output,
// RequestReferer};` above so the `register_cops!` ident below resolves
// unchanged.

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RequireDependency
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RequireDependency;

#[cop(
    name = "Rails/RequireDependency",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RequireDependency {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RequireDependency);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ResponseParsedBody
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ResponseParsedBody;

#[cop(
    name = "Rails/ResponseParsedBody",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ResponseParsedBody {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ResponseParsedBody);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ReversibleMigration
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ReversibleMigration;

#[cop(
    name = "Rails/ReversibleMigration",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ReversibleMigration {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ReversibleMigration);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ReversibleMigrationMethodDefinition
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ReversibleMigrationMethodDefinition;

#[cop(
    name = "Rails/ReversibleMigrationMethodDefinition",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ReversibleMigrationMethodDefinition {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ReversibleMigrationMethodDefinition);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RootJoinChain
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RootJoinChain;

#[cop(
    name = "Rails/RootJoinChain",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RootJoinChain {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RootJoinChain);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RootPathnameMethods
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RootPathnameMethods;

#[cop(
    name = "Rails/RootPathnameMethods",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RootPathnameMethods {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RootPathnameMethods);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/RootPublicPath
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct RootPublicPath;

#[cop(
    name = "Rails/RootPublicPath",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RootPublicPath {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(RootPublicPath);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/SafeNavigation
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct SafeNavigation;

#[cop(
    name = "Rails/SafeNavigation",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl SafeNavigation {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(SafeNavigation);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/SafeNavigationWithBlank
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct SafeNavigationWithBlank;

#[cop(
    name = "Rails/SafeNavigationWithBlank",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl SafeNavigationWithBlank {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(SafeNavigationWithBlank);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/SaveBang
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct SaveBang;

#[cop(
    name = "Rails/SaveBang",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl SaveBang {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(SaveBang);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/SchemaComment
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct SchemaComment;

#[cop(
    name = "Rails/SchemaComment",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl SchemaComment {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(SchemaComment);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ScopeArgs
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ScopeArgs;

#[cop(
    name = "Rails/ScopeArgs",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ScopeArgs {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ScopeArgs);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/SelectMap
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct SelectMap;

#[cop(
    name = "Rails/SelectMap",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl SelectMap {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(SelectMap);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ShortI18n
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ShortI18n;

#[cop(
    name = "Rails/ShortI18n",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ShortI18n {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ShortI18n);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/SkipsModelValidations
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct SkipsModelValidations;

#[cop(
    name = "Rails/SkipsModelValidations",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl SkipsModelValidations {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(SkipsModelValidations);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/SquishedSQLHeredocs
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct SquishedSQLHeredocs;

#[cop(
    name = "Rails/SquishedSQLHeredocs",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl SquishedSQLHeredocs {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(SquishedSQLHeredocs);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/StripHeredoc
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct StripHeredoc;

#[cop(
    name = "Rails/StripHeredoc",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl StripHeredoc {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(StripHeredoc);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/StrongParametersExpect
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct StrongParametersExpect;

#[cop(
    name = "Rails/StrongParametersExpect",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl StrongParametersExpect {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(StrongParametersExpect);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/TableNameAssignment
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct TableNameAssignment;

#[cop(
    name = "Rails/TableNameAssignment",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl TableNameAssignment {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(TableNameAssignment);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ThreeStateBooleanColumn
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ThreeStateBooleanColumn;

#[cop(
    name = "Rails/ThreeStateBooleanColumn",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ThreeStateBooleanColumn {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ThreeStateBooleanColumn);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/TimeZone
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct TimeZone;

#[cop(
    name = "Rails/TimeZone",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl TimeZone {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(TimeZone);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/TimeZoneAssignment
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct TimeZoneAssignment;

#[cop(
    name = "Rails/TimeZoneAssignment",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl TimeZoneAssignment {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(TimeZoneAssignment);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ToFormattedS
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ToFormattedS;

#[cop(
    name = "Rails/ToFormattedS",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ToFormattedS {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ToFormattedS);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ToSWithArgument
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct ToSWithArgument;

#[cop(
    name = "Rails/ToSWithArgument",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ToSWithArgument {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(ToSWithArgument);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/TopLevelHashWithIndifferentAccess
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct TopLevelHashWithIndifferentAccess;

#[cop(
    name = "Rails/TopLevelHashWithIndifferentAccess",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl TopLevelHashWithIndifferentAccess {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(TopLevelHashWithIndifferentAccess);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/TransactionExitStatement
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct TransactionExitStatement;

#[cop(
    name = "Rails/TransactionExitStatement",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl TransactionExitStatement {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(TransactionExitStatement);

// `UniqBeforePluck` promoted to real cop in
// `cops::rails::uniq_before_pluck`.

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/UniqueValidationWithoutIndex
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct UniqueValidationWithoutIndex;

#[cop(
    name = "Rails/UniqueValidationWithoutIndex",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl UniqueValidationWithoutIndex {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(UniqueValidationWithoutIndex);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/UnknownEnv
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct UnknownEnv;

#[cop(
    name = "Rails/UnknownEnv",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl UnknownEnv {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(UnknownEnv);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/UnusedIgnoredColumns
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct UnusedIgnoredColumns;

#[cop(
    name = "Rails/UnusedIgnoredColumns",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl UnusedIgnoredColumns {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(UnusedIgnoredColumns);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/UnusedRenderContent
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct UnusedRenderContent;

#[cop(
    name = "Rails/UnusedRenderContent",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl UnusedRenderContent {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(UnusedRenderContent);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/Validation
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct Validation;

#[cop(
    name = "Rails/Validation",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl Validation {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(Validation);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/WhereEquals
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct WhereEquals;

#[cop(
    name = "Rails/WhereEquals",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl WhereEquals {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(WhereEquals);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/WhereExists
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct WhereExists;

#[cop(
    name = "Rails/WhereExists",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl WhereExists {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(WhereExists);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/WhereMissing
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct WhereMissing;

#[cop(
    name = "Rails/WhereMissing",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl WhereMissing {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(WhereMissing);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/WhereNot
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct WhereNot;

#[cop(
    name = "Rails/WhereNot",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl WhereNot {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(WhereNot);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/WhereNotWithMultipleConditions
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct WhereNotWithMultipleConditions;

#[cop(
    name = "Rails/WhereNotWithMultipleConditions",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl WhereNotWithMultipleConditions {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(WhereNotWithMultipleConditions);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/WhereRange
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
#[derive(Default)]
pub struct WhereRange;

#[cop(
    name = "Rails/WhereRange",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl WhereRange {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}
submit_cop!(WhereRange);
