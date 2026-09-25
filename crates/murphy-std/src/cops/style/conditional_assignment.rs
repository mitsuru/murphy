//! `Style/ConditionalAssignment` — use return value of conditional for assignment.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ConditionalAssignment
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Both `EnforcedStyle` directions are implemented, including the
//!   `on_send` comparison form and `masgn` (assign_inside).
//!
//!   `assign_to_condition` (default, `on_if`/`on_case`/`on_case_match`): flags
//!   `if`/`elsif`/`else`, `case`/`when`+`else`, and `case`/`in`+`else` where
//!   every branch's tail assigns to the same target with the same assignment
//!   node type. Recognised assignment kinds: `lvasgn`/`ivasgn`/`gvasgn`/
//!   `cvasgn`/`casgn`, `op_asgn`/`or_asgn`/`and_asgn`, and comparison-form
//!   `send` (`<<`, `=~`, `!~`, `<=>`, `<`, `>`, `[]=`, setter `foo.bar = …`,
//!   plus any method ending in `=` such as `==`/`<=`/`>=`/`!=`, matching
//!   upstream's `(send _recv {:[]= :<< :=~ :!~ :<=> #end_with_eq? :< :>} ...)`
//!   pattern). `masgn` tails are excluded, matching upstream's
//!   `statements.none?(&:masgn_type?)`. Mixed types (`bar = 1`/`bar += 2`)
//!   are not flagged (`assignment_types_match?`). Single-expression
//!   parentheses are unwrapped (Murphy unwraps fully; upstream unwraps one
//!   level and suppresses single-paren branches under
//!   `SingleLineConditionsOnly` — Murphy flags `(bar = 1)` where upstream
//!   would not; intentional divergence, documented here).
//!   Ternary (`a ? b = 1 : b = 2`, including `bar <<`/`==` forms) is detected,
//!   gated by `IncludeTernaryExpressions` (default true).
//!   `SingleLineConditionsOnly` (default true) suppresses multi-statement
//!   branches, matching upstream. `correction_exceeds_line_limit?` is modelled
//!   via `Cx::max_line_length()` (assumes `Layout/LineLength` enabled, which
//!   is RuboCop's default) — long rewrites are suppressed.
//!
//!   `assign_inside_condition` (`on_lvasgn`/`ivasgn`/`gvasgn`/`cvasgn`/`casgn`
//!   plus `on_op_asgn`/`or_asgn`/`and_asgn`, `on_masgn`, and comparison-form
//!   `on_send`): flags `bar = if foo … end` / `bar += if … end` /
//!   `bar << if … end` / `bar[0] = if … end` / `a, b = if … end` / `case` /
//!   `case`/`in` where the RHS is a conditional (not an allowed ternary),
//!   with the offense on the assignment. Honours `return unless else_branch`
//!   (so `x = if foo; 1; end` is not flagged) and `SingleLineConditionsOnly`
//!   (multi-statement branches suppress the offense by default), matching
//!   upstream. `mlhs`/`resbody` children and `for`-loop pseudo-assignments are
//!   skipped (`assignment_rhs_exist?` + `for` guard). Nested
//!   `assign_inside_condition` follows upstream's `ignore_node` semantics:
//!   a non-`send` assignment inside another assign-inside candidate is
//!   suppressed (outer fires once); `send` candidates never suppress inner
//!   `send`s, matching upstream's `on_send` (which skips
//!   `part_of_ignored_node?`), so `x = if … y << if … end … end` reports twice.
//!
//!   Autocorrect is implemented both directions via whole-node replacement:
//!   assign-to-condition moves the assignment outside (`bar = if … end`,
//!   `bar << if … end`); assign-inside moves it into each branch
//!   (`if … bar = 1 … end`). Ternary element assignments (`bar <<`, setters,
//!   comparisons except `[]=`) are parenthesised (`bar << (c ? 1 : 2)`) to
//!   preserve precedence, matching upstream. `end`-keyword re-indentation
//!   (`Layout/EndAlignment` `indent`) is not applied — Murphy keeps the
//!   original `end` column; upstream pads `end` when `EndAlignment`
//!   `AlignWith: keyword`. The offense range is the conditional/assignment's
//!   first line (Murphy house style) rather than upstream's whole-node range.
//! ```

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, Range, cop};

const MSG: &str = "Use the return of the conditional for variable assignment and comparison.";
const ASSIGN_INSIDE_MSG: &str = "Assign variables inside of conditionals.";

#[derive(Default)]
pub struct ConditionalAssignment;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "assign_to_condition")]
    AssignToCondition,
    #[option(value = "assign_inside_condition")]
    AssignInsideCondition,
}

#[derive(CopOptions)]
pub struct ConditionalAssignmentOptions {
    #[option(
        name = "EnforcedStyle",
        default = "assign_to_condition",
        description = "Enforced style for conditional assignment."
    )]
    pub enforced_style: EnforcedStyle,
    #[option(
        name = "SingleLineConditionsOnly",
        default = true,
        description = "Whether to only flag conditionals whose branches are single statements."
    )]
    pub single_line_conditions_only: bool,
    #[option(
        name = "IncludeTernaryExpressions",
        default = true,
        description = "Whether to include ternary expressions in the check."
    )]
    pub include_ternary_expressions: bool,
}

