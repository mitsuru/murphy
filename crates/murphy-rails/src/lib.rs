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

// `ActionControllerFlashBeforeRender` promoted to real cop in
// `cops::rails::action_controller_flash_before_render`.

// `ActionFilter` promoted to real cop in
// `cops::rails::action_filter`.

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

// `PluralizationGrammar` promoted to real cop in
// `cops::rails::pluralization_grammar`.

// `Presence` promoted to real cop in
// `cops::rails::presence`.

// `Present` promoted to real cop in
// `cops::rails::present`.

// `RakeEnvironment` promoted to real cop in
// `cops::rails::rake_environment`.

// `ReadWriteAttribute` promoted to real cop in
// `cops::rails::read_write_attribute`.

// `RedirectBackOrTo` promoted to real cop in
// `cops::rails::redirect_back_or_to`.

// `RedundantActiveRecordAllMethod` promoted to real cop in
// `cops::rails::redundant_active_record_all_method`.

// `RedundantAllowNil` promoted to real cop in
// `cops::rails::redundant_allow_nil`.

// `RedundantForeignKey` promoted to real cop in
// `cops::rails::redundant_foreign_key`.

// `RedundantPresenceValidationOnBelongsTo` promoted to real cop in
// `cops::rails::redundant_presence_validation_on_belongs_to`.

// `RedundantReceiverInWithOptions` promoted to real cop in
// `cops::rails::redundant_receiver_in_with_options`.
// `RedundantTravelBack` promoted to real cop in
// `cops::rails::redundant_travel_back`.
// `ReflectionClassName` promoted to real cop in
// `cops::rails::reflection_class_name`.
// `RelativeDateConstant` promoted to real cop in
// `cops::rails::relative_date_constant`.
// `RenderInline` promoted to real cop in
// `cops::rails::render_inline`.
// `RenderPlainText` promoted to real cop in
// `cops::rails::render_plain_text`.
// `RequireDependency` promoted to real cop in
// `cops::rails::require_dependency`.
// `ResponseParsedBody` promoted to real cop in
// `cops::rails::response_parsed_body`.
// `ReversibleMigration` promoted to real cop in
// `cops::rails::reversible_migration`.
// `ReversibleMigrationMethodDefinition` promoted to real cop in
// `cops::rails::reversible_migration_method_definition`.

// `RootJoinChain` promoted to real cop in
// `cops::rails::root_join_chain`.

// `RootPathnameMethods` promoted to real cop in
// `cops::rails::root_pathname_methods`.

// `RootPublicPath` promoted to real cop in
// `cops::rails::root_public_path`.

// `SafeNavigation` promoted to real cop in
// `cops::rails::safe_navigation`.

// `SafeNavigationWithBlank` promoted to real cop in
// `cops::rails::safe_navigation_with_blank`.

// `SaveBang` promoted to real cop in
// `cops::rails::save_bang`.

// `SchemaComment` promoted to real cop in
// `cops::rails::schema_comment`.

// `ScopeArgs` promoted to real cop in
// `cops::rails::scope_args`.

// `SelectMap` promoted to real cop in
// `cops::rails::select_map`.

// `ShortI18n` promoted to real cop in
// `cops::rails::short_i18n`.

// `SkipsModelValidations` promoted to real cop in
// `cops::rails::skips_model_validations`.

// `SquishedSQLHeredocs` promoted to real cop in
// `cops::rails::squished_sql_heredocs`.

// `StripHeredoc` promoted to real cop in
// `cops::rails::strip_heredoc`.

// `StrongParametersExpect` promoted to real cop in
// `cops::rails::strong_parameters_expect`.

// `TableNameAssignment` promoted to real cop in
// `cops::rails::table_name_assignment`.

// `ThreeStateBooleanColumn` promoted to real cop in
// `cops::rails::three_state_boolean_column`.

// `TimeZone` promoted to real cop in
// `cops::rails::time_zone`.

// `TimeZoneAssignment` promoted to real cop in
// `cops::rails::time_zone_assignment`.

// `ToFormattedS` promoted to real cop in
// `cops::rails::to_formatted_s`.

// `ToSWithArgument` promoted to real cop in
// `cops::rails::to_s_with_argument`.

// `TopLevelHashWithIndifferentAccess` promoted to real cop in
// `cops::rails::top_level_hash_with_indifferent_access`.

// `TransactionExitStatement` promoted to real cop in
// `cops::rails::transaction_exit_statement`.

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

// `UnknownEnv` promoted to real cop in
// `cops::rails::unknown_env`.

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

// `UnusedRenderContent` promoted to real cop in
// `cops::rails::unused_render_content`.

// `Validation` promoted to real cop in
// `cops::rails::validation`.

// `WhereEquals` promoted to real cop in
// `cops::rails::where_equals`.

// `WhereExists` promoted to real cop in
// `cops::rails::where_exists`.

// `WhereMissing` promoted to real cop in
// `cops::rails::where_missing`.

// `WhereNot` promoted to real cop in
// `cops::rails::where_not`.

// `WhereNotWithMultipleConditions` promoted to real cop in
// `cops::rails::where_not_with_multiple_conditions`.

// `WhereRange` promoted to real cop in
// `cops::rails::where_range`.
