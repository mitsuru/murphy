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

// Cops in separate files register themselves via submit_cop! in their own modules.
pub mod cops;

// cop の登録は各 cop ファイル (inline stub / cops::rails::* 両方) の submit_cop!(T) が担う。
register_cops!(mode = dynamic);

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
/// upstream_cop: Rails/ActionControllerTestCase
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(ActionControllerTestCase);

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
/// upstream_cop: Rails/ActiveRecordAliases
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(ActiveRecordAliases);

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
/// upstream_cop: Rails/ApplicationController
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(ApplicationController);

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
/// upstream_cop: Rails/ApplicationRecord
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(ApplicationRecord);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ArelStar
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(ArelStar);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/AttributeDefaultBlockValue
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(AttributeDefaultBlockValue);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/BelongsTo
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(BelongsTo);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/Blank
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(Blank);

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

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/CompactBlank
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(CompactBlank);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ContentTag
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(ContentTag);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/CreateTableWithTimestamps
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(CreateTableWithTimestamps);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/DangerousColumnNames
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(DangerousColumnNames);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/Date
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(Date);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/DefaultScope
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(DefaultScope);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/Delegate
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(Delegate);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/DelegateAllowBlank
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(DelegateAllowBlank);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/DeprecatedActiveModelErrorsMethods
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(DeprecatedActiveModelErrorsMethods);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/DotSeparatedKeys
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(DotSeparatedKeys);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/DuplicateAssociation
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(DuplicateAssociation);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/DuplicateScope
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(DuplicateScope);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/DurationArithmetic
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(DurationArithmetic);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/DynamicFindBy
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(DynamicFindBy);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/EagerEvaluationLogMessage
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(EagerEvaluationLogMessage);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/EnumHash
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(EnumHash);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/EnumSyntax
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(EnumSyntax);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/EnumUniqueness
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(EnumUniqueness);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/Env
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(Env);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/EnvLocal
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(EnvLocal);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/EnvironmentComparison
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(EnvironmentComparison);

// `EnvironmentVariableAccess` promoted to real cop in
// `cops::rails::environment_variable_access`.

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/Exit
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(Exit);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/ExpandedDateRange
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(ExpandedDateRange);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/FilePath
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(FilePath);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/FindBy
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(FindBy);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/FindById
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(FindById);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/FindByOrAssignmentMemoization
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(FindByOrAssignmentMemoization);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/FindEach
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(FindEach);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/FreezeTime
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(FreezeTime);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/HasAndBelongsToMany
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(HasAndBelongsToMany);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/HasManyOrHasOneDependent
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(HasManyOrHasOneDependent);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/HelperInstanceVariable
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(HelperInstanceVariable);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/HttpPositionalArguments
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(HttpPositionalArguments);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/HttpStatus
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(HttpStatus);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/HttpStatusNameConsistency
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(HttpStatusNameConsistency);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/I18nLazyLookup
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(I18nLazyLookup);

// `I18nLocaleAssignment` promoted to real cop in
// `cops::rails::i18n_locale_assignment`.

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/I18nLocaleTexts
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(I18nLocaleTexts);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/IgnoredColumnsAssignment
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(IgnoredColumnsAssignment);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/IgnoredSkipActionFilterOption
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(IgnoredSkipActionFilterOption);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/IndexBy
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(IndexBy);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/IndexWith
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(IndexWith);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/Inquiry
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(Inquiry);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/InverseOf
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(InverseOf);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/LexicallyScopedActionFilter
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(LexicallyScopedActionFilter);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/LinkToBlank
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(LinkToBlank);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/MailerName
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(MailerName);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/MatchRoute
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(MatchRoute);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/MigrationClassName
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(MigrationClassName);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/MultipleRoutePaths
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(MultipleRoutePaths);

// `NegateInclude` promoted to real cop in
// `cops::rails::negate_include`.

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/NotNullColumn
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(NotNullColumn);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/OrderArguments
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(OrderArguments);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/OrderById
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(OrderById);

// `Output` is now a real cop in `cops::rails::output` — `pub use`d at
// the crate root via the `use cops::rails::Output;` above so the
// `register_cops!` ident below resolves unchanged.

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/OutputSafety
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(OutputSafety);

// `Pick` is now a real cop in `cops::rails::pick` — `pub use`d at the
// crate root via the `use cops::rails::{AssertNot, Output, Pick,
// RequestReferer};` above so the `register_cops!` ident below resolves
// unchanged.

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/Pluck
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(Pluck);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/PluckId
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(PluckId);

/// ## RuboCop parity
///
/// ```murphy-parity
/// upstream: rubocop-rails
/// upstream_cop: Rails/PluckInWhere
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(PluckInWhere);

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
/// upstream_cop: Rails/RefuteMethods
/// upstream_version_checked: 2.35.0
/// status: stub
/// gap_issues:
///   - murphy-4gd.1
/// notes: >
///   Arena-migration stub registered for config/listing compatibility; real implementation is pending.
/// ```
///
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
submit_cop!(RefuteMethods);

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


