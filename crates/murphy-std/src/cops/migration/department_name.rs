//! `Migration/DepartmentName` — check that cop names in `rubocop:disable`,
//! `rubocop:enable`, and `rubocop:todo` directive comments are given with their
//! department name (e.g. `Layout/LineLength`, not a bare `LineLength`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Migration/DepartmentName
//! upstream_version_checked: 1.87.0
//! version_added: "0.75"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Hand-rolled port of RuboCop's `DISABLE_COMMENT_FORMAT` regex
//!   (`/\A(# *rubocop *: *((dis|en)able|todo) +)(.*)/`) — no `regex` dependency
//!   in murphy-std. The cop-name list is tokenised by `scan_tokens` into
//!   maximal comma runs and maximal non-comma runs (a faithful-enough analogue
//!   of RuboCop's `cop_names.scan(/[^,]+|\W+/)` for offense detection; it
//!   differs only in that `", "` becomes `","` + `" Bar"` instead of one
//!   `", "` separator, which is why the offense-range calc below skips a token's
//!   leading whitespace). Detection covers the three directive modes
//!   (`disable`/`enable`/`todo`; `push`/`pop` are deliberately out of scope,
//!   matching RuboCop's regex), the comma-separated token list, the `break` on
//!   the first token containing a character outside `[A-Za-z/, ]` (which
//!   terminates the scan at a trailing `-- comment`), and the three "valid
//!   token" branches: a token containing any non-word char (`/\W+/` partial
//!   match — e.g. has a slash, space, or dash), the `[A-Za-z]+/[A-Za-z]+|all`
//!   partial match, and a registered department. Offense range = the trimmed
//!   bare cop name (leading whitespace skipped), byte-precise per RuboCop's
//!   `range_between(begin_pos + offset, + name.length)`.
//!
//!   Autocorrect: prepends the department via the static `BARE_TO_QUALIFIED`
//!   table (e.g. `AbcSize` -> `Metrics/AbcSize`), mirroring RuboCop's
//!   `Registry.global.qualified_cop_name` with `warn: false`. Unknown bare
//!   names (e.g. `Foo`) and ambiguous ones (`MultipleComparison`,
//!   `SelfAssignment`, which map to two departments) report an offense with
//!   no correction, matching RuboCop's non-correctable / error cases without
//!   crashing. Legacy bare names (e.g. `AlignArguments`) fall back to the
//!   `LEGACY_COP_NAMES` table, mirroring `qualified_legacy_cop_name` (which
//!   returns the old qualified name, e.g. `Layout/AlignArguments`).
//!
//!   GAP — department set: RuboCop's `department?` consults the live registry,
//!   so with rubocop-rails / rubocop-rspec loaded, bare `Rails` / `RSpec` are
//!   accepted as departments. murphy-std ships only the core department set, so
//!   a bare `# rubocop:disable RSpec` would flag here. Acceptable documented
//!   divergence (murphy-std cannot see plugin departments), not a tracked gap.
//! ```

use murphy_plugin_api::{Cx, NoOptions, Range, cop};

const MSG: &str = "Department name is missing.";

/// RuboCop's core department set (`Registry.global.departments` with no plugins
/// loaded, verified against rubocop 1.87.0). A bare token matching one of these
/// is a department reference, not a department-less cop name, so it is accepted.
const CORE_DEPARTMENTS: &[&str] = &[
    "Bundler", "Gemspec", "Layout", "Lint", "Metrics", "Migration", "Naming", "Security", "Style",
];