#[cop(
    name = "Style/ConditionalAssignment",
    description = "Use the return of conditional for variable assignment.",
    default_severity = "warning",
    default_enabled = true,
    options = ConditionalAssignmentOptions,
    safe_autocorrect = true,
)]
impl ConditionalAssignment {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ConditionalAssignmentOptions>();
        if opts.enforced_style != EnforcedStyle::AssignToCondition {
            return;
        }
        // `return if node.elsif?` — the chain is checked from its outermost `if`.
        if cx.is_elsif(node) {
            return;
        }
        // `return if allowed_ternary?(node)`.
        if cx.is_ternary(node) && !opts.include_ternary_expressions {
            return;
        }
        let Some(branches) = collect_branches(node, cx) else {
            return;
        };
        check_assign_to(node, &branches, &opts, cx);
    }

    #[on_node(kind = "case")]
    fn check_case(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ConditionalAssignmentOptions>();
        if opts.enforced_style != EnforcedStyle::AssignToCondition {
            return;
        }
        let Some(branches) = collect_branches(node, cx) else {
            return;
        };
        check_assign_to(node, &branches, &opts, cx);
    }

    #[on_node(kind = "case_match")]
    fn check_case_match(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ConditionalAssignmentOptions>();
        if opts.enforced_style != EnforcedStyle::AssignToCondition {
            return;
        }
        let Some(branches) = collect_branches(node, cx) else {
            return;
        };
        check_assign_to(node, &branches, &opts, cx);
    }

    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "casgn")]
    fn check_casgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    // Upstream aliases `on_op_asgn`/`on_or_asgn`/`on_and_asgn` to the same
    // handler, so `x += if … end` / `x ||= case … end` are reported on the
    // shorthand-assign node itself. The inner `*vasgn` write target is skipped
    // by the `shorthand_asgn?` guard in `check_assign_inside`, so the offense
    // fires exactly once, on the op-assign node.
    #[on_node(kind = "op_asgn")]
    fn check_op_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "or_asgn")]
    fn check_or_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "and_asgn")]
    fn check_and_asgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    #[on_node(kind = "masgn")]
    fn check_masgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }

    // Upstream `on_send` (comparison form only). Unlike the `ASSIGNMENT_TYPES`
    // loop, it does NOT check `part_of_ignored_node?`, so nested sends
    // double-fire — replicated here by skipping the ancestor suppression for
    // sends (see `check_assign_inside`).
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_comparison_send(node, cx) {
            return;
        }
        self.check_assign_inside(node, cx);
    }

    // `foo[i] = rhs` bracket assignment. Murphy's translator currently emits
    // `send :[]=` for this shape, but the `indexasgn` variant exists for
    // forward-compatibility — handle it like the `[]=` send form.
    #[on_node(kind = "indexasgn")]
    fn check_indexasgn(&self, node: NodeId, cx: &Cx<'_>) {
        self.check_assign_inside(node, cx);
    }
}

impl ConditionalAssignment {
    /// `EnforcedStyle: assign_inside_condition` — flag `bar = if foo … end`
    /// where the RHS is a conditional that is not an allowed ternary.
    fn check_assign_inside(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<ConditionalAssignmentOptions>();
        if opts.enforced_style != EnforcedStyle::AssignInsideCondition {
            return;
        }
        // Upstream `on_send` skips `part_of_ignored_node?`; all other
        // assignment kinds suppress when nested inside another candidate.
        // `is_comparison_send` / `indexasgn` are the send family here.
        let is_send_family = matches!(cx.kind(node), NodeKind::Send { .. } | NodeKind::IndexAsgn { .. });
        if !is_send_family && is_part_of_ignored(node, &opts, cx) {
            return;
        }
        // `return if node.parent&.shorthand_asgn?` — the value-less write
        // target of an `op_asgn`/`or_asgn`/`and_asgn` is itself a `*vasgn`
        // node; skip it so `x += if … end` is reported once, on the op-asgn.
        if let Some(parent) = cx.parent(node).get()
            && matches!(
                cx.kind(parent),
                NodeKind::OpAsgn { .. } | NodeKind::OrAsgn { .. } | NodeKind::AndAsgn { .. }
            )
        {
            return;
        }
        // `assignment_rhs_exist?`: a child of `mlhs` (e.g. `a` in `a, b = …`)
        // or `resbody` (`rescue => e`) is a pseudo-assignment without its own
        // RHS — never a candidate.
        if let Some(parent) = cx.parent(node).get()
            && matches!(cx.kind(parent), NodeKind::Mlhs(_) | NodeKind::Resbody { .. })
        {
            return;
        }
        // Ignore pseudo-assignments without rhs in `for` nodes
        // (`for x in …` binds `x` without a value).
        if let Some(parent) = cx.parent(node).get()
            && matches!(cx.kind(parent), NodeKind::For { .. })
        {
            return;
        }
        // The RHS conditional plus the original RHS node (for lhs slicing).
        let Some((rhs_original, rhs_unwrapped)) = assign_inside_rhs(node, cx) else {
            return;
        };
        let rhs = unwrap_single_begin(rhs_unwrapped, cx);
        // `candidate_condition?`: must be `if`/`case`/`case_match`, excluding a
        // ternary when `IncludeTernaryExpressions` is off.
        if !is_candidate_condition(rhs, &opts, cx) {
            return;
        }
        // Upstream `check_assignment_to_condition`:
        //   `_condition, *branches, else_branch = *assignment`
        //   `return unless else_branch`
        //   `return if allowed_single_line?([*branches, else_branch])`
        // `collect_branches` returns `None` when there is no final `else`
        // (e.g. `x = if foo; 1; end`), which RuboCop never flags.
        let Some(branches) = collect_branches(rhs, cx) else {
            return;
        };
        // `allowed_single_line?`: under `SingleLineConditionsOnly` (default
        // true), a multi-statement branch suppresses the offense.
        if opts.single_line_conditions_only
            && branches
                .iter()
                .any(|&b| matches!(multi_statement_tail(b, cx), MultiStatement::Begin(_)))
        {
            return;
        }
        cx.emit_offense(
            first_line_range(cx.range(node), cx.source()),
            ASSIGN_INSIDE_MSG,
            None,
        );
        emit_assign_inside_correction(node, rhs_original, rhs, &branches, cx);
    }
}

/// `EnforcedStyle: assign_to_condition` — flag a conditional whose branches
/// all assign to the same target, then rewrite it to move the assignment out.
fn check_assign_to(
    node: NodeId,
    branches: &[NodeId],
    opts: &ConditionalAssignmentOptions,
    cx: &Cx<'_>,
) {
    // `allowed_ternary?` for the `if` case is checked by the caller; `case`
    // nodes are never ternary. `allowed_statements?` + `allowed_single_line?`
    // + `correction_exceeds_line_limit?` mirror upstream `check_node`.
    if cx.is_ternary(node) && !opts.include_ternary_expressions {
        return;
    }
    let Some(info) = assign_to_info(branches, opts, cx) else {
        return;
    };
    if correction_exceeds_line_limit(node, &info.lhs, cx) {
        return;
    }
    cx.emit_offense(first_line_range(cx.range(node), cx.source()), MSG, None);
    emit_assign_to_correction(node, &info, cx);
}

/// The per-branch assignment details for an `assign_to_condition` offense:
/// the normalised lhs plus each branch tail and its RHS value.
struct AssignToInfo {
    lhs: String,
    tails: Vec<(NodeId, NodeId)>,
    /// True when the first tail is an element (non-`[]=`) send — the ternary
    /// rewrite must parenthesise to preserve precedence.
    element_assignment: bool,
}

