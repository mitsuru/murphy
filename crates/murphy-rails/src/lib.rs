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

use murphy_plugin_api::{Cx, NoOptions, cop, register_cops};

// Cops promoted out of the stub macro into real arena dispatch live
// under `cops::<namespace>::<name>` (mirrors `murphy-rspec` /
// `murphy-std`). The `register_cops!` list below re-exports them as
// bare idents via `use`, keeping the registration table flat.
pub mod cops;
use cops::rails::{
    AssertNot, EnvironmentVariableAccess, I18nLocaleAssignment, NegateInclude, Output, Pick,
    RequestReferer, UniqBeforePluck,
};

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

#[derive(Default)]
pub struct ActionControllerTestCase;

#[cop(
    name = "Rails/ActionControllerTestCase",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ActionControllerTestCase {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

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

#[derive(Default)]
pub struct ActiveRecordAliases;

#[cop(
    name = "Rails/ActiveRecordAliases",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ActiveRecordAliases {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

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

#[derive(Default)]
pub struct ApplicationController;

#[cop(
    name = "Rails/ApplicationController",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ApplicationController {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

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

#[derive(Default)]
pub struct ApplicationRecord;

#[cop(
    name = "Rails/ApplicationRecord",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ApplicationRecord {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct ArelStar;

#[cop(
    name = "Rails/ArelStar",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ArelStar {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct AttributeDefaultBlockValue;

#[cop(
    name = "Rails/AttributeDefaultBlockValue",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl AttributeDefaultBlockValue {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct BelongsTo;

#[cop(
    name = "Rails/BelongsTo",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl BelongsTo {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct Blank;

#[cop(
    name = "Rails/Blank",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl Blank {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

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

#[derive(Default)]
pub struct CompactBlank;

#[cop(
    name = "Rails/CompactBlank",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl CompactBlank {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct ContentTag;

#[cop(
    name = "Rails/ContentTag",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ContentTag {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct CreateTableWithTimestamps;

#[cop(
    name = "Rails/CreateTableWithTimestamps",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl CreateTableWithTimestamps {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct DangerousColumnNames;

#[cop(
    name = "Rails/DangerousColumnNames",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl DangerousColumnNames {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct Date;

#[cop(
    name = "Rails/Date",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl Date {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct DefaultScope;

#[cop(
    name = "Rails/DefaultScope",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl DefaultScope {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct Delegate;

#[cop(
    name = "Rails/Delegate",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl Delegate {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct DelegateAllowBlank;

#[cop(
    name = "Rails/DelegateAllowBlank",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl DelegateAllowBlank {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct DeprecatedActiveModelErrorsMethods;

#[cop(
    name = "Rails/DeprecatedActiveModelErrorsMethods",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl DeprecatedActiveModelErrorsMethods {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct DotSeparatedKeys;

#[cop(
    name = "Rails/DotSeparatedKeys",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl DotSeparatedKeys {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct DuplicateAssociation;

#[cop(
    name = "Rails/DuplicateAssociation",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl DuplicateAssociation {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct DuplicateScope;

#[cop(
    name = "Rails/DuplicateScope",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl DuplicateScope {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct DurationArithmetic;

#[cop(
    name = "Rails/DurationArithmetic",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl DurationArithmetic {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct DynamicFindBy;

#[cop(
    name = "Rails/DynamicFindBy",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl DynamicFindBy {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct EagerEvaluationLogMessage;

#[cop(
    name = "Rails/EagerEvaluationLogMessage",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl EagerEvaluationLogMessage {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct EnumHash;

#[cop(
    name = "Rails/EnumHash",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl EnumHash {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct EnumSyntax;

#[cop(
    name = "Rails/EnumSyntax",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl EnumSyntax {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct EnumUniqueness;

#[cop(
    name = "Rails/EnumUniqueness",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl EnumUniqueness {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct Env;

#[cop(
    name = "Rails/Env",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl Env {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct EnvLocal;

#[cop(
    name = "Rails/EnvLocal",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl EnvLocal {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct EnvironmentComparison;

#[cop(
    name = "Rails/EnvironmentComparison",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl EnvironmentComparison {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

// `EnvironmentVariableAccess` promoted to real cop in
// `cops::rails::environment_variable_access`.

#[derive(Default)]
pub struct Exit;

#[cop(
    name = "Rails/Exit",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl Exit {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct ExpandedDateRange;

#[cop(
    name = "Rails/ExpandedDateRange",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl ExpandedDateRange {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct FilePath;

#[cop(
    name = "Rails/FilePath",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl FilePath {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct FindBy;

#[cop(
    name = "Rails/FindBy",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl FindBy {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct FindById;

#[cop(
    name = "Rails/FindById",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl FindById {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct FindByOrAssignmentMemoization;

#[cop(
    name = "Rails/FindByOrAssignmentMemoization",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl FindByOrAssignmentMemoization {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct FindEach;

#[cop(
    name = "Rails/FindEach",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl FindEach {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct FreezeTime;

#[cop(
    name = "Rails/FreezeTime",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl FreezeTime {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct HasAndBelongsToMany;

#[cop(
    name = "Rails/HasAndBelongsToMany",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl HasAndBelongsToMany {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct HasManyOrHasOneDependent;

#[cop(
    name = "Rails/HasManyOrHasOneDependent",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl HasManyOrHasOneDependent {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct HelperInstanceVariable;

#[cop(
    name = "Rails/HelperInstanceVariable",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl HelperInstanceVariable {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct HttpPositionalArguments;

#[cop(
    name = "Rails/HttpPositionalArguments",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl HttpPositionalArguments {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct HttpStatus;

#[cop(
    name = "Rails/HttpStatus",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl HttpStatus {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct HttpStatusNameConsistency;

#[cop(
    name = "Rails/HttpStatusNameConsistency",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl HttpStatusNameConsistency {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct I18nLazyLookup;

#[cop(
    name = "Rails/I18nLazyLookup",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl I18nLazyLookup {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

// `I18nLocaleAssignment` promoted to real cop in
// `cops::rails::i18n_locale_assignment`.

#[derive(Default)]
pub struct I18nLocaleTexts;

#[cop(
    name = "Rails/I18nLocaleTexts",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl I18nLocaleTexts {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct IgnoredColumnsAssignment;

#[cop(
    name = "Rails/IgnoredColumnsAssignment",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl IgnoredColumnsAssignment {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct IgnoredSkipActionFilterOption;

#[cop(
    name = "Rails/IgnoredSkipActionFilterOption",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl IgnoredSkipActionFilterOption {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct IndexBy;

#[cop(
    name = "Rails/IndexBy",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl IndexBy {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct IndexWith;

#[cop(
    name = "Rails/IndexWith",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl IndexWith {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct Inquiry;

#[cop(
    name = "Rails/Inquiry",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl Inquiry {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct InverseOf;

#[cop(
    name = "Rails/InverseOf",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl InverseOf {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct LexicallyScopedActionFilter;

#[cop(
    name = "Rails/LexicallyScopedActionFilter",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl LexicallyScopedActionFilter {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct LinkToBlank;

#[cop(
    name = "Rails/LinkToBlank",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl LinkToBlank {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct MailerName;

#[cop(
    name = "Rails/MailerName",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl MailerName {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct MatchRoute;

#[cop(
    name = "Rails/MatchRoute",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl MatchRoute {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct MigrationClassName;

#[cop(
    name = "Rails/MigrationClassName",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl MigrationClassName {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct MultipleRoutePaths;

#[cop(
    name = "Rails/MultipleRoutePaths",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl MultipleRoutePaths {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

// `NegateInclude` promoted to real cop in
// `cops::rails::negate_include`.

#[derive(Default)]
pub struct NotNullColumn;

#[cop(
    name = "Rails/NotNullColumn",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl NotNullColumn {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct OrderArguments;

#[cop(
    name = "Rails/OrderArguments",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl OrderArguments {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct OrderById;

#[cop(
    name = "Rails/OrderById",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl OrderById {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

// `Output` is now a real cop in `cops::rails::output` — `pub use`d at
// the crate root via the `use cops::rails::Output;` above so the
// `register_cops!` ident below resolves unchanged.

#[derive(Default)]
pub struct OutputSafety;

#[cop(
    name = "Rails/OutputSafety",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl OutputSafety {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

// `Pick` is now a real cop in `cops::rails::pick` — `pub use`d at the
// crate root via the `use cops::rails::{AssertNot, Output, Pick,
// RequestReferer};` above so the `register_cops!` ident below resolves
// unchanged.

#[derive(Default)]
pub struct Pluck;

#[cop(
    name = "Rails/Pluck",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl Pluck {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct PluckId;

#[cop(
    name = "Rails/PluckId",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl PluckId {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

#[derive(Default)]
pub struct PluckInWhere;

#[cop(
    name = "Rails/PluckInWhere",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl PluckInWhere {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

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

#[derive(Default)]
pub struct RefuteMethods;

#[cop(
    name = "Rails/RefuteMethods",
    description = "Rails cop pending arena migration (cf. murphy-au8). Stub registered for config compatibility.",
    default_enabled = false,
    options = NoOptions,
)]
impl RefuteMethods {
    #[on_new_investigation]
    fn investigate(&self, _cx: &Cx<'_>) {}
}

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

// `RequestReferer` is now a real cop in `cops::rails::request_referer`
// — `pub use`d at the crate root via the `use cops::rails::{Output,
// RequestReferer};` above so the `register_cops!` ident below resolves
// unchanged.

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

// `UniqBeforePluck` promoted to real cop in
// `cops::rails::uniq_before_pluck`.

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

register_cops!(
    mode = dynamic,
    ActionControllerFlashBeforeRender,
    ActionControllerTestCase,
    ActionFilter,
    ActionOrder,
    ActiveRecordAliases,
    ActiveRecordCallbacksOrder,
    ActiveRecordOverride,
    ActiveSupportAliases,
    ActiveSupportOnLoad,
    AddColumnIndex,
    AfterCommitOverride,
    ApplicationController,
    ApplicationJob,
    ApplicationMailer,
    ApplicationRecord,
    ArelStar,
    AssertNot,
    AttributeDefaultBlockValue,
    BelongsTo,
    Blank,
    BulkChangeTable,
    CompactBlank,
    ContentTag,
    CreateTableWithTimestamps,
    DangerousColumnNames,
    Date,
    DefaultScope,
    Delegate,
    DelegateAllowBlank,
    DeprecatedActiveModelErrorsMethods,
    DotSeparatedKeys,
    DuplicateAssociation,
    DuplicateScope,
    DurationArithmetic,
    DynamicFindBy,
    EagerEvaluationLogMessage,
    EnumHash,
    EnumSyntax,
    EnumUniqueness,
    Env,
    EnvLocal,
    EnvironmentComparison,
    EnvironmentVariableAccess,
    Exit,
    ExpandedDateRange,
    FilePath,
    FindBy,
    FindById,
    FindByOrAssignmentMemoization,
    FindEach,
    FreezeTime,
    HasAndBelongsToMany,
    HasManyOrHasOneDependent,
    HelperInstanceVariable,
    HttpPositionalArguments,
    HttpStatus,
    HttpStatusNameConsistency,
    I18nLazyLookup,
    I18nLocaleAssignment,
    I18nLocaleTexts,
    IgnoredColumnsAssignment,
    IgnoredSkipActionFilterOption,
    IndexBy,
    IndexWith,
    Inquiry,
    InverseOf,
    LexicallyScopedActionFilter,
    LinkToBlank,
    MailerName,
    MatchRoute,
    MigrationClassName,
    MultipleRoutePaths,
    NegateInclude,
    NotNullColumn,
    OrderArguments,
    OrderById,
    Output,
    OutputSafety,
    Pick,
    Pluck,
    PluckId,
    PluckInWhere,
    PluralizationGrammar,
    Presence,
    Present,
    RakeEnvironment,
    ReadWriteAttribute,
    RedirectBackOrTo,
    RedundantActiveRecordAllMethod,
    RedundantAllowNil,
    RedundantForeignKey,
    RedundantPresenceValidationOnBelongsTo,
    RedundantReceiverInWithOptions,
    RedundantTravelBack,
    ReflectionClassName,
    RefuteMethods,
    RelativeDateConstant,
    RenderInline,
    RenderPlainText,
    RequestReferer,
    RequireDependency,
    ResponseParsedBody,
    ReversibleMigration,
    ReversibleMigrationMethodDefinition,
    RootJoinChain,
    RootPathnameMethods,
    RootPublicPath,
    SafeNavigation,
    SafeNavigationWithBlank,
    SaveBang,
    SchemaComment,
    ScopeArgs,
    SelectMap,
    ShortI18n,
    SkipsModelValidations,
    SquishedSQLHeredocs,
    StripHeredoc,
    StrongParametersExpect,
    TableNameAssignment,
    ThreeStateBooleanColumn,
    TimeZone,
    TimeZoneAssignment,
    ToFormattedS,
    ToSWithArgument,
    TopLevelHashWithIndifferentAccess,
    TransactionExitStatement,
    UniqBeforePluck,
    UniqueValidationWithoutIndex,
    UnknownEnv,
    UnusedIgnoredColumns,
    UnusedRenderContent,
    Validation,
    WhereEquals,
    WhereExists,
    WhereMissing,
    WhereNot,
    WhereNotWithMultipleConditions,
    WhereRange,
);
