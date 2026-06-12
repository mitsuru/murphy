//! `Lint/Void` — Possible use of operator/literal/variable in void context.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/Void
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Known v1 limitations: (1) `not expr` keyword (NodeKind::Not) is not
//!   dispatched as a Send, so it is not flagged by the operator check. The
//!   Send form `!expr` is correctly handled. (2) `__ENCODING__` / `__LINE__` /
//!   `__FILE__` special keywords are not emitted as Const by the translator,
//!   so the variable/keyword check does not fire on them. (3) Autocorrect for
//!   operator expr in void context replaces the entire Send node with
//!   receiver + newline + first arg, which is semantically equivalent to
//!   RuboCop's operator removal but may produce slightly different whitespace
//!   around adjacent lines. All other RuboCop parity items verified: binary
//!   and unary operators, variables, constants, literals, self, defined?,
//!   lambda/proc/lambda {}/proc {}/Proc.new {}, each-block exclusion, setter
//!   method autocorrect suppression, guard-clause autocorrect suppression,
//!   CheckForMethodsWithNoSideEffects option, ensure body, explicit begin/end,
//!   case/when, case/in, for loop, and ternary expression handling.
//! ```
//!
//! ## Matched shapes
//!
//! - Binary operators (`* / % + - == === != < > <= >= <=>`) in void context.
//! - Unary operators (`+@ -@ ~ !`) in void context.
//! - Variable reads (`var`, `@var`, `@@var`, `$var`) in void context.
//! - Constants in void context.
//! - Literals (`1`, `2.0`, `:test`, `/test/`, `[1]`, `{}`) in void context.
//! - `self` in void context.
//! - `defined?` expression in void context.
//! - Lambda/proc (`-> {}`, `lambda {}`, `proc {}`, `Proc.new {}`) in void context.
//! - Non-mutating methods (`sort`, `collect`, `map`, `flatten`, etc.) in void
//!   context, when `CheckForMethodsWithNoSideEffects: true`.
//!
//! `each` blocks are excluded from void detection to prevent false positives
//! (the expression inside `each` is the filter/predicate, not void).
//!
//! ## Why this shape
//!
//! Mirrors RuboCop's `Lint/Void` which subscribes to `on_begin` / `on_kwbegin`
//! / `on_block` / `on_numblock` / `on_itblock` / `on_ensure`. The core loop
//! walks `Begin`/`Kwbegin` children and checks every expression except the last
//! (or, in a void context like `def` body, every expression). For block bodies
//! that are a single non-begin expression, the expression is checked directly.
//!
//! ## Autocorrect
//!
//! - **Operators**: `a + b` → `a\nb` (replaces the Send with receiver + newline
//!   + first argument). Unary `!b`, `+b`, `~b` → replaced by argument source.
//!     Dot-form `b.!` → replaced by receiver source.
//! - **Variables / constants / literals / self**: removed (with surrounding
//!   whitespace on the left). Autocorrect is suppressed when the expression is
//!   inside an `if`/`case`/`when`/`in_pattern` (guard clause) or inside a
//!   setter method (`def foo=`).
//! - **Non-mutating methods**: method name is replaced with the suggestion
//!   (`sort` → `sort!`, `collect` → `each`).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, Symbol, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct Void;

/// Options for [`Void`].
#[derive(CopOptions)]
pub struct VoidOptions {
    /// Whether to check non-mutating methods like `sort`, `collect`, etc.
    /// when used in void context. Defaults to `false` matching RuboCop.
    #[option(name = "CheckForMethodsWithNoSideEffects", 
        default = false,
        description = "If true, check non-mutating methods used in void context."
    )]
    pub check_for_methods_with_no_side_effects: bool,
}

// ── operator sets (matches RuboCop's Lint/Void) ──────────────────────────────

/// Unary operators flagged in void context (sign and logical).
const UNARY_OPERATORS: &[&str] = &["+@", "-@", "~", "!"];

/// All operators combined — used for the quick method-name check.
const OPERATORS: &[&str] = &[
    "*", "/", "%", "+", "-", "==", "===", "!=", "<", ">", "<=", ">=", "<=>",
    "+@", "-@", "~", "!",
];

/// Non-mutating methods that have a bang (`!`) counterpart.
const NONMUTATING_METHODS_WITH_BANG: &[&str] = &[
    "capitalize", "chomp", "chop", "compact", "delete_prefix", "delete_suffix",
    "downcase", "encode", "flatten", "gsub", "lstrip", "merge", "next",
    "reject", "reverse", "rotate", "rstrip", "scrub", "select", "shuffle",
    "slice", "sort", "sort_by", "squeeze", "strip", "sub", "succ", "swapcase",
    "tr", "tr_s", "transform_values", "unicode_normalize", "uniq", "upcase",
];

/// Methods replaceable by `each` (`collect` / `map`).
const METHODS_REPLACEABLE_BY_EACH: &[&str] = &["collect", "map"];



// ── message constants ───────────────────────────────────────────────────────

const SELF_MSG: &str = "`self` used in void context.";

// ── keywords treated as variables (parity with RuboCop's special_keyword?) ──

const SPECIAL_KEYWORD_VARS: &[&str] = &["__ENCODING__", "__LINE__", "__FILE__"];

