//! Cop trait + single-pass visitor dispatch (design §4).
//!
//! A [`Cop`] is a **read-only** rule: it inspects AST nodes and pushes
//! [`Offense`]s into a sink. Cops never mutate the AST — the trait only
//! exposes a `&node` and an offense `sink`, deliberately giving no surface
//! for mutation (design §4 "read-only traversal + text-edit suggestions").
//!
//! [`run_cops`] walks the shared immutable AST **once** ([ADR 0001]: one
//! prism parse, one traversal) and, for every node a hook covers, dispatches
//! to *every* cop. The single pass — not re-walking per cop — is the
//! load-bearing performance property.
//!
//! Phase 1 exposes exactly one visitor hook (`on_call_node`); more hooks are
//! added when a cop needs them (YAGNI).

use crate::Offense;
use crate::parse::Ast;
use ruby_prism::Visit;
use std::time::Instant;

/// Per-file context handed to a cop on each visit.
///
/// Intentionally minimal (YAGNI): the file path is needed for
/// [`Offense::file`], and the source bytes let a real cop (Task 5) compute
/// and slice byte ranges ([ADR 0001]: offsets index into exactly these bytes).
pub struct CopContext<'a> {
    /// Path of the file being linted (for [`Offense::file`]).
    pub file: &'a str,
    /// The source bytes the AST was parsed from (offense byte offsets index
    /// into exactly these bytes).
    pub source: &'a [u8],
}

pub struct CallDispatchRestriction {
    pub method_name: Vec<u8>,
    pub dispatch_id: usize,
}

pub struct NodeDispatchRestriction {
    pub node_kind: Vec<u8>,
    pub dispatch_id: usize,
}

struct RestrictedCallCop<'a> {
    cop: &'a dyn Cop,
    dispatch_id: usize,
}

struct RestrictedNodeCop<'a> {
    cop: &'a dyn Cop,
    dispatch_id: usize,
}

/// A read-only lint rule (design §4).
///
/// A cop inspects nodes and pushes [`Offense`]s into `sink`. It is given an
/// immutable borrow of the node and no means to mutate the tree, by design.
///
/// `Send + Sync` (ADR 0002 phase-2 flag) so a cop can be fanned across OS
/// threads for all-core parallel dispatch (design §3; Task 5 wires the actual
/// parallelism). This is the *minimal* bound — just the two auto-markers, no
/// `'static`/`Clone`/etc. — so a future Phase-3 mruby-backed cop wrapper that
/// moves to a worker thread (ADR 0003) can still satisfy it. Phase-1 cops are
/// stateless unit structs and auto-satisfy it with no impl change.
///
/// Phase-3 trap: a Phase-3 mruby cop satisfies this bound only because the
/// wrapper holds `Send + Sync` data (config, script path, `Arc<AstContext>`);
/// the `mrb_state` is created on the per-cop worker thread (ADR 0003) and MUST
/// NOT be stored in a cop struct field — `mrb_state` is not `Sync`, so storing
/// it would silently break this bound (and the ADR 0002 drop-order rule).
pub trait Cop: Send + Sync {
    /// The cop's name (e.g. used for [`Offense::cop_name`]).
    fn name(&self) -> &str;

    /// Called once per file before AST traversal.
    fn inspect_file(&self, _ctx: &CopContext<'_>, _sink: &mut Vec<Offense>) {}

    /// Called once per file after AST traversal.
    fn after_file(&self, _ctx: &CopContext<'_>, _sink: &mut Vec<Offense>) {}

    /// Called once per call node during the single AST traversal.
    fn on_call_node(
        &self,
        node: &ruby_prism::CallNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
    );

    fn observes_call_nodes(&self) -> bool {
        true
    }

    fn on_restricted_call_node(
        &self,
        node: &ruby_prism::CallNode<'_>,
        ctx: &CopContext<'_>,
        sink: &mut Vec<Offense>,
        _dispatch_id: usize,
    ) {
        self.on_call_node(node, ctx, sink);
    }

    /// Optional RuboCop-style `RESTRICT_ON_SEND` method-name filter. Cops that
    /// return `Some` are dispatched only for matching call names.
    fn restrict_on_send(&self) -> Option<&[CallDispatchRestriction]> {
        None
    }

    fn on_restricted_node(
        &self,
        _node: &ruby_prism::Node<'_>,
        _node_kind: &[u8],
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
        _dispatch_id: usize,
    ) {
    }

    /// Optional RuboCop-style `on_<node_type>` dispatch filter. Node kind names
    /// are Murphy/Prism names such as `class`, `def`, `hash`, `string`, and
    /// `call`; compatibility aliases can be layered on top by pack adapters.
    fn restrict_on_node(&self) -> Option<&[NodeDispatchRestriction]> {
        None
    }

    /// Called once per if/unless node during the single AST traversal.
    fn on_if_node(
        &self,
        _node: &ruby_prism::IfNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }

    /// Called once per return node during the single AST traversal.
    fn on_return_node(
        &self,
        _node: &ruby_prism::ReturnNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }

    /// Called once per case node during the single AST traversal.
    fn on_case_node(
        &self,
        _node: &ruby_prism::CaseNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }

    /// Called once per unless node during the single AST traversal.
    fn on_unless_node(
        &self,
        _node: &ruby_prism::UnlessNode<'_>,
        _ctx: &CopContext<'_>,
        _sink: &mut Vec<Offense>,
    ) {
    }
}

/// Internal visitor that performs the single AST pass and fans every visited
/// node out to every cop.
struct Dispatcher<'a> {
    cops: &'a [&'a dyn Cop],
    unrestricted_cops: Vec<&'a dyn Cop>,
    restricted_call_cops: std::collections::BTreeMap<Vec<u8>, Vec<RestrictedCallCop<'a>>>,
    restricted_node_cops: std::collections::BTreeMap<Vec<u8>, Vec<RestrictedNodeCop<'a>>>,
    ctx: CopContext<'a>,
    sink: &'a mut Vec<Offense>,
}