/// Bare cop-name -> qualified cop-name table for RuboCop core 1.87.0
/// (`Registry.global.cops`, 606 entries). Mirrors
/// `Registry.global.qualified_cop_name(name, nil, warn: false)` for the
/// single-match case: a bare name mapping to exactly one department resolves
/// to that qualified name. Bare names with zero matches fall through to
/// [`qualified_legacy_cop_name`]; bare names with two matches (currently
/// `MultipleComparison` and `SelfAssignment`) are ambiguous — RuboCop raises
/// `AmbiguousCopName`, so we report an offense with no correction instead of
/// crashing.
const BARE_TO_QUALIFIED: &[(&str, &str)] = &[
    ("DuplicatedGem", "Bundler/DuplicatedGem"),
    ("DuplicatedGroup", "Bundler/DuplicatedGroup"),
    ("GemComment", "Bundler/GemComment"),
    ("GemFilename", "Bundler/GemFilename"),
    ("GemVersion", "Bundler/GemVersion"),
    ("InsecureProtocolSource", "Bundler/InsecureProtocolSource"),
    ("OrderedGems", "Bundler/OrderedGems"),
    ("AddRuntimeDependency", "Gemspec/AddRuntimeDependency"),
    ("AttributeAssignment", "Gemspec/AttributeAssignment"),
    ("DependencyVersion", "Gemspec/DependencyVersion"),
    ("DeprecatedAttributeAssignment", "Gemspec/DeprecatedAttributeAssignment"),
    ("DevelopmentDependencies", "Gemspec/DevelopmentDependencies"),
    ("DuplicatedAssignment", "Gemspec/DuplicatedAssignment"),
    ("OrderedDependencies", "Gemspec/OrderedDependencies"),
    ("RequireMFA", "Gemspec/RequireMFA"),
    ("RequiredRubyVersion", "Gemspec/RequiredRubyVersion"),
    ("RubyVersionGlobalsUsage", "Gemspec/RubyVersionGlobalsUsage"),
    ("AccessModifierIndentation", "Layout/AccessModifierIndentation"),
    ("ArgumentAlignment", "Layout/ArgumentAlignment"),
    ("ArrayAlignment", "Layout/ArrayAlignment"),
    ("AssignmentIndentation", "Layout/AssignmentIndentation"),
    ("BeginEndAlignment", "Layout/BeginEndAlignment"),
    ("BlockAlignment", "Layout/BlockAlignment"),
    ("BlockEndNewline", "Layout/BlockEndNewline"),
    ("CaseIndentation", "Layout/CaseIndentation"),
    ("ClassStructure", "Layout/ClassStructure"),
    ("ClosingHeredocIndentation", "Layout/ClosingHeredocIndentation"),
    ("ClosingParenthesisIndentation", "Layout/ClosingParenthesisIndentation"),
    ("CommentIndentation", "Layout/CommentIndentation"),
    ("ConditionPosition", "Layout/ConditionPosition"),
    ("DefEndAlignment", "Layout/DefEndAlignment"),
    ("DotPosition", "Layout/DotPosition"),
    ("ElseAlignment", "Layout/ElseAlignment"),
    ("EmptyComment", "Layout/EmptyComment"),
    ("EmptyLineAfterGuardClause", "Layout/EmptyLineAfterGuardClause"),
    ("EmptyLineAfterMagicComment", "Layout/EmptyLineAfterMagicComment"),
    ("EmptyLineAfterMultilineCondition", "Layout/EmptyLineAfterMultilineCondition"),
    ("EmptyLineBetweenDefs", "Layout/EmptyLineBetweenDefs"),
    ("EmptyLines", "Layout/EmptyLines"),
    ("EmptyLinesAfterModuleInclusion", "Layout/EmptyLinesAfterModuleInclusion"),
    ("EmptyLinesAroundAccessModifier", "Layout/EmptyLinesAroundAccessModifier"),
    ("EmptyLinesAroundArguments", "Layout/EmptyLinesAroundArguments"),
    ("EmptyLinesAroundAttributeAccessor", "Layout/EmptyLinesAroundAttributeAccessor"),
    ("EmptyLinesAroundBeginBody", "Layout/EmptyLinesAroundBeginBody"),
    ("EmptyLinesAroundBlockBody", "Layout/EmptyLinesAroundBlockBody"),
    ("EmptyLinesAroundClassBody", "Layout/EmptyLinesAroundClassBody"),
    ("EmptyLinesAroundExceptionHandlingKeywords", "Layout/EmptyLinesAroundExceptionHandlingKeywords"),
    ("EmptyLinesAroundMethodBody", "Layout/EmptyLinesAroundMethodBody"),
    ("EmptyLinesAroundModuleBody", "Layout/EmptyLinesAroundModuleBody"),
    ("EndAlignment", "Layout/EndAlignment"),
    ("EndOfLine", "Layout/EndOfLine"),
    ("ExtraSpacing", "Layout/ExtraSpacing"),
    ("FirstArgumentIndentation", "Layout/FirstArgumentIndentation"),
    ("FirstArrayElementIndentation", "Layout/FirstArrayElementIndentation"),
    ("FirstArrayElementLineBreak", "Layout/FirstArrayElementLineBreak"),
    ("FirstHashElementIndentation", "Layout/FirstHashElementIndentation"),
    ("FirstHashElementLineBreak", "Layout/FirstHashElementLineBreak"),
    ("FirstMethodArgumentLineBreak", "Layout/FirstMethodArgumentLineBreak"),
    ("FirstMethodParameterLineBreak", "Layout/FirstMethodParameterLineBreak"),
    ("FirstParameterIndentation", "Layout/FirstParameterIndentation"),
    ("HashAlignment", "Layout/HashAlignment"),
    ("HeredocArgumentClosingParenthesis", "Layout/HeredocArgumentClosingParenthesis"),
    ("HeredocIndentation", "Layout/HeredocIndentation"),
    ("IndentationConsistency", "Layout/IndentationConsistency"),
    ("IndentationStyle", "Layout/IndentationStyle"),
    ("IndentationWidth", "Layout/IndentationWidth"),
    ("InitialIndentation", "Layout/InitialIndentation"),
    ("LeadingCommentSpace", "Layout/LeadingCommentSpace"),
    ("LeadingEmptyLines", "Layout/LeadingEmptyLines"),
    ("LineContinuationLeadingSpace", "Layout/LineContinuationLeadingSpace"),
    ("LineContinuationSpacing", "Layout/LineContinuationSpacing"),
    ("LineEndStringConcatenationIndentation", "Layout/LineEndStringConcatenationIndentation"),
    ("LineLength", "Layout/LineLength"),
    ("MultilineArrayBraceLayout", "Layout/MultilineArrayBraceLayout"),
    ("MultilineArrayLineBreaks", "Layout/MultilineArrayLineBreaks"),
    ("MultilineAssignmentLayout", "Layout/MultilineAssignmentLayout"),
    ("MultilineBlockLayout", "Layout/MultilineBlockLayout"),
    ("MultilineHashBraceLayout", "Layout/MultilineHashBraceLayout"),
    ("MultilineHashKeyLineBreaks", "Layout/MultilineHashKeyLineBreaks"),
    ("MultilineMethodArgumentLineBreaks", "Layout/MultilineMethodArgumentLineBreaks"),
    ("MultilineMethodCallBraceLayout", "Layout/MultilineMethodCallBraceLayout"),
    ("MultilineMethodCallIndentation", "Layout/MultilineMethodCallIndentation"),
    ("MultilineMethodDefinitionBraceLayout", "Layout/MultilineMethodDefinitionBraceLayout"),
    ("MultilineMethodParameterLineBreaks", "Layout/MultilineMethodParameterLineBreaks"),
    ("MultilineOperationIndentation", "Layout/MultilineOperationIndentation"),
    ("ParameterAlignment", "Layout/ParameterAlignment"),
    ("RedundantLineBreak", "Layout/RedundantLineBreak"),
    ("RescueEnsureAlignment", "Layout/RescueEnsureAlignment"),
    ("SingleLineBlockChain", "Layout/SingleLineBlockChain"),
    ("SpaceAfterColon", "Layout/SpaceAfterColon"),
    ("SpaceAfterComma", "Layout/SpaceAfterComma"),
    ("SpaceAfterMethodName", "Layout/SpaceAfterMethodName"),
    ("SpaceAfterNot", "Layout/SpaceAfterNot"),
    ("SpaceAfterSemicolon", "Layout/SpaceAfterSemicolon"),
    ("SpaceAroundBlockParameters", "Layout/SpaceAroundBlockParameters"),
    ("SpaceAroundEqualsInParameterDefault", "Layout/SpaceAroundEqualsInParameterDefault"),
    ("SpaceAroundKeyword", "Layout/SpaceAroundKeyword"),
    ("SpaceAroundMethodCallOperator", "Layout/SpaceAroundMethodCallOperator"),
    ("SpaceAroundOperators", "Layout/SpaceAroundOperators"),
    ("SpaceBeforeBlockBraces", "Layout/SpaceBeforeBlockBraces"),
    ("SpaceBeforeBrackets", "Layout/SpaceBeforeBrackets"),
    ("SpaceBeforeComma", "Layout/SpaceBeforeComma"),
    ("SpaceBeforeComment", "Layout/SpaceBeforeComment"),
    ("SpaceBeforeFirstArg", "Layout/SpaceBeforeFirstArg"),
    ("SpaceBeforeSemicolon", "Layout/SpaceBeforeSemicolon"),
    ("SpaceInLambdaLiteral", "Layout/SpaceInLambdaLiteral"),
    ("SpaceInsideArrayLiteralBrackets", "Layout/SpaceInsideArrayLiteralBrackets"),
    ("SpaceInsideArrayPercentLiteral", "Layout/SpaceInsideArrayPercentLiteral"),
    ("SpaceInsideBlockBraces", "Layout/SpaceInsideBlockBraces"),
    ("SpaceInsideHashLiteralBraces", "Layout/SpaceInsideHashLiteralBraces"),
    ("SpaceInsideParens", "Layout/SpaceInsideParens"),
    ("SpaceInsidePercentLiteralDelimiters", "Layout/SpaceInsidePercentLiteralDelimiters"),
    ("SpaceInsideRangeLiteral", "Layout/SpaceInsideRangeLiteral"),
    ("SpaceInsideReferenceBrackets", "Layout/SpaceInsideReferenceBrackets"),
    ("SpaceInsideStringInterpolation", "Layout/SpaceInsideStringInterpolation"),
    ("TrailingEmptyLines", "Layout/TrailingEmptyLines"),
    ("TrailingWhitespace", "Layout/TrailingWhitespace"),
    ("AmbiguousAssignment", "Lint/AmbiguousAssignment"),
    ("AmbiguousBlockAssociation", "Lint/AmbiguousBlockAssociation"),
    ("AmbiguousOperator", "Lint/AmbiguousOperator"),
    ("AmbiguousOperatorPrecedence", "Lint/AmbiguousOperatorPrecedence"),
    ("AmbiguousRange", "Lint/AmbiguousRange"),
    ("AmbiguousRegexpLiteral", "Lint/AmbiguousRegexpLiteral"),
    ("ArrayLiteralInRegexp", "Lint/ArrayLiteralInRegexp"),
    ("AssignmentInCondition", "Lint/AssignmentInCondition"),
    ("BigDecimalNew", "Lint/BigDecimalNew"),
    ("BinaryOperatorWithIdenticalOperands", "Lint/BinaryOperatorWithIdenticalOperands"),
    ("BooleanSymbol", "Lint/BooleanSymbol"),
    ("CircularArgumentReference", "Lint/CircularArgumentReference"),
    ("ConstantDefinitionInBlock", "Lint/ConstantDefinitionInBlock"),
    ("ConstantOverwrittenInRescue", "Lint/ConstantOverwrittenInRescue"),
    ("ConstantReassignment", "Lint/ConstantReassignment"),
    ("ConstantResolution", "Lint/ConstantResolution"),
    ("CopDirectiveSyntax", "Lint/CopDirectiveSyntax"),
    ("DataDefineOverride", "Lint/DataDefineOverride"),
    ("Debugger", "Lint/Debugger"),
    ("DeprecatedClassMethods", "Lint/DeprecatedClassMethods"),
    ("DeprecatedConstants", "Lint/DeprecatedConstants"),
    ("DeprecatedOpenSSLConstant", "Lint/DeprecatedOpenSSLConstant"),
    ("DisjunctiveAssignmentInConstructor", "Lint/DisjunctiveAssignmentInConstructor"),
    ("DuplicateBranch", "Lint/DuplicateBranch"),
    ("DuplicateCaseCondition", "Lint/DuplicateCaseCondition"),
    ("DuplicateElsifCondition", "Lint/DuplicateElsifCondition"),
    ("DuplicateHashKey", "Lint/DuplicateHashKey"),
    ("DuplicateMagicComment", "Lint/DuplicateMagicComment"),
    ("DuplicateMatchPattern", "Lint/DuplicateMatchPattern"),
    ("DuplicateMethods", "Lint/DuplicateMethods"),
    ("DuplicateRegexpCharacterClassElement", "Lint/DuplicateRegexpCharacterClassElement"),
    ("DuplicateRequire", "Lint/DuplicateRequire"),
    ("DuplicateRescueException", "Lint/DuplicateRescueException"),
    ("DuplicateSetElement", "Lint/DuplicateSetElement"),
    ("EachWithObjectArgument", "Lint/EachWithObjectArgument"),
    ("ElseLayout", "Lint/ElseLayout"),
    ("EmptyBlock", "Lint/EmptyBlock"),
    ("EmptyClass", "Lint/EmptyClass"),
    ("EmptyConditionalBody", "Lint/EmptyConditionalBody"),
    ("EmptyEnsure", "Lint/EmptyEnsure"),
    ("EmptyExpression", "Lint/EmptyExpression"),
    ("EmptyFile", "Lint/EmptyFile"),
    ("EmptyInPattern", "Lint/EmptyInPattern"),
    ("EmptyInterpolation", "Lint/EmptyInterpolation"),
    ("EmptyWhen", "Lint/EmptyWhen"),
    ("EnsureReturn", "Lint/EnsureReturn"),
    ("ErbNewArguments", "Lint/ErbNewArguments"),
    ("FlipFlop", "Lint/FlipFlop"),
    ("FloatComparison", "Lint/FloatComparison"),
    ("FloatOutOfRange", "Lint/FloatOutOfRange"),
    ("FormatParameterMismatch", "Lint/FormatParameterMismatch"),
    ("HashCompareByIdentity", "Lint/HashCompareByIdentity"),
    ("HashNewWithKeywordArgumentsAsDefault", "Lint/HashNewWithKeywordArgumentsAsDefault"),
    ("HeredocMethodCallPosition", "Lint/HeredocMethodCallPosition"),
    ("IdentityComparison", "Lint/IdentityComparison"),
    ("ImplicitStringConcatenation", "Lint/ImplicitStringConcatenation"),
    ("IncompatibleIoSelectWithFiberScheduler", "Lint/IncompatibleIoSelectWithFiberScheduler"),
    ("IneffectiveAccessModifier", "Lint/IneffectiveAccessModifier"),
    ("InheritException", "Lint/InheritException"),
    ("InterpolationCheck", "Lint/InterpolationCheck"),
    ("ItWithoutArgumentsInBlock", "Lint/ItWithoutArgumentsInBlock"),
    ("LambdaWithoutLiteralBlock", "Lint/LambdaWithoutLiteralBlock"),
    ("LiteralAsCondition", "Lint/LiteralAsCondition"),
    ("LiteralAssignmentInCondition", "Lint/LiteralAssignmentInCondition"),
    ("LiteralInInterpolation", "Lint/LiteralInInterpolation"),
    ("Loop", "Lint/Loop"),
    ("MissingCopEnableDirective", "Lint/MissingCopEnableDirective"),
    ("MissingSuper", "Lint/MissingSuper"),
    ("MixedCaseRange", "Lint/MixedCaseRange"),
    ("MixedRegexpCaptureTypes", "Lint/MixedRegexpCaptureTypes"),
    ("MultipleComparison", "Lint/MultipleComparison"),
    ("NestedMethodDefinition", "Lint/NestedMethodDefinition"),
    ("NestedPercentLiteral", "Lint/NestedPercentLiteral"),
    ("NextWithoutAccumulator", "Lint/NextWithoutAccumulator"),
    ("NoReturnInBeginEndBlocks", "Lint/NoReturnInBeginEndBlocks"),
    ("NonAtomicFileOperation", "Lint/NonAtomicFileOperation"),
    ("NonDeterministicRequireOrder", "Lint/NonDeterministicRequireOrder"),
    ("NonLocalExitFromIterator", "Lint/NonLocalExitFromIterator"),
    ("NumberConversion", "Lint/NumberConversion"),
    ("NumberedParameterAssignment", "Lint/NumberedParameterAssignment"),
    ("NumericOperationWithConstantResult", "Lint/NumericOperationWithConstantResult"),
    ("OrAssignmentToConstant", "Lint/OrAssignmentToConstant"),
    ("OrderedMagicComments", "Lint/OrderedMagicComments"),
    ("OutOfRangeRegexpRef", "Lint/OutOfRangeRegexpRef"),
    ("ParenthesesAsGroupedExpression", "Lint/ParenthesesAsGroupedExpression"),
    ("PercentStringArray", "Lint/PercentStringArray"),
    ("PercentSymbolArray", "Lint/PercentSymbolArray"),
    ("RaiseException", "Lint/RaiseException"),
    ("RandOne", "Lint/RandOne"),
    ("RedundantCopDisableDirective", "Lint/RedundantCopDisableDirective"),
    ("RedundantCopEnableDirective", "Lint/RedundantCopEnableDirective"),
    ("RedundantDirGlobSort", "Lint/RedundantDirGlobSort"),
    ("RedundantRegexpQuantifiers", "Lint/RedundantRegexpQuantifiers"),
    ("RedundantRequireStatement", "Lint/RedundantRequireStatement"),
    ("RedundantSafeNavigation", "Lint/RedundantSafeNavigation"),
    ("RedundantSplatExpansion", "Lint/RedundantSplatExpansion"),
    ("RedundantStringCoercion", "Lint/RedundantStringCoercion"),
    ("RedundantTypeConversion", "Lint/RedundantTypeConversion"),
    ("RedundantWithIndex", "Lint/RedundantWithIndex"),
    ("RedundantWithObject", "Lint/RedundantWithObject"),
    ("RefinementImportMethods", "Lint/RefinementImportMethods"),
    ("RegexpAsCondition", "Lint/RegexpAsCondition"),
    ("RequireParentheses", "Lint/RequireParentheses"),
    ("RequireRangeParentheses", "Lint/RequireRangeParentheses"),
    ("RequireRelativeSelfPath", "Lint/RequireRelativeSelfPath"),
    ("RescueException", "Lint/RescueException"),
    ("RescueType", "Lint/RescueType"),
    ("ReturnInVoidContext", "Lint/ReturnInVoidContext"),
    ("SafeNavigationChain", "Lint/SafeNavigationChain"),
    ("SafeNavigationConsistency", "Lint/SafeNavigationConsistency"),
    ("SafeNavigationWithEmpty", "Lint/SafeNavigationWithEmpty"),
    ("ScriptPermission", "Lint/ScriptPermission"),
    ("SelfAssignment", "Lint/SelfAssignment"),
    ("SendWithMixinArgument", "Lint/SendWithMixinArgument"),
    ("ShadowedArgument", "Lint/ShadowedArgument"),
    ("ShadowedException", "Lint/ShadowedException"),
    ("ShadowingOuterLocalVariable", "Lint/ShadowingOuterLocalVariable"),
    ("SharedMutableDefault", "Lint/SharedMutableDefault"),
    ("StructNewOverride", "Lint/StructNewOverride"),
    ("SuppressedException", "Lint/SuppressedException"),
    ("SuppressedExceptionInNumberConversion", "Lint/SuppressedExceptionInNumberConversion"),
    ("SymbolConversion", "Lint/SymbolConversion"),
    ("Syntax", "Lint/Syntax"),
    ("ToEnumArguments", "Lint/ToEnumArguments"),
    ("ToJSON", "Lint/ToJSON"),
    ("TopLevelReturnWithArgument", "Lint/TopLevelReturnWithArgument"),
    ("TrailingCommaInAttributeDeclaration", "Lint/TrailingCommaInAttributeDeclaration"),
    ("TripleQuotes", "Lint/TripleQuotes"),
    ("UnderscorePrefixedVariableName", "Lint/UnderscorePrefixedVariableName"),
    ("UnescapedBracketInRegexp", "Lint/UnescapedBracketInRegexp"),
    ("UnexpectedBlockArity", "Lint/UnexpectedBlockArity"),
    ("UnifiedInteger", "Lint/UnifiedInteger"),
    ("UnmodifiedReduceAccumulator", "Lint/UnmodifiedReduceAccumulator"),
    ("UnreachableCode", "Lint/UnreachableCode"),
    ("UnreachableLoop", "Lint/UnreachableLoop"),
    ("UnreachablePatternBranch", "Lint/UnreachablePatternBranch"),
    ("UnusedBlockArgument", "Lint/UnusedBlockArgument"),
    ("UnusedMethodArgument", "Lint/UnusedMethodArgument"),
    ("UriEscapeUnescape", "Lint/UriEscapeUnescape"),
    ("UriRegexp", "Lint/UriRegexp"),
    ("UselessAccessModifier", "Lint/UselessAccessModifier"),
    ("UselessAssignment", "Lint/UselessAssignment"),
    ("UselessConstantScoping", "Lint/UselessConstantScoping"),
    ("UselessDefaultValueArgument", "Lint/UselessDefaultValueArgument"),
    ("UselessDefined", "Lint/UselessDefined"),
    ("UselessElseWithoutRescue", "Lint/UselessElseWithoutRescue"),
    ("UselessMethodDefinition", "Lint/UselessMethodDefinition"),
    ("UselessNumericOperation", "Lint/UselessNumericOperation"),
    ("UselessOr", "Lint/UselessOr"),
    ("UselessRescue", "Lint/UselessRescue"),
    ("UselessRuby2Keywords", "Lint/UselessRuby2Keywords"),
    ("UselessSetterCall", "Lint/UselessSetterCall"),
    ("UselessTimes", "Lint/UselessTimes"),
    ("Void", "Lint/Void"),
    ("AbcSize", "Metrics/AbcSize"),
    ("BlockLength", "Metrics/BlockLength"),
    ("BlockNesting", "Metrics/BlockNesting"),
    ("ClassLength", "Metrics/ClassLength"),
    ("CollectionLiteralLength", "Metrics/CollectionLiteralLength"),
    ("CyclomaticComplexity", "Metrics/CyclomaticComplexity"),
    ("MethodLength", "Metrics/MethodLength"),
    ("ModuleLength", "Metrics/ModuleLength"),
    ("ParameterLists", "Metrics/ParameterLists"),
    ("PerceivedComplexity", "Metrics/PerceivedComplexity"),
    ("DepartmentName", "Migration/DepartmentName"),
    ("AccessorMethodName", "Naming/AccessorMethodName"),
    ("AsciiIdentifiers", "Naming/AsciiIdentifiers"),
    ("BinaryOperatorParameterName", "Naming/BinaryOperatorParameterName"),
    ("BlockForwarding", "Naming/BlockForwarding"),
    ("BlockParameterName", "Naming/BlockParameterName"),
    ("ClassAndModuleCamelCase", "Naming/ClassAndModuleCamelCase"),
    ("ConstantName", "Naming/ConstantName"),
    ("FileName", "Naming/FileName"),
    ("HeredocDelimiterCase", "Naming/HeredocDelimiterCase"),
    ("HeredocDelimiterNaming", "Naming/HeredocDelimiterNaming"),
    ("InclusiveLanguage", "Naming/InclusiveLanguage"),
    ("MemoizedInstanceVariableName", "Naming/MemoizedInstanceVariableName"),
    ("MethodName", "Naming/MethodName"),
    ("MethodParameterName", "Naming/MethodParameterName"),
    ("PredicateMethod", "Naming/PredicateMethod"),
    ("PredicatePrefix", "Naming/PredicatePrefix"),
    ("RescuedExceptionsVariableName", "Naming/RescuedExceptionsVariableName"),
    ("VariableName", "Naming/VariableName"),
    ("VariableNumber", "Naming/VariableNumber"),
    ("CompoundHash", "Security/CompoundHash"),
    ("Eval", "Security/Eval"),
    ("IoMethods", "Security/IoMethods"),
    ("JSONLoad", "Security/JSONLoad"),
    ("MarshalLoad", "Security/MarshalLoad"),
    ("Open", "Security/Open"),
    ("YAMLLoad", "Security/YAMLLoad"),
    ("AccessModifierDeclarations", "Style/AccessModifierDeclarations"),
    ("AccessorGrouping", "Style/AccessorGrouping"),
    ("Alias", "Style/Alias"),
    ("AmbiguousEndlessMethodDefinition", "Style/AmbiguousEndlessMethodDefinition"),
    ("AndOr", "Style/AndOr"),
    ("ArgumentsForwarding", "Style/ArgumentsForwarding"),
    ("ArrayCoercion", "Style/ArrayCoercion"),
    ("ArrayFirstLast", "Style/ArrayFirstLast"),
    ("ArrayIntersect", "Style/ArrayIntersect"),
    ("ArrayIntersectWithSingleElement", "Style/ArrayIntersectWithSingleElement"),
    ("ArrayJoin", "Style/ArrayJoin"),
    ("AsciiComments", "Style/AsciiComments"),
    ("Attr", "Style/Attr"),
    ("AutoResourceCleanup", "Style/AutoResourceCleanup"),
    ("BarePercentLiterals", "Style/BarePercentLiterals"),
    ("BeginBlock", "Style/BeginBlock"),
    ("BisectedAttrAccessor", "Style/BisectedAttrAccessor"),
    ("BitwisePredicate", "Style/BitwisePredicate"),
    ("BlockComments", "Style/BlockComments"),
    ("BlockDelimiters", "Style/BlockDelimiters"),
    ("CaseEquality", "Style/CaseEquality"),
    ("CaseLikeIf", "Style/CaseLikeIf"),
    ("CharacterLiteral", "Style/CharacterLiteral"),
    ("ClassAndModuleChildren", "Style/ClassAndModuleChildren"),
    ("ClassCheck", "Style/ClassCheck"),
    ("ClassEqualityComparison", "Style/ClassEqualityComparison"),
    ("ClassMethods", "Style/ClassMethods"),
    ("ClassMethodsDefinitions", "Style/ClassMethodsDefinitions"),
    ("ClassVars", "Style/ClassVars"),
    ("CollectionCompact", "Style/CollectionCompact"),
    ("CollectionMethods", "Style/CollectionMethods"),
    ("CollectionQuerying", "Style/CollectionQuerying"),
    ("ColonMethodCall", "Style/ColonMethodCall"),
    ("ColonMethodDefinition", "Style/ColonMethodDefinition"),
    ("CombinableDefined", "Style/CombinableDefined"),
    ("CombinableLoops", "Style/CombinableLoops"),
    ("CommandLiteral", "Style/CommandLiteral"),
    ("CommentAnnotation", "Style/CommentAnnotation"),
    ("CommentedKeyword", "Style/CommentedKeyword"),
    ("ComparableBetween", "Style/ComparableBetween"),
    ("ComparableClamp", "Style/ComparableClamp"),
    ("ConcatArrayLiterals", "Style/ConcatArrayLiterals"),
    ("ConditionalAssignment", "Style/ConditionalAssignment"),
    ("ConstantVisibility", "Style/ConstantVisibility"),
    ("Copyright", "Style/Copyright"),
    ("DataInheritance", "Style/DataInheritance"),
    ("DateTime", "Style/DateTime"),
    ("DefWithParentheses", "Style/DefWithParentheses"),
    ("DigChain", "Style/DigChain"),
    ("Dir", "Style/Dir"),
    ("DirEmpty", "Style/DirEmpty"),
    ("DisableCopsWithinSourceCodeDirective", "Style/DisableCopsWithinSourceCodeDirective"),
    ("DocumentDynamicEvalDefinition", "Style/DocumentDynamicEvalDefinition"),
    ("Documentation", "Style/Documentation"),
    ("DocumentationMethod", "Style/DocumentationMethod"),
    ("DoubleCopDisableDirective", "Style/DoubleCopDisableDirective"),
    ("DoubleNegation", "Style/DoubleNegation"),
    ("EachForSimpleLoop", "Style/EachForSimpleLoop"),
    ("EachWithObject", "Style/EachWithObject"),
    ("EmptyBlockParameter", "Style/EmptyBlockParameter"),
    ("EmptyCaseCondition", "Style/EmptyCaseCondition"),
    ("EmptyClassDefinition", "Style/EmptyClassDefinition"),
    ("EmptyElse", "Style/EmptyElse"),
    ("EmptyHeredoc", "Style/EmptyHeredoc"),
    ("EmptyLambdaParameter", "Style/EmptyLambdaParameter"),
    ("EmptyLiteral", "Style/EmptyLiteral"),
    ("EmptyMethod", "Style/EmptyMethod"),
    ("EmptyStringInsideInterpolation", "Style/EmptyStringInsideInterpolation"),
    ("Encoding", "Style/Encoding"),
    ("EndBlock", "Style/EndBlock"),
    ("EndlessMethod", "Style/EndlessMethod"),
    ("EnvHome", "Style/EnvHome"),
    ("EvalWithLocation", "Style/EvalWithLocation"),
    ("EvenOdd", "Style/EvenOdd"),
    ("ExactRegexpMatch", "Style/ExactRegexpMatch"),
    ("ExpandPathArguments", "Style/ExpandPathArguments"),
    ("ExplicitBlockArgument", "Style/ExplicitBlockArgument"),
    ("ExponentialNotation", "Style/ExponentialNotation"),
    ("FetchEnvVar", "Style/FetchEnvVar"),
    ("FileEmpty", "Style/FileEmpty"),
    ("FileNull", "Style/FileNull"),
    ("FileOpen", "Style/FileOpen"),
    ("FileRead", "Style/FileRead"),
    ("FileTouch", "Style/FileTouch"),
    ("FileWrite", "Style/FileWrite"),
    ("FloatDivision", "Style/FloatDivision"),
    ("For", "Style/For"),
    ("FormatString", "Style/FormatString"),
    ("FormatStringToken", "Style/FormatStringToken"),
    ("FrozenStringLiteralComment", "Style/FrozenStringLiteralComment"),
    ("GlobalStdStream", "Style/GlobalStdStream"),
    ("GlobalVars", "Style/GlobalVars"),
    ("GuardClause", "Style/GuardClause"),
    ("HashAsLastArrayItem", "Style/HashAsLastArrayItem"),
    ("HashConversion", "Style/HashConversion"),
    ("HashEachMethods", "Style/HashEachMethods"),
    ("HashExcept", "Style/HashExcept"),
    ("HashFetchChain", "Style/HashFetchChain"),
    ("HashLikeCase", "Style/HashLikeCase"),
    ("HashLookupMethod", "Style/HashLookupMethod"),
    ("HashSlice", "Style/HashSlice"),
    ("HashSyntax", "Style/HashSyntax"),
    ("HashTransformKeys", "Style/HashTransformKeys"),
    ("HashTransformValues", "Style/HashTransformValues"),
    ("IdenticalConditionalBranches", "Style/IdenticalConditionalBranches"),
    ("IfInsideElse", "Style/IfInsideElse"),
    ("IfUnlessModifier", "Style/IfUnlessModifier"),
    ("IfUnlessModifierOfIfUnless", "Style/IfUnlessModifierOfIfUnless"),
    ("IfWithBooleanLiteralBranches", "Style/IfWithBooleanLiteralBranches"),
    ("IfWithSemicolon", "Style/IfWithSemicolon"),
    ("ImplicitRuntimeError", "Style/ImplicitRuntimeError"),
    ("InPatternThen", "Style/InPatternThen"),
    ("InfiniteLoop", "Style/InfiniteLoop"),
    ("InlineComment", "Style/InlineComment"),
    ("InverseMethods", "Style/InverseMethods"),
    ("InvertibleUnlessCondition", "Style/InvertibleUnlessCondition"),
    ("IpAddresses", "Style/IpAddresses"),
    ("ItAssignment", "Style/ItAssignment"),
    ("ItBlockParameter", "Style/ItBlockParameter"),
    ("KeywordArgumentsMerging", "Style/KeywordArgumentsMerging"),
    ("KeywordParametersOrder", "Style/KeywordParametersOrder"),
    ("Lambda", "Style/Lambda"),
    ("LambdaCall", "Style/LambdaCall"),
    ("LineEndConcatenation", "Style/LineEndConcatenation"),
    ("MagicCommentFormat", "Style/MagicCommentFormat"),
    ("MapCompactWithConditionalBlock", "Style/MapCompactWithConditionalBlock"),
    ("MapIntoArray", "Style/MapIntoArray"),
    ("MapJoin", "Style/MapJoin"),
    ("MapToHash", "Style/MapToHash"),
    ("MapToSet", "Style/MapToSet"),
    ("MethodCallWithArgsParentheses", "Style/MethodCallWithArgsParentheses"),
    ("MethodCallWithoutArgsParentheses", "Style/MethodCallWithoutArgsParentheses"),
    ("MethodCalledOnDoEndBlock", "Style/MethodCalledOnDoEndBlock"),
    ("MethodDefParentheses", "Style/MethodDefParentheses"),
    ("MinMax", "Style/MinMax"),
    ("MinMaxComparison", "Style/MinMaxComparison"),
    ("MissingElse", "Style/MissingElse"),
    ("MissingRespondToMissing", "Style/MissingRespondToMissing"),
    ("MixinGrouping", "Style/MixinGrouping"),
    ("MixinUsage", "Style/MixinUsage"),
    ("ModuleFunction", "Style/ModuleFunction"),
    ("ModuleMemberExistenceCheck", "Style/ModuleMemberExistenceCheck"),
    ("MultilineBlockChain", "Style/MultilineBlockChain"),
    ("MultilineIfModifier", "Style/MultilineIfModifier"),
    ("MultilineIfThen", "Style/MultilineIfThen"),
    ("MultilineInPatternThen", "Style/MultilineInPatternThen"),
    ("MultilineMemoization", "Style/MultilineMemoization"),
    ("MultilineMethodSignature", "Style/MultilineMethodSignature"),
    ("MultilineTernaryOperator", "Style/MultilineTernaryOperator"),
    ("MultilineWhenThen", "Style/MultilineWhenThen"),
    ("MultipleComparison", "Style/MultipleComparison"),
    ("MutableConstant", "Style/MutableConstant"),
    ("NegatedIf", "Style/NegatedIf"),
    ("NegatedIfElseCondition", "Style/NegatedIfElseCondition"),
    ("NegatedUnless", "Style/NegatedUnless"),
    ("NegatedWhile", "Style/NegatedWhile"),
    ("NegativeArrayIndex", "Style/NegativeArrayIndex"),
    ("NestedFileDirname", "Style/NestedFileDirname"),
    ("NestedModifier", "Style/NestedModifier"),
    ("NestedParenthesizedCalls", "Style/NestedParenthesizedCalls"),
    ("NestedTernaryOperator", "Style/NestedTernaryOperator"),
    ("Next", "Style/Next"),
    ("NilComparison", "Style/NilComparison"),
    ("NilLambda", "Style/NilLambda"),
    ("NonNilCheck", "Style/NonNilCheck"),
    ("Not", "Style/Not"),
    ("NumberedParameters", "Style/NumberedParameters"),
    ("NumberedParametersLimit", "Style/NumberedParametersLimit"),
    ("NumericLiteralPrefix", "Style/NumericLiteralPrefix"),
    ("NumericLiterals", "Style/NumericLiterals"),
    ("NumericPredicate", "Style/NumericPredicate"),
    ("ObjectThen", "Style/ObjectThen"),
    ("OneClassPerFile", "Style/OneClassPerFile"),
    ("OneLineConditional", "Style/OneLineConditional"),
    ("OpenStructUse", "Style/OpenStructUse"),
    ("OperatorMethodCall", "Style/OperatorMethodCall"),
    ("OptionHash", "Style/OptionHash"),
    ("OptionalArguments", "Style/OptionalArguments"),
    ("OptionalBooleanParameter", "Style/OptionalBooleanParameter"),
    ("OrAssignment", "Style/OrAssignment"),
    ("ParallelAssignment", "Style/ParallelAssignment"),
    ("ParenthesesAroundCondition", "Style/ParenthesesAroundCondition"),
    ("PartitionInsteadOfDoubleSelect", "Style/PartitionInsteadOfDoubleSelect"),
    ("PercentLiteralDelimiters", "Style/PercentLiteralDelimiters"),
    ("PercentQLiterals", "Style/PercentQLiterals"),
    ("PerlBackrefs", "Style/PerlBackrefs"),
    ("PredicateWithKind", "Style/PredicateWithKind"),
    ("PreferredHashMethods", "Style/PreferredHashMethods"),
    ("Proc", "Style/Proc"),
    ("QuotedSymbols", "Style/QuotedSymbols"),
    ("RaiseArgs", "Style/RaiseArgs"),
    ("RandomWithOffset", "Style/RandomWithOffset"),
    ("ReduceToHash", "Style/ReduceToHash"),
    ("RedundantArgument", "Style/RedundantArgument"),
    ("RedundantArrayConstructor", "Style/RedundantArrayConstructor"),
    ("RedundantArrayFlatten", "Style/RedundantArrayFlatten"),
    ("RedundantAssignment", "Style/RedundantAssignment"),
    ("RedundantBegin", "Style/RedundantBegin"),
    ("RedundantCapitalW", "Style/RedundantCapitalW"),
    ("RedundantCondition", "Style/RedundantCondition"),
    ("RedundantConditional", "Style/RedundantConditional"),
    ("RedundantConstantBase", "Style/RedundantConstantBase"),
    ("RedundantCurrentDirectoryInPath", "Style/RedundantCurrentDirectoryInPath"),
    ("RedundantDoubleSplatHashBraces", "Style/RedundantDoubleSplatHashBraces"),
    ("RedundantEach", "Style/RedundantEach"),
    ("RedundantException", "Style/RedundantException"),
    ("RedundantFetchBlock", "Style/RedundantFetchBlock"),
    ("RedundantFileExtensionInRequire", "Style/RedundantFileExtensionInRequire"),
    ("RedundantFilterChain", "Style/RedundantFilterChain"),
    ("RedundantFormat", "Style/RedundantFormat"),
    ("RedundantFreeze", "Style/RedundantFreeze"),
    ("RedundantHeredocDelimiterQuotes", "Style/RedundantHeredocDelimiterQuotes"),
    ("RedundantInitialize", "Style/RedundantInitialize"),
    ("RedundantInterpolation", "Style/RedundantInterpolation"),
    ("RedundantInterpolationUnfreeze", "Style/RedundantInterpolationUnfreeze"),
    ("RedundantLineContinuation", "Style/RedundantLineContinuation"),
    ("RedundantMinMaxBy", "Style/RedundantMinMaxBy"),
    ("RedundantParentheses", "Style/RedundantParentheses"),
    ("RedundantPercentQ", "Style/RedundantPercentQ"),
    ("RedundantRegexpArgument", "Style/RedundantRegexpArgument"),
    ("RedundantRegexpCharacterClass", "Style/RedundantRegexpCharacterClass"),
    ("RedundantRegexpConstructor", "Style/RedundantRegexpConstructor"),
    ("RedundantRegexpEscape", "Style/RedundantRegexpEscape"),
    ("RedundantReturn", "Style/RedundantReturn"),
    ("RedundantSelf", "Style/RedundantSelf"),
    ("RedundantSelfAssignment", "Style/RedundantSelfAssignment"),
    ("RedundantSelfAssignmentBranch", "Style/RedundantSelfAssignmentBranch"),
    ("RedundantSort", "Style/RedundantSort"),
    ("RedundantSortBy", "Style/RedundantSortBy"),
    ("RedundantStringEscape", "Style/RedundantStringEscape"),
    ("RedundantStructKeywordInit", "Style/RedundantStructKeywordInit"),
    ("RegexpLiteral", "Style/RegexpLiteral"),
    ("RequireOrder", "Style/RequireOrder"),
    ("RescueModifier", "Style/RescueModifier"),
    ("RescueStandardError", "Style/RescueStandardError"),
    ("ReturnNil", "Style/ReturnNil"),
    ("ReturnNilInPredicateMethodDefinition", "Style/ReturnNilInPredicateMethodDefinition"),
    ("ReverseFind", "Style/ReverseFind"),
    ("SafeNavigation", "Style/SafeNavigation"),
    ("SafeNavigationChainLength", "Style/SafeNavigationChainLength"),
    ("Sample", "Style/Sample"),
    ("SelectByKind", "Style/SelectByKind"),
    ("SelectByRange", "Style/SelectByRange"),
    ("SelectByRegexp", "Style/SelectByRegexp"),
    ("SelfAssignment", "Style/SelfAssignment"),
    ("Semicolon", "Style/Semicolon"),
    ("Send", "Style/Send"),
    ("SendWithLiteralMethodName", "Style/SendWithLiteralMethodName"),
    ("SignalException", "Style/SignalException"),
    ("SingleArgumentDig", "Style/SingleArgumentDig"),
    ("SingleLineBlockParams", "Style/SingleLineBlockParams"),
    ("SingleLineDoEndBlock", "Style/SingleLineDoEndBlock"),
    ("SingleLineMethods", "Style/SingleLineMethods"),
    ("SlicingWithRange", "Style/SlicingWithRange"),
    ("SoleNestedConditional", "Style/SoleNestedConditional"),
    ("SpecialGlobalVars", "Style/SpecialGlobalVars"),
    ("StabbyLambdaParentheses", "Style/StabbyLambdaParentheses"),
    ("StaticClass", "Style/StaticClass"),
    ("StderrPuts", "Style/StderrPuts"),
    ("StringChars", "Style/StringChars"),
    ("StringConcatenation", "Style/StringConcatenation"),
    ("StringHashKeys", "Style/StringHashKeys"),
    ("StringLiterals", "Style/StringLiterals"),
    ("StringLiteralsInInterpolation", "Style/StringLiteralsInInterpolation"),
    ("StringMethods", "Style/StringMethods"),
    ("Strip", "Style/Strip"),
    ("StructInheritance", "Style/StructInheritance"),
    ("SuperArguments", "Style/SuperArguments"),
    ("SuperWithArgsParentheses", "Style/SuperWithArgsParentheses"),
    ("SwapValues", "Style/SwapValues"),
    ("SymbolArray", "Style/SymbolArray"),
    ("SymbolLiteral", "Style/SymbolLiteral"),
    ("SymbolProc", "Style/SymbolProc"),
    ("TallyMethod", "Style/TallyMethod"),
    ("TernaryParentheses", "Style/TernaryParentheses"),
    ("TopLevelMethodDefinition", "Style/TopLevelMethodDefinition"),
    ("TrailingBodyOnClass", "Style/TrailingBodyOnClass"),
    ("TrailingBodyOnMethodDefinition", "Style/TrailingBodyOnMethodDefinition"),
    ("TrailingBodyOnModule", "Style/TrailingBodyOnModule"),
    ("TrailingCommaInArguments", "Style/TrailingCommaInArguments"),
    ("TrailingCommaInArrayLiteral", "Style/TrailingCommaInArrayLiteral"),
    ("TrailingCommaInBlockArgs", "Style/TrailingCommaInBlockArgs"),
    ("TrailingCommaInHashLiteral", "Style/TrailingCommaInHashLiteral"),
    ("TrailingMethodEndStatement", "Style/TrailingMethodEndStatement"),
    ("TrailingUnderscoreVariable", "Style/TrailingUnderscoreVariable"),
    ("TrivialAccessors", "Style/TrivialAccessors"),
    ("UnlessElse", "Style/UnlessElse"),
    ("UnlessLogicalOperators", "Style/UnlessLogicalOperators"),
    ("UnpackFirst", "Style/UnpackFirst"),
    ("VariableInterpolation", "Style/VariableInterpolation"),
    ("WhenThen", "Style/WhenThen"),
    ("WhileUntilDo", "Style/WhileUntilDo"),
    ("WhileUntilModifier", "Style/WhileUntilModifier"),
    ("WordArray", "Style/WordArray"),
    ("YAMLFileRead", "Style/YAMLFileRead"),
    ("YodaCondition", "Style/YodaCondition"),
    ("YodaExpression", "Style/YodaExpression"),
    ("ZeroLengthPredicate", "Style/ZeroLengthPredicate"),
];

