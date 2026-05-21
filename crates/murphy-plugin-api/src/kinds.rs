//! Node-kind string constants for the ruby-prism Node variants.
//!
//! Each constant carries the **snake_case wire name** that Murphy's
//! native dispatch ABI consumes (matching
//! `murphy_core::cop::prism_node_kind` exactly). Plugin authors should
//! reference these constants from `#[on_node(kind = ...)]` attributes so
//! that typos become compile-time errors rather than silent no-ops.
//!
//! The const identifier is the `SCREAMING_SNAKE_CASE` form of the Rust
//! enum variant name (e.g. `CALL_NODE` for `ruby_prism::Node::CallNode`)
//! and the string value is the snake_case wire form (`"call"`).
//!
//! # Maintenance on ruby-prism upgrade
//!
//! `murphy_core::cop::prism_node_kind` is an exhaustive `match` over
//! `ruby_prism::Node`, so any new variant fails the workspace build
//! immediately. When that happens:
//!
//! 1. Add the new arm to `prism_node_kind` so murphy-core compiles.
//! 2. Add the matching `pub const FOO_NODE: &str = "foo";` here, the entry
//!    in [`ALL`], and bump [`COUNT`].
//! 3. The `kinds_all_matches_murphy_core_prism_node_kind` integration
//!    test (in `tests/prism_kinds_coverage.rs`) goes green when the two
//!    lists agree.

/// `ruby_prism::Node::AliasGlobalVariableNode` — wire name `"alias_global_variable"`.
pub const ALIAS_GLOBAL_VARIABLE_NODE: &str = "alias_global_variable";

/// `ruby_prism::Node::AliasMethodNode` — wire name `"alias_method"`.
pub const ALIAS_METHOD_NODE: &str = "alias_method";

/// `ruby_prism::Node::AlternationPatternNode` — wire name `"alternation_pattern"`.
pub const ALTERNATION_PATTERN_NODE: &str = "alternation_pattern";

/// `ruby_prism::Node::AndNode` — wire name `"and"`.
pub const AND_NODE: &str = "and";

/// `ruby_prism::Node::ArgumentsNode` — wire name `"arguments"`.
pub const ARGUMENTS_NODE: &str = "arguments";

/// `ruby_prism::Node::ArrayNode` — wire name `"array"`.
pub const ARRAY_NODE: &str = "array";

/// `ruby_prism::Node::ArrayPatternNode` — wire name `"array_pattern"`.
pub const ARRAY_PATTERN_NODE: &str = "array_pattern";

/// `ruby_prism::Node::AssocNode` — wire name `"assoc"`.
pub const ASSOC_NODE: &str = "assoc";

/// `ruby_prism::Node::AssocSplatNode` — wire name `"assoc_splat"`.
pub const ASSOC_SPLAT_NODE: &str = "assoc_splat";

/// `ruby_prism::Node::BackReferenceReadNode` — wire name `"back_reference_read"`.
pub const BACK_REFERENCE_READ_NODE: &str = "back_reference_read";

/// `ruby_prism::Node::BeginNode` — wire name `"begin"`.
pub const BEGIN_NODE: &str = "begin";

/// `ruby_prism::Node::BlockArgumentNode` — wire name `"block_argument"`.
pub const BLOCK_ARGUMENT_NODE: &str = "block_argument";

/// `ruby_prism::Node::BlockLocalVariableNode` — wire name `"block_local_variable"`.
pub const BLOCK_LOCAL_VARIABLE_NODE: &str = "block_local_variable";

/// `ruby_prism::Node::BlockNode` — wire name `"block"`.
pub const BLOCK_NODE: &str = "block";

/// `ruby_prism::Node::BlockParameterNode` — wire name `"block_parameter"`.
pub const BLOCK_PARAMETER_NODE: &str = "block_parameter";

/// `ruby_prism::Node::BlockParametersNode` — wire name `"block_parameters"`.
pub const BLOCK_PARAMETERS_NODE: &str = "block_parameters";

/// `ruby_prism::Node::BreakNode` — wire name `"break"`.
pub const BREAK_NODE: &str = "break";

/// `ruby_prism::Node::CallAndWriteNode` — wire name `"call_and_write"`.
pub const CALL_AND_WRITE_NODE: &str = "call_and_write";