/// All branch tails assign to the same target with the same assignment node
/// type, none is `masgn`, and (under `SingleLineConditionsOnly`) none is a
/// multi-statement begin. Returns the tails + RHS nodes for correction.
/// Mirrors upstream `allowed_statements?` + `allowed_single_line?`.
fn assign_to_info(
    branches: &[NodeId],
    opts: &ConditionalAssignmentOptions,
    cx: &Cx<'_>,
) -> Option<AssignToInfo> {
    let mut first: Option<(String, u8)> = None;
    let mut tails: Vec<(NodeId, NodeId)> = Vec::with_capacity(branches.len());
    for &branch in branches {
        // `allowed_single_line?`: SingleLineConditionsOnly + a multi-statement
        // begin branch → not flagged. `tail` of a single-expr begin is that
        // expr; otherwise the whole branch.
        let tail = match multi_statement_tail(branch, cx) {
            MultiStatement::Single(id) => id,
            MultiStatement::Begin(_) => {
                if opts.single_line_conditions_only {
                    return None;
                }
                // Under SingleLineConditionsOnly: false the tail of a
                // multi-statement branch is its last statement. `tail` in
                // upstream returns `Array(branch).last` without unwrapping a
                // parenthesised last statement (see
                // `accepts_parenthesized_tail_in_multi_statement_branch`).
                match *cx.kind(branch) {
                    NodeKind::Begin(list) => match cx.list(list) {
                        [.., last] => *last,
                        [] => return None,
                    },
                    _ => return None,
                }
            }
        };
        let (lhs, tag) = assignment_target(tail, cx)?;
        // Upstream `allowed_statements?`: `statements.none?(&:masgn_type?)`.
        // `assignment_target` returns `None` for `masgn`, so masgn tails
        // already fall out here; the explicit guard documents the parity.
        if matches!(cx.kind(tail), NodeKind::Masgn { .. }) {
            return None;
        }
        let rhs = assignment_rhs(tail, cx)?;
        match &first {
            None => first = Some((lhs, tag)),
            Some((flhs, ftag)) => {
                if *flhs != lhs || *ftag != tag {
                    return None;
                }
            }
        }
        tails.push((tail, rhs));
    }
    let (lhs, _) = first?;
    if tails.is_empty() {
        return None;
    }
    // Ternary element assignment needs parens (`bar << (c ? 1 : 2)`).
    let element_assignment = match *cx.kind(tails[0].0) {
        NodeKind::Send { method, .. } => cx.symbol_str(method) != "[]=",
        NodeKind::IndexAsgn { .. } => false,
        _ => false,
    };
    Some(AssignToInfo {
        lhs,
        tails,
        element_assignment,
    })
}

/// A conditional that `assign_inside_condition` should move the assignment
/// into: `if`/`case`/`case_match`, excluding a ternary when
/// `IncludeTernaryExpressions` is off. Mirrors upstream `candidate_condition?`.
fn is_candidate_condition(node: NodeId, opts: &ConditionalAssignmentOptions, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::If { .. } => opts.include_ternary_expressions || !cx.is_ternary(node),
        NodeKind::Case { .. } | NodeKind::CaseMatch { .. } => true,
        _ => false,
    }
}

/// Collect the body of every branch of an `if`/`case`/`case_match` conditional:
/// the `then`, each `elsif`, and the final `else` for `if`; each `when`/`in`
/// body and the `else` for `case`/`case_match`. Returns `None` when there is no
/// final `else` branch (upstream `return unless else_branch`), or when the node
/// is not a recognised conditional. Mirrors `expand_elses` + `expand_when_branches`.
fn collect_branches(node: NodeId, cx: &Cx<'_>) -> Option<Vec<NodeId>> {
    match *cx.kind(node) {
        NodeKind::If { then_, else_, .. } => {
            let then_id = then_.get()?;
            let mut branches: Vec<NodeId> = vec![then_id];
            let mut cursor = else_;
            loop {
                // No final `else` (or an `elsif` with no `else`) → not flagged.
                let branch = cursor.get()?;
                if matches!(cx.kind(branch), NodeKind::If { .. }) && cx.is_elsif(branch) {
                    let NodeKind::If { then_, else_, .. } = *cx.kind(branch) else {
                        return None;
                    };
                    branches.push(then_.get()?);
                    cursor = else_;
                } else {
                    branches.push(branch);
                    break;
                }
            }
            Some(branches)
        }
        NodeKind::Case { else_, whens, .. } => {
            let else_id = else_.get()?;
            let mut branches: Vec<NodeId> = Vec::with_capacity(cx.list(whens).len() + 1);
            for &wc in cx.list(whens) {
                let NodeKind::When { body, .. } = *cx.kind(wc) else {
                    return None;
                };
                branches.push(body.get()?);
            }
            branches.push(else_id);
            Some(branches)
        }
        NodeKind::CaseMatch { in_patterns, else_body, .. } => {
            let else_id = else_body.get()?;
            let mut branches: Vec<NodeId> = Vec::with_capacity(cx.list(in_patterns).len() + 1);
            for &ip in cx.list(in_patterns) {
                let NodeKind::InPattern { body, .. } = *cx.kind(ip) else {
                    return None;
                };
                branches.push(body.get()?);
            }
            branches.push(else_id);
            Some(branches)
        }
        _ => None,
    }
}

enum MultiStatement {
    /// Single-expression branch (after unwrapping single-expr begins).
    Single(NodeId),
    /// Multi-statement begin; the contained `NodeId` is its tail statement.
    Begin(NodeId),
}

/// Distinguish a single-statement branch from a multi-statement begin, and
/// return the tail statement either way. Mirrors upstream `tail(branch)`
/// (`branch.begin_type? ? Array(branch).last : branch`).
fn multi_statement_tail(node: NodeId, cx: &Cx<'_>) -> MultiStatement {
    if let NodeKind::Begin(list) = cx.kind(node) {
        let items = cx.list(*list);
        match items {
            [] => MultiStatement::Single(node),
            [single] => MultiStatement::Single(unwrap_single_begin(*single, cx)),
            [.., last] => MultiStatement::Begin(*last),
        }
    } else {
        MultiStatement::Single(node)
    }
}

/// Fully unwrap single-expression parentheses (`((expr))` → `expr`).
fn unwrap_single_begin(mut node: NodeId, cx: &Cx<'_>) -> NodeId {
    while let NodeKind::Begin(children) = cx.kind(node) {
        match cx.list(*children) {
            [single] => node = *single,
            _ => break,
        }
    }
    node
}