/// Legacy (renamed/removed/split) cop names from `config/obsoletion.yml`
/// (rubocop 1.87.0), in file order. Mirrors
/// `ConfigObsoletion.legacy_cop_names` + `qualified_legacy_cop_name`'s
/// `detect { |n| n.split('/')[1] == cop_name }`: first match wins, and the
/// old qualified name is returned as-is (e.g. `AlignArguments` ->
/// `Layout/AlignArguments`, not the new `Layout/ArgumentAlignment`).
const LEGACY_COP_NAMES: &[&str] = &[
    "Layout/AlignArguments",
    "Layout/AlignArray",
    "Layout/AlignHash",
    "Layout/AlignParameters",
    "Layout/IndentArray",
    "Layout/IndentAssignment",
    "Layout/IndentFirstArgument",
    "Layout/IndentFirstArrayElement",
    "Layout/IndentFirstHashElement",
    "Layout/IndentFirstParameter",
    "Layout/IndentHash",
    "Layout/IndentHeredoc",
    "Layout/LeadingBlankLines",
    "Layout/Tab",
    "Layout/TrailingBlankLines",
    "Lint/BlockAlignment",
    "Lint/DefEndAlignment",
    "Lint/DuplicatedKey",
    "Lint/EndAlignment",
    "Lint/EndInMethod",
    "Lint/Eval",
    "Lint/HandleExceptions",
    "Lint/MultipleCompare",
    "Lint/StringConversionInInterpolation",
    "Lint/UnneededCopDisableDirective",
    "Lint/UnneededCopEnableDirective",
    "Lint/UnneededRequireStatement",
    "Lint/UnneededSplatExpansion",
    "Metrics/LineLength",
    "Naming/PredicateName",
    "Naming/UncommunicativeBlockParamName",
    "Naming/UncommunicativeMethodParamName",
    "Style/AccessorMethodName",
    "Style/AsciiIdentifiers",
    "Style/ClassAndModuleCamelCase",
    "Style/ConstantName",
    "Style/DeprecatedHashMethods",
    "Style/FileName",
    "Style/FlipFlop",
    "Style/MethodCallParentheses",
    "Style/MethodName",
    "Style/OpMethod",
    "Style/PredicateName",
    "Style/SingleSpaceBeforeFirstArg",
    "Style/UnneededCapitalW",
    "Style/UnneededCondition",
    "Style/UnneededInterpolation",
    "Style/UnneededPercentQ",
    "Style/UnneededSort",
    "Style/VariableName",
    "Style/VariableNumber",
    "Gemspec/DateAssignment",
    "Layout/SpaceAfterControlKeyword",
    "Layout/SpaceBeforeModifierKeyword",
    "Lint/InvalidCharacterLiteral",
    "Lint/RescueWithoutErrorClass",
    "Lint/SpaceBeforeFirstArg",
    "Lint/UselessComparison",
    "Style/BracesAroundHashParameters",
    "Style/MethodMissingSuper",
    "Style/SpaceAfterControlKeyword",
    "Style/SpaceBeforeModifierKeyword",
    "Style/TrailingComma",
    "Style/TrailingCommaInLiteral",
    "Style/MethodMissing",
];