#[cop(
    name = "Lint/Void",
    description = "Possible use of operator/literal/variable in void context.",
    default_severity = "warning",
    default_enabled = true,
    options = VoidOptions
)]
impl Void {
    // ── begin / kwbegin handlers ───────────────────────────────────────

    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        let children = match *cx.kind(node) {
            NodeKind::Begin(list) | NodeKind::Kwbegin(list) => cx.list(list).to_vec(),
            _ => return,
        };
        if children.is_empty() {
            return;
        }

        // When a Begin/Kwbegin is directly inside another Begin/Kwbegin, it's
        // transparent grouping (like `(expr)`). All inner children should be
        // checked — the void-context position is determined by the node's
        // position within the outer Begin.
        let parent_is_begin = cx.parent(node).get().is_some_and(|p| {
            matches!(*cx.kind(p), NodeKind::Begin(_) | NodeKind::Kwbegin(_))
        });

        let inside_each_block = is_inside_each_block(node, cx);
        let last_idx = if parent_is_begin {
            transparent_last_idx(node, &children, cx)
        } else {
            let in_void = is_begin_in_void_context(node, cx);
            if !in_void || inside_each_block {
                children.len().saturating_sub(1)
            } else {
                children.len()
            }
        };

        for &child in &children[..last_idx] {
            check_void_op(child, cx);
            check_expression(child, cx);
        }
    }

    #[on_node(kind = "kwbegin")]
    fn check_kwbegin(&self, node: NodeId, cx: &Cx<'_>) {
        let children = match *cx.kind(node) {
            NodeKind::Kwbegin(list) => cx.list(list).to_vec(),
            _ => return,
        };
        if children.is_empty() {
            return;
        }

        let parent_is_begin = cx.parent(node).get().is_some_and(|p| {
            matches!(*cx.kind(p), NodeKind::Begin(_) | NodeKind::Kwbegin(_))
        });

        let inside_each_block = is_inside_each_block(node, cx);
        let last_idx = if parent_is_begin {
            transparent_last_idx(node, &children, cx)
        } else {
            let in_void = is_begin_in_void_context(node, cx);
            if !in_void || inside_each_block {
                children.len().saturating_sub(1)
            } else {
                children.len()
            }
        };

        for &child in &children[..last_idx] {
            check_void_op(child, cx);
            check_expression(child, cx);
        }
    }

    // ── block handlers ─────────────────────────────────────────────────

    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_body(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_numitblock_body(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_numitblock_body(node, cx);
    }

    // ── ensure handler ─────────────────────────────────────────────────

    #[on_node(kind = "ensure")]
    fn check_ensure(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Ensure { ensure_, .. } = *cx.kind(node) else {
            return;
        };
        let Some(body) = ensure_.get() else {
            return;
        };
        // If the ensure body is a Begin, it's already covered by on_begin.
        if matches!(*cx.kind(body), NodeKind::Begin(_) | NodeKind::Kwbegin(_)) {
            return;
        }
        check_void_op(body, cx);
        check_expression(body, cx);
    }
}

// ── begin / kwbegin helpers ─────────────────────────────────────────────────

/// Compute `last_idx` for a transparent Begin/Kwbegin (inside another
/// Begin/Kwbegin). If this begin IS the last child of the parent AND the
/// parent is not a void context, pop this begin's last child (it's the
/// return value of the parent sequence).
fn transparent_last_idx(node: NodeId, children: &[NodeId], cx: &Cx<'_>) -> usize {
    let Some(parent) = cx.parent(node).get() else {
        return children.len();
    };
    let siblings = match *cx.kind(parent) {
        NodeKind::Begin(list) | NodeKind::Kwbegin(list) => cx.list(list),
        _ => return children.len(),
    };
    let is_last_in_parent = siblings.last() == Some(&node);
    // A parent Begin is never a void context — only Def/Block/For/etc are.
    // If this begin is the last child of a non-void parent, its last child
    // is the return value → pop it.
    if is_last_in_parent {
        children.len().saturating_sub(1)
    } else {
        children.len()
    }
}

/// Returns `true` when `begin_id` (a `Begin`/`Kwbegin` node) is the body
/// of a parent whose expressions are evaluated in void context — matching
/// RuboCop's `in_void_context?` for `begin` nodes.
fn is_begin_in_void_context(begin_id: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(begin_id).get() else {
        return false;
    };
    // Check that begin_id IS the body of its parent.
    let is_body = match *cx.kind(parent) {
        NodeKind::Def { body, .. } => body.get() == Some(begin_id),
        NodeKind::Defs { body, .. } => body.get() == Some(begin_id),
        NodeKind::Class { body, .. } => body.get() == Some(begin_id),
        NodeKind::Module { body, .. } => body.get() == Some(begin_id),
        NodeKind::Sclass { body, .. } => body.get() == Some(begin_id),
        NodeKind::For { body, .. } => body.get() == Some(begin_id),
        NodeKind::Ensure { ensure_, .. } => ensure_.get() == Some(begin_id),
        NodeKind::Block { body, .. } => body.get() == Some(begin_id),
        NodeKind::Numblock { body, .. } => body.get() == Some(begin_id),
        NodeKind::Itblock { body, .. } => body.get() == Some(begin_id),
        _ => false,
    };
    if !is_body {
        return false;
    }
    match *cx.kind(parent) {
        // Def is void context only for initialize/setter methods per Murphy's
        // `cx.is_void_context` (matching RuboCop's def.void_context?).
        NodeKind::Def { .. } | NodeKind::Defs { .. } => cx.is_void_context(parent),
        NodeKind::Class { .. } | NodeKind::Module { .. } | NodeKind::Sclass { .. }
        | NodeKind::For { .. } => true,
        // Ensure is NOT a void context — RuboCop's on_ensure handles
        // single-expression bodies directly, and begin bodies inside ensure
        // are handled by on_begin with in_void_context? = false.
        NodeKind::Block { .. } | NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => {
            // Blocks are void context only for `each`/`tap` methods.
            is_each_or_tap_block(parent, cx)
        }
        _ => false,
    }
}

