//! murphy-rails — Rails-focused dynamic plugin pack (cdylib).
//!
//! 138 RuboCop-rails cops registered as **arena-migration stubs** via
//! `register_cops!(mode = dynamic, …)`. Each stub uses `KINDS = &[]`
//! (file-visit dispatch) with a no-op `check` and `DEFAULT_ENABLED =
//! Some(false)`, so the cop is inert at runtime but is enumerable by
//! `murphy cops list` and accepts `[cops.rules."Rails/..."]` config
//! sections without error (`§14a` of
//! `docs/plans/2026-05-22-plugin-reboot-design.md`).
//!
//! Individual cops are migrated to the real arena AST by `murphy-au8`
//! subtasks — for each migrated cop the corresponding
//! `rails_stub_cop!` invocation here is replaced by a full
//! `#[cop(...)]` implementation, and the cop name is removed from the
//! `is_cop_disabled_by_default` hardcode list in
//! `crates/murphy-core/src/config.rs` (cleanup tracked by
//! `murphy-bnd`).
//!
//! Cop names mirror RuboCop-rails 2.35.0 (rebuilt from the
//! pre-`murphy-9cr.22` rails crate; see `git show
//! 46a1de6^:crates/murphy-rails/src/cops/rails/`).

use murphy_plugin_api::{Cop, Cx, NoOptions, NodeCop, NodeId, NodeKindTag, register_cops};

/// Emit a stateless, no-op Rails cop stub. Each stub:
/// - implements `Cop` with `NAME = $name` and `DEFAULT_ENABLED = Some(false)`
/// - implements `NodeCop` with `KINDS = &[]` (file-visit dispatch) and an
///   empty `check` body
///
/// The `DESCRIPTION` carries the arena-migration context so `murphy cops
/// list --format=json` is self-explanatory even before `murphy-bnd`
/// normalises the `status` field.
macro_rules! rails_stub_cop {
    ($ident:ident, $name:literal) => {
        #[derive(Default)]
        pub struct $ident;
        impl Cop for $ident {
            type Options = NoOptions;
            const NAME: &'static str = $name;
            const DESCRIPTION: &'static str = "Rails cop pending arena migration (cf. murphy-au8). \
                 Stub registered for config compatibility.";
            const DEFAULT_ENABLED: Option<bool> = Some(false);
        }
        impl NodeCop for $ident {
            const KINDS: &'static [NodeKindTag] = &[];
            fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
        }
    };
}