/// `ruby_prism::Node::CallNode` — wire name `"call"`.
pub const CALL_NODE: &str = "call";

/// `ruby_prism::Node::CallOperatorWriteNode` — wire name `"call_operator_write"`.
pub const CALL_OPERATOR_WRITE_NODE: &str = "call_operator_write";

/// `ruby_prism::Node::CallOrWriteNode` — wire name `"call_or_write"`.
pub const CALL_OR_WRITE_NODE: &str = "call_or_write";

/// `ruby_prism::Node::CallTargetNode` — wire name `"call_target"`.
pub const CALL_TARGET_NODE: &str = "call_target";

/// `ruby_prism::Node::CapturePatternNode` — wire name `"capture_pattern"`.
pub const CAPTURE_PATTERN_NODE: &str = "capture_pattern";

/// `ruby_prism::Node::CaseMatchNode` — wire name `"case_match"`.
pub const CASE_MATCH_NODE: &str = "case_match";

/// `ruby_prism::Node::CaseNode` — wire name `"case"`.
pub const CASE_NODE: &str = "case";

/// `ruby_prism::Node::ClassNode` — wire name `"class"`.
pub const CLASS_NODE: &str = "class";

/// `ruby_prism::Node::ClassVariableAndWriteNode` — wire name `"class_variable_and_write"`.
pub const CLASS_VARIABLE_AND_WRITE_NODE: &str = "class_variable_and_write";

/// `ruby_prism::Node::ClassVariableOperatorWriteNode` — wire name `"class_variable_operator_write"`.
pub const CLASS_VARIABLE_OPERATOR_WRITE_NODE: &str = "class_variable_operator_write";

/// `ruby_prism::Node::ClassVariableOrWriteNode` — wire name `"class_variable_or_write"`.
pub const CLASS_VARIABLE_OR_WRITE_NODE: &str = "class_variable_or_write";

/// `ruby_prism::Node::ClassVariableReadNode` — wire name `"class_variable_read"`.
pub const CLASS_VARIABLE_READ_NODE: &str = "class_variable_read";

/// `ruby_prism::Node::ClassVariableTargetNode` — wire name `"class_variable_target"`.
pub const CLASS_VARIABLE_TARGET_NODE: &str = "class_variable_target";

/// `ruby_prism::Node::ClassVariableWriteNode` — wire name `"class_variable_write"`.
pub const CLASS_VARIABLE_WRITE_NODE: &str = "class_variable_write";

/// `ruby_prism::Node::ConstantAndWriteNode` — wire name `"constant_and_write"`.
pub const CONSTANT_AND_WRITE_NODE: &str = "constant_and_write";

/// `ruby_prism::Node::ConstantOperatorWriteNode` — wire name `"constant_operator_write"`.
pub const CONSTANT_OPERATOR_WRITE_NODE: &str = "constant_operator_write";

/// `ruby_prism::Node::ConstantOrWriteNode` — wire name `"constant_or_write"`.
pub const CONSTANT_OR_WRITE_NODE: &str = "constant_or_write";

/// `ruby_prism::Node::ConstantPathAndWriteNode` — wire name `"constant_path_and_write"`.
pub const CONSTANT_PATH_AND_WRITE_NODE: &str = "constant_path_and_write";

/// `ruby_prism::Node::ConstantPathNode` — wire name `"constant_path"`.
pub const CONSTANT_PATH_NODE: &str = "constant_path";

/// `ruby_prism::Node::ConstantPathOperatorWriteNode` — wire name `"constant_path_operator_write"`.
pub const CONSTANT_PATH_OPERATOR_WRITE_NODE: &str = "constant_path_operator_write";

/// `ruby_prism::Node::ConstantPathOrWriteNode` — wire name `"constant_path_or_write"`.
pub const CONSTANT_PATH_OR_WRITE_NODE: &str = "constant_path_or_write";

/// `ruby_prism::Node::ConstantPathTargetNode` — wire name `"constant_path_target"`.
pub const CONSTANT_PATH_TARGET_NODE: &str = "constant_path_target";

/// `ruby_prism::Node::ConstantPathWriteNode` — wire name `"constant_path_write"`.
pub const CONSTANT_PATH_WRITE_NODE: &str = "constant_path_write";