impl<'pr> Visit<'pr> for Dispatcher<'_> {
    fn visit_branch_node_enter(&mut self, node: ruby_prism::Node<'pr>) {
        self.dispatch_node(&node);
    }

    fn visit_leaf_node_enter(&mut self, node: ruby_prism::Node<'pr>) {
        self.dispatch_node(&node);
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        // Single pass, all cops per node: every cop sees this node before we
        // move on (no re-walking the tree per cop).
        for cop in &self.unrestricted_cops {
            cop.on_call_node(node, &self.ctx, self.sink);
        }
        if let Some(cops) = self.restricted_call_cops.get(node.name().as_slice()) {
            for entry in cops {
                entry
                    .cop
                    .on_restricted_call_node(node, &self.ctx, self.sink, entry.dispatch_id);
            }
        }
        // REQUIRED: descend into nested calls (e.g. `foo.bar(baz)`); without
        // this only top-level calls are visited (see spikes/prism_poc).
        ruby_prism::visit_call_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        for cop in self.cops {
            cop.on_if_node(node, &self.ctx, self.sink);
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_return_node(&mut self, node: &ruby_prism::ReturnNode<'pr>) {
        for cop in self.cops {
            cop.on_return_node(node, &self.ctx, self.sink);
        }
        ruby_prism::visit_return_node(self, node);
    }

    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode<'pr>) {
        for cop in self.cops {
            cop.on_case_node(node, &self.ctx, self.sink);
        }
        ruby_prism::visit_case_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        for cop in self.cops {
            cop.on_unless_node(node, &self.ctx, self.sink);
        }
        ruby_prism::visit_unless_node(self, node);
    }
}

impl Dispatcher<'_> {
    fn dispatch_node(&mut self, node: &ruby_prism::Node<'_>) {
        let node_kind = prism_node_kind(node);
        if let Some(cops) = self.restricted_node_cops.get(node_kind) {
            for entry in cops {
                entry.cop.on_restricted_node(
                    node,
                    node_kind,
                    &self.ctx,
                    self.sink,
                    entry.dispatch_id,
                );
            }
        }
    }
}