/// Whether `node` is an assignment-like `send` in upstream's
/// `assignment_type?` pattern:
/// `(send _recv {:[]= :<< :=~ :!~ :<=> #end_with_eq? :< :>} ...)`.
/// `#end_with_eq?` matches any method ending in `=` (including `==`, `<=`,
/// `>=`, `!=`, `===`), so the check is a name test plus arity (sends without
/// arguments cannot be assignments).
fn is_comparison_send(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Send { method, args, .. } = *cx.kind(node) else {
        return false;
    };
    if cx.list(args).is_empty() {
        return false;
    }
    let name = cx.symbol_str(method);
    matches!(name, "[]=" | "<<" | "=~" | "!~" | "<=>" | "<" | ">") || name.ends_with('=')
}

/// The normalised left-hand-side string for a comparison-form `send`,
/// mirroring upstream `lhs_for_send`: `recv[indices] = ` for `[]=`,
/// `recv.attr = ` for setter calls, otherwise `recv method `.
fn send_lhs(node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let NodeKind::Send { method, .. } = *cx.kind(node) else {
        return None;
    };
    let name = cx.symbol_str(method).to_string();
    if !(matches!(name.as_str(), "[]=" | "<<" | "=~" | "!~" | "<=>" | "<" | ">")
        || name.ends_with('='))
    {
        return None;
    }
    let recv_src = cx
        .call_receiver(node)
        .get()
        .map(|r| cx.raw_source(cx.range(r)).to_string())
        .unwrap_or_default();
    if name == "[]=" {
        let argv = cx.call_arguments(node);
        if argv.len() < 2 {
            return None;
        }
        let indices = &argv[..argv.len() - 1];
        let parts: Vec<String> = indices
            .iter()
            .map(|&a| cx.raw_source(cx.range(a)).to_string())
            .collect();
        Some(format!("{}[{}] = ", recv_src, parts.join(", ")))
    } else if cx.is_setter_method(node) {
        let trimmed = name.strip_suffix('=').unwrap_or(&name);
        if recv_src.is_empty() {
            Some(format!("{trimmed} = "))
        } else {
            Some(format!("{recv_src}.{trimmed} = "))
        }
    } else {
        Some(format!("{recv_src} {name} "))
    }
}

/// The RHS value of an assignment tail (the part kept when the assignment is
/// moved outside the conditional). `None` when there is no value (e.g. the
/// value-less write target of a shorthand assign).
fn assignment_rhs(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(node) {
        NodeKind::Lvasgn { value, .. }
        | NodeKind::Ivasgn { value, .. }
        | NodeKind::Gvasgn { value, .. }
        | NodeKind::Cvasgn { value, .. }
        | NodeKind::Casgn { value, .. } => value.get(),
        NodeKind::OpAsgn { value, .. }
        | NodeKind::OrAsgn { value, .. }
        | NodeKind::AndAsgn { value, .. } => Some(value),
        NodeKind::Send { .. } => {
            if !is_comparison_send(node, cx) {
                return None;
            }
            cx.last_argument(node).get()
        }
        NodeKind::IndexAsgn { value, .. } => Some(value),
        _ => None,
    }
}

/// The RHS conditional of an `assign_inside_condition` candidate: the original
/// RHS node (for lhs slicing) plus the value to test. `None` when there is no
/// RHS or the node kind cannot carry a conditional.
fn assign_inside_rhs(node: NodeId, cx: &Cx<'_>) -> Option<(NodeId, NodeId)> {
    match *cx.kind(node) {
        NodeKind::Lvasgn { value, .. }
        | NodeKind::Ivasgn { value, .. }
        | NodeKind::Gvasgn { value, .. }
        | NodeKind::Cvasgn { value, .. }
        | NodeKind::Casgn { value, .. } => {
            let rhs = value.get()?;
            Some((rhs, rhs))
        }
        NodeKind::OpAsgn { value, .. }
        | NodeKind::OrAsgn { value, .. }
        | NodeKind::AndAsgn { value, .. } => Some((value, value)),
        NodeKind::Masgn { rhs, .. } => Some((rhs, rhs)),
        NodeKind::Send { .. } => {
            if !is_comparison_send(node, cx) {
                return None;
            }
            let rhs = cx.last_argument(node).get()?;
            Some((rhs, rhs))
        }
        NodeKind::IndexAsgn { value, .. } => Some((value, value)),
        _ => None,
    }
}

/// Whether `node` is any assignment type upstream would `ignore_node`
/// (the `ASSIGNMENT_TYPES` loop plus comparison-form `send`).
fn is_assignment_like(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Lvasgn { .. }
        | NodeKind::Ivasgn { .. }
        | NodeKind::Gvasgn { .. }
        | NodeKind::Cvasgn { .. }
        | NodeKind::Casgn { .. }
        | NodeKind::OpAsgn { .. }
        | NodeKind::OrAsgn { .. }
        | NodeKind::AndAsgn { .. }
        | NodeKind::Masgn { .. }
        | NodeKind::IndexAsgn { .. } => true,
        NodeKind::Send { .. } => is_comparison_send(node, cx),
        _ => false,
    }
}

/// Upstream `part_of_ignored_node?` for the non-`send` assignment kinds:
/// `true` when an ancestor assignment would have called `ignore_node` —
/// i.e. any ancestor assignment-like node (style is always
/// `assign_inside_condition` here) whose parent is not `mlhs`/`resbody`
/// and which is not itself a shorthand-assign write target. The check is
/// purely ancestral, so it works regardless of dispatch visit order and
/// replicates the "outer fires once" behaviour, including the case where the
/// outer candidate never flags (no `else`) yet still suppresses the inner.
fn is_part_of_ignored(
    node: NodeId,
    _opts: &ConditionalAssignmentOptions,
    cx: &Cx<'_>,
) -> bool {
    for anc in cx.ancestors(node) {
        if anc == node {
            continue;
        }
        if !is_assignment_like(anc, cx) {
            continue;
        }
        // The ancestor must itself have been ignored: not a shorthand write
        // target and not an mlhs/resbody child.
        if let Some(parent) = cx.parent(anc).get()
            && matches!(
                cx.kind(parent),
                NodeKind::OpAsgn { .. } | NodeKind::OrAsgn { .. } | NodeKind::AndAsgn { .. }
            )
        {
            continue;
        }
        if let Some(parent) = cx.parent(anc).get()
            && matches!(cx.kind(parent), NodeKind::Mlhs(_) | NodeKind::Resbody { .. })
        {
            continue;
        }
        return true;
    }
    false
}