/// `ruby_prism::Node::ConstantReadNode` — wire name `"constant_read"`.
pub const CONSTANT_READ_NODE: &str = "constant_read";

/// `ruby_prism::Node::ConstantTargetNode` — wire name `"constant_target"`.
pub const CONSTANT_TARGET_NODE: &str = "constant_target";

/// `ruby_prism::Node::ConstantWriteNode` — wire name `"constant_write"`.
pub const CONSTANT_WRITE_NODE: &str = "constant_write";

/// `ruby_prism::Node::DefNode` — wire name `"def"`.
pub const DEF_NODE: &str = "def";

/// `ruby_prism::Node::DefinedNode` — wire name `"defined"`.
pub const DEFINED_NODE: &str = "defined";

/// `ruby_prism::Node::ElseNode` — wire name `"else"`.
pub const ELSE_NODE: &str = "else";

/// `ruby_prism::Node::EmbeddedStatementsNode` — wire name `"embedded_statements"`.
pub const EMBEDDED_STATEMENTS_NODE: &str = "embedded_statements";

/// `ruby_prism::Node::EmbeddedVariableNode` — wire name `"embedded_variable"`.
pub const EMBEDDED_VARIABLE_NODE: &str = "embedded_variable";

/// `ruby_prism::Node::EnsureNode` — wire name `"ensure"`.
pub const ENSURE_NODE: &str = "ensure";

/// `ruby_prism::Node::FalseNode` — wire name `"false"`.
pub const FALSE_NODE: &str = "false";

/// `ruby_prism::Node::FindPatternNode` — wire name `"find_pattern"`.
pub const FIND_PATTERN_NODE: &str = "find_pattern";

/// `ruby_prism::Node::FlipFlopNode` — wire name `"flip_flop"`.
pub const FLIP_FLOP_NODE: &str = "flip_flop";

/// `ruby_prism::Node::FloatNode` — wire name `"float"`.
pub const FLOAT_NODE: &str = "float";

/// `ruby_prism::Node::ForNode` — wire name `"for"`.
pub const FOR_NODE: &str = "for";

/// `ruby_prism::Node::ForwardingArgumentsNode` — wire name `"forwarding_arguments"`.
pub const FORWARDING_ARGUMENTS_NODE: &str = "forwarding_arguments";

/// `ruby_prism::Node::ForwardingParameterNode` — wire name `"forwarding_parameter"`.
pub const FORWARDING_PARAMETER_NODE: &str = "forwarding_parameter";

/// `ruby_prism::Node::ForwardingSuperNode` — wire name `"forwarding_super"`.
pub const FORWARDING_SUPER_NODE: &str = "forwarding_super";

/// `ruby_prism::Node::GlobalVariableAndWriteNode` — wire name `"global_variable_and_write"`.
pub const GLOBAL_VARIABLE_AND_WRITE_NODE: &str = "global_variable_and_write";

/// `ruby_prism::Node::GlobalVariableOperatorWriteNode` — wire name `"global_variable_operator_write"`.
pub const GLOBAL_VARIABLE_OPERATOR_WRITE_NODE: &str = "global_variable_operator_write";

/// `ruby_prism::Node::GlobalVariableOrWriteNode` — wire name `"global_variable_or_write"`.
pub const GLOBAL_VARIABLE_OR_WRITE_NODE: &str = "global_variable_or_write";

/// `ruby_prism::Node::GlobalVariableReadNode` — wire name `"global_variable_read"`.
pub const GLOBAL_VARIABLE_READ_NODE: &str = "global_variable_read";

/// `ruby_prism::Node::GlobalVariableTargetNode` — wire name `"global_variable_target"`.
pub const GLOBAL_VARIABLE_TARGET_NODE: &str = "global_variable_target";

/// `ruby_prism::Node::GlobalVariableWriteNode` — wire name `"global_variable_write"`.
pub const GLOBAL_VARIABLE_WRITE_NODE: &str = "global_variable_write";

/// `ruby_prism::Node::HashNode` — wire name `"hash"`.
pub const HASH_NODE: &str = "hash";

/// `ruby_prism::Node::HashPatternNode` — wire name `"hash_pattern"`.
pub const HASH_PATTERN_NODE: &str = "hash_pattern";