#[derive(Default)]
pub struct DepartmentName;

#[cop(
    name = "Migration/DepartmentName",
    description = "Check that cop names in rubocop:disable (etc) comments are given with department name.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl DepartmentName {
    #[on_new_investigation]
    fn check_file(&self, cx: &Cx<'_>) {
        for comment in cx.comments() {
            let text = cx.raw_source(comment.range);

            // `next if comment.text !~ DISABLE_COMMENT_FORMAT` — only
            // `disable`/`enable`/`todo` directives, with the prefix captured so
            // its length is the starting `offset` into the comment.
            let Some(prefix_len) = directive_prefix_len(text) else {
                continue;
            };

            // `offset` tracks the byte position of the current token within the
            // comment, mirroring RuboCop's `offset += name.length`.
            let mut offset = prefix_len;
            let cop_names = &text[prefix_len..];

            for token in scan_tokens(cop_names) {
                let trimmed = token.trim();

                if !valid_content_token(trimmed) {
                    // Offense range = the trimmed bare cop name. The comma-run
                    // scan groups `", "` as `","` + `" Bar"`, so a flagged token
                    // can carry leading whitespace (e.g. `" Bar"` after the
                    // comma); advance past it so the range starts at the first
                    // byte of `trimmed`, matching RuboCop's `begin_pos`.
                    let leading_ws = token.len() - token.trim_start().len();
                    let start = comment.range.start + offset as u32 + leading_ws as u32;
                    let range = Range { start, end: start + trimmed.len() as u32 };
                    cx.emit_offense(range, MSG, None);
                    // Autocorrect: `Registry.global.qualified_cop_name` +
                    // `qualified_legacy_cop_name` fallback. Unknown and
                    // ambiguous bare names yield `None` (offense only, no
                    // correction), matching RuboCop's non-correctable cases.
                    if let Some(qualified) = qualified_cop_name(trimmed) {
                        cx.emit_edit(range, qualified);
                    }
                }

                // `break if contain_unexpected_character_for_department_name?`.
                // Stops the scan at the first token containing a character
                // outside `[A-Za-z/, ]` — this is what terminates scanning at a
                // trailing `-- comment` so prose words are never flagged.
                if contains_unexpected_character(token) {
                    break;
                }

                offset += token.len();
            }
        }
    }
}