/// Upstream `correction_exceeds_line_limit?` (assign-to-condition only):
/// when the rewrite (`lhs` + longest surviving line) would overflow
/// `Layout/LineLength.Max`, the offense is suppressed so autocorrect never
/// introduces a line-length violation. Assumes `Layout/LineLength` is enabled
/// (RuboCop's default); `Cx::max_line_length()` carries the configured max.
fn correction_exceeds_line_limit(node: NodeId, lhs: &str, cx: &Cx<'_>) -> bool {
    let max = cx.max_line_length();
    // Rebuild upstream's assignment regex: optional leading whitespace plus
    // the lhs with every literal space widened to `\s*`.
    let escaped = murphy_plugin_api::regex::escape(lhs);
    let pattern = escaped.replace(' ', r"\s*");
    let re_str = format!(r"\s*{pattern}");
    let stripped_longest: Option<usize> = murphy_plugin_api::regex::Regex::new(&re_str)
        .ok()
        .map(|re| {
            cx.raw_source(cx.range(node))
                .lines()
                .map(|line| {
                    let no_nl = line.strip_suffix('\r').unwrap_or(line);
                    re.replace(no_nl, "").chars().count()
                })
                .max()
                .unwrap_or(0)
        });
    let longest = stripped_longest.unwrap_or_else(|| {
        cx.raw_source(cx.range(node))
            .lines()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0)
    });
    lhs.chars().count() + longest > max
}

/// Emit the assign-to-condition rewrite as a single whole-node replacement:
/// `lhs` + conditional with each branch-tail assignment replaced by its RHS.
/// Ternary element assignments (`bar <<`, setters, comparisons except `[]=`)
/// wrap the ternary in parens to preserve precedence.
fn emit_assign_to_correction(node: NodeId, info: &AssignToInfo, cx: &Cx<'_>) {
    let outer = cx.range(node);
    let src = cx.source();
    let mut edits: Vec<(Range, String)> = Vec::with_capacity(info.tails.len());
    for &(tail, rhs) in &info.tails {
        edits.push((cx.range(tail), cx.raw_source(cx.range(rhs)).to_string()));
    }
    edits.sort_by_key(|(r, _)| r.start);
    let mut inner = String::new();
    let mut cursor = outer.start as usize;
    for (r, rep) in &edits {
        if r.start < outer.start || r.end > outer.end {
            continue;
        }
        if (r.start as usize) < cursor {
            continue;
        }
        inner.push_str(&src[cursor..r.start as usize]);
        inner.push_str(rep);
        cursor = r.end as usize;
    }
    inner.push_str(&src[cursor..outer.end as usize]);
    // Element (non-`[]=`) sends need parens around a ternary rewrite to
    // preserve precedence (`bar << (c ? 1 : 2)`); otherwise `<<` would bind
    // to the condition. Matches upstream `TernaryCorrector`.
    let replacement = if cx.is_ternary(node) && info.element_assignment {
        format!("{}({inner})", info.lhs)
    } else {
        format!("{}{}", info.lhs, inner)
    };
    cx.emit_edit(outer, &replacement);
}

/// Emit the assign-inside rewrite as a single whole-assignment replacement:
/// the conditional with `lhs` inserted before each branch tail.
fn emit_assign_inside_correction(
    node: NodeId,
    rhs_original: NodeId,
    cond: NodeId,
    branches: &[NodeId],
    cx: &Cx<'_>,
) {
    let outer = cx.range(node);
    let src = cx.source();
    // `lhs` is the original assignment prefix (`bar = `, `bar << `,
    // `bar[0] = `, `a, b = `), sliced from the outer start to the original
    // RHS start so original spacing is preserved and any parentheses around
    // the conditional are dropped (upstream strips them for ternaries).
    let rhs_start = cx.range(rhs_original).start as usize;
    if rhs_start < outer.start as usize || rhs_start > outer.end as usize {
        return;
    }
    let lhs = src[outer.start as usize..rhs_start].to_string();
    let cond_range = cx.range(cond);
    let mut edits: Vec<(Range, String)> = Vec::with_capacity(branches.len());
    for &branch in branches {
        let tail = match multi_statement_tail(branch, cx) {
            MultiStatement::Single(id) => id,
            MultiStatement::Begin(_) => match *cx.kind(branch) {
                NodeKind::Begin(list) => match cx.list(list) {
                    [.., last] => *last,
                    [] => continue,
                },
                _ => continue,
            },
        };
        // The tail may itself be parenthesised (`(1)`); inserting before the
        // unwrapped inner keeps the parens (`(bar = 1)`), matching upstream's
        // `insert_before(tail, assignment)`.
        let inner = match multi_statement_tail(tail, cx) {
            MultiStatement::Single(id) => id,
            MultiStatement::Begin(id) => id,
        };
        let tail_src = cx.raw_source(cx.range(inner));
        edits.push((cx.range(inner), format!("{lhs}{tail_src}")));
    }
    edits.sort_by_key(|(r, _)| r.start);
    let mut corrected = String::new();
    let mut cursor = cond_range.start as usize;
    for (r, rep) in &edits {
        if r.start < cond_range.start || r.end > cond_range.end {
            continue;
        }
        if (r.start as usize) < cursor {
            continue;
        }
        corrected.push_str(&src[cursor..r.start as usize]);
        corrected.push_str(rep);
        cursor = r.end as usize;
    }
    corrected.push_str(&src[cursor..cond_range.end as usize]);
    cx.emit_edit(outer, &corrected);
}

/// The canonical left-hand-side string and a node-type tag for an assignment.
/// `None` if `node` is not an assignment Murphy recognises. The tag enforces
/// upstream's `assignment_types_match?` (e.g. `bar = 1` and `bar += 2` differ).
/// `masgn` returns `None` (upstream `allowed_statements?` excludes it via
/// `statements.none?(&:masgn_type?)`).
fn assignment_target(node: NodeId, cx: &Cx<'_>) -> Option<(String, u8)> {
    match cx.kind(node) {
        NodeKind::Lvasgn { name, .. } => Some((format!("{} = ", cx.symbol_str(*name)), 1)),
        NodeKind::Ivasgn { name, .. } => Some((format!("{} = ", cx.symbol_str(*name)), 2)),
        NodeKind::Gvasgn { name, .. } => Some((format!("{} = ", cx.symbol_str(*name)), 3)),
        NodeKind::Cvasgn { name, .. } => Some((format!("{} = ", cx.symbol_str(*name)), 4)),
        NodeKind::Casgn { .. } => cx.const_name(node).map(|n| (format!("{n} = "), 5)),
        NodeKind::OpAsgn { target, op, .. } => {
            let lhs = op_asgn_target_name(*target, cx)?;
            Some((format!("{lhs} {}= ", cx.symbol_str(*op)), 6))
        }
        NodeKind::OrAsgn { target, .. } => {
            let lhs = op_asgn_target_name(*target, cx)?;
            Some((format!("{lhs} ||= "), 7))
        }
        NodeKind::AndAsgn { target, .. } => {
            let lhs = op_asgn_target_name(*target, cx)?;
            Some((format!("{lhs} &&= "), 8))
        }
        NodeKind::Send { .. } => send_lhs(node, cx).map(|s| (s, 9)),
        NodeKind::IndexAsgn {
            receiver, args, ..
        } => {
            let recv = cx.raw_source(cx.range(*receiver)).to_string();
            let parts: Vec<String> = cx
                .list(*args)
                .iter()
                .map(|&a| cx.raw_source(cx.range(a)).to_string())
                .collect();
            Some((format!("{recv}[{}] = ", parts.join(", ")), 9))
        }
        _ => None,
    }
}