/// `ruby_prism::Node::IfNode` — wire name `"if"`.
pub const IF_NODE: &str = "if";

/// `ruby_prism::Node::ImaginaryNode` — wire name `"imaginary"`.
pub const IMAGINARY_NODE: &str = "imaginary";

/// `ruby_prism::Node::ImplicitNode` — wire name `"implicit"`.
pub const IMPLICIT_NODE: &str = "implicit";

/// `ruby_prism::Node::ImplicitRestNode` — wire name `"implicit_rest"`.
pub const IMPLICIT_REST_NODE: &str = "implicit_rest";

/// `ruby_prism::Node::InNode` — wire name `"in"`.
pub const IN_NODE: &str = "in";

/// `ruby_prism::Node::IndexAndWriteNode` — wire name `"index_and_write"`.
pub const INDEX_AND_WRITE_NODE: &str = "index_and_write";

/// `ruby_prism::Node::IndexOperatorWriteNode` — wire name `"index_operator_write"`.
pub const INDEX_OPERATOR_WRITE_NODE: &str = "index_operator_write";

/// `ruby_prism::Node::IndexOrWriteNode` — wire name `"index_or_write"`.
pub const INDEX_OR_WRITE_NODE: &str = "index_or_write";

/// `ruby_prism::Node::IndexTargetNode` — wire name `"index_target"`.
pub const INDEX_TARGET_NODE: &str = "index_target";

/// `ruby_prism::Node::InstanceVariableAndWriteNode` — wire name `"instance_variable_and_write"`.
pub const INSTANCE_VARIABLE_AND_WRITE_NODE: &str = "instance_variable_and_write";

/// `ruby_prism::Node::InstanceVariableOperatorWriteNode` — wire name `"instance_variable_operator_write"`.
pub const INSTANCE_VARIABLE_OPERATOR_WRITE_NODE: &str = "instance_variable_operator_write";

/// `ruby_prism::Node::InstanceVariableOrWriteNode` — wire name `"instance_variable_or_write"`.
pub const INSTANCE_VARIABLE_OR_WRITE_NODE: &str = "instance_variable_or_write";

/// `ruby_prism::Node::InstanceVariableReadNode` — wire name `"instance_variable_read"`.
pub const INSTANCE_VARIABLE_READ_NODE: &str = "instance_variable_read";

/// `ruby_prism::Node::InstanceVariableTargetNode` — wire name `"instance_variable_target"`.
pub const INSTANCE_VARIABLE_TARGET_NODE: &str = "instance_variable_target";

/// `ruby_prism::Node::InstanceVariableWriteNode` — wire name `"instance_variable_write"`.
pub const INSTANCE_VARIABLE_WRITE_NODE: &str = "instance_variable_write";

/// `ruby_prism::Node::IntegerNode` — wire name `"integer"`.
pub const INTEGER_NODE: &str = "integer";

/// `ruby_prism::Node::InterpolatedMatchLastLineNode` — wire name `"interpolated_match_last_line"`.
pub const INTERPOLATED_MATCH_LAST_LINE_NODE: &str = "interpolated_match_last_line";

/// `ruby_prism::Node::InterpolatedRegularExpressionNode` — wire name `"interpolated_regular_expression"`.
pub const INTERPOLATED_REGULAR_EXPRESSION_NODE: &str = "interpolated_regular_expression";

/// `ruby_prism::Node::InterpolatedStringNode` — wire name `"interpolated_string"`.
pub const INTERPOLATED_STRING_NODE: &str = "interpolated_string";

/// `ruby_prism::Node::InterpolatedSymbolNode` — wire name `"interpolated_symbol"`.
pub const INTERPOLATED_SYMBOL_NODE: &str = "interpolated_symbol";

/// `ruby_prism::Node::InterpolatedXStringNode` — wire name `"interpolated_xstring"`.
pub const INTERPOLATED_X_STRING_NODE: &str = "interpolated_xstring";

/// `ruby_prism::Node::ItLocalVariableReadNode` — wire name `"it_local_variable_read"`.
pub const IT_LOCAL_VARIABLE_READ_NODE: &str = "it_local_variable_read";

/// `ruby_prism::Node::ItParametersNode` — wire name `"it_parameters"`.
pub const IT_PARAMETERS_NODE: &str = "it_parameters";