/// Returns `true` when `node` is a `Block`/`Numblock`/`Itblock` whose
/// call method is `each` or `tap`.
fn is_each_or_tap_block(node: NodeId, cx: &Cx<'_>) -> bool {
    cx.method_name(node).is_some_and(|name| name == "each" || name == "tap")
}

/// Returns `true` when `node` has an ancestor block (including numblock
/// or itblock) whose method is `each`.
fn is_inside_each_block(node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        let kind = cx.kind(ancestor);
        let method = match kind {
            NodeKind::Block { call, .. } => match *cx.kind(*call) {
                NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => {
                    cx.symbol_str(method)
                }
                _ => continue,
            },
            NodeKind::Numblock { send, .. } => match *cx.kind(*send) {
                NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => {
                    cx.symbol_str(method)
                }
                _ => continue,
            },
            NodeKind::Itblock { send, .. } => match *cx.kind(*send) {
                NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => {
                    cx.symbol_str(method)
                }
                _ => continue,
            },
            _ => continue,
        };
        if method == "each" {
            return true;
        }
    }
    false
}

// ── block body checks (on_block / on_numblock / on_itblock) ────────────────

/// Handle `#[on_node(kind = "block")]`: check the body if it's a single
/// non-begin expression and the block is not `each`.
fn check_block_body(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Block { call: _, body, .. } = *cx.kind(node) else {
        return;
    };
    let Some(body_id) = body.get() else {
        return;
    };
    // Only check single-expression body (non-Begin/Kwbegin).
    if matches!(*cx.kind(body_id), NodeKind::Begin(_) | NodeKind::Kwbegin(_)) {
        return;
    }
    // Skip if not in void context (checked via the block's call method).
    if !is_each_or_tap_block(node, cx) {
        return;
    }
    // For each/tap blocks, the single body expression is treated differently:
    // RuboCop skips the block body entirely when it's `each`.
    // Actually, `is_each_or_tap_block` already returned true. Let's check:
    // RuboCop's on_block: return if node.method?(:each)
    // So for each blocks, we DON'T check the body.
    let method = cx.method_name(node);
    if method == Some("each") {
        return;
    }
    // For tap blocks (and others that are void context):
    // The single body expression IS checked.
    check_void_op(body_id, cx);
    check_expression(body_id, cx);
}

/// Handle numblock / itblock dispatch.
fn check_numitblock_body(node: NodeId, cx: &Cx<'_>) {
    let body = match *cx.kind(node) {
        NodeKind::Numblock { body, .. } => body,
        NodeKind::Itblock { body, .. } => body,
        _ => return,
    };
    let Some(body_id) = body.get() else {
        return;
    };
    if matches!(*cx.kind(body_id), NodeKind::Begin(_) | NodeKind::Kwbegin(_)) {
        return;
    }
    if !is_each_or_tap_block(node, cx) {
        return;
    }
    let method = cx.method_name(node);
    if method == Some("each") {
        return;
    }
    check_void_op(body_id, cx);
    check_expression(body_id, cx);
}

// ── core check functions ────────────────────────────────────────────────────

/// Dispatch entry for a single expression: checks for if/case/case_match
/// branching or direct void checks.
fn check_expression(expr: NodeId, cx: &Cx<'_>) {
    match *cx.kind(expr) {
        NodeKind::If { then_, else_, .. } => {
            // RuboCop checks both the then-branch (body) and the else-branch
            // of an if/unless/ternary expression. For `x unless cond`, the
            // else-branch holds the expression `x`.
            if let Some(body) = then_.get() {
                check_void_expression_nodes(body, cx);
            }
            if let Some(body) = else_.get() {
                check_void_expression_nodes(body, cx);
            }
        }
        NodeKind::Case { whens, else_, .. } => {
            for when_id in cx.list(whens) {
                if let NodeKind::When { body, .. } = *cx.kind(*when_id)
                    && let Some(body_id) = body.get() {
                        check_expression(body_id, cx);
                    }
            }
            if let Some(else_id) = else_.get() {
                check_expression(else_id, cx);
            }
        }
        NodeKind::CaseMatch { in_patterns, else_body, .. } => {
            for in_id in cx.list(in_patterns) {
                if let NodeKind::InPattern { body, .. } = *cx.kind(*in_id)
                    && let Some(body_id) = body.get() {
                        check_expression(body_id, cx);
                    }
            }
            if let Some(else_id) = else_body.get() {
                check_expression(else_id, cx);
            }
        }
        _ => {
            check_void_expression_nodes(expr, cx);
        }
    }
}