/// `DISABLE_COMMENT_FORMAT = /\A(# *rubocop *: *((dis|en)able|todo) +)(.*)/`.
/// Returns the byte length of capture group 1 (the directive prefix, including
/// its trailing run of spaces) when `text` matches, else `None`. Spaces only —
/// RuboCop's regex uses literal ` `, not `\s`, so tabs do not match.
fn directive_prefix_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;

    // `#`
    if bytes.first() != Some(&b'#') {
        return None;
    }
    i += 1;
    i += count_spaces(&bytes[i..]);

    // `rubocop`
    let rest = text.get(i..)?;
    let rest = rest.strip_prefix("rubocop")?;
    i = text.len() - rest.len();
    i += count_spaces(&bytes[i..]);

    // `:`
    if bytes.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    i += count_spaces(&bytes[i..]);

    // `(dis|en)able|todo`
    let rest = text.get(i..)?;
    let mode = ["disable", "enable", "todo"]
        .into_iter()
        .find(|m| rest.starts_with(m))?;
    i += mode.len();

    // ` +` — at least one trailing space is required by the regex.
    let trailing = count_spaces(&bytes[i..]);
    if trailing == 0 {
        return None;
    }
    i += trailing;

    Some(i)
}

/// Count the leading run of ASCII space (0x20) bytes.
fn count_spaces(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|&&b| b == b' ').count()
}