/// `ruby_prism::Node::KeywordHashNode` — wire name `"keyword_hash"`.
pub const KEYWORD_HASH_NODE: &str = "keyword_hash";

/// `ruby_prism::Node::KeywordRestParameterNode` — wire name `"keyword_rest_parameter"`.
pub const KEYWORD_REST_PARAMETER_NODE: &str = "keyword_rest_parameter";

/// `ruby_prism::Node::LambdaNode` — wire name `"lambda"`.
pub const LAMBDA_NODE: &str = "lambda";

/// `ruby_prism::Node::LocalVariableAndWriteNode` — wire name `"local_variable_and_write"`.
pub const LOCAL_VARIABLE_AND_WRITE_NODE: &str = "local_variable_and_write";

/// `ruby_prism::Node::LocalVariableOperatorWriteNode` — wire name `"local_variable_operator_write"`.
pub const LOCAL_VARIABLE_OPERATOR_WRITE_NODE: &str = "local_variable_operator_write";

/// `ruby_prism::Node::LocalVariableOrWriteNode` — wire name `"local_variable_or_write"`.
pub const LOCAL_VARIABLE_OR_WRITE_NODE: &str = "local_variable_or_write";

/// `ruby_prism::Node::LocalVariableReadNode` — wire name `"local_variable_read"`.
pub const LOCAL_VARIABLE_READ_NODE: &str = "local_variable_read";

/// `ruby_prism::Node::LocalVariableTargetNode` — wire name `"local_variable_target"`.
pub const LOCAL_VARIABLE_TARGET_NODE: &str = "local_variable_target";

/// `ruby_prism::Node::LocalVariableWriteNode` — wire name `"local_variable_write"`.
pub const LOCAL_VARIABLE_WRITE_NODE: &str = "local_variable_write";

/// `ruby_prism::Node::MatchLastLineNode` — wire name `"match_last_line"`.
pub const MATCH_LAST_LINE_NODE: &str = "match_last_line";

/// `ruby_prism::Node::MatchPredicateNode` — wire name `"match_predicate"`.
pub const MATCH_PREDICATE_NODE: &str = "match_predicate";

/// `ruby_prism::Node::MatchRequiredNode` — wire name `"match_required"`.
pub const MATCH_REQUIRED_NODE: &str = "match_required";

/// `ruby_prism::Node::MatchWriteNode` — wire name `"match_write"`.
pub const MATCH_WRITE_NODE: &str = "match_write";

/// `ruby_prism::Node::MissingNode` — wire name `"missing"`.
pub const MISSING_NODE: &str = "missing";

/// `ruby_prism::Node::ModuleNode` — wire name `"module"`.
pub const MODULE_NODE: &str = "module";

/// `ruby_prism::Node::MultiTargetNode` — wire name `"multi_target"`.
pub const MULTI_TARGET_NODE: &str = "multi_target";

/// `ruby_prism::Node::MultiWriteNode` — wire name `"multi_write"`.
pub const MULTI_WRITE_NODE: &str = "multi_write";

/// `ruby_prism::Node::NextNode` — wire name `"next"`.
pub const NEXT_NODE: &str = "next";

/// `ruby_prism::Node::NilNode` — wire name `"nil"`.
pub const NIL_NODE: &str = "nil";

/// `ruby_prism::Node::NoKeywordsParameterNode` — wire name `"no_keywords_parameter"`.
pub const NO_KEYWORDS_PARAMETER_NODE: &str = "no_keywords_parameter";

/// `ruby_prism::Node::NumberedParametersNode` — wire name `"numbered_parameters"`.
pub const NUMBERED_PARAMETERS_NODE: &str = "numbered_parameters";

/// `ruby_prism::Node::NumberedReferenceReadNode` — wire name `"numbered_reference_read"`.
pub const NUMBERED_REFERENCE_READ_NODE: &str = "numbered_reference_read";

/// `ruby_prism::Node::OptionalKeywordParameterNode` — wire name `"optional_keyword_parameter"`.
pub const OPTIONAL_KEYWORD_PARAMETER_NODE: &str = "optional_keyword_parameter";

/// `ruby_prism::Node::OptionalParameterNode` — wire name `"optional_parameter"`.
pub const OPTIONAL_PARAMETER_NODE: &str = "optional_parameter";