/// Check a non-branching expression for void-ness: literal, variable,
/// self, defined?, lambda/proc, and optionally non-mutating methods.
fn check_void_expression_nodes(expr: NodeId, cx: &Cx<'_>) {
    check_literal(expr, cx);
    check_var(expr, cx);
    check_self(expr, cx);
    check_void_expression(expr, cx);

    let opts = cx.options_or_default::<VoidOptions>();
    if opts.check_for_methods_with_no_side_effects {
        check_nonmutating(expr, cx);
    }
}

// ── specific checks ─────────────────────────────────────────────────────────

/// Check `expr` for void operators.
fn check_void_op(node: NodeId, cx: &Cx<'_>) {
    let (method_id, receiver, args_list) = match *cx.kind(node) {
        NodeKind::Send {
            receiver, method, args, ..
        } => (method, receiver, cx.list(args)),
        NodeKind::Csend {
            receiver, method, args, ..
        } => (method, OptNodeId::from(Some(receiver)), cx.list(args)),
        _ => return,
    };

    let method_str = cx.symbol_str(method_id);

    if !OPERATORS.contains(&method_str) {
        return;
    }

    // For non-unary operators with a dot form and no args: skip
    // (matched as method call, not operator — e.g. `a.+` without arguments).
    if !UNARY_OPERATORS.contains(&method_str) && args_list.is_empty() {
        return;
    }

    // Check for dot form with no args for unary operators —
    // those ARE flagged (e.g. `b.!`, `b.+@`).

    let msg = format!("Operator `{method_str}` used in void context.");
    // Emit on the operator range (method name in source), not the full Send.
    let op_range = method_name_range(node, cx, method_id);
    cx.emit_offense(op_range, &msg, None);

    // Autocorrect.
    if args_list.is_empty() {
        // Dot-form unary: replace whole node with receiver source.
        if let Some(recv) = receiver.get() {
            let recv_src = cx.raw_source(cx.range(recv));
            cx.emit_edit(cx.range(node), recv_src);
        }
    } else {
        // Binary or unary with args (like `!b`, `+b`, `a + b`).
        let mut result = String::new();
        if let Some(recv) = receiver.get() {
            result.push_str(cx.raw_source(cx.range(recv)));
        }
        result.push('\n');
        result.push_str(cx.raw_source(cx.range(args_list[0])));
        cx.emit_edit(cx.range(node), &result);
    }
}

/// Check `expr` for void variables or constants.
fn check_var(node: NodeId, cx: &Cx<'_>) {
    let (name_sym, is_const, is_keyword) = match *cx.kind(node) {
        NodeKind::Lvar(name)
        | NodeKind::Ivar(name)
        | NodeKind::Cvar(name)
        | NodeKind::Gvar(name) => (name, false, false),
        NodeKind::Const { name, .. } => {
            let name_str = cx.symbol_str(name);
            if SPECIAL_KEYWORD_VARS.contains(&name_str) {
                // Treat __ENCODING__, __LINE__, __FILE__ as
                // variables, not constants (RuboCop parity).
                (name, false, true)
            } else {
                (name, true, false)
            }
        }
        _ => return,
    };

    let name_str = cx.symbol_str(name_sym);
    let (msg, range) = if is_const && !is_keyword {
        (format!("Constant `{name_str}` used in void context."), cx.range(node))
    } else {
        let r = cx.range(node);
        let name_range = Range {
            start: r.start,
            end: (r.start + name_str.len() as u32).min(r.end),
        };
        (format!("Variable `{name_str}` used in void context."), name_range)
    };

    cx.emit_offense(range, &msg, None);

    // Autocorrect: use expression-level removal, with guard-clause
    // suppression.
    autocorrect_void_expression(node, cx);
}

/// Check `expr` for void literals.
fn check_literal(node: NodeId, cx: &Cx<'_>) {
    // Exclude nil, xstr, and range_type? per RuboCop's check_literal.
    if matches!(*cx.kind(node), NodeKind::Nil) {
        return;
    }
    if !is_entirely_literal(node, cx) {
        return;
    }
    let src = cx.raw_source(cx.range(node));
    let msg = format!("Literal `{src}` used in void context.");
    cx.emit_offense(cx.range(node), &msg, None);
    autocorrect_void_expression(node, cx);
}

/// Check `expr` for void `self`.
fn check_self(node: NodeId, cx: &Cx<'_>) {
    if !matches!(*cx.kind(node), NodeKind::SelfExpr) {
        return;
    }
    cx.emit_offense(cx.range(node), SELF_MSG, None);
    autocorrect_void_expression(node, cx);
}

/// Check `expr` for void `defined?` or lambda/proc expressions.
fn check_void_expression(node: NodeId, cx: &Cx<'_>) {
    if matches!(*cx.kind(node), NodeKind::Defined(_) | NodeKind::Lambda) {
        let src = cx.raw_source(cx.range(node));
        let msg = format!("`{src}` used in void context.");
        cx.emit_offense(cx.range(node), &msg, None);
        autocorrect_void_expression(node, cx);
        return;
    }

    // Check for lambda/proc blocks: lambda { }, proc { }, Proc.new { }
    if let NodeKind::Block { call, .. }
    | NodeKind::Numblock { send: call, .. }
    | NodeKind::Itblock { send: call, .. } = *cx.kind(node)
        && is_lambda_or_proc_call(call, cx) {
            let src = cx.raw_source(cx.range(node));
            let msg = format!("`{src}` used in void context.");
            cx.emit_offense(cx.range(node), &msg, None);
            autocorrect_void_expression(node, cx);
        }
}