/// Reproduce Ruby's `cop_names.scan(/[^,]+|\W+/)`: the input splits into maximal
/// runs of non-comma bytes (`[^,]+`) and maximal runs of commas (the only ASCII
/// chars matched by `\W+` once non-comma chars are claimed by `[^,]+` first).
fn scan_tokens(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let is_comma = bytes[i] == b',';
        let start = i;
        while i < bytes.len() && (bytes[i] == b',') == is_comma {
            i += 1;
        }
        tokens.push(&s[start..i]);
    }
    tokens
}

/// `Registry.global.qualified_cop_name(name, nil, warn: false)` for core cops:
/// a bare name mapping to exactly one qualified name resolves to it; zero
/// matches fall through to [`qualified_legacy_cop_name`]; two matches
/// (ambiguous) yield `None` so the caller reports an offense with no
/// correction instead of raising `AmbiguousCopName` like RuboCop does.
fn qualified_cop_name(bare: &str) -> Option<&'static str> {
    let mut found: Option<&'static str> = None;
    for &(b, q) in BARE_TO_QUALIFIED {
        if b == bare {
            if found.is_some() {
                // Ambiguous (`MultipleComparison`, `SelfAssignment`).
                return None;
            }
            found = Some(q);
        }
    }
    if found.is_some() {
        return found;
    }
    qualified_legacy_cop_name(bare)
}