/// `ruby_prism::Node::OrNode` — wire name `"or"`.
pub const OR_NODE: &str = "or";

/// `ruby_prism::Node::ParametersNode` — wire name `"parameters"`.
pub const PARAMETERS_NODE: &str = "parameters";

/// `ruby_prism::Node::ParenthesesNode` — wire name `"parentheses"`.
pub const PARENTHESES_NODE: &str = "parentheses";

/// `ruby_prism::Node::PinnedExpressionNode` — wire name `"pinned_expression"`.
pub const PINNED_EXPRESSION_NODE: &str = "pinned_expression";

/// `ruby_prism::Node::PinnedVariableNode` — wire name `"pinned_variable"`.
pub const PINNED_VARIABLE_NODE: &str = "pinned_variable";

/// `ruby_prism::Node::PostExecutionNode` — wire name `"post_execution"`.
pub const POST_EXECUTION_NODE: &str = "post_execution";

/// `ruby_prism::Node::PreExecutionNode` — wire name `"pre_execution"`.
pub const PRE_EXECUTION_NODE: &str = "pre_execution";

/// `ruby_prism::Node::ProgramNode` — wire name `"program"`.
pub const PROGRAM_NODE: &str = "program";

/// `ruby_prism::Node::RangeNode` — wire name `"range"`.
pub const RANGE_NODE: &str = "range";

/// `ruby_prism::Node::RationalNode` — wire name `"rational"`.
pub const RATIONAL_NODE: &str = "rational";

/// `ruby_prism::Node::RedoNode` — wire name `"redo"`.
pub const REDO_NODE: &str = "redo";

/// `ruby_prism::Node::RegularExpressionNode` — wire name `"regular_expression"`.
pub const REGULAR_EXPRESSION_NODE: &str = "regular_expression";

/// `ruby_prism::Node::RequiredKeywordParameterNode` — wire name `"required_keyword_parameter"`.
pub const REQUIRED_KEYWORD_PARAMETER_NODE: &str = "required_keyword_parameter";

/// `ruby_prism::Node::RequiredParameterNode` — wire name `"required_parameter"`.
pub const REQUIRED_PARAMETER_NODE: &str = "required_parameter";

/// `ruby_prism::Node::RescueModifierNode` — wire name `"rescue_modifier"`.
pub const RESCUE_MODIFIER_NODE: &str = "rescue_modifier";

/// `ruby_prism::Node::RescueNode` — wire name `"rescue"`.
pub const RESCUE_NODE: &str = "rescue";

/// `ruby_prism::Node::RestParameterNode` — wire name `"rest_parameter"`.
pub const REST_PARAMETER_NODE: &str = "rest_parameter";

/// `ruby_prism::Node::RetryNode` — wire name `"retry"`.
pub const RETRY_NODE: &str = "retry";

/// `ruby_prism::Node::ReturnNode` — wire name `"return"`.
pub const RETURN_NODE: &str = "return";

/// `ruby_prism::Node::SelfNode` — wire name `"self"`.
pub const SELF_NODE: &str = "self";

/// `ruby_prism::Node::ShareableConstantNode` — wire name `"shareable_constant"`.
pub const SHAREABLE_CONSTANT_NODE: &str = "shareable_constant";

/// `ruby_prism::Node::SingletonClassNode` — wire name `"singleton_class"`.
pub const SINGLETON_CLASS_NODE: &str = "singleton_class";

/// `ruby_prism::Node::SourceEncodingNode` — wire name `"source_encoding"`.
pub const SOURCE_ENCODING_NODE: &str = "source_encoding";

/// `ruby_prism::Node::SourceFileNode` — wire name `"source_file"`.
pub const SOURCE_FILE_NODE: &str = "source_file";

/// `ruby_prism::Node::SourceLineNode` — wire name `"source_line"`.
pub const SOURCE_LINE_NODE: &str = "source_line";

/// `ruby_prism::Node::SplatNode` — wire name `"splat"`.
pub const SPLAT_NODE: &str = "splat";

/// `ruby_prism::Node::StatementsNode` — wire name `"statements"`.
pub const STATEMENTS_NODE: &str = "statements";

/// `ruby_prism::Node::StringNode` — wire name `"string"`.
pub const STRING_NODE: &str = "string";