rails_stub_cop!(
    ActionControllerFlashBeforeRender,
    "Rails/ActionControllerFlashBeforeRender"
);
rails_stub_cop!(ActionControllerTestCase, "Rails/ActionControllerTestCase");
rails_stub_cop!(ActionFilter, "Rails/ActionFilter");
rails_stub_cop!(ActionOrder, "Rails/ActionOrder");
rails_stub_cop!(ActiveRecordAliases, "Rails/ActiveRecordAliases");
rails_stub_cop!(
    ActiveRecordCallbacksOrder,
    "Rails/ActiveRecordCallbacksOrder"
);
rails_stub_cop!(ActiveRecordOverride, "Rails/ActiveRecordOverride");
rails_stub_cop!(ActiveSupportAliases, "Rails/ActiveSupportAliases");
rails_stub_cop!(ActiveSupportOnLoad, "Rails/ActiveSupportOnLoad");
rails_stub_cop!(AddColumnIndex, "Rails/AddColumnIndex");
rails_stub_cop!(AfterCommitOverride, "Rails/AfterCommitOverride");
rails_stub_cop!(ApplicationController, "Rails/ApplicationController");
rails_stub_cop!(ApplicationJob, "Rails/ApplicationJob");
rails_stub_cop!(ApplicationMailer, "Rails/ApplicationMailer");
rails_stub_cop!(ApplicationRecord, "Rails/ApplicationRecord");
rails_stub_cop!(ArelStar, "Rails/ArelStar");
rails_stub_cop!(AssertNot, "Rails/AssertNot");
rails_stub_cop!(
    AttributeDefaultBlockValue,
    "Rails/AttributeDefaultBlockValue"
);
rails_stub_cop!(BelongsTo, "Rails/BelongsTo");
rails_stub_cop!(Blank, "Rails/Blank");
rails_stub_cop!(BulkChangeTable, "Rails/BulkChangeTable");
rails_stub_cop!(CompactBlank, "Rails/CompactBlank");
rails_stub_cop!(ContentTag, "Rails/ContentTag");
rails_stub_cop!(CreateTableWithTimestamps, "Rails/CreateTableWithTimestamps");
rails_stub_cop!(DangerousColumnNames, "Rails/DangerousColumnNames");
rails_stub_cop!(Date, "Rails/Date");
rails_stub_cop!(DefaultScope, "Rails/DefaultScope");
rails_stub_cop!(Delegate, "Rails/Delegate");
rails_stub_cop!(DelegateAllowBlank, "Rails/DelegateAllowBlank");
rails_stub_cop!(
    DeprecatedActiveModelErrorsMethods,
    "Rails/DeprecatedActiveModelErrorsMethods"
);
rails_stub_cop!(DotSeparatedKeys, "Rails/DotSeparatedKeys");
rails_stub_cop!(DuplicateAssociation, "Rails/DuplicateAssociation");
rails_stub_cop!(DuplicateScope, "Rails/DuplicateScope");
rails_stub_cop!(DurationArithmetic, "Rails/DurationArithmetic");
rails_stub_cop!(DynamicFindBy, "Rails/DynamicFindBy");
rails_stub_cop!(EagerEvaluationLogMessage, "Rails/EagerEvaluationLogMessage");
rails_stub_cop!(EnumHash, "Rails/EnumHash");
rails_stub_cop!(EnumSyntax, "Rails/EnumSyntax");
rails_stub_cop!(EnumUniqueness, "Rails/EnumUniqueness");
rails_stub_cop!(Env, "Rails/Env");
rails_stub_cop!(EnvLocal, "Rails/EnvLocal");
rails_stub_cop!(EnvironmentComparison, "Rails/EnvironmentComparison");
rails_stub_cop!(EnvironmentVariableAccess, "Rails/EnvironmentVariableAccess");
rails_stub_cop!(Exit, "Rails/Exit");
rails_stub_cop!(ExpandedDateRange, "Rails/ExpandedDateRange");
rails_stub_cop!(FilePath, "Rails/FilePath");
rails_stub_cop!(FindBy, "Rails/FindBy");
rails_stub_cop!(FindById, "Rails/FindById");
rails_stub_cop!(
    FindByOrAssignmentMemoization,
    "Rails/FindByOrAssignmentMemoization"
);
rails_stub_cop!(FindEach, "Rails/FindEach");
rails_stub_cop!(FreezeTime, "Rails/FreezeTime");
rails_stub_cop!(HasAndBelongsToMany, "Rails/HasAndBelongsToMany");
rails_stub_cop!(HasManyOrHasOneDependent, "Rails/HasManyOrHasOneDependent");
rails_stub_cop!(HelperInstanceVariable, "Rails/HelperInstanceVariable");
rails_stub_cop!(HttpPositionalArguments, "Rails/HttpPositionalArguments");
rails_stub_cop!(HttpStatus, "Rails/HttpStatus");
rails_stub_cop!(HttpStatusNameConsistency, "Rails/HttpStatusNameConsistency");
rails_stub_cop!(I18nLazyLookup, "Rails/I18nLazyLookup");
rails_stub_cop!(I18nLocaleAssignment, "Rails/I18nLocaleAssignment");
rails_stub_cop!(I18nLocaleTexts, "Rails/I18nLocaleTexts");
rails_stub_cop!(IgnoredColumnsAssignment, "Rails/IgnoredColumnsAssignment");
rails_stub_cop!(
    IgnoredSkipActionFilterOption,
    "Rails/IgnoredSkipActionFilterOption"
);
rails_stub_cop!(IndexBy, "Rails/IndexBy");
rails_stub_cop!(IndexWith, "Rails/IndexWith");
rails_stub_cop!(Inquiry, "Rails/Inquiry");
rails_stub_cop!(InverseOf, "Rails/InverseOf");
rails_stub_cop!(
    LexicallyScopedActionFilter,
    "Rails/LexicallyScopedActionFilter"
);
rails_stub_cop!(LinkToBlank, "Rails/LinkToBlank");
rails_stub_cop!(MailerName, "Rails/MailerName");
rails_stub_cop!(MatchRoute, "Rails/MatchRoute");
rails_stub_cop!(MigrationClassName, "Rails/MigrationClassName");
rails_stub_cop!(MultipleRoutePaths, "Rails/MultipleRoutePaths");
rails_stub_cop!(NegateInclude, "Rails/NegateInclude");
rails_stub_cop!(NotNullColumn, "Rails/NotNullColumn");
rails_stub_cop!(OrderArguments, "Rails/OrderArguments");
rails_stub_cop!(OrderById, "Rails/OrderById");
rails_stub_cop!(Output, "Rails/Output");
rails_stub_cop!(OutputSafety, "Rails/OutputSafety");
rails_stub_cop!(Pick, "Rails/Pick");
rails_stub_cop!(Pluck, "Rails/Pluck");
rails_stub_cop!(PluckId, "Rails/PluckId");
rails_stub_cop!(PluckInWhere, "Rails/PluckInWhere");
rails_stub_cop!(PluralizationGrammar, "Rails/PluralizationGrammar");
rails_stub_cop!(Presence, "Rails/Presence");
rails_stub_cop!(Present, "Rails/Present");
rails_stub_cop!(RakeEnvironment, "Rails/RakeEnvironment");
rails_stub_cop!(ReadWriteAttribute, "Rails/ReadWriteAttribute");
rails_stub_cop!(RedirectBackOrTo, "Rails/RedirectBackOrTo");
rails_stub_cop!(
    RedundantActiveRecordAllMethod,
    "Rails/RedundantActiveRecordAllMethod"
);
rails_stub_cop!(RedundantAllowNil, "Rails/RedundantAllowNil");
rails_stub_cop!(RedundantForeignKey, "Rails/RedundantForeignKey");
rails_stub_cop!(
    RedundantPresenceValidationOnBelongsTo,
    "Rails/RedundantPresenceValidationOnBelongsTo"
);
rails_stub_cop!(
    RedundantReceiverInWithOptions,
    "Rails/RedundantReceiverInWithOptions"
);
rails_stub_cop!(RedundantTravelBack, "Rails/RedundantTravelBack");
rails_stub_cop!(ReflectionClassName, "Rails/ReflectionClassName");
rails_stub_cop!(RefuteMethods, "Rails/RefuteMethods");
rails_stub_cop!(RelativeDateConstant, "Rails/RelativeDateConstant");
rails_stub_cop!(RenderInline, "Rails/RenderInline");
rails_stub_cop!(RenderPlainText, "Rails/RenderPlainText");
rails_stub_cop!(RequestReferer, "Rails/RequestReferer");
rails_stub_cop!(RequireDependency, "Rails/RequireDependency");
rails_stub_cop!(ResponseParsedBody, "Rails/ResponseParsedBody");
rails_stub_cop!(ReversibleMigration, "Rails/ReversibleMigration");
rails_stub_cop!(
    ReversibleMigrationMethodDefinition,
    "Rails/ReversibleMigrationMethodDefinition"
);
rails_stub_cop!(RootJoinChain, "Rails/RootJoinChain");
rails_stub_cop!(RootPathnameMethods, "Rails/RootPathnameMethods");
rails_stub_cop!(RootPublicPath, "Rails/RootPublicPath");
rails_stub_cop!(SafeNavigation, "Rails/SafeNavigation");
rails_stub_cop!(SafeNavigationWithBlank, "Rails/SafeNavigationWithBlank");
rails_stub_cop!(SaveBang, "Rails/SaveBang");
rails_stub_cop!(SchemaComment, "Rails/SchemaComment");
rails_stub_cop!(ScopeArgs, "Rails/ScopeArgs");
rails_stub_cop!(SelectMap, "Rails/SelectMap");
rails_stub_cop!(ShortI18n, "Rails/ShortI18n");
rails_stub_cop!(SkipsModelValidations, "Rails/SkipsModelValidations");
rails_stub_cop!(SquishedSQLHeredocs, "Rails/SquishedSQLHeredocs");
rails_stub_cop!(StripHeredoc, "Rails/StripHeredoc");
rails_stub_cop!(StrongParametersExpect, "Rails/StrongParametersExpect");
rails_stub_cop!(TableNameAssignment, "Rails/TableNameAssignment");
rails_stub_cop!(ThreeStateBooleanColumn, "Rails/ThreeStateBooleanColumn");
rails_stub_cop!(TimeZone, "Rails/TimeZone");
rails_stub_cop!(TimeZoneAssignment, "Rails/TimeZoneAssignment");
rails_stub_cop!(ToFormattedS, "Rails/ToFormattedS");
rails_stub_cop!(ToSWithArgument, "Rails/ToSWithArgument");
rails_stub_cop!(
    TopLevelHashWithIndifferentAccess,
    "Rails/TopLevelHashWithIndifferentAccess"
);
rails_stub_cop!(TransactionExitStatement, "Rails/TransactionExitStatement");
rails_stub_cop!(UniqBeforePluck, "Rails/UniqBeforePluck");
rails_stub_cop!(
    UniqueValidationWithoutIndex,
    "Rails/UniqueValidationWithoutIndex"
);
rails_stub_cop!(UnknownEnv, "Rails/UnknownEnv");
rails_stub_cop!(UnusedIgnoredColumns, "Rails/UnusedIgnoredColumns");
rails_stub_cop!(UnusedRenderContent, "Rails/UnusedRenderContent");
rails_stub_cop!(Validation, "Rails/Validation");
rails_stub_cop!(WhereEquals, "Rails/WhereEquals");
rails_stub_cop!(WhereExists, "Rails/WhereExists");
rails_stub_cop!(WhereMissing, "Rails/WhereMissing");
rails_stub_cop!(WhereNot, "Rails/WhereNot");
rails_stub_cop!(
    WhereNotWithMultipleConditions,
    "Rails/WhereNotWithMultipleConditions"
);
rails_stub_cop!(WhereRange, "Rails/WhereRange");

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