/// The variable name of an op-assign target (a value-less write node).
fn op_asgn_target_name(target: NodeId, cx: &Cx<'_>) -> Option<String> {
    match cx.kind(target) {
        NodeKind::Lvasgn { name, .. }
        | NodeKind::Ivasgn { name, .. }
        | NodeKind::Gvasgn { name, .. }
        | NodeKind::Cvasgn { name, .. } => Some(cx.symbol_str(*name).to_string()),
        NodeKind::Casgn { .. } => cx.const_name(target),
        _ => None,
    }
}

fn first_line_range(range: Range, source: &str) -> Range {
    let bytes = source.as_bytes();
    let mut end = range.start as usize;
    while end < range.end as usize && end < bytes.len() && bytes[end] != b'\n' {
        end += 1;
    }
    Range { start: range.start, end: end as u32 }
}

#[cfg(test)]
mod tests {
    use super::{ConditionalAssignment, ConditionalAssignmentOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    fn opts(
        style: EnforcedStyle,
        single_line_conditions_only: bool,
        include_ternary_expressions: bool,
    ) -> ConditionalAssignmentOptions {
        ConditionalAssignmentOptions {
            enforced_style: style,
            single_line_conditions_only,
            include_ternary_expressions,
        }
    }

    fn assign_inside() -> ConditionalAssignmentOptions {
        opts(EnforcedStyle::AssignInsideCondition, true, true)
    }

    #[test]
    fn flags_if_else_same_assignment() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              bar = 1
            else
              bar = 2
            end
        "});
    }