pub fn prism_node_kind(node: &ruby_prism::Node<'_>) -> &'static [u8] {
    match node {
        ruby_prism::Node::AliasGlobalVariableNode { .. } => b"alias_global_variable",
        ruby_prism::Node::AliasMethodNode { .. } => b"alias_method",
        ruby_prism::Node::AlternationPatternNode { .. } => b"alternation_pattern",
        ruby_prism::Node::AndNode { .. } => b"and",
        ruby_prism::Node::ArgumentsNode { .. } => b"arguments",
        ruby_prism::Node::ArrayNode { .. } => b"array",
        ruby_prism::Node::ArrayPatternNode { .. } => b"array_pattern",
        ruby_prism::Node::AssocNode { .. } => b"assoc",
        ruby_prism::Node::AssocSplatNode { .. } => b"assoc_splat",
        ruby_prism::Node::BackReferenceReadNode { .. } => b"back_reference_read",
        ruby_prism::Node::BeginNode { .. } => b"begin",
        ruby_prism::Node::BlockArgumentNode { .. } => b"block_argument",
        ruby_prism::Node::BlockLocalVariableNode { .. } => b"block_local_variable",
        ruby_prism::Node::BlockNode { .. } => b"block",
        ruby_prism::Node::BlockParameterNode { .. } => b"block_parameter",
        ruby_prism::Node::BlockParametersNode { .. } => b"block_parameters",
        ruby_prism::Node::BreakNode { .. } => b"break",
        ruby_prism::Node::CallAndWriteNode { .. } => b"call_and_write",
        ruby_prism::Node::CallNode { .. } => b"call",
        ruby_prism::Node::CallOperatorWriteNode { .. } => b"call_operator_write",
        ruby_prism::Node::CallOrWriteNode { .. } => b"call_or_write",
        ruby_prism::Node::CallTargetNode { .. } => b"call_target",
        ruby_prism::Node::CapturePatternNode { .. } => b"capture_pattern",
        ruby_prism::Node::CaseMatchNode { .. } => b"case_match",
        ruby_prism::Node::CaseNode { .. } => b"case",
        ruby_prism::Node::ClassNode { .. } => b"class",
        ruby_prism::Node::ClassVariableAndWriteNode { .. } => b"class_variable_and_write",
        ruby_prism::Node::ClassVariableOperatorWriteNode { .. } => b"class_variable_operator_write",
        ruby_prism::Node::ClassVariableOrWriteNode { .. } => b"class_variable_or_write",
        ruby_prism::Node::ClassVariableReadNode { .. } => b"class_variable_read",
        ruby_prism::Node::ClassVariableTargetNode { .. } => b"class_variable_target",
        ruby_prism::Node::ClassVariableWriteNode { .. } => b"class_variable_write",
        ruby_prism::Node::ConstantAndWriteNode { .. } => b"constant_and_write",
        ruby_prism::Node::ConstantOperatorWriteNode { .. } => b"constant_operator_write",
        ruby_prism::Node::ConstantOrWriteNode { .. } => b"constant_or_write",
        ruby_prism::Node::ConstantPathAndWriteNode { .. } => b"constant_path_and_write",
        ruby_prism::Node::ConstantPathNode { .. } => b"constant_path",
        ruby_prism::Node::ConstantPathOperatorWriteNode { .. } => b"constant_path_operator_write",
        ruby_prism::Node::ConstantPathOrWriteNode { .. } => b"constant_path_or_write",
        ruby_prism::Node::ConstantPathTargetNode { .. } => b"constant_path_target",
        ruby_prism::Node::ConstantPathWriteNode { .. } => b"constant_path_write",
        ruby_prism::Node::ConstantReadNode { .. } => b"constant_read",
        ruby_prism::Node::ConstantTargetNode { .. } => b"constant_target",
        ruby_prism::Node::ConstantWriteNode { .. } => b"constant_write",
        ruby_prism::Node::DefNode { .. } => b"def",
        ruby_prism::Node::DefinedNode { .. } => b"defined",
        ruby_prism::Node::ElseNode { .. } => b"else",
        ruby_prism::Node::EmbeddedStatementsNode { .. } => b"embedded_statements",
        ruby_prism::Node::EmbeddedVariableNode { .. } => b"embedded_variable",
        ruby_prism::Node::EnsureNode { .. } => b"ensure",
        ruby_prism::Node::FalseNode { .. } => b"false",
        ruby_prism::Node::FindPatternNode { .. } => b"find_pattern",
        ruby_prism::Node::FlipFlopNode { .. } => b"flip_flop",
        ruby_prism::Node::FloatNode { .. } => b"float",
        ruby_prism::Node::ForNode { .. } => b"for",
        ruby_prism::Node::ForwardingArgumentsNode { .. } => b"forwarding_arguments",
        ruby_prism::Node::ForwardingParameterNode { .. } => b"forwarding_parameter",
        ruby_prism::Node::ForwardingSuperNode { .. } => b"forwarding_super",
        ruby_prism::Node::GlobalVariableAndWriteNode { .. } => b"global_variable_and_write",
        ruby_prism::Node::GlobalVariableOperatorWriteNode { .. } => {
            b"global_variable_operator_write"
        }
        ruby_prism::Node::GlobalVariableOrWriteNode { .. } => b"global_variable_or_write",
        ruby_prism::Node::GlobalVariableReadNode { .. } => b"global_variable_read",
        ruby_prism::Node::GlobalVariableTargetNode { .. } => b"global_variable_target",
        ruby_prism::Node::GlobalVariableWriteNode { .. } => b"global_variable_write",
        ruby_prism::Node::HashNode { .. } => b"hash",
        ruby_prism::Node::HashPatternNode { .. } => b"hash_pattern",
        ruby_prism::Node::IfNode { .. } => b"if",
        ruby_prism::Node::ImaginaryNode { .. } => b"imaginary",
        ruby_prism::Node::ImplicitNode { .. } => b"implicit",
        ruby_prism::Node::ImplicitRestNode { .. } => b"implicit_rest",
        ruby_prism::Node::InNode { .. } => b"in",
        ruby_prism::Node::IndexAndWriteNode { .. } => b"index_and_write",
        ruby_prism::Node::IndexOperatorWriteNode { .. } => b"index_operator_write",
        ruby_prism::Node::IndexOrWriteNode { .. } => b"index_or_write",
        ruby_prism::Node::IndexTargetNode { .. } => b"index_target",
        ruby_prism::Node::InstanceVariableAndWriteNode { .. } => b"instance_variable_and_write",
        ruby_prism::Node::InstanceVariableOperatorWriteNode { .. } => {
            b"instance_variable_operator_write"
        }
        ruby_prism::Node::InstanceVariableOrWriteNode { .. } => b"instance_variable_or_write",
        ruby_prism::Node::InstanceVariableReadNode { .. } => b"instance_variable_read",
        ruby_prism::Node::InstanceVariableTargetNode { .. } => b"instance_variable_target",
        ruby_prism::Node::InstanceVariableWriteNode { .. } => b"instance_variable_write",
        ruby_prism::Node::IntegerNode { .. } => b"integer",
        ruby_prism::Node::InterpolatedMatchLastLineNode { .. } => b"interpolated_match_last_line",
        ruby_prism::Node::InterpolatedRegularExpressionNode { .. } => {
            b"interpolated_regular_expression"
        }
        ruby_prism::Node::InterpolatedStringNode { .. } => b"interpolated_string",
        ruby_prism::Node::InterpolatedSymbolNode { .. } => b"interpolated_symbol",
        ruby_prism::Node::InterpolatedXStringNode { .. } => b"interpolated_xstring",
        ruby_prism::Node::ItLocalVariableReadNode { .. } => b"it_local_variable_read",
        ruby_prism::Node::ItParametersNode { .. } => b"it_parameters",
        ruby_prism::Node::KeywordHashNode { .. } => b"keyword_hash",
        ruby_prism::Node::KeywordRestParameterNode { .. } => b"keyword_rest_parameter",
        ruby_prism::Node::LambdaNode { .. } => b"lambda",
        ruby_prism::Node::LocalVariableAndWriteNode { .. } => b"local_variable_and_write",
        ruby_prism::Node::LocalVariableOperatorWriteNode { .. } => b"local_variable_operator_write",
        ruby_prism::Node::LocalVariableOrWriteNode { .. } => b"local_variable_or_write",
        ruby_prism::Node::LocalVariableReadNode { .. } => b"local_variable_read",
        ruby_prism::Node::LocalVariableTargetNode { .. } => b"local_variable_target",
        ruby_prism::Node::LocalVariableWriteNode { .. } => b"local_variable_write",
        ruby_prism::Node::MatchLastLineNode { .. } => b"match_last_line",
        ruby_prism::Node::MatchPredicateNode { .. } => b"match_predicate",
        ruby_prism::Node::MatchRequiredNode { .. } => b"match_required",
        ruby_prism::Node::MatchWriteNode { .. } => b"match_write",
        ruby_prism::Node::MissingNode { .. } => b"missing",
        ruby_prism::Node::ModuleNode { .. } => b"module",
        ruby_prism::Node::MultiTargetNode { .. } => b"multi_target",
        ruby_prism::Node::MultiWriteNode { .. } => b"multi_write",
        ruby_prism::Node::NextNode { .. } => b"next",
        ruby_prism::Node::NilNode { .. } => b"nil",
        ruby_prism::Node::NoKeywordsParameterNode { .. } => b"no_keywords_parameter",
        ruby_prism::Node::NumberedParametersNode { .. } => b"numbered_parameters",
        ruby_prism::Node::NumberedReferenceReadNode { .. } => b"numbered_reference_read",
        ruby_prism::Node::OptionalKeywordParameterNode { .. } => b"optional_keyword_parameter",
        ruby_prism::Node::OptionalParameterNode { .. } => b"optional_parameter",
        ruby_prism::Node::OrNode { .. } => b"or",
        ruby_prism::Node::ParametersNode { .. } => b"parameters",
        ruby_prism::Node::ParenthesesNode { .. } => b"parentheses",
        ruby_prism::Node::PinnedExpressionNode { .. } => b"pinned_expression",
        ruby_prism::Node::PinnedVariableNode { .. } => b"pinned_variable",
        ruby_prism::Node::PostExecutionNode { .. } => b"post_execution",
        ruby_prism::Node::PreExecutionNode { .. } => b"pre_execution",
        ruby_prism::Node::ProgramNode { .. } => b"program",
        ruby_prism::Node::RangeNode { .. } => b"range",
        ruby_prism::Node::RationalNode { .. } => b"rational",
        ruby_prism::Node::RedoNode { .. } => b"redo",
        ruby_prism::Node::RegularExpressionNode { .. } => b"regular_expression",
        ruby_prism::Node::RequiredKeywordParameterNode { .. } => b"required_keyword_parameter",
        ruby_prism::Node::RequiredParameterNode { .. } => b"required_parameter",
        ruby_prism::Node::RescueModifierNode { .. } => b"rescue_modifier",
        ruby_prism::Node::RescueNode { .. } => b"rescue",
        ruby_prism::Node::RestParameterNode { .. } => b"rest_parameter",
        ruby_prism::Node::RetryNode { .. } => b"retry",
        ruby_prism::Node::ReturnNode { .. } => b"return",
        ruby_prism::Node::SelfNode { .. } => b"self",
        ruby_prism::Node::ShareableConstantNode { .. } => b"shareable_constant",
        ruby_prism::Node::SingletonClassNode { .. } => b"singleton_class",
        ruby_prism::Node::SourceEncodingNode { .. } => b"source_encoding",
        ruby_prism::Node::SourceFileNode { .. } => b"source_file",
        ruby_prism::Node::SourceLineNode { .. } => b"source_line",
        ruby_prism::Node::SplatNode { .. } => b"splat",
        ruby_prism::Node::StatementsNode { .. } => b"statements",
        ruby_prism::Node::StringNode { .. } => b"string",
        ruby_prism::Node::SuperNode { .. } => b"super",
        ruby_prism::Node::SymbolNode { .. } => b"symbol",
        ruby_prism::Node::TrueNode { .. } => b"true",
        ruby_prism::Node::UndefNode { .. } => b"undef",
        ruby_prism::Node::UnlessNode { .. } => b"unless",
        ruby_prism::Node::UntilNode { .. } => b"until",
        ruby_prism::Node::WhenNode { .. } => b"when",
        ruby_prism::Node::WhileNode { .. } => b"while",
        ruby_prism::Node::XStringNode { .. } => b"xstring",
        ruby_prism::Node::YieldNode { .. } => b"yield",
    }
}

/// Walk `ast` **once** and dispatch every call node to every cop.
///
/// Read-only: cops only push [`Offense`]s into `sink` (design §4).
pub fn run_cops(ast: &Ast<'_>, file: &str, cops: &[Box<dyn Cop>], sink: &mut Vec<Offense>) {
    let cop_refs: Vec<&dyn Cop> = cops.iter().map(|cop| cop.as_ref()).collect();
    run_cops_ref(ast, file, &cop_refs, sink);
}

/// Same dispatch as [`run_cops`], but for explicit cop references.
pub fn run_cop(ast: &Ast<'_>, file: &str, cop: &dyn Cop, sink: &mut Vec<Offense>) {
    run_cops_ref(ast, file, &[cop], sink);
}

/// Timing data for a single cop execution split by phase.
#[derive(Clone, Copy, Debug, Default)]
pub struct CopRunTimings {
    /// `cop.inspect_file` wall time in microseconds.
    pub inspect_file_micros: u64,
    /// AST dispatch (call/if/return/case/unless) wall time in microseconds.
    pub dispatch_micros: u64,
}

/// Same dispatch as [`run_cop`], but split by phase.
///
/// `inspect_file_micros` measures only `inspect_file` execution.
/// `dispatch_micros` measures the AST walk and any dispatch callbacks.
pub fn run_cop_timed(
    ast: &Ast<'_>,
    file: &str,
    cop: &dyn Cop,
    sink: &mut Vec<Offense>,
) -> CopRunTimings {
    let mut unrestricted_cops = Vec::new();
    let mut restricted_call_cops: std::collections::BTreeMap<Vec<u8>, Vec<RestrictedCallCop<'_>>> =
        std::collections::BTreeMap::new();
    let mut restricted_node_cops: std::collections::BTreeMap<Vec<u8>, Vec<RestrictedNodeCop<'_>>> =
        std::collections::BTreeMap::new();

    if let Some(dispatches) = cop.restrict_on_send() {
        for dispatch in dispatches {
            restricted_call_cops
                .entry(dispatch.method_name.clone())
                .or_default()
                .push(RestrictedCallCop {
                    cop,
                    dispatch_id: dispatch.dispatch_id,
                });
        }
    } else if cop.observes_call_nodes() {
        unrestricted_cops.push(cop);
    }
    if let Some(dispatches) = cop.restrict_on_node() {
        for dispatch in dispatches {
            restricted_node_cops
                .entry(dispatch.node_kind.clone())
                .or_default()
                .push(RestrictedNodeCop {
                    cop,
                    dispatch_id: dispatch.dispatch_id,
                });
        }
    }

    let mut dispatcher = Dispatcher {
        cops: &[cop],
        unrestricted_cops,
        restricted_call_cops,
        restricted_node_cops,
        ctx: CopContext {
            file,
            source: ast.source(),
        },
        sink,
    };

    let inspect_file_started = Instant::now();
    for &cop in dispatcher.cops {
        cop.inspect_file(&dispatcher.ctx, dispatcher.sink);
    }
    let inspect_file_micros = duration_micros(inspect_file_started.elapsed());

    let dispatch_started = Instant::now();
    dispatcher.visit(&ast.root());
    for &cop in dispatcher.cops {
        cop.after_file(&dispatcher.ctx, dispatcher.sink);
    }
    let dispatch_micros = duration_micros(dispatch_started.elapsed());

    CopRunTimings {
        inspect_file_micros,
        dispatch_micros,
    }
}

fn duration_micros(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

fn run_cops_ref(ast: &Ast<'_>, file: &str, cops: &[&dyn Cop], sink: &mut Vec<Offense>) {
    let mut unrestricted_cops = Vec::new();
    let mut restricted_call_cops: std::collections::BTreeMap<Vec<u8>, Vec<RestrictedCallCop<'_>>> =
        std::collections::BTreeMap::new();
    let mut restricted_node_cops: std::collections::BTreeMap<Vec<u8>, Vec<RestrictedNodeCop<'_>>> =
        std::collections::BTreeMap::new();
    for &cop in cops {
        if let Some(dispatches) = cop.restrict_on_send() {
            for dispatch in dispatches {
                restricted_call_cops
                    .entry(dispatch.method_name.clone())
                    .or_default()
                    .push(RestrictedCallCop {
                        cop,
                        dispatch_id: dispatch.dispatch_id,
                    });
            }
        } else if cop.observes_call_nodes() {
            unrestricted_cops.push(cop);
        }
        if let Some(dispatches) = cop.restrict_on_node() {
            for dispatch in dispatches {
                restricted_node_cops
                    .entry(dispatch.node_kind.clone())
                    .or_default()
                    .push(RestrictedNodeCop {
                        cop,
                        dispatch_id: dispatch.dispatch_id,
                    });
            }
        }
    }
    let mut dispatcher = Dispatcher {
        cops,
        unrestricted_cops,
        restricted_call_cops,
        restricted_node_cops,
        ctx: CopContext {
            file,
            source: ast.source(),
        },
        sink,
    };
    for &cop in cops {
        cop.inspect_file(&dispatcher.ctx, dispatcher.sink);
    }
    dispatcher.visit(&ast.root());
    for &cop in cops {
        cop.after_file(&dispatcher.ctx, dispatcher.sink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::{Range, Severity};

    /// Test-only cop that pushes one trivial offense per call node. It exists
    /// solely to prove dispatch fires once per call node, for every cop.
    #[derive(Default)]
    struct CountingStubCop;

    impl Cop for CountingStubCop {
        fn name(&self) -> &str {
            "Murphy/CountingStub"
        }

        fn on_call_node(
            &self,
            _node: &ruby_prism::CallNode<'_>,
            _ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            sink.push(Offense {
                file: "t.rb".into(),
                cop_name: self.name().into(),
                range: Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                severity: Severity::Warning,
                message: "stub".into(),
                autocorrect: None,
            });
        }
    }

    #[test]
    fn dispatch_invokes_cop_per_call_node() {
        // ADR 0001 Ruby semantics: bare `foo`, `bar` parse as receiver-less
        // CallNodes, so this source has exactly 2 call nodes.
        let ast = parse("foo; bar\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(CountingStubCop)];
        run_cops(&ast, "t.rb", &cops, &mut sink);
        assert_eq!(sink.len(), 2);
    }

    #[test]
    fn dispatch_fans_every_node_out_to_every_cop() {
        // 2 call nodes (`foo`, `bar`) × 2 cops, one offense each → 4.
        // Fails if multi-cop fan-out regresses to dispatching a single cop.
        let ast = parse("foo; bar\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(CountingStubCop), Box::new(CountingStubCop)];
        run_cops(&ast, "t.rb", &cops, &mut sink);
        assert_eq!(sink.len(), 4);
    }

    struct FileOnlyStubCop;

    impl Cop for FileOnlyStubCop {
        fn name(&self) -> &str {
            "Test/FileOnly"
        }

        fn observes_call_nodes(&self) -> bool {
            false
        }

        fn on_call_node(
            &self,
            _node: &ruby_prism::CallNode<'_>,
            _ctx: &CopContext<'_>,
            _sink: &mut Vec<Offense>,
        ) {
            panic!("file-only cops must not receive call nodes");
        }
    }

    #[test]
    fn dispatch_skips_cops_that_do_not_observe_call_nodes() {
        let ast = parse("foo; bar\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(FileOnlyStubCop)];
        run_cops(&ast, "t.rb", &cops, &mut sink);
        assert!(sink.is_empty());
    }

    /// Test-only stub cop "A": one offense per call node at the selector
    /// (`message_loc`) range. Distinct `name()` from [`StubCopB`] so the
    /// Task-2 total-order `cop_name` tiebreak yields a deterministic
    /// interleave. Never compiled into the binary (`#[cfg(test)]` only).
    #[derive(Default)]
    struct StubCopA;

    impl Cop for StubCopA {
        fn name(&self) -> &str {
            "Test/StubA"
        }

        fn on_call_node(
            &self,
            node: &ruby_prism::CallNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            let Some(loc) = node.message_loc() else {
                return;
            };
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range::from_prism_location(&loc),
                Severity::Warning,
                "stub",
            ));
        }
    }

    /// Test-only stub cop "B": identical shape to [`StubCopA`] but a distinct
    /// `name()`, so two-cop fan-out over a multi-call source produces a fully
    /// deterministic aggregated `Vec` (`Test/StubA` < `Test/StubB`).
    #[derive(Default)]
    struct StubCopB;

    impl Cop for StubCopB {
        fn name(&self) -> &str {
            "Test/StubB"
        }

        fn on_call_node(
            &self,
            node: &ruby_prism::CallNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            let Some(loc) = node.message_loc() else {
                return;
            };
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range::from_prism_location(&loc),
                Severity::Warning,
                "stub",
            ));
        }
    }

    #[test]
    fn two_distinct_cops_dispatch_and_total_order_is_deterministic() {
        // SCOPE: this proves SEQUENTIAL input-order independence only — a
        // single-threaded `run_cops` whose output is made deterministic by
        // `aggregate`'s Task-2 total order. It does NOT exercise Task 5's
        // scenario (offenses from multiple files merged across rayon threads),
        // so it is NOT evidence of parallel-dispatch determinism. Task 5
        // (murphy-aom) MUST add its own parallel-dispatch determinism test;
        // this test does not cover that.
        //
        // `foo; bar\n`: `foo` selector = bytes 0..3, `bar` selector = 5..8
        // (ADR 0001 bare-identifier CallNodes). 2 cops × 2 nodes = 4
        // offenses; `aggregate`'s Task-2 total order
        // `(file, start, end, cop_name, message, severity)` makes the
        // combined Vec fully deterministic: per offset, `Test/StubA` sorts
        // before `Test/StubB`.
        let src = "foo; bar\n";
        let ast = parse(src).unwrap();
        let mut sink = Vec::new();
        // ADR 0002 phase-2 flag: `Cop` is `Send + Sync` so cops can be
        // fanned across OS threads (Task 5 parallel dispatch). This static
        // assertion fails to compile until the supertrait bound is added.
        fn assert_send_sync<T: Send + Sync + ?Sized>() {}
        assert_send_sync::<dyn Cop>();

        // Constructed directly (not `::default()`) to match this file's
        // existing `CountingStubCop` convention and clippy's
        // `default_constructed_unit_structs` lint; `#[derive(Default)]` on
        // each stub still satisfies the ADR 0002 forward-flag requirement.
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(StubCopA), Box::new(StubCopB)];
        run_cops(&ast, "t.rb", &cops, &mut sink);
        let out = crate::aggregate(sink);

        let foo = Range {
            start_offset: 0,
            end_offset: 3,
        };
        let bar = Range {
            start_offset: 5,
            end_offset: 8,
        };
        let expected = vec![
            Offense::new("t.rb", "Test/StubA", foo, Severity::Warning, "stub"),
            Offense::new("t.rb", "Test/StubB", foo, Severity::Warning, "stub"),
            Offense::new("t.rb", "Test/StubA", bar, Severity::Warning, "stub"),
            Offense::new("t.rb", "Test/StubB", bar, Severity::Warning, "stub"),
        ];
        assert_eq!(out, expected);
    }

    #[test]
    fn dispatch_recurses_into_nested_calls() {
        // `foo(bar(baz))` is 3 nested CallNodes; all must be visited.
        // Fails if the `ruby_prism::visit_call_node` recurse line is removed.
        let ast = parse("foo(bar(baz))\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(CountingStubCop)];
        run_cops(&ast, "t.rb", &cops, &mut sink);
        assert_eq!(sink.len(), 3);
    }

    struct NodeKindStubCop {
        dispatches: Vec<NodeDispatchRestriction>,
    }

    impl Cop for NodeKindStubCop {
        fn name(&self) -> &str {
            "Test/NodeKind"
        }

        fn observes_call_nodes(&self) -> bool {
            false
        }

        fn on_call_node(
            &self,
            _node: &ruby_prism::CallNode<'_>,
            _ctx: &CopContext<'_>,
            _sink: &mut Vec<Offense>,
        ) {
        }

        fn restrict_on_node(&self) -> Option<&[NodeDispatchRestriction]> {
            Some(&self.dispatches)
        }

        fn on_restricted_node(
            &self,
            node: &ruby_prism::Node<'_>,
            node_kind: &[u8],
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
            dispatch_id: usize,
        ) {
            let message = format!("{}:{dispatch_id}", String::from_utf8_lossy(node_kind));
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range::from_prism_location(&node.location()),
                Severity::Warning,
                &message,
            ));
        }
    }

    #[test]
    fn dispatch_invokes_restricted_node_hooks_for_prism_node_kinds() {
        let ast = parse("class User\n  def name\n    { name: \"x\" }\n  end\nend\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(NodeKindStubCop {
            dispatches: vec![
                NodeDispatchRestriction {
                    node_kind: b"class".to_vec(),
                    dispatch_id: 1,
                },
                NodeDispatchRestriction {
                    node_kind: b"def".to_vec(),
                    dispatch_id: 2,
                },
                NodeDispatchRestriction {
                    node_kind: b"hash".to_vec(),
                    dispatch_id: 3,
                },
                NodeDispatchRestriction {
                    node_kind: b"string".to_vec(),
                    dispatch_id: 4,
                },
            ],
        })];

        run_cops(&ast, "t.rb", &cops, &mut sink);

        let messages = sink
            .iter()
            .map(|offense| offense.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(messages, ["class:1", "def:2", "hash:3", "string:4"]);
    }

    #[derive(Default)]
    struct FileHookStubCop;

    impl Cop for FileHookStubCop {
        fn name(&self) -> &str {
            "Test/FileHook"
        }

        fn on_call_node(
            &self,
            _node: &ruby_prism::CallNode<'_>,
            _ctx: &CopContext<'_>,
            _sink: &mut Vec<Offense>,
        ) {
        }

        fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: ctx.source.len() as u32,
                },
                Severity::Warning,
                "file hook",
            ));
        }
    }

    #[test]
    fn dispatch_invokes_file_hook_once_per_cop_before_ast_walk() {
        let ast = parse("foo\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(FileHookStubCop), Box::new(CountingStubCop)];

        run_cops(&ast, "t.rb", &cops, &mut sink);

        assert_eq!(sink.len(), 2);
        assert_eq!(sink[0].cop_name, "Test/FileHook");
        assert_eq!(sink[1].cop_name, "Murphy/CountingStub");
    }

    #[derive(Default)]
    struct LifecycleHookStubCop;

    impl Cop for LifecycleHookStubCop {
        fn name(&self) -> &str {
            "Test/LifecycleHook"
        }

        fn inspect_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Warning,
                "before",
            ));
        }

        fn on_call_node(
            &self,
            _node: &ruby_prism::CallNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Warning,
                "during",
            ));
        }

        fn after_file(&self, ctx: &CopContext<'_>, sink: &mut Vec<Offense>) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Warning,
                "after",
            ));
        }
    }

    #[test]
    fn dispatch_invokes_lifecycle_hooks_around_ast_walk() {
        let ast = parse("foo\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(LifecycleHookStubCop)];

        run_cops(&ast, "t.rb", &cops, &mut sink);

        let messages = sink
            .iter()
            .map(|offense| offense.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(messages, ["before", "during", "after"]);
    }

    #[derive(Default)]
    struct IfHookStubCop;

    impl Cop for IfHookStubCop {
        fn name(&self) -> &str {
            "Test/IfHook"
        }

        fn on_call_node(
            &self,
            _node: &ruby_prism::CallNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Warning,
                "call hook",
            ));
        }

        fn on_if_node(
            &self,
            _node: &ruby_prism::IfNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Warning,
                "if hook",
            ));
        }
    }

    #[test]
    fn dispatch_invokes_if_hook_for_if_nodes() {
        let ast = parse("if foo\nbar\nend\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(IfHookStubCop)];

        run_cops(&ast, "t.rb", &cops, &mut sink);

        assert_eq!(sink.len(), 3);
        assert_eq!(sink[0].cop_name, "Test/IfHook");
        assert_eq!(sink[0].message, "if hook");
        assert_eq!(sink[1].message, "call hook");
        assert_eq!(sink[2].message, "call hook");
    }

    #[derive(Default)]
    struct UnlessHookStubCop;

    impl Cop for UnlessHookStubCop {
        fn name(&self) -> &str {
            "Test/UnlessHook"
        }

        fn on_call_node(
            &self,
            _node: &ruby_prism::CallNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Warning,
                "call hook",
            ));
        }

        fn on_unless_node(
            &self,
            _node: &ruby_prism::UnlessNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Warning,
                "unless hook",
            ));
        }
    }

    #[test]
    fn dispatch_invokes_unless_hook_for_unless_nodes() {
        let ast = parse("unless foo\nbar\nend\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(UnlessHookStubCop)];

        run_cops(&ast, "t.rb", &cops, &mut sink);

        assert_eq!(sink.len(), 3);
        assert_eq!(sink[0].cop_name, "Test/UnlessHook");
        assert_eq!(sink[0].message, "unless hook");
        assert_eq!(sink[1].message, "call hook");
        assert_eq!(sink[2].message, "call hook");
    }

    #[derive(Default)]
    struct ReturnHookStubCop;

    impl Cop for ReturnHookStubCop {
        fn name(&self) -> &str {
            "Test/ReturnHook"
        }

        fn on_call_node(
            &self,
            _node: &ruby_prism::CallNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Warning,
                "call hook",
            ));
        }

        fn on_return_node(
            &self,
            _node: &ruby_prism::ReturnNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Warning,
                "return hook",
            ));
        }
    }

    #[test]
    fn dispatch_invokes_return_hook_for_return_nodes() {
        let ast = parse("def m\nreturn foo\nend\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(ReturnHookStubCop)];

        run_cops(&ast, "t.rb", &cops, &mut sink);

        assert_eq!(sink.len(), 2);
        assert_eq!(sink[0].cop_name, "Test/ReturnHook");
        assert_eq!(sink[0].message, "return hook");
        assert_eq!(sink[1].message, "call hook");
    }

    #[derive(Default)]
    struct CaseHookStubCop;

    impl Cop for CaseHookStubCop {
        fn name(&self) -> &str {
            "Test/CaseHook"
        }

        fn on_call_node(
            &self,
            _node: &ruby_prism::CallNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Warning,
                "call hook",
            ));
        }

        fn on_case_node(
            &self,
            _node: &ruby_prism::CaseNode<'_>,
            ctx: &CopContext<'_>,
            sink: &mut Vec<Offense>,
        ) {
            sink.push(Offense::new(
                ctx.file,
                self.name(),
                Range {
                    start_offset: 0,
                    end_offset: 0,
                },
                Severity::Warning,
                "case hook",
            ));
        }
    }

    #[test]
    fn dispatch_invokes_case_hook_for_case_nodes() {
        let ast = parse("case foo\nwhen 1\nbar\nend\n").unwrap();
        let mut sink = Vec::new();
        let cops: Vec<Box<dyn Cop>> = vec![Box::new(CaseHookStubCop)];

        run_cops(&ast, "t.rb", &cops, &mut sink);

        assert_eq!(sink.len(), 3);
        assert_eq!(sink[0].cop_name, "Test/CaseHook");
        assert_eq!(sink[0].message, "case hook");
        assert_eq!(sink[1].message, "call hook");
        assert_eq!(sink[2].message, "call hook");
    }
}