/// Check `expr` for void non-mutating methods (when option enabled).
fn check_nonmutating(node: NodeId, cx: &Cx<'_>) {
    let (method_id, is_block_form) = match *cx.kind(node) {
        NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => (method, false),
        NodeKind::Block { call, .. }
        | NodeKind::Numblock { send: call, .. }
        | NodeKind::Itblock { send: call, .. } => {
            match *cx.kind(call) {
                NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => (method, true),
                _ => return,
            }
        }
        _ => return,
    };

    let method_str = cx.symbol_str(method_id);

    let suggestion: String = if METHODS_REPLACEABLE_BY_EACH.contains(&method_str) {
        "each".to_string()
    } else if NONMUTATING_METHODS_WITH_BANG.contains(&method_str) {
        format!("{}!", method_str)
    } else {
        return;
    };

    let msg = format!("Method `#{method_str}` used in void context. Did you mean `#{suggestion}`?");
    cx.emit_offense(cx.range(node), &msg, None);

    // Autocorrect: replace method name with suggestion.
    let target_id = if is_block_form {
        match *cx.kind(node) {
            NodeKind::Block { call, .. } | NodeKind::Numblock { send: call, .. } => call,
            NodeKind::Itblock { send, .. } => send,
            _ => return,
        }
    } else {
        node
    };
    let method_sym = match *cx.kind(target_id) {
        NodeKind::Send { method, .. } => method,
        NodeKind::Csend { method, .. } => method,
        _ => return,
    };
    let method_range = symbol_range(cx.raw_source(cx.range(target_id)), cx.range(target_id).start, method_sym, cx);
    cx.emit_edit(method_range, &suggestion);
}

// ── helper predicates ───────────────────────────────────────────────────────

/// Returns `true` if `node` is a Send/Csend to `proc`, `lambda`, or
/// `Proc.new`.
fn is_lambda_or_proc_call(call: NodeId, cx: &Cx<'_>) -> bool {
    // Stabby lambda `-> { }` uses a Lambda marker as the call.
    if matches!(*cx.kind(call), NodeKind::Lambda) {
        return true;
    }
    let (method, opt_receiver) = match *cx.kind(call) {
        NodeKind::Send { receiver, method, .. } => (method, receiver),
        NodeKind::Csend { receiver, method, .. } => (method, OptNodeId::from(Some(receiver))),
        _ => return false,
    };
    let method_str = cx.symbol_str(method);
    if (method_str == "proc" || method_str == "lambda") && opt_receiver.get().is_none() {
        return true;
    }
    if method_str == "new"
        && let Some(recv) = opt_receiver.get()
            && let NodeKind::Const { name, .. } = *cx.kind(recv) {
                return cx.symbol_str(name) == "Proc";
            }
    false
}

/// Returns `true` if `node` is an entirely literal expression (all
/// elements/nested elements are literals). Matches RuboCop's
/// `entirely_literal?`.
fn is_entirely_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Nil
        | NodeKind::True_
        | NodeKind::False_
        | NodeKind::Int(_)
        | NodeKind::Float(_)
        | NodeKind::Str(_)
        | NodeKind::Sym(_)
        | NodeKind::Rational(_)
        | NodeKind::Complex(_)
        | NodeKind::Regexp { .. } => true,
        NodeKind::Array(list) => cx.list(list).iter().all(|&child| is_entirely_literal(child, cx)),
        NodeKind::Hash(list) => {
            cx.list(list).iter().all(|&child| {
                if let NodeKind::Pair { key, value } = *cx.kind(child) {
                    is_entirely_literal(key, cx) && is_entirely_literal(value, cx)
                } else {
                    false
                }
            })
        }
        NodeKind::Send { receiver, method, .. } => {
            cx.symbol_str(method) == "freeze"
                && receiver.get().is_some_and(|r| is_entirely_literal(r, cx))
        }
        NodeKind::Csend { receiver, method, .. } => {
            cx.symbol_str(method) == "freeze"
                && is_entirely_literal(receiver, cx)
        }
        _ => false,
    }
}

/// Returns `true` when the expression's parent is an if/case/when/
/// in_pattern — indicating it should not be autocorrected (guard clause).
fn is_in_guard_clause(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    matches!(
        *cx.kind(parent),
        NodeKind::If { .. }
            | NodeKind::Case { .. }
            | NodeKind::When { .. }
            | NodeKind::InPattern { .. }
            | NodeKind::CaseMatch { .. }
    )
}

/// Returns `true` when `node` is inside a setter method (`def foo=`).
fn is_inside_setter_method(node: NodeId, cx: &Cx<'_>) -> bool {
    for ancestor in cx.ancestors(node) {
        if let NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } = *cx.kind(ancestor) {
            let name_str = cx.symbol_str(name);
            if name_str.ends_with('=') && !matches!(name_str.as_bytes(), [b'=']) {
                // Assignment method (ends with = and is not `==`).
                use murphy_plugin_api::method_predicates;
                return method_predicates::is_assignment_method(name_str);
            }
            return false;
        }
    }
    false
}