/// `qualified_legacy_cop_name(cop_name)` — first `LEGACY_COP_NAMES` entry
/// whose bare part (`split('/')[1]`) equals `cop_name`, or `None`.
fn qualified_legacy_cop_name(bare: &str) -> Option<&'static str> {
    LEGACY_COP_NAMES.iter().copied().find(|legacy| {
        legacy.split_once('/').is_some_and(|(_, b)| b == bare)
    })
}

/// `valid_content_token?(content_token)` — a token is acceptable when it
/// matches `/\W+/` (contains any non-word char), matches
/// `%r{[A-Za-z]+/[A-Za-z]+|all}` (qualified name or the `all` keyword), or is a
/// registered department. Both regexes are `match?` (partial), so a substring
/// match suffices.
fn valid_content_token(token: &str) -> bool {
    contains_non_word_char(token)
        || contains_qualified_name_or_all(token)
        || CORE_DEPARTMENTS.contains(&token)
}

/// `/\W+/.match?` — true when the token contains at least one char that is not
/// `[A-Za-z0-9_]`. An empty token has no such char and so does not match.
fn contains_non_word_char(token: &str) -> bool {
    token.chars().any(|c| !(c.is_ascii_alphanumeric() || c == '_'))
}

/// `%r{[A-Za-z]+/[A-Za-z]+|all}.match?` — partial match for either a
/// `Letters/Letters` substring or the literal substring `all`. The
/// `Letters/Letters` branch is, in practice, shadowed by `contains_non_word_char`
/// (any `/` is already a non-word char), faithfully mirroring RuboCop's own
/// redundant alternation against its leading `/\W+/` check — kept for a 1:1 port.
fn contains_qualified_name_or_all(token: &str) -> bool {
    token.contains("all") || contains_qualified_name(token)
}