/// `ruby_prism::Node::SuperNode` — wire name `"super"`.
pub const SUPER_NODE: &str = "super";

/// `ruby_prism::Node::SymbolNode` — wire name `"symbol"`.
pub const SYMBOL_NODE: &str = "symbol";

/// `ruby_prism::Node::TrueNode` — wire name `"true"`.
pub const TRUE_NODE: &str = "true";

/// `ruby_prism::Node::UndefNode` — wire name `"undef"`.
pub const UNDEF_NODE: &str = "undef";

/// `ruby_prism::Node::UnlessNode` — wire name `"unless"`.
pub const UNLESS_NODE: &str = "unless";

/// `ruby_prism::Node::UntilNode` — wire name `"until"`.
pub const UNTIL_NODE: &str = "until";

/// `ruby_prism::Node::WhenNode` — wire name `"when"`.
pub const WHEN_NODE: &str = "when";

/// `ruby_prism::Node::WhileNode` — wire name `"while"`.
pub const WHILE_NODE: &str = "while";

/// `ruby_prism::Node::XStringNode` — wire name `"xstring"`.
pub const X_STRING_NODE: &str = "xstring";

/// `ruby_prism::Node::YieldNode` — wire name `"yield"`.
pub const YIELD_NODE: &str = "yield";