/// Compute the byte-range of the method / operator name within a Send/Csend.
/// This is used for offense range (pointing at the operator token).
fn method_name_range(node: NodeId, cx: &Cx<'_>, sym: Symbol) -> Range {
    let name = cx.symbol_str(sym);
    let r = cx.range(node);
    let src = cx.raw_source(r);
    // For `a + b`, find `+` between receiver and first arg.
    // For `!b`, find `!` at the start.
    // For `a.+` (dot-call), find `+` after the `.`.
    // For `a&.+` (safe-nav), find `+` after `&.`.
    // Look for the name as a standalone sequence in the source range.
    if let Some(i) = find_method_in_source(src, name) {
        return Range {
            start: r.start + i as u32,
            end: r.start + (i + name.len()) as u32,
        };
    }
    // Fallback: look after the last `.` or `&.` separator.
    if let Some(dot) = src.rfind('.') {
        let method_start = r.start + (dot + 1) as u32;
        return Range {
            start: method_start,
            end: method_start + name.len() as u32,
        };
    }
    // Fallback: method name at node start.
    Range {
        start: r.start,
        end: r.start + name.len() as u32,
    }
}

/// Find the position of `name` in `src`, with special handling for binary
/// operator names (single-char symbols like `*`, `+`, etc.) to avoid matching
/// within the receiver or arguments.
fn find_method_in_source(src: &str, name: &str) -> Option<usize> {
    let bytes = src.as_bytes();
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty() {
        return None;
    }
    // For single-char operators, find the character that appears exactly
    // once in the source (the operator between receiver and args).
    if name_bytes.len() == 1 {
        let ch = name_bytes[0];
        // Find the occurrence that's surrounded by spaces or at start/end.
        // For `a + b`, `+` appears once.
        // For `a.+()`, `.` appears and then `+` appears once.
        let mut count = 0u8;
        let mut last_pos = None;
        for (i, &b) in bytes.iter().enumerate() {
            if b == ch {
                count += 1;
                last_pos = Some(i);
            }
        }
        if count == 1 {
            return last_pos;
        }
        // For multi-occurrence chars (like `-` in `a - -b`), fall through
        // to the general search.
    }
    // General search: find the name that is NOT in the first identifier
    // (receiver) position and NOT after the last `(` (arguments).
    // For safety, just find the last occurrence of name in the source
    // (operators tend to appear after the receiver).
    if let Some(pos) = src.rfind(name) {
        return Some(pos);
    }
    // Fallback: first occurrence.
    src.find(name)
}

/// Compute the byte-range of a method symbol within its Send/Csend source.
fn symbol_range(src: &str, node_start: u32, sym: Symbol, cx: &Cx<'_>) -> Range {
    let name = cx.symbol_str(sym);
    // For `foo.bar(baz)` or `foo.bar`, find the `.bar` or `&.bar` part.
    if let Some(dot) = src.rfind('.') {
        let method_start = node_start + (dot + 1) as u32;
        return Range {
            start: method_start,
            end: method_start + name.len() as u32,
        };
    }
    // Bare method call `foo` — the name is at the start.
    Range {
        start: node_start,
        end: node_start + name.len() as u32,
    }
}

// ── autocorrect helpers ────────────────────────────────────────────────────

/// Emit an edit to remove a void expression, with surrounding whitespace
/// on the left (matching RuboCop's `range_with_surrounding_space(side: :left)`).
/// Suppressed for guard-clause positions and setter methods.
fn autocorrect_void_expression(node: NodeId, cx: &Cx<'_>) {
    // Do not autocorrect when the expression is inside a guard clause
    // (if/case/when/in_pattern) or inside a setter method.
    if is_in_guard_clause(node, cx) || is_inside_setter_method(node, cx) {
        return;
    }

    let range = cx.range(node);
    let source = cx.source();
    let start = range.start as usize;

    // Expand left to include whitespace before the expression, including
    // a preceding newline so the autocorrect does not leave a blank line.
    let mut left = start;
    while left > 0 {
        let b = source.as_bytes()[left - 1];
        if b == b' ' || b == b'\t' {
            left -= 1;
        } else if b == b'\n' {
            left -= 1;
            break;
        } else {
            break;
        }
    }

    cx.emit_edit(
        Range {
            start: left as u32,
            end: range.end,
        },
        "",
    );
}

#[cfg(test)]
mod tests {
    use super::Void;
    use murphy_plugin_api::test_support::{indoc, run_cop, test, run_cop_with_edits};

    // ── binary operators ───────────────────────────────────────────────