/// Partial match for `[A-Za-z]+/[A-Za-z]+`: a run of ASCII letters, a `/`, then
/// another run of ASCII letters, appearing anywhere in `token`.
fn contains_qualified_name(token: &str) -> bool {
    let bytes = token.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'/' {
            continue;
        }
        let left = bytes[..i].iter().rev().take_while(|&&c| c.is_ascii_alphabetic()).count();
        let right = bytes[i + 1..].iter().take_while(|&&c| c.is_ascii_alphabetic()).count();
        if left >= 1 && right >= 1 {
            return true;
        }
    }
    false
}

/// `contain_unexpected_character_for_department_name?(name)` —
/// `name.match?(%r{[^A-Za-z/, ]})`, true when `name` contains any char outside
/// the set `[A-Za-z/, ]` (letters, slash, comma, space). Operates on the
/// untrimmed token, matching RuboCop.
fn contains_unexpected_character(name: &str) -> bool {
    name.chars()
        .any(|c| !(c.is_ascii_alphabetic() || c == '/' || c == ',' || c == ' '))
}

murphy_plugin_api::submit_cop!(DepartmentName);

#[cfg(test)]
mod tests {
    use super::DepartmentName;
    use murphy_plugin_api::test_support::test;

    // ---- no-offense cases ----

    #[test]
    fn accepts_qualified_cop_name() {
        test::<DepartmentName>().expect_no_offenses("# rubocop:disable Layout/LineLength\n");
    }

    #[test]
    fn accepts_bare_department() {
        // `Layout` is a registered department, not a department-less cop name.
        test::<DepartmentName>().expect_no_offenses("# rubocop:disable Layout\n");
    }

    #[test]
    fn accepts_all() {
        test::<DepartmentName>().expect_no_offenses("# rubocop:disable all\n");
    }

    #[test]
    fn accepts_multiple_qualified_cops() {
        test::<DepartmentName>()
            .expect_no_offenses("# rubocop:disable Layout/LineLength, Style/Encoding\n");
    }

    #[test]
    fn accepts_qualified_cop_with_trailing_comment() {
        // The `--` comment contains chars outside `[A-Za-z/, ]`, so the scan
        // breaks before reaching the prose words.
        test::<DepartmentName>().expect_no_offenses(
            "# rubocop:disable Layout/LineLength -- Because Reasons Here\n",
        );
    }

    #[test]
    fn accepts_token_containing_all_substring() {
        // `Marshalling` contains the substring `all`, so the `all` partial match
        // makes it valid (a RuboCop quirk we faithfully reproduce).
        test::<DepartmentName>().expect_no_offenses("# rubocop:disable Marshalling\n");
    }

    #[test]
    fn ignores_push_directive() {
        // `push`/`pop` are not matched by `DISABLE_COMMENT_FORMAT`.
        test::<DepartmentName>().expect_no_offenses("# rubocop:push AbcSize\n");
    }

    #[test]
    fn ignores_non_directive_comment() {
        test::<DepartmentName>().expect_no_offenses("# just a comment AbcSize\n");
    }

    #[test]
    fn requires_trailing_space_after_mode() {
        // ` +` requires at least one space after the mode; `disable` glued to
        // the cop name is not a directive.
        test::<DepartmentName>().expect_no_offenses("# rubocop:disableAbcSize\n");
    }

    // ---- offense cases ----

    #[test]
    fn flags_bare_cop_name() {
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:disable LineLength\n",
            "                        ^^^^^^^^^^ Department name is missing.\n",
        ));
    }

    #[test]
    fn flags_bare_cop_name_enable() {
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:enable AbcSize\n",
            "                       ^^^^^^^ Department name is missing.\n",
        ));
    }

    #[test]
    fn flags_bare_cop_name_todo() {
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:todo AbcSize\n",
            "                     ^^^^^^^ Department name is missing.\n",
        ));
    }

    #[test]
    fn flags_only_the_bare_cop_in_a_mixed_list() {
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:disable AbcSize, Metrics/MethodLength\n",
            "                        ^^^^^^^ Department name is missing.\n",
        ));
    }

    #[test]
    fn flags_each_bare_cop_in_a_list() {
        // RuboCop 1.87.0 reports both at cols 25 and 30 (verified against
        // standalone rubocop) — `Foo` then `Bar` after the `, ` separator.
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:disable Foo, Bar\n",
            "                        ^^^ Department name is missing.\n",
            "                             ^^^ Department name is missing.\n",
        ));
    }

    #[test]
    fn flags_lowercase_bare_cop_name() {
        // Pure word chars, not a department, no slash, no `all` substring.
        test::<DepartmentName>().expect_offense(concat!(
            "x = 1 # rubocop:disable abc\n",
            "                        ^^^ Department name is missing.\n",
        ));
    }

    // ---- autocorrect ----

    #[test]
    fn corrects_bare_cop_name() {
        test::<DepartmentName>().expect_correction(
            concat!(
                "x = 1 # rubocop:disable AbcSize\n",
                "                        ^^^^^^^ Department name is missing.\n",
            ),
            "x = 1 # rubocop:disable Metrics/AbcSize\n",
        );
    }

    #[test]
    fn corrects_bare_cop_name_enable() {
        test::<DepartmentName>().expect_correction(
            concat!(
                "x = 1 # rubocop:enable LineLength\n",
                "                       ^^^^^^^^^^ Department name is missing.\n",
            ),
            "x = 1 # rubocop:enable Layout/LineLength\n",
        );
    }

    #[test]
    fn corrects_bare_cop_name_todo() {
        test::<DepartmentName>().expect_correction(
            concat!(
                "x = 1 # rubocop:todo AbcSize\n",
                "                     ^^^^^^^ Department name is missing.\n",
            ),
            "x = 1 # rubocop:todo Metrics/AbcSize\n",
        );
    }

    #[test]
    fn corrects_only_the_bare_cop_in_a_mixed_list() {
        test::<DepartmentName>().expect_correction(
            concat!(
                "x = 1 # rubocop:disable AbcSize, Metrics/MethodLength\n",
                "                        ^^^^^^^ Department name is missing.\n",
            ),
            "x = 1 # rubocop:disable Metrics/AbcSize, Metrics/MethodLength\n",
        );
    }

    #[test]
    fn corrects_two_known_cops_in_a_list() {
        test::<DepartmentName>().expect_correction(
            concat!(
                "x = 1 # rubocop:disable AbcSize, LineLength\n",
                "                        ^^^^^^^ Department name is missing.\n",
                "                                 ^^^^^^^^^^ Department name is missing.\n",
            ),
            "x = 1 # rubocop:disable Metrics/AbcSize, Layout/LineLength\n",
        );
    }

    #[test]
    fn leaves_unknown_bare_names_uncorrected() {
        // Unknown cops are flagged but not corrected (RuboCop: not correctable).
        test::<DepartmentName>().expect_no_corrections("x = 1 # rubocop:disable Foo\n");
    }

    #[test]
    fn leaves_ambiguous_bare_names_uncorrected() {
        // `MultipleComparison` / `SelfAssignment` exist in two departments;
        // RuboCop raises `AmbiguousCopName`, so we flag with no correction.
        test::<DepartmentName>()
            .expect_no_corrections("x = 1 # rubocop:disable MultipleComparison\n");
        test::<DepartmentName>()
            .expect_no_corrections("x = 1 # rubocop:disable SelfAssignment\n");
    }

    #[test]
    fn corrects_legacy_bare_cop_name() {
        // Legacy fallback: `AlignArguments` -> `Layout/AlignArguments`
        // (the old qualified name, not the new `Layout/ArgumentAlignment`).
        test::<DepartmentName>().expect_correction(
            concat!(
                "x = 1 # rubocop:disable AlignArguments\n",
                "                        ^^^^^^^^^^^^^^ Department name is missing.\n",
            ),
            "x = 1 # rubocop:disable Layout/AlignArguments\n",
        );
    }

    #[test]
    fn qualified_lookup_resolves_known_cops() {
        assert_eq!(super::qualified_cop_name("AbcSize"), Some("Metrics/AbcSize"));
        assert_eq!(super::qualified_cop_name("LineLength"), Some("Layout/LineLength"));
        assert_eq!(
            super::qualified_cop_name("AlignArguments"),
            Some("Layout/AlignArguments")
        );
        assert_eq!(super::qualified_cop_name("Foo"), None);
        assert_eq!(super::qualified_cop_name("MultipleComparison"), None);
        assert_eq!(super::qualified_cop_name("SelfAssignment"), None);
    }
}