/// Every node-kind wire name, sorted by Rust enum variant name.
///
/// Useful for plugin-side schema sanity checks and for the unit test
/// that guards against drift between this list and ruby-prism's actual
/// `Node` enum.
pub const ALL: &[&str] = &[
    ALIAS_GLOBAL_VARIABLE_NODE,
    ALIAS_METHOD_NODE,
    ALTERNATION_PATTERN_NODE,
    AND_NODE,
    ARGUMENTS_NODE,
    ARRAY_NODE,
    ARRAY_PATTERN_NODE,
    ASSOC_NODE,
    ASSOC_SPLAT_NODE,
    BACK_REFERENCE_READ_NODE,
    BEGIN_NODE,
    BLOCK_ARGUMENT_NODE,
    BLOCK_LOCAL_VARIABLE_NODE,
    BLOCK_NODE,
    BLOCK_PARAMETER_NODE,
    BLOCK_PARAMETERS_NODE,
    BREAK_NODE,
    CALL_AND_WRITE_NODE,
    CALL_NODE,
    CALL_OPERATOR_WRITE_NODE,
    CALL_OR_WRITE_NODE,
    CALL_TARGET_NODE,
    CAPTURE_PATTERN_NODE,
    CASE_MATCH_NODE,
    CASE_NODE,
    CLASS_NODE,
    CLASS_VARIABLE_AND_WRITE_NODE,
    CLASS_VARIABLE_OPERATOR_WRITE_NODE,
    CLASS_VARIABLE_OR_WRITE_NODE,
    CLASS_VARIABLE_READ_NODE,
    CLASS_VARIABLE_TARGET_NODE,
    CLASS_VARIABLE_WRITE_NODE,
    CONSTANT_AND_WRITE_NODE,
    CONSTANT_OPERATOR_WRITE_NODE,
    CONSTANT_OR_WRITE_NODE,
    CONSTANT_PATH_AND_WRITE_NODE,
    CONSTANT_PATH_NODE,
    CONSTANT_PATH_OPERATOR_WRITE_NODE,
    CONSTANT_PATH_OR_WRITE_NODE,
    CONSTANT_PATH_TARGET_NODE,
    CONSTANT_PATH_WRITE_NODE,
    CONSTANT_READ_NODE,
    CONSTANT_TARGET_NODE,
    CONSTANT_WRITE_NODE,
    DEF_NODE,
    DEFINED_NODE,
    ELSE_NODE,
    EMBEDDED_STATEMENTS_NODE,
    EMBEDDED_VARIABLE_NODE,
    ENSURE_NODE,
    FALSE_NODE,
    FIND_PATTERN_NODE,
    FLIP_FLOP_NODE,
    FLOAT_NODE,
    FOR_NODE,
    FORWARDING_ARGUMENTS_NODE,
    FORWARDING_PARAMETER_NODE,
    FORWARDING_SUPER_NODE,
    GLOBAL_VARIABLE_AND_WRITE_NODE,
    GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
    GLOBAL_VARIABLE_OR_WRITE_NODE,
    GLOBAL_VARIABLE_READ_NODE,
    GLOBAL_VARIABLE_TARGET_NODE,
    GLOBAL_VARIABLE_WRITE_NODE,
    HASH_NODE,
    HASH_PATTERN_NODE,
    IF_NODE,
    IMAGINARY_NODE,
    IMPLICIT_NODE,
    IMPLICIT_REST_NODE,
    IN_NODE,
    INDEX_AND_WRITE_NODE,
    INDEX_OPERATOR_WRITE_NODE,
    INDEX_OR_WRITE_NODE,
    INDEX_TARGET_NODE,
    INSTANCE_VARIABLE_AND_WRITE_NODE,
    INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
    INSTANCE_VARIABLE_OR_WRITE_NODE,
    INSTANCE_VARIABLE_READ_NODE,
    INSTANCE_VARIABLE_TARGET_NODE,
    INSTANCE_VARIABLE_WRITE_NODE,
    INTEGER_NODE,
    INTERPOLATED_MATCH_LAST_LINE_NODE,
    INTERPOLATED_REGULAR_EXPRESSION_NODE,
    INTERPOLATED_STRING_NODE,
    INTERPOLATED_SYMBOL_NODE,
    INTERPOLATED_X_STRING_NODE,
    IT_LOCAL_VARIABLE_READ_NODE,
    IT_PARAMETERS_NODE,
    KEYWORD_HASH_NODE,
    KEYWORD_REST_PARAMETER_NODE,
    LAMBDA_NODE,
    LOCAL_VARIABLE_AND_WRITE_NODE,
    LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
    LOCAL_VARIABLE_OR_WRITE_NODE,
    LOCAL_VARIABLE_READ_NODE,
    LOCAL_VARIABLE_TARGET_NODE,
    LOCAL_VARIABLE_WRITE_NODE,
    MATCH_LAST_LINE_NODE,
    MATCH_PREDICATE_NODE,
    MATCH_REQUIRED_NODE,
    MATCH_WRITE_NODE,
    MISSING_NODE,
    MODULE_NODE,
    MULTI_TARGET_NODE,
    MULTI_WRITE_NODE,
    NEXT_NODE,
    NIL_NODE,
    NO_KEYWORDS_PARAMETER_NODE,
    NUMBERED_PARAMETERS_NODE,
    NUMBERED_REFERENCE_READ_NODE,
    OPTIONAL_KEYWORD_PARAMETER_NODE,
    OPTIONAL_PARAMETER_NODE,
    OR_NODE,
    PARAMETERS_NODE,
    PARENTHESES_NODE,
    PINNED_EXPRESSION_NODE,
    PINNED_VARIABLE_NODE,
    POST_EXECUTION_NODE,
    PRE_EXECUTION_NODE,
    PROGRAM_NODE,
    RANGE_NODE,
    RATIONAL_NODE,
    REDO_NODE,
    REGULAR_EXPRESSION_NODE,
    REQUIRED_KEYWORD_PARAMETER_NODE,
    REQUIRED_PARAMETER_NODE,
    RESCUE_MODIFIER_NODE,
    RESCUE_NODE,
    REST_PARAMETER_NODE,
    RETRY_NODE,
    RETURN_NODE,
    SELF_NODE,
    SHAREABLE_CONSTANT_NODE,
    SINGLETON_CLASS_NODE,
    SOURCE_ENCODING_NODE,
    SOURCE_FILE_NODE,
    SOURCE_LINE_NODE,
    SPLAT_NODE,
    STATEMENTS_NODE,
    STRING_NODE,
    SUPER_NODE,
    SYMBOL_NODE,
    TRUE_NODE,
    UNDEF_NODE,
    UNLESS_NODE,
    UNTIL_NODE,
    WHEN_NODE,
    WHILE_NODE,
    X_STRING_NODE,
    YIELD_NODE,
];

/// Total number of ruby-prism `Node` variants this crate knows about.
///
/// The integration test in `tests/prism_kinds_coverage.rs` walks the real
/// `ruby_prism::Node` enum and asserts equality, so a prism upgrade that
/// adds or removes variants will fail the build until this module is
/// regenerated.
pub const COUNT: usize = 151;