    #[test]
    fn flags_nested_parenthesized_assignments() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              ((bar = 1))
            else
              ((bar = 2))
            end
        "});
    }

    #[test]
    fn accepts_if_else_different_vars() {
        test::<ConditionalAssignment>()
            .expect_no_offenses("if foo\n  bar = 1\nelse\n  baz = 2\nend\n");
    }

    #[test]
    fn accepts_direct_assignment() {
        test::<ConditionalAssignment>()
            .expect_no_offenses("bar = if foo\n  1\nelse\n  2\nend\n");
    }

    #[test]
    fn flags_case_when_same_assignment() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            case foo
            ^^^^^^^^ Use the return of the conditional for variable assignment and comparison.
            when 'a'
              bar = 1
            else
              bar = 2
            end
        "});
    }

    #[test]
    fn flags_if_elsif_else_chain() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              bar = 1
            elsif baz
              bar = 2
            else
              bar = 3
            end
        "});
    }

    #[test]
    fn accepts_elsif_chain_with_mismatched_var() {
        test::<ConditionalAssignment>().expect_no_offenses(indoc! {"
            if foo
              bar = 1
            elsif baz
              other = 2
            else
              bar = 3
            end
        "});
    }

    #[test]
    fn flags_op_asgn_branches() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              bar += 1
            else
              bar += 2
            end
        "});
    }

    #[test]
    fn accepts_mixed_assignment_types() {
        // `assignment_types_match?`: `bar = 1` and `bar += 2` differ.
        test::<ConditionalAssignment>()
            .expect_no_offenses("if foo\n  bar = 1\nelse\n  bar += 2\nend\n");
    }

    #[test]
    fn flags_case_in_same_assignment() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            case foo
            ^^^^^^^^ Use the return of the conditional for variable assignment and comparison.
            in 1
              bar = 1
            else
              bar = 2
            end
        "});
    }

    #[test]
    fn flags_ternary_assignment() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            foo? ? bar = 1 : bar = 2
            ^^^^^^^^^^^^^^^^^^^^^^^^ Use the return of the conditional for variable assignment and comparison.
        "});
    }

    #[test]
    fn accepts_ternary_when_include_ternary_disabled() {
        test::<ConditionalAssignment>()
            .with_options(&opts(EnforcedStyle::AssignToCondition, true, false))
            .expect_no_offenses("foo? ? bar = 1 : bar = 2\n");
    }

    #[test]
    fn accepts_multi_statement_branch_with_single_line_only() {
        // SingleLineConditionsOnly: true (default) → multi-statement branch
        // tail is not flagged.
        test::<ConditionalAssignment>().expect_no_offenses(indoc! {"
            if foo
              do_something
              bar = 1
            else
              do_other
              bar = 2
            end
        "});
    }

    #[test]
    fn flags_multi_statement_branch_when_single_line_only_disabled() {
        test::<ConditionalAssignment>()
            .with_options(&opts(EnforcedStyle::AssignToCondition, false, true))
            .expect_offense(indoc! {"
                if foo
                ^^^^^^ Use the return of the conditional for variable assignment and comparison.
                  do_something
                  bar = 1
                else
                  do_other
                  bar = 2
                end
            "});
    }

    #[test]
    fn accepts_parenthesized_tail_in_multi_statement_branch() {
        // RuboCop's `tail` (`branch.begin_type? ? Array(branch).last : branch`)
        // unwraps exactly ONE level: a multi-statement branch unwraps to its
        // last statement, but a *parenthesized* assignment tail `(bar = 1)` is
        // itself a begin node, so the assignment check fails and no offense is
        // reported. Murphy must not fully unwrap here. Verified against rubocop
        // 1.87 (no offense), even with SingleLineConditionsOnly disabled.
        test::<ConditionalAssignment>()
            .with_options(&opts(EnforcedStyle::AssignToCondition, false, true))
            .expect_no_offenses(
                "if foo\n  do_a\n  (bar = 1)\nelse\n  do_b\n  (bar = 2)\nend\n",
            );
    }

    #[test]
    fn accepts_if_without_else_branch() {
        test::<ConditionalAssignment>().expect_no_offenses("if foo\n  bar = 1\nend\n");
    }

    // --- EnforcedStyle: assign_inside_condition ---

    #[test]
    fn assign_inside_flags_assign_to_if() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                bar = if foo
                ^^^^^^^^^^^^ Assign variables inside of conditionals.
                  1
                else
                  2
                end
            "});
    }

    #[test]
    fn assign_inside_flags_assign_to_case() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                bar = case foo
                ^^^^^^^^^^^^^^ Assign variables inside of conditionals.
                when 'a'
                  1
                else
                  2
                end
            "});
    }

    #[test]
    fn assign_inside_flags_op_asgn_to_if() {
        // Upstream aliases `on_op_asgn` to the same handler. Verified against
        // rubocop 1.87: `bar += if … end` reports on the op-assign node.
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                bar += if foo
                ^^^^^^^^^^^^^ Assign variables inside of conditionals.
                  1
                else
                  2
                end
            "});
    }

    #[test]
    fn assign_inside_flags_or_asgn_to_case() {
        // `bar ||= case … end` must also fire (on_or_asgn alias).
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                bar ||= case foo
                ^^^^^^^^^^^^^^^^ Assign variables inside of conditionals.
                when 'a'
                  1
                else
                  2
                end
            "});
    }

    #[test]
    fn assign_inside_accepts_if_without_else() {
        // Verified against rubocop 1.87: `x = if foo; 1; end` is silent.
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_no_offenses("bar = if foo\n  1\nend\n");
    }

    #[test]
    fn assign_inside_accepts_multi_statement_branch_by_default() {
        // SingleLineConditionsOnly defaults true; a multi-statement branch
        // suppresses the offense. Verified against rubocop 1.87 (silent).
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_no_offenses("bar = if foo\n  a\n  b\nelse\n  c\nend\n");
    }

    #[test]
    fn assign_inside_flags_multi_statement_branch_when_single_line_only_disabled() {
        test::<ConditionalAssignment>()
            .with_options(&opts(EnforcedStyle::AssignInsideCondition, false, true))
            .expect_offense(indoc! {"
                bar = if foo
                ^^^^^^^^^^^^ Assign variables inside of conditionals.
                  a
                  b
                else
                  c
                end
            "});
    }

    #[test]
    fn assign_inside_default_style_does_not_flag_assign_to_condition() {
        // Default style is assign_to_condition; `bar = if … end` is the *good*
        // form and must not be reported.
        test::<ConditionalAssignment>().expect_no_offenses("bar = if foo\n  1\nelse\n  2\nend\n");
    }

    #[test]
    fn assign_to_condition_style_does_not_flag_inside_assignment() {
        // Inverse: default style must not fire on the inside form's good output.
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_no_offenses("if foo\n  bar = 1\nelse\n  bar = 2\nend\n");
    }

    // --- on_send comparison form (assign_to_condition) ---

    #[test]
    fn flags_send_shovel_branches() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              bar << 1
            else
              bar << 2
            end
        "});
    }

    #[test]
    fn flags_send_comparison_branches() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              bar < 1
            else
              bar < 2
            end
        "});
    }

    #[test]
    fn flags_send_index_assign_branches() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              bar[0] = 1
            else
              bar[0] = 2
            end
        "});
    }

    #[test]
    fn accepts_send_index_assign_different_indices() {
        // `lhs_for_send` includes indices: `bar[0] = ` vs `bar[1] = ` differ.
        test::<ConditionalAssignment>()
            .expect_no_offenses("if foo\n  bar[0] = 1\nelse\n  bar[1] = 2\nend\n");
    }

    #[test]
    fn flags_send_setter_branches() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            if foo
            ^^^^^^ Use the return of the conditional for variable assignment and comparison.
              bar.baz = 1
            else
              bar.baz = 2
            end
        "});
    }

    #[test]
    fn accepts_send_different_receivers() {
        test::<ConditionalAssignment>()
            .expect_no_offenses("if foo\n  bar << 1\nelse\n  baz << 2\nend\n");
    }

    #[test]
    fn accepts_send_mixed_methods() {
        // Same `:send` type but different lhs (`bar << ` vs `bar < `).
        test::<ConditionalAssignment>()
            .expect_no_offenses("if foo\n  bar << 1\nelse\n  bar < 2\nend\n");
    }

    #[test]
    fn accepts_send_mixed_with_lvasgn() {
        // `assignment_types_match?`: `:send` vs `:lvasgn` differ.
        test::<ConditionalAssignment>()
            .expect_no_offenses("if foo\n  bar << 1\nelse\n  bar = 2\nend\n");
    }

    #[test]
    fn flags_send_ternary_branches() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            foo? ? bar << 1 : bar << 2
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the return of the conditional for variable assignment and comparison.
        "});
    }

    #[test]
    fn flags_send_case_branches() {
        test::<ConditionalAssignment>().expect_offense(indoc! {"
            case foo
            ^^^^^^^^ Use the return of the conditional for variable assignment and comparison.
            when 'a'
              bar << 1
            else
              bar << 2
            end
        "});
    }

    #[test]
    fn accepts_masgn_branches_in_assign_to() {
        // Upstream `allowed_statements?` excludes masgn tails.
        test::<ConditionalAssignment>()
            .expect_no_offenses("if foo\n  a, b = 1, 2\nelse\n  a, b = 3, 4\nend\n");
    }

    // --- on_send / masgn (assign_inside_condition) ---

    #[test]
    fn assign_inside_flags_send_shovel_to_if() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                bar << if foo
                ^^^^^^^^^^^^^ Assign variables inside of conditionals.
                  1
                else
                  2
                end
            "});
    }

    #[test]
    fn assign_inside_flags_send_setter_to_if() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                bar.baz = if foo
                ^^^^^^^^^^^^^^^^ Assign variables inside of conditionals.
                  1
                else
                  2
                end
            "});
    }

    #[test]
    fn assign_inside_flags_send_index_to_if() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                bar[0] = if foo
                ^^^^^^^^^^^^^^^ Assign variables inside of conditionals.
                  1
                else
                  2
                end
            "});
    }

    #[test]
    fn assign_inside_flags_masgn_to_if() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_offense(indoc! {"
                a, b = if foo
                ^^^^^^^^^^^^^ Assign variables inside of conditionals.
                  [1, 2]
                else
                  [3, 4]
                end
            "});
    }

    #[test]
    fn assign_inside_accepts_bare_send_without_conditional() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_no_offenses("bar << 1\n");
    }

    #[test]
    fn assign_inside_accepts_bare_masgn_without_conditional() {
        // `a, b = [1, 2]` has no conditional RHS — no offense.
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_no_offenses("a, b = [1, 2]\n");
    }

    #[test]
    fn assign_inside_outer_fires_once_for_nested_lvasgn() {
        // Upstream `ignore_node`: outer `x = if … y = if … end … end` reports
        // once (outer). Murphy replicates via ancestor suppression.
        use murphy_plugin_api::test_support::run_cop_with_options;
        let offenses = run_cop_with_options::<ConditionalAssignment>(
            "x = if foo\n  y = if bar\n    1\n  else\n    2\n  end\nelse\n  3\nend\n",
            &assign_inside(),
        );
        assert_eq!(offenses.len(), 1, "expected single outer offense, got {offenses:?}");
    }

    #[test]
    fn assign_inside_nested_send_double_fires_like_upstream() {
        // Upstream `on_send` skips `part_of_ignored_node?`, so an inner send
        // inside an outer lvasgn still fires (2 offenses). Replicated here.
        use murphy_plugin_api::test_support::run_cop_with_options;
        let offenses = run_cop_with_options::<ConditionalAssignment>(
            "x = if foo\n  y << if bar\n    1\n  else\n    2\n  end\nelse\n  3\nend\n",
            &assign_inside(),
        );
        assert_eq!(offenses.len(), 2, "expected upstream-like double fire, got {offenses:?}");
    }

    #[test]
    fn assign_inside_masgn_reports_once_not_per_target() {
        // `a, b = if … end` is one offense (outer masgn); the value-less
        // `mlhs` children never fire on their own.
        use murphy_plugin_api::test_support::run_cop_with_options;
        let offenses = run_cop_with_options::<ConditionalAssignment>(
            "a, b = if foo\n  [1, 2]\nelse\n  [3, 4]\nend\n",
            &assign_inside(),
        );
        assert_eq!(offenses.len(), 1, "expected single masgn offense, got {offenses:?}");
    }

    // --- autocorrect ---

    #[test]
    fn corrects_if_else_to_assign_outside() {
        test::<ConditionalAssignment>().expect_correction(
            indoc! {"
                if foo
                ^^^^^^ Use the return of the conditional for variable assignment and comparison.
                  bar = 1
                else
                  bar = 2
                end
            "},
            "bar = if foo\n  1\nelse\n  2\nend\n",
        );
    }

    #[test]
    fn corrects_send_branches_to_assign_outside() {
        test::<ConditionalAssignment>().expect_correction(
            indoc! {"
                if foo
                ^^^^^^ Use the return of the conditional for variable assignment and comparison.
                  bar << 1
                else
                  bar << 2
                end
            "},
            "bar << if foo\n  1\nelse\n  2\nend\n",
        );
    }

    #[test]
    fn corrects_ternary_to_assign_outside() {
        test::<ConditionalAssignment>().expect_correction(
            indoc! {"
                foo? ? bar = 1 : bar = 2
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use the return of the conditional for variable assignment and comparison.
            "},
            "bar = foo? ? 1 : 2\n",
        );
    }

    #[test]
    fn corrects_ternary_send_with_parens() {
        // Element assignment needs parens to preserve precedence.
        test::<ConditionalAssignment>().expect_correction(
            indoc! {"
                foo? ? bar << 1 : bar << 2
                ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the return of the conditional for variable assignment and comparison.
            "},
            "bar << (foo? ? 1 : 2)\n",
        );
    }

    #[test]
    fn assign_inside_corrects_assign_to_inside() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_correction(
                indoc! {"
                    bar = if foo
                    ^^^^^^^^^^^^ Assign variables inside of conditionals.
                      1
                    else
                      2
                    end
                "},
                "if foo\n  bar = 1\nelse\n  bar = 2\nend\n",
            );
    }

    #[test]
    fn assign_inside_corrects_send_to_inside() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_correction(
                indoc! {"
                    bar << if foo
                    ^^^^^^^^^^^^^ Assign variables inside of conditionals.
                      1
                    else
                      2
                    end
                "},
                "if foo\n  bar << 1\nelse\n  bar << 2\nend\n",
            );
    }

    #[test]
    fn corrects_case_to_assign_outside() {
        test::<ConditionalAssignment>().expect_correction(
            indoc! {"
                case foo
                ^^^^^^^^ Use the return of the conditional for variable assignment and comparison.
                when 'a'
                  bar = 1
                else
                  bar = 2
                end
            "},
            "bar = case foo\nwhen 'a'\n  1\nelse\n  2\nend\n",
        );
    }

    #[test]
    fn corrects_elsif_chain_to_assign_outside() {
        test::<ConditionalAssignment>().expect_correction(
            indoc! {"
                if foo
                ^^^^^^ Use the return of the conditional for variable assignment and comparison.
                  bar = 1
                elsif baz
                  bar = 2
                else
                  bar = 3
                end
            "},
            "bar = if foo\n  1\nelsif baz\n  2\nelse\n  3\nend\n",
        );
    }

    #[test]
    fn suppresses_long_rewrite_over_line_limit() {
        // `correction_exceeds_line_limit?`: lhs + longest line > max suppresses.
        use murphy_plugin_api::test_support::run_cop_with_options;
        let long_val = "x".repeat(100);
        let src = format!("if foo\n  bar = {long_val}\nelse\n  bar = 2\nend\n");
        let opts = opts(EnforcedStyle::AssignToCondition, true, true);
        // Default max (120) flags; tiny max suppresses.
        let default_offenses = run_cop_with_options::<ConditionalAssignment>(&src, &opts);
        assert_eq!(default_offenses.len(), 1);
        let tiny = test::<ConditionalAssignment>()
            .with_options(&opts)
            .with_max_line_length(10);
        tiny.expect_no_offenses(&src);
    }

    #[test]
    fn assign_inside_corrects_masgn_to_inside() {
        test::<ConditionalAssignment>()
            .with_options(&assign_inside())
            .expect_correction(
                indoc! {"
                    a, b = if foo
                    ^^^^^^^^^^^^^ Assign variables inside of conditionals.
                      [1, 2]
                    else
                      [3, 4]
                    end
                "},
                "if foo\n  a, b = [1, 2]\nelse\n  a, b = [3, 4]\nend\n",
            );
    }
}
murphy_plugin_api::submit_cop!(ConditionalAssignment);