    #[test]
    fn flags_void_binary_op() {
        test::<Void>().expect_offense(indoc! {r#"
            a * b
              ^ Operator `*` used in void context.
            top
        "#});
    }

    #[test]
    fn accepts_binary_op_on_last_line() {
        test::<Void>().expect_no_offenses(indoc! {r#"
            top
            a * b
        "#});
    }

    #[test]
    fn accepts_binary_op_alone() {
        test::<Void>().expect_no_offenses("a * b\n");
    }

    #[test]
    fn flags_binary_op_parenthesized() {
        test::<Void>().expect_offense(indoc! {r#"
            (a * b)
               ^ Operator `*` used in void context.
            top
        "#});
    }

    #[test]
    fn accepts_parenthesized_op_on_last_line() {
        test::<Void>().expect_no_offenses(indoc! {r#"
            top
            (a * b)
        "#});
    }

    #[test]
    fn accepts_dot_method_binary_op_no_args() {
        test::<Void>().expect_no_offenses(indoc! {r#"
            a.+
            top
        "#});
    }

    #[test]
    fn flags_dot_method_binary_op_with_args() {
        test::<Void>().expect_offense(indoc! {r#"
            a.+(b)
              ^ Operator `+` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_safe_nav_binary_op_with_args() {
        test::<Void>().expect_offense(indoc! {r#"
            a&.+(b)
               ^ Operator `+` used in void context.
            top
        "#});
    }

    // ── unary operators ────────────────────────────────────────────────

    #[test]
    fn flags_unary_plus() {
        test::<Void>().expect_offense(indoc! {r#"
            +b
            ^^ Operator `+@` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_unary_minus() {
        test::<Void>().expect_offense(indoc! {r#"
            -b
            ^^ Operator `-@` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_unary_not() {
        test::<Void>().expect_offense(indoc! {r#"
            !b
            ^ Operator `!` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_unary_tilde() {
        test::<Void>().expect_offense(indoc! {r#"
            ~b
            ^ Operator `~` used in void context.
            top
        "#});
    }

    #[test]
    fn accepts_unary_op_on_last_line() {
        test::<Void>().expect_no_offenses(indoc! {r#"
            top
            !b
        "#});
    }

    #[test]
    fn flags_unary_op_dot_form() {
        test::<Void>().expect_offense(indoc! {r#"
            b.!
              ^ Operator `!` used in void context.
            b.!
        "#});
    }

    #[test]
    fn flags_unary_op_safe_nav() {
        test::<Void>().expect_offense(indoc! {r#"
            b&.!
               ^ Operator `!` used in void context.
            b&.!
        "#});
    }

    // ── variables ──────────────────────────────────────────────────────

    #[test]
    fn flags_void_local_var() {
        test::<Void>().expect_offense(indoc! {r#"
            x = 5
            x
            ^ Variable `x` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_ivar() {
        test::<Void>().expect_offense(indoc! {r#"
            @x = 5
            @x
            ^^ Variable `@x` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_cvar() {
        test::<Void>().expect_offense(indoc! {r#"
            @@x = 5
            @@x
            ^^^ Variable `@@x` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_gvar() {
        test::<Void>().expect_offense(indoc! {r#"
            $x = 5
            $x
            ^^ Variable `$x` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_var_with_guard() {
        test::<Void>().expect_offense(indoc! {r#"
            x = 5
            x unless condition
            ^ Variable `x` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_var_in_ternary() {
        test::<Void>().expect_offense(indoc! {r#"
            x = 5
            condition ? x : nil
                        ^ Variable `x` used in void context.
            top
        "#});
    }
    // ── constants ──────────────────────────────────────────────────────

    #[test]
    fn flags_void_constant() {
        test::<Void>().expect_offense(indoc! {r#"
            CONST = 5
            CONST
            ^^^^^ Constant `CONST` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_constant_with_guard() {
        test::<Void>().expect_offense(indoc! {r#"
            CONST = 5
            CONST unless condition
            ^^^^^ Constant `CONST` used in void context.
            top
        "#});
    }

    // ── literals ───────────────────────────────────────────────────────

    #[test]
    fn flags_void_int_literal() {
        test::<Void>().expect_offense(indoc! {r#"
            42
            ^^ Literal `42` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_float_literal() {
        test::<Void>().expect_offense(indoc! {r#"
            2.0
            ^^^ Literal `2.0` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_symbol_literal() {
        test::<Void>().expect_offense(indoc! {r#"
            :test
            ^^^^^ Literal `:test` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_regexp_literal() {
        test::<Void>().expect_offense(indoc! {r#"
            /test/
            ^^^^^^ Literal `/test/` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_array_literal_all_literals() {
        test::<Void>().expect_offense(indoc! {r#"
            [1, 2]
            ^^^^^^ Literal `[1, 2]` used in void context.
            top
        "#});
    }

    #[test]
    fn accepts_array_with_non_literals() {
        test::<Void>().expect_no_offenses(indoc! {r#"
            [foo, bar]
            top
        "#});
    }

    #[test]
    fn flags_void_hash_literal_all_literals() {
        test::<Void>().expect_offense(indoc! {r#"
            {a: 1}
            ^^^^^^ Literal `{a: 1}` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_frozen_literal() {
        test::<Void>().expect_offense(indoc! {r#"
            'foo'.freeze
            ^^^^^^^^^^^^ Literal `'foo'.freeze` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_literal_in_method_body() {
        test::<Void>().expect_offense(indoc! {r#"
            def something
              42
              ^^ Literal `42` used in void context.
              top
            end
        "#});
    }

    // ── self ───────────────────────────────────────────────────────────

    #[test]
    fn flags_void_self() {
        test::<Void>().expect_offense(indoc! {r#"
            self; top
            ^^^^ `self` used in void context.
        "#});
    }

    // ── defined? and lambda/proc ───────────────────────────────────────

    #[test]
    fn flags_void_defined() {
        test::<Void>().expect_offense(indoc! {r#"
            defined?(x)
            ^^^^^^^^^^^ `defined?(x)` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_lambda_block() {
        test::<Void>().expect_offense(indoc! {r#"
            def foo
              lambda { bar }
              ^^^^^^^^^^^^^^ `lambda { bar }` used in void context.
              top
            end
        "#});
    }

    #[test]
    fn flags_void_lambda_expr() {
        test::<Void>().expect_offense(indoc! {r#"
            def foo
              -> { bar }
              ^^^^^^^^^^ `-> { bar }` used in void context.
              top
            end
        "#});
    }

    #[test]
    fn flags_void_lambda_expr_short() {
        test::<Void>().expect_offense(indoc! {r#"
            -> { bar }
            ^^^^^^^^^^ `-> { bar }` used in void context.
            top
        "#});
    }

    #[test]
    fn flags_void_proc() {
        test::<Void>().expect_offense(indoc! {r#"
            def foo
              proc { bar }
              ^^^^^^^^^^^^ `proc { bar }` used in void context.
              top
            end
        "#});
    }

    #[test]
    fn accepts_lambda_call_on_line() {
        test::<Void>().expect_no_offenses(indoc! {r#"
            def foo
              -> { bar }.call
              top
            end
        "#});
    }

    // ── each block ─────────────────────────────────────────────────────

    #[test]
    fn accepts_each_block_last_expression() {
        test::<Void>().expect_no_offenses(indoc! {r#"
            array.each do |item|
              item == 42
            end
            top
        "#});
    }

    #[test]
    fn flags_non_last_in_each_block() {
        test::<Void>().expect_offense(indoc! {r#"
            array.each do |item|
              42
              ^^ Literal `42` used in void context.
              item
            end
            top
        "#});
    }

    // ── ensure ─────────────────────────────────────────────────────────

    #[test]
    fn flags_void_literal_in_ensure() {
        test::<Void>().expect_offense(indoc! {r#"
            def foo
              bar
            ensure
              42
              ^^ Literal `42` used in void context.
              42
            end
        "#});
    }

    // ── explicit begin/end ─────────────────────────────────────────────

    #[test]
    fn handles_explicit_begin() {
        test::<Void>().expect_offense(indoc! {r#"
            begin
              1
              ^ Literal `1` used in void context.
              2
            end
        "#});
    }

    // ── setter method (no autocorrect) ─────────────────────────────────

    #[test]
    fn flags_void_literal_in_setter() {
        test::<Void>().expect_offense(indoc! {r#"
            def foo=(rhs)
              42
              ^^ Literal `42` used in void context.
              42
              ^^ Literal `42` used in void context.
            end
        "#});
    }

    // ── CheckForMethodsWithNoSideEffects (off by default) ──────────────

    #[test]
    fn does_not_flag_nonmutating_by_default() {
        test::<Void>().expect_no_offenses(indoc! {r#"
            x.sort
            top(x)
        "#});
    }

    // ── for loop ───────────────────────────────────────────────────────

    #[test]
    fn flags_void_literals_in_for() {
        // For body is a void context in RuboCop/Murphy, so both
        // (non-last and last) expressions are checked.
        let offenses = run_cop::<Void>("for item in array do\n42\n42\nend\ntop\n");
        assert_eq!(offenses.len(), 2, "both 42 literals should be flagged in for");
    }

    // ── autocorrect: operator ─────────────────────────────────────────

    #[test]
    fn corrects_binary_op() {
        test::<Void>().expect_correction(
            indoc! {r#"
                a * b
                  ^ Operator `*` used in void context.
                top
            "#},
            "a\nb\ntop\n",
        );
    }

    #[test]
    fn corrects_void_var() {
        test::<Void>().expect_correction(
            indoc! {r#"
                x = 5
                x
                ^ Variable `x` used in void context.
                top
            "#},
            "x = 5\ntop\n",
        );
    }

    #[test]
    fn does_not_correct_guard_clause_var() {
        // Guard clause (x if condition) → no autocorrect.
        let run = run_cop_with_edits::<Void>("x = 5\nx unless condition\ntop\n");
        assert!(!run.offenses.is_empty(), "should have offense");
        assert_eq!(run.edits.len(), 0, "should have no autocorrect edits");
    }

    #[test]
    fn does_not_correct_void_literal_in_setter() {
        let run = run_cop_with_edits::<Void>("def foo=(rhs)\n42\n42\nend\n");
        assert!(!run.offenses.is_empty(), "should have offenses");
        assert_eq!(
            run.edits.len(),
            0,
            "setter method should have no autocorrect"
        );
    }

    // ── non-literal arrays/hashes are accepted ─────────────────────────

    #[test]
    fn accepts_array_with_non_literal_values_in_method() {
        test::<Void>().expect_no_offenses(indoc! {r#"
            def something
              [foo, bar]
              baz
            end
        "#});
    }

    #[test]
    fn accepts_hash_with_non_literal_values() {
        test::<Void>().expect_no_offenses(indoc! {r#"
            def something
              {k1: foo, k2: bar}
              baz
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(Void);
