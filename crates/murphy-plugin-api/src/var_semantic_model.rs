//! Variable semantic model: scope/variable tracking built from the arena AST.
//!
//! [`VarSemanticModel::build`] does a single-pass DFS over the arena, keying
//! each scope by the boundary node (`Def`, `Defs`, `Block`, `Lambda`, `Class`,
//! `Module`, `Sclass`) that introduces it.  Variables, arguments, assignments,
//! and references are collected per scope without crossing scope boundaries.

use std::collections::HashMap;

use murphy_ast::{Ast, NodeId, NodeKind, Symbol};

/// The full variable semantic model for one file.
pub struct VarSemanticModel {
    scopes: HashMap<NodeId, ScopeInfo>,
}

/// Information about one lexical scope.
pub struct ScopeInfo {
    /// The boundary node of the enclosing scope, or `None` for the root scope.
    pub parent_scope: Option<NodeId>,
    /// Variables (locals + arguments) declared in this scope.
    pub variables: Vec<Variable>,
}

/// A local variable (or argument) tracked within one scope.
pub struct Variable {
    /// The interned name symbol.
    pub name: Symbol,
    /// `true` when first introduced as a formal parameter.
    pub is_argument: bool,
    /// The node where this variable is first introduced (first arg / first
    /// assignment).
    pub declaration_node: NodeId,
    /// All assignment sites (plain and compound) within this scope.
    pub assignments: Vec<Assignment>,
    /// All read sites within this scope.
    pub references: Vec<Reference>,
}

/// One assignment site for a local variable.
pub struct Assignment {
    /// The `Lvasgn` (or compound-assign) node that writes the variable.
    pub node_id: NodeId,
    /// Byte position of the assignment's end (exclusive).
    pub end: u32,
    /// `true` when at least one later read of this variable can observe this
    /// write (branch-aware: exclusive-branch reads are excluded).
    pub is_referenced: bool,
}

/// One read site for a local variable.
pub struct Reference {
    /// The `Lvar` node that reads the variable.
    pub node_id: NodeId,
    /// Byte position of the reference start.
    pub pos: u32,
}

// ── Branch-aware dominance analysis ──────────────────────────────────────────

/// Walk up from `node` to `root` via `ast.parent()`, collecting
/// `(parent, child)` pairs at each branch-introducing ancestor.
/// Returns the chain reversed (outermost first).
fn barrier_chain(ast: &Ast, root: NodeId, node: NodeId) -> Vec<(NodeId, NodeId)> {
    let mut chain: Vec<(NodeId, NodeId)> = Vec::new();
    let mut current = node;
    while let Some(parent) = ast.parent(current).get() {
        if parent == root {
            break;
        }
        if is_branch_barrier(ast, parent) {
            chain.push((parent, current));
        }
        current = parent;
    }
    chain.reverse();
    chain
}

/// Returns `true` for nodes that introduce exclusive branches.
///
/// `Resbody`/`Ensure` are intentionally NOT barriers. `Rescue` IS a barrier,
/// but an *asymmetric* one: its arms (begin `body`, each `Resbody`, and `else`)
/// are mutually exclusive for *domination* (`chain_is_prefix`), yet the begin
/// `body` arm stays read-compatible with every sibling arm
/// (`barrier_condition_is_compatible`) because exception control flow carries a
/// partial begin-body write into the rescue/else/fall-through paths.
fn is_branch_barrier(ast: &Ast, node: NodeId) -> bool {
    matches!(
        *ast.kind(node),
        NodeKind::If { .. }
            | NodeKind::Case { .. }
            | NodeKind::When { .. }
            | NodeKind::CaseMatch { .. }
            | NodeKind::While { .. }
            | NodeKind::Until { .. }
            | NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Itblock { .. }
            | NodeKind::Rescue { .. }
    )
}

/// Returns `true` if `short` is a prefix of `long` (outermost-first).
/// When `short = chain(w')` and `long = chain(w)`, this means `w'` is
/// in the same chunk as `w` or a shallower one that `w` must fall through to.
fn chain_is_prefix(short: &[(NodeId, NodeId)], long: &[(NodeId, NodeId)]) -> bool {
    short.len() <= long.len() && short == &long[..short.len()]
}

/// Returns `false` if any shared barrier has conflicting arm choices
/// (i.e. the two nodes are in exclusive branches).
fn paths_compatible(ast: &Ast, a: &[(NodeId, NodeId)], b: &[(NodeId, NodeId)]) -> bool {
    for (barrier_a, arm_a) in a {
        for (barrier_b, arm_b) in b {
            if barrier_a == barrier_b
                && arm_a != arm_b
                && !barrier_condition_is_compatible(ast, *barrier_a, *arm_a, *arm_b)
            {
                return false;
            }
        }
    }
    true
}

fn barrier_condition_is_compatible(ast: &Ast, barrier: NodeId, a: NodeId, b: NodeId) -> bool {
    match *ast.kind(barrier) {
        NodeKind::If { cond, .. } | NodeKind::While { cond, .. } | NodeKind::Until { cond, .. } => {
            a == cond || b == cond
        }
        // The begin body (`body`) flows into every rescue/else/after arm via
        // exception control flow, so a begin-body write stays observable by a
        // read in any sibling arm. Resbody-vs-resbody and resbody-vs-else stay
        // exclusive. (Domination via `chain_is_prefix` is unaffected — only
        // read-compatibility relaxes here.)
        NodeKind::Rescue { body, .. } => body.get() == Some(a) || body.get() == Some(b),
        _ => false,
    }
}

/// Returns `true` if `node` is inside the `body` arm of an enclosing `Rescue`
/// or `Ensure`. Writes here can be interrupted by exceptions, so they don't
/// dominate later writes.
fn is_in_protected_begin_body(ast: &Ast, root: NodeId, node: NodeId) -> bool {
    let mut current = node;
    while let Some(parent) = ast.parent(current).get() {
        if parent == root {
            return false;
        }
        let parent_kind = *ast.kind(parent);
        let body = match parent_kind {
            NodeKind::Rescue { body, .. } | NodeKind::Ensure { body, .. } => body,
            _ => {
                current = parent;
                continue;
            }
        };
        if body.get() == Some(current) {
            return true;
        }
        current = parent;
    }
    false
}

/// Which arm of an enclosing `Rescue` a node sits in.
enum RescueArm {
    Body,
    Else,
    Resbody(NodeId),
}

/// Nearest `Rescue` ancestor of `node` whose protected `body` arm (transitively)
/// contains `node`, or `None` if there is none before `root`.
fn enclosing_protected_rescue(ast: &Ast, root: NodeId, node: NodeId) -> Option<NodeId> {
    let mut current = node;
    while let Some(parent) = ast.parent(current).get() {
        if parent == root {
            return None;
        }
        if let NodeKind::Rescue { body, .. } = *ast.kind(parent)
            && body.get() == Some(current)
        {
            return Some(parent);
        }
        current = parent;
    }
    None
}

/// Classify which arm of `rescue` a node lives in, but only when the node is an
/// *unconditional* statement of that arm — i.e. no branch barrier and no
/// short-circuit operator (`&&`/`||`) sits strictly between the node and
/// `rescue`. A write guarded by an inner branch (e.g. `else; x = 2 if foo`) or
/// short-circuited (e.g. `else; cond && (x = 2)`) is not a guaranteed overwrite
/// of that arm's exit, so it returns `None` and cannot contribute to a
/// distributed kill.
fn rescue_arm_of(ast: &Ast, rescue: NodeId, node: NodeId) -> Option<RescueArm> {
    let NodeKind::Rescue { body, else_, .. } = *ast.kind(rescue) else {
        return None;
    };
    let mut current = node;
    while let Some(parent) = ast.parent(current).get() {
        if parent == rescue {
            if body.get() == Some(current) {
                return Some(RescueArm::Body);
            }
            if else_.get() == Some(current) {
                return Some(RescueArm::Else);
            }
            if matches!(*ast.kind(current), NodeKind::Resbody { .. }) {
                return Some(RescueArm::Resbody(current));
            }
            return None;
        }
        // A branch barrier (if/case/loop/block) or a short-circuit operator
        // (`&&`/`||`) between the write and the arm means the write may be
        // skipped, so it is not a guaranteed overwrite of that arm's exit.
        if is_branch_barrier(ast, parent)
            || matches!(
                *ast.kind(parent),
                NodeKind::And { .. } | NodeKind::Or { .. }
            )
        {
            return None;
        }
        current = parent;
    }
    None
}

/// RuboCop-parity fallback for a begin-body write whose kill is *distributed*
/// across an enclosing `Rescue`'s sibling arms.
///
/// The branch-chain dominance model treats the `else` and each `resbody` arm as
/// mutually exclusive from the protected `body`, so a body write that is killed
/// only by the *combination* of every sibling arm — the `else` arm on the
/// no-exception path and every `resbody` arm on its exception path — is never
/// matched by a single prefix-dominator. This reconstructs that distributed kill
/// for a read positioned after the whole construct:
///
/// `begin; x = 1; rescue; x = 3; else; x = 2; end; use(x)` — every exit of the
/// rescue overwrites `x` before the trailing read, so `x = 1` is useless
/// (RuboCop 1.87 flags it).
///
/// Sound (no false positives): requires the read to lie *after* the construct,
/// an unconditional `else`-arm overwrite (no-exception path), and an
/// unconditional overwrite in *every* `resbody` arm (each exception path). A
/// `rescue` arm that does not overwrite (e.g. `rescue; log`) leaves the body
/// write observable on that path, so the kill does not apply. A missing `else`
/// arm leaves it observable on the no-exception fall-through. A write reached
/// through an inner branch or a short-circuit `&&`/`||` is not counted (see
/// `rescue_arm_of`), so a conditional overwrite never triggers the kill.
///
/// KNOWN LIMITATIONS (false-negative direction only, tracked in murphy-w5za —
/// same conservative-approximation tolerance as the loop-body handling):
///   1. A missing `else` arm whose no-exception path is instead covered by a
///      *later unconditional write in the begin body itself* is not recognised
///      (`begin; x = 1; x = 2; rescue; x = 3; end; use(x)` — RuboCop flags
///      `x = 1`, we miss it). The `else_.is_none()` guard returns early.
///   2. A read positioned *inside* an arm, but after that arm has already
///      overwritten the variable on every reaching path, is not recognised
///      (`begin; x = 1; rescue; x = 2; use(x); else; x = 3; end; use(x)` —
///      RuboCop flags `x = 1`). The `read_pos <= rescue.end` guard
///      conservatively disables the kill for any in-construct read. Refining
///      either requires per-arm intra-arm dominance and is deferred.
fn begin_body_distributed_kill(
    ast: &Ast,
    root: NodeId,
    assignments: &[Assignment],
    body_idx: usize,
    read_pos: u32,
) -> bool {
    let body_write = &assignments[body_idx];
    let Some(rescue) = enclosing_protected_rescue(ast, root, body_write.node_id) else {
        return false;
    };
    // Only sound when the read lies entirely past the construct: a read inside
    // any arm could observe the body write before that arm's overwrite.
    if read_pos <= ast.range(rescue).end {
        return false;
    }
    let NodeKind::Rescue { else_, .. } = *ast.kind(rescue) else {
        return false;
    };
    // No `else` arm ⇒ the no-exception path falls through with the body value
    // intact ⇒ the body write is live on that path.
    if else_.get().is_none() {
        return false;
    }
    let mut else_killed = false;
    let mut killed_resbodies: Vec<NodeId> = Vec::new();
    for (j, w) in assignments.iter().enumerate() {
        if j == body_idx || w.end <= body_write.end {
            continue;
        }
        match rescue_arm_of(ast, rescue, w.node_id) {
            Some(RescueArm::Else) => else_killed = true,
            Some(RescueArm::Resbody(rb)) if !killed_resbodies.contains(&rb) => {
                killed_resbodies.push(rb);
            }
            _ => {}
        }
    }
    if !else_killed {
        return false;
    }
    // Every `resbody` arm of the rescue must contain an overwrite.
    ast.children(rescue)
        .filter(|&c| matches!(*ast.kind(c), NodeKind::Resbody { .. }))
        .all(|rb| killed_resbodies.contains(&rb))
}

/// Returns `true` if `node` is inside the loop body of an enclosing `While`,
/// `Until`, or `For`. Assignments here are conservatively marked as referenced
/// since the next loop iteration may read them.
fn is_in_loop_body(ast: &Ast, root: NodeId, node: NodeId) -> bool {
    let mut current = node;
    loop {
        let parent = match ast.parent(current).get() {
            Some(p) => p,
            None => return false,
        };
        if parent == root {
            return false;
        }
        match *ast.kind(parent) {
            NodeKind::While { body, .. } | NodeKind::Until { body, .. }
                if body.get() == Some(current) =>
            {
                return true;
            }
            NodeKind::For { body, .. } if body.get() == Some(current) => {
                return true;
            }
            _ => {}
        }
        current = parent;
    }
}

/// Returns `true` if `node` is inside a `Rescue` whose resbody subtree contains
/// a `Retry`. RuboCop treats such a `begin..rescue..end` as a loop
/// (`process_rescue` -> `process_loop`), so writes inside it may be read on the
/// next iteration via the retry back-edge and must not be flagged.
///
/// This mirrors RuboCop 1.87's `process_rescue` -> `process_loop`: it
/// loop-ifies the *whole* rescue, so — like the `is_in_loop_body`
/// approximation — every write inside a retry-rescue is blanket-marked
/// referenced. The descent over `subtree_contains_retry` matches RuboCop's
/// detection scope (`resbody_node.each_descendant.any?(&:retry_type?)` also
/// descends into nested `begin..rescue`).
///
/// KNOWN DIVERGENCE (false-negative, tracked in murphy-w5za): RuboCop still
/// flags a write that has *no* reference anywhere in the loop, whereas this
/// blanket-mark suppresses it — same approximation as the documented
/// `while`/`until`/`for` loop-body handling. This also manifests when the
/// `retry` lives in a nested *inner* rescue: a never-read write in the outer
/// resbody is flagged by RuboCop but missed here. The case does not occur in
/// the Mastodon corpus this fix targets; refining it is deferred to murphy-w5za.
///
/// `retry_cache` memoizes "does this `Rescue`'s resbody subtree contain a
/// `Retry`" so the subtree DFS runs at most once per `Rescue` node across all
/// assignments in the scope (see `safe-rust-patterns.md`: avoid redundant
/// repeated work / per-node allocation in the hot analysis path).
fn is_in_retry_rescue(
    ast: &Ast,
    root: NodeId,
    node: NodeId,
    retry_cache: &mut HashMap<NodeId, bool>,
) -> bool {
    let mut current = node;
    while let Some(parent) = ast.parent(current).get() {
        if parent == root {
            return false;
        }
        if matches!(*ast.kind(parent), NodeKind::Rescue { .. }) {
            // `ast.children(Rescue)` yields body + resbodies + else; only the
            // `Resbody` children carry the legal `retry` back-edge, so scan
            // those. Memoize per `Rescue` so this DFS runs at most once.
            let contains = *retry_cache.entry(parent).or_insert_with(|| {
                ast.children(parent)
                    .filter(|&c| matches!(*ast.kind(c), NodeKind::Resbody { .. }))
                    .any(|rb| subtree_contains_retry(ast, rb))
            });
            if contains {
                return true;
            }
        }
        current = parent;
    }
    false
}

/// DFS over `node`'s subtree for a `Retry`. Descends through nested
/// `begin..rescue` boundaries by design — see `is_in_retry_rescue` for why this
/// matches RuboCop's `each_descendant` scope. Runs at most once per `Rescue`
/// node thanks to `is_in_retry_rescue`'s `retry_cache`.
fn subtree_contains_retry(ast: &Ast, node: NodeId) -> bool {
    matches!(*ast.kind(node), NodeKind::Retry)
        || ast.children(node).any(|c| subtree_contains_retry(ast, c))
}

/// Returns `true` if `node` sits inside a block/lambda body that is nested
/// *within* the variable's declaring scope (`scope_root`) — i.e. the
/// assignment writes a variable captured from an enclosing scope.
///
/// RuboCop's `Lint/UselessAssignment` never flags such a write: the block may
/// run zero or many times, so the value is indeterminate and the write cannot
/// be proven dead (matches `Variable#captured_by_block?`). Verified against
/// standalone rubocop 1.87.0.
///
/// The walk stops (returning `false`) at a `Def`/`Defs`/`Class`/`Module`/
/// `Sclass` boundary: Ruby locals do not cross those, so a same-named variable
/// resolved across one is a (pre-existing) resolution artifact, not a real
/// capture — we must not force-mark it referenced and thereby mask a genuine
/// offense inside the nested method/class body.
fn is_in_captured_block(ast: &Ast, scope_root: NodeId, node: NodeId) -> bool {
    let mut current = node;
    while let Some(parent) = ast.parent(current).get() {
        if parent == scope_root {
            return false;
        }
        match *ast.kind(parent) {
            // Only the deferred block BODY captures the variable. The block's
            // receiver call and its arguments (`items.each(n = 1) { … }`) run in
            // the parent scope at the call site, so an assignment reached via the
            // `call`/`send` child is NOT captured — fall through and keep walking
            // outward.
            NodeKind::Block { call, .. } if current != call => return true,
            NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } if current != send => {
                return true;
            }
            NodeKind::Lambda => return true,
            // Hard local-scope boundary: stop without claiming capture.
            NodeKind::Def { .. }
            | NodeKind::Defs { .. }
            | NodeKind::Class { .. }
            | NodeKind::Module { .. }
            | NodeKind::Sclass { .. } => return false,
            _ => {}
        }
        current = parent;
    }
    false
}

/// Compute `is_referenced` for every `Assignment` in `scope` once the DFS
/// has fully populated the scope's variables, assignments, and references.
fn analyze_scope_is_referenced(ast: &Ast, scope_root: NodeId, scope: &mut ScopeInfo) {
    // Per-scope memo: "does this `Rescue`'s resbody subtree contain a `Retry`".
    // Declared once here (not per-assignment) so the subtree DFS amortizes to at
    // most one run per `Rescue` node across every variable/assignment in the
    // scope. Keyed by `Rescue` `NodeId`, which is stable for the scope's lifetime.
    let mut retry_cache: HashMap<NodeId, bool> = HashMap::new();
    for var in &mut scope.variables {
        // Pre-compute branch chains for all assignments and references.
        let asgn_chains: Vec<Vec<(NodeId, NodeId)>> = var
            .assignments
            .iter()
            .map(|a| barrier_chain(ast, scope_root, a.node_id))
            .collect();
        let ref_chains: Vec<Vec<(NodeId, NodeId)>> = var
            .references
            .iter()
            .map(|r| barrier_chain(ast, scope_root, r.node_id))
            .collect();

        for i in 0..var.assignments.len() {
            let asgn_node = var.assignments[i].node_id;
            let asgn_end = var.assignments[i].end;

            // Loop body OR retry-rescue (RuboCop process_loop): always
            // referenced — the next iteration may read it.
            if is_in_loop_body(ast, scope_root, asgn_node)
                || is_in_retry_rescue(ast, scope_root, asgn_node, &mut retry_cache)
            {
                var.assignments[i].is_referenced = true;
                continue;
            }

            // Captured-by-block write: an assignment physically inside a block
            // (nested under this variable's declaring scope) is always
            // referenced — the block's run count is indeterminate, so the write
            // can't be proven dead (RuboCop `captured_by_block?`). In-block
            // writes also never act as dominating overwrites of outer writes,
            // since `Block` is a branch barrier and `chain_is_prefix` rejects a
            // deeper chain — so outer dataflow (e.g. `n = 0` killed by an outer
            // `n = 1`) is unaffected.
            if is_in_captured_block(ast, scope_root, asgn_node) {
                var.assignments[i].is_referenced = true;
                continue;
            }

            // Earliest later read that is on a compatible control-flow path.
            let next_read_pos = var
                .references
                .iter()
                .enumerate()
                .filter(|(_, r)| r.pos > asgn_end)
                .filter(|(k, _)| paths_compatible(ast, &ref_chains[*k], &asgn_chains[i]))
                .map(|(_, r)| r.pos)
                .min();

            // Earliest later write that dominates this write (same or shallower
            // branch), not in a protected begin body.
            let dominating_overwrite = var
                .assignments
                .iter()
                .enumerate()
                .filter(|(j, w)| *j != i && w.end > asgn_end)
                .filter(|(j, _)| chain_is_prefix(&asgn_chains[*j], &asgn_chains[i]))
                .filter(|(_, w)| !is_in_protected_begin_body(ast, scope_root, w.node_id))
                .min_by_key(|(_, w)| w.end);

            var.assignments[i].is_referenced = match (next_read_pos, dominating_overwrite) {
                (None, _) => false,
                (Some(r), Some((_, w))) if w.end <= r => false,
                // No single prefix-dominator kills this write before the read,
                // but a begin-body write may still be killed on *every* exit of
                // its rescue by the combination of sibling arms (else + every
                // resbody). RuboCop's CFG model catches this; reconstruct it.
                (Some(r), _) => {
                    !begin_body_distributed_kill(ast, scope_root, &var.assignments, i, r)
                }
            };
        }
    }
}

// ── Internal work item ────────────────────────────────────────────────────────

/// Stack item for the DFS: a node to visit and the scope it belongs to.
struct WorkItem {
    node: NodeId,
    /// The boundary-node key for the scope that owns this node.
    scope: NodeId,
}

// ── impl VarSemanticModel ─────────────────────────────────────────────────────

impl VarSemanticModel {
    /// Build the model in a single DFS pass over the arena.
    pub fn build(ast: &Ast) -> Self {
        let root = ast.root();

        let mut scopes: HashMap<NodeId, ScopeInfo> = HashMap::new();

        // Insert the root scope.  The root node serves as its own boundary key.
        scopes.insert(
            root,
            ScopeInfo {
                parent_scope: None,
                variables: Vec::new(),
            },
        );

        // Seed the work stack.  When the root node is itself a scope boundary
        // (the common `def foo; … end` shape), its scope is already inserted
        // above, so seed its children under the root scope.  When the root is
        // a plain semantic node — e.g. a one-statement file `x = 1` whose root
        // is the `Lvasgn` itself — push the root node so it flows through its
        // own match arm and the assignment is recorded.  Pushing root into a
        // scope-creating arm would self-parent the root scope, so only the
        // non-scope case takes the single-node path.
        let root_is_scope = matches!(
            *ast.kind(root),
            NodeKind::Block { .. }
                | NodeKind::Numblock { .. }
                | NodeKind::Itblock { .. }
                | NodeKind::Def { .. }
                | NodeKind::Defs { .. }
                | NodeKind::Lambda
                | NodeKind::Class { .. }
                | NodeKind::Module { .. }
                | NodeKind::Sclass { .. }
        );
        // Push in reverse order so that `pop()` yields source-order nodes.
        let mut stack: Vec<WorkItem> = if root_is_scope {
            ast.children(root)
                .rev()
                .map(|node| WorkItem { node, scope: root })
                .collect()
        } else {
            vec![WorkItem {
                node: root,
                scope: root,
            }]
        };

        while let Some(WorkItem { node, scope }) = stack.pop() {
            match *ast.kind(node) {
                // ── Scope boundaries ────────────────────────────────────────

                // Block: `call` belongs to the PARENT scope (e.g. the receiver
                // `foo` in `foo.bar { |x| x }` is read in the outer scope).
                // Only `args` and `body` belong to the new block scope.
                NodeKind::Block { call, args, body } => {
                    scopes.insert(
                        node,
                        ScopeInfo {
                            parent_scope: Some(scope),
                            variables: Vec::new(),
                        },
                    );
                    // call → parent scope
                    stack.push(WorkItem { node: call, scope });
                    // args and body → new block scope (body first so args pops first)
                    if let Some(body_id) = body.get() {
                        stack.push(WorkItem {
                            node: body_id,
                            scope: node,
                        });
                    }
                    stack.push(WorkItem {
                        node: args,
                        scope: node,
                    });
                }

                // Numblock: numbered-parameter block; `send` belongs to parent scope.
                NodeKind::Numblock { send, body, .. } => {
                    scopes.insert(
                        node,
                        ScopeInfo {
                            parent_scope: Some(scope),
                            variables: Vec::new(),
                        },
                    );
                    // send → parent scope
                    stack.push(WorkItem { node: send, scope });
                    // body → new block scope
                    if let Some(body_id) = body.get() {
                        stack.push(WorkItem {
                            node: body_id,
                            scope: node,
                        });
                    }
                }

                // Itblock: `it`-parameter block; `send` belongs to parent scope.
                NodeKind::Itblock { send, body } => {
                    scopes.insert(
                        node,
                        ScopeInfo {
                            parent_scope: Some(scope),
                            variables: Vec::new(),
                        },
                    );
                    // send → parent scope
                    stack.push(WorkItem { node: send, scope });
                    // body → new block scope
                    if let Some(body_id) = body.get() {
                        stack.push(WorkItem {
                            node: body_id,
                            scope: node,
                        });
                    }
                }

                // Other scope boundaries: all children belong to the new scope.
                NodeKind::Def { .. }
                | NodeKind::Defs { .. }
                | NodeKind::Lambda
                | NodeKind::Class { .. }
                | NodeKind::Module { .. }
                | NodeKind::Sclass { .. } => {
                    // Create a new scope keyed by this boundary node.
                    scopes.insert(
                        node,
                        ScopeInfo {
                            parent_scope: Some(scope),
                            variables: Vec::new(),
                        },
                    );
                    // Children of this boundary belong to the NEW scope.
                    for child in ast.children(node).rev() {
                        stack.push(WorkItem {
                            node: child,
                            scope: node,
                        });
                    }
                }

                // ── Argument nodes (belong to current scope) ────────────────
                NodeKind::Arg(name)
                | NodeKind::Restarg(name)
                | NodeKind::Kwarg(name)
                | NodeKind::Kwrestarg(name)
                | NodeKind::Blockarg(name) => {
                    if !Self::is_underscore_prefix(name, ast) {
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                        Self::find_or_declare_arg(scope_info, name, node);
                    }
                    // These are leaf nodes; no children to push.
                }

                // ── Shadow arg: `|x; y|` where `y` is block-local ───────────
                // Declares a variable in the current (block) scope so that
                // Lint/ShadowingOuterLocalVariable can detect when a shadow arg
                // shadows an outer variable.
                NodeKind::Shadowarg(name) => {
                    if !Self::is_underscore_prefix(name, ast) {
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                        Self::find_or_declare_arg(scope_info, name, node);
                    }
                    // Leaf node; no children.
                }

                NodeKind::Optarg { name, default } => {
                    if !Self::is_underscore_prefix(name, ast) {
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                        Self::find_or_declare_arg(scope_info, name, node);
                    }
                    // Recurse into the default expression.
                    stack.push(WorkItem {
                        node: default,
                        scope,
                    });
                }

                NodeKind::Kwoptarg { name, default } => {
                    if !Self::is_underscore_prefix(name, ast) {
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                        Self::find_or_declare_arg(scope_info, name, node);
                    }
                    stack.push(WorkItem {
                        node: default,
                        scope,
                    });
                }

                // ── Plain assignment: `x = expr` ────────────────────────────
                NodeKind::Lvasgn { name, value } => {
                    // Only a full assignment (with value) registers an Assignment.
                    // A value-less Lvasgn is a target placeholder inside
                    // OpAsgn/OrAsgn/AndAsgn/Masgn; it is handled by those arms.
                    // If traversal reaches one directly, it is an exposed
                    // binding target such as regexp named captures under
                    // MatchWithLvasgn.
                    if let Some(val_id) = value.get() {
                        if !Self::is_underscore_prefix(name, ast) {
                            let end = ast.range(node).end;
                            let target_scope =
                                Self::scope_containing_variable(&scopes, scope, name)
                                    .unwrap_or(scope);
                            let scope_info =
                                scopes.get_mut(&target_scope).expect("scope must exist");
                            let var = Self::find_or_declare_local(scope_info, name, node);
                            var.assignments.push(Assignment {
                                node_id: node,
                                end,
                                is_referenced: false,
                            });
                        }
                        // Recurse into the value expression.
                        stack.push(WorkItem {
                            node: val_id,
                            scope,
                        });
                    } else if !Self::is_underscore_prefix(name, ast) {
                        let end = ast
                            .parent(node)
                            .get()
                            .map(|parent| ast.range(parent).end)
                            .unwrap_or_else(|| ast.range(node).end);
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                        let var = Self::find_or_declare_local(scope_info, name, node);
                        var.assignments.push(Assignment {
                            node_id: node,
                            end,
                            is_referenced: false,
                        });
                    }
                    // Value-less targets have no children to recurse into.
                }

                // ── Pattern matching binding: `in [x]` / `in {x: y}` ───────
                NodeKind::MatchVar(name) => {
                    if !Self::is_underscore_prefix(name, ast) {
                        let end = ast.range(node).end;
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                        let var = Self::find_or_declare_local(scope_info, name, node);
                        var.assignments.push(Assignment {
                            node_id: node,
                            end,
                            is_referenced: false,
                        });
                    }
                }

                // ── Compound assignment: `x op= expr` ───────────────────────
                NodeKind::OpAsgn { target, value, .. } => {
                    // Target is always a value-less write node; for Lvasgn:
                    // push a Reference (read side) + an Assignment (write side).
                    if let NodeKind::Lvasgn { name, .. } = *ast.kind(target)
                        && !Self::is_underscore_prefix(name, ast)
                    {
                        let target_range = ast.range(target);
                        let asgn_end = ast.range(node).end;
                        let target_scope =
                            Self::scope_containing_variable(&scopes, scope, name).unwrap_or(scope);
                        let scope_info = scopes.get_mut(&target_scope).expect("scope must exist");
                        let var = Self::find_or_declare_local(scope_info, name, target);
                        var.references.push(Reference {
                            node_id: target,
                            pos: target_range.start,
                        });
                        var.assignments.push(Assignment {
                            node_id: node,
                            end: asgn_end,
                            is_referenced: false,
                        });
                        // Value-less Lvasgn has no sub-children; only push value.
                    } else {
                        // Non-local target (e.g. attr/index write): recurse into
                        // target so any lvar reads inside it are collected.
                        stack.push(WorkItem {
                            node: target,
                            scope,
                        });
                    }
                    stack.push(WorkItem { node: value, scope });
                }

                // ── ||= / &&= ────────────────────────────────────────────────
                NodeKind::OrAsgn { target, value } | NodeKind::AndAsgn { target, value } => {
                    if let NodeKind::Lvasgn { name, .. } = *ast.kind(target)
                        && !Self::is_underscore_prefix(name, ast)
                    {
                        let target_range = ast.range(target);
                        let asgn_end = ast.range(node).end;
                        let target_scope =
                            Self::scope_containing_variable(&scopes, scope, name).unwrap_or(scope);
                        let scope_info = scopes.get_mut(&target_scope).expect("scope must exist");
                        let var = Self::find_or_declare_local(scope_info, name, target);
                        var.references.push(Reference {
                            node_id: target,
                            pos: target_range.start,
                        });
                        var.assignments.push(Assignment {
                            node_id: node,
                            end: asgn_end,
                            is_referenced: false,
                        });
                        // Value-less Lvasgn has no sub-children; only push value.
                    } else {
                        // Non-local target: recurse so inner lvar reads are collected.
                        stack.push(WorkItem {
                            node: target,
                            scope,
                        });
                    }
                    stack.push(WorkItem { node: value, scope });
                }

                // ── Multiple assignment: `a, b = rhs` ───────────────────────
                NodeKind::Masgn { lhs, rhs } => {
                    // Walk the Mlhs recursively to collect Lvasgn targets.
                    Self::collect_mlhs_targets(ast, lhs, node, scope, &mut scopes);
                    // Recurse into the RHS.
                    stack.push(WorkItem { node: rhs, scope });
                }

                // ── Exception variable binding: `rescue Exc => e` ───────────
                NodeKind::Resbody { var, .. } => {
                    // Classify the binding variable as an assignment in this scope.
                    if let Some(var_id) = var.get()
                        && let NodeKind::Lvasgn { name, .. } = *ast.kind(var_id)
                        && !Self::is_underscore_prefix(name, ast)
                    {
                        let end = ast.range(var_id).end;
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                        let v = Self::find_or_declare_local(scope_info, name, var_id);
                        v.assignments.push(Assignment {
                            node_id: var_id,
                            end,
                            is_referenced: false,
                        });
                    }
                    // Recurse into exception class list and body, but NOT the
                    // var node (it's a value-less Lvasgn already handled above).
                    for child in ast.children(node).rev() {
                        if var.get() == Some(child) {
                            // Skip the var: already classified above.
                            continue;
                        }
                        stack.push(WorkItem { node: child, scope });
                    }
                }

                // ── `for x in iter; body; end` ──────────────────────────────
                NodeKind::For { var, iter, body } => {
                    Self::collect_for_var(ast, &mut scopes, scope, var, iter);
                    // Recurse into iter and body (var targets have no sub-children).
                    stack.push(WorkItem { node: iter, scope });
                    if let Some(b) = body.get() {
                        stack.push(WorkItem { node: b, scope });
                    }
                }

                // ── Variable read ────────────────────────────────────────────
                NodeKind::Lvar(name) => {
                    if !Self::is_underscore_prefix(name, ast) {
                        let pos = ast.range(node).start;
                        let target_scope =
                            Self::scope_containing_variable(&scopes, scope, name).unwrap_or(scope);
                        let scope_info = scopes.get_mut(&target_scope).expect("scope must exist");
                        let var = Self::find_or_declare_local(scope_info, name, node);
                        var.references.push(Reference { node_id: node, pos });
                    }
                    // Leaf node; no children.
                }

                NodeKind::Pair { key, value }
                    if matches!(ast.kind(key), NodeKind::Sym(_))
                        && matches!(ast.kind(value), NodeKind::Unknown) =>
                {
                    if let NodeKind::Sym(name) = *ast.kind(key)
                        && !Self::is_underscore_prefix(name, ast)
                    {
                        let pos = ast.range(key).start;
                        let target_scope =
                            Self::scope_containing_variable(&scopes, scope, name).unwrap_or(scope);
                        let scope_info = scopes.get_mut(&target_scope).expect("scope must exist");
                        let var = Self::find_or_declare_local(scope_info, name, key);
                        var.references.push(Reference { node_id: key, pos });
                    }
                }

                // ── All other nodes: classify children under the same scope ──
                _ => {
                    for child in ast.children(node).rev() {
                        stack.push(WorkItem { node: child, scope });
                    }
                }
            }
        }

        // Post-pass: compute is_referenced for every assignment in every scope.
        for (&root_id, scope) in scopes.iter_mut() {
            analyze_scope_is_referenced(ast, root_id, scope);
        }

        VarSemanticModel { scopes }
    }

    /// Retrieve the `ScopeInfo` keyed by `boundary_node`.
    pub fn scope(&self, boundary_node: NodeId) -> Option<&ScopeInfo> {
        self.scopes.get(&boundary_node)
    }

    /// Iterate over all scopes: `(boundary_node_id, &ScopeInfo)`.
    ///
    /// Sorted by `NodeId` so the iteration order is deterministic
    /// (`HashMap` iteration order is not).
    pub fn scopes(&self) -> impl Iterator<Item = (NodeId, &ScopeInfo)> {
        let mut pairs: Vec<(NodeId, &ScopeInfo)> =
            self.scopes.iter().map(|(&id, s)| (id, s)).collect();
        pairs.sort_by_key(|(id, _)| id.0);
        pairs.into_iter()
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Returns `true` if the symbol starts with `_` (intentionally-unused marker).
    fn is_underscore_prefix(name: Symbol, ast: &Ast) -> bool {
        ast.interner().resolve(name.0).starts_with('_')
    }

    /// Find or insert an argument variable in `scope_info`.
    fn find_or_declare_arg(
        scope_info: &mut ScopeInfo,
        name: Symbol,
        node: NodeId,
    ) -> &mut Variable {
        let pos = scope_info.variables.iter().position(|v| v.name == name);
        if let Some(idx) = pos {
            &mut scope_info.variables[idx]
        } else {
            scope_info.variables.push(Variable {
                name,
                is_argument: true,
                declaration_node: node,
                assignments: Vec::new(),
                references: Vec::new(),
            });
            scope_info.variables.last_mut().unwrap()
        }
    }

    /// Find or insert a local-variable entry in `scope_info`.
    fn find_or_declare_local(
        scope_info: &mut ScopeInfo,
        name: Symbol,
        node: NodeId,
    ) -> &mut Variable {
        let pos = scope_info.variables.iter().position(|v| v.name == name);
        if let Some(idx) = pos {
            &mut scope_info.variables[idx]
        } else {
            scope_info.variables.push(Variable {
                name,
                is_argument: false,
                declaration_node: node,
                assignments: Vec::new(),
                references: Vec::new(),
            });
            scope_info.variables.last_mut().unwrap()
        }
    }

    fn scope_containing_variable(
        scopes: &HashMap<NodeId, ScopeInfo>,
        scope: NodeId,
        name: Symbol,
    ) -> Option<NodeId> {
        let mut current = Some(scope);
        while let Some(scope_id) = current {
            let scope_info = scopes.get(&scope_id)?;
            if scope_info.variables.iter().any(|var| var.name == name) {
                return Some(scope_id);
            }
            current = scope_info.parent_scope;
        }
        None
    }

    /// Walk an `Mlhs` node recursively, pushing `Assignment` entries for each
    /// `Lvasgn` target found.
    fn collect_mlhs_targets(
        ast: &Ast,
        mlhs_node: NodeId,
        asgn_node: NodeId,
        scope: NodeId,
        scopes: &mut HashMap<NodeId, ScopeInfo>,
    ) {
        for child in ast.children(mlhs_node) {
            match *ast.kind(child) {
                NodeKind::Lvasgn { name, .. }
                    if !ast.interner().resolve(name.0).starts_with('_') =>
                {
                    let end = ast.range(asgn_node).end;
                    let target_scope =
                        Self::scope_containing_variable(scopes, scope, name).unwrap_or(scope);
                    let scope_info = scopes.get_mut(&target_scope).expect("scope must exist");
                    let var = Self::find_or_declare_local(scope_info, name, child);
                    var.assignments.push(Assignment {
                        node_id: child,
                        end,
                        is_referenced: false,
                    });
                }
                NodeKind::Mlhs(_) => {
                    // Nested Mlhs (e.g. `(a, b), c = ...`).
                    Self::collect_mlhs_targets(ast, child, asgn_node, scope, scopes);
                }
                _ => {}
            }
        }
    }

    /// Handle `For { var, .. }` — var may be a plain `Lvasgn` or an `Mlhs`
    /// for destructuring loops (`for a, b in list`).
    ///
    /// In `for x in iter`, Ruby evaluates `iter` *before* binding `x`, so the
    /// assignment's `end` must sit *after* the iter expression's byte range —
    /// we use `iter`'s end. Otherwise `x = [1]; for x in x; end` would wrongly
    /// treat the earlier `x = [1]` as overwritten before its read inside
    /// `iter`. Using the *whole* for-node's end would overshoot past the body
    /// and exclude in-loop reads, so `iter`'s end is the right boundary.
    fn collect_for_var(
        ast: &Ast,
        scopes: &mut HashMap<NodeId, ScopeInfo>,
        scope: NodeId,
        var: NodeId,
        iter: NodeId,
    ) {
        match *ast.kind(var) {
            NodeKind::Lvasgn { name, .. } if !Self::is_underscore_prefix(name, ast) => {
                let end = ast.range(iter).end;
                let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                let v = Self::find_or_declare_local(scope_info, name, var);
                v.assignments.push(Assignment {
                    node_id: var,
                    end,
                    is_referenced: false,
                });
            }
            NodeKind::Mlhs(_) => {
                // Destructuring: `for a, b in list` — walk Mlhs children.
                Self::collect_mlhs_targets(ast, var, iter, scope, scopes);
            }
            _ => {}
        }
    }
}

// ── impl ScopeInfo ────────────────────────────────────────────────────────────

impl ScopeInfo {
    /// Navigate to the parent scope's `ScopeInfo`, if any.
    pub fn parent_scope<'a>(&self, model: &'a VarSemanticModel) -> Option<&'a ScopeInfo> {
        self.parent_scope.and_then(|id| model.scopes.get(&id))
    }

    /// All variables in this scope.
    pub fn variables(&self) -> &[Variable] {
        &self.variables
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use murphy_translate::translate;

    /// For a single-statement program, `translate` sets root to the statement
    /// itself (no `Begin` wrapper).  Helper to find the first node matching a
    /// predicate via DFS.
    fn find_node<F: Fn(&NodeKind) -> bool>(ast: &Ast, pred: F) -> Option<NodeId> {
        let mut stack: Vec<NodeId> = vec![ast.root()];
        while let Some(id) = stack.pop() {
            if pred(ast.kind(id)) {
                return Some(id);
            }
            for c in ast.children(id).rev() {
                stack.push(c);
            }
        }
        None
    }

    #[test]
    fn collects_scope_and_variable() {
        let ast = translate("def foo; x = 1; end", "test.rb");
        let model = VarSemanticModel::build(&ast);

        // Root IS the Def node for a single-statement file.
        let def_id = find_node(&ast, |k| matches!(k, NodeKind::Def { .. })).expect("def node");

        let scope = model.scope(def_id).expect("scope for def");
        assert_eq!(scope.variables.len(), 1, "should have variable x");
        assert_eq!(scope.variables[0].assignments.len(), 1);
        assert_eq!(scope.variables[0].references.len(), 0);
        assert!(!scope.variables[0].is_argument);
    }

    #[test]
    fn collects_argument() {
        let ast = translate("def foo(x); end", "test.rb");
        let model = VarSemanticModel::build(&ast);

        let def_id = find_node(&ast, |k| matches!(k, NodeKind::Def { .. })).expect("def node");

        let scope = model.scope(def_id).expect("scope for def");
        assert_eq!(scope.variables.len(), 1);
        assert!(scope.variables[0].is_argument);
    }

    #[test]
    fn nested_scope_has_parent() {
        let ast = translate("def foo; [1].each {|x| x }; end", "test.rb");
        let model = VarSemanticModel::build(&ast);

        // Should have at least two scopes: the def and the block.
        assert!(
            model.scopes().count() >= 2,
            "should have def + block scopes"
        );

        // The block scope should have a parent pointing to the def scope.
        let def_id = find_node(&ast, |k| matches!(k, NodeKind::Def { .. })).expect("def node");
        let block_id =
            find_node(&ast, |k| matches!(k, NodeKind::Block { .. })).expect("block node");

        let block_scope = model.scope(block_id).expect("block scope");
        assert_eq!(
            block_scope.parent_scope,
            Some(def_id),
            "block scope parent should be the def scope"
        );
    }

    /// Block.call (e.g. the receiver `arr` in `arr.each { |x| x }`) must be
    /// attributed to the OUTER scope, not the block scope.
    #[test]
    fn block_call_attributed_to_outer_scope() {
        // `arr` is assigned in the outer (def) scope, then used as the receiver
        // of `.each`.  The `Lvar arr` read is part of `Block.call`, so it must
        // land in the def scope, not in the block scope.
        let ast = translate("def foo; arr = [1]; arr.each { |x| x }; end", "test.rb");
        let model = VarSemanticModel::build(&ast);

        let def_id = find_node(&ast, |k| matches!(k, NodeKind::Def { .. })).expect("def node");
        let block_id =
            find_node(&ast, |k| matches!(k, NodeKind::Block { .. })).expect("block node");

        let def_scope = model.scope(def_id).expect("def scope");
        let block_scope = model.scope(block_id).expect("block scope");

        // `arr` should appear in the def scope (1 assignment + 1 reference).
        let arr_in_def = def_scope
            .variables
            .iter()
            .find(|v| ast.interner().resolve(v.name.0) == "arr");
        assert!(
            arr_in_def.is_some(),
            "`arr` must be tracked in the def scope"
        );
        let arr = arr_in_def.unwrap();
        assert_eq!(arr.assignments.len(), 1, "`arr` has one assignment");
        assert_eq!(
            arr.references.len(),
            1,
            "`arr` has one reference (the block call)"
        );

        // `arr` must NOT appear in the block scope.
        let arr_in_block = block_scope
            .variables
            .iter()
            .find(|v| ast.interner().resolve(v.name.0) == "arr");
        assert!(
            arr_in_block.is_none(),
            "`arr` must NOT appear in the block scope"
        );
    }

    // ── is_referenced tests ───────────────────────────────────────────────────

    /// Helper: find the first scope node matching a predicate.
    fn find_scope_node(
        ast: &Ast,
        root: NodeId,
        pred: impl Fn(&NodeKind) -> bool,
    ) -> Option<NodeId> {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if pred(ast.kind(node)) {
                return Some(node);
            }
            for c in ast.children(node).rev() {
                stack.push(c);
            }
        }
        None
    }

    /// Helper: resolve a `Symbol` to its string.
    fn resolve_sym(ast: &Ast, sym: Symbol) -> &str {
        ast.interner().resolve(sym.0)
    }

    #[test]
    fn root_lvasgn_assignment_is_collected() {
        // One-statement file: the root node IS the `Lvasgn`, not a scope
        // boundary. `build` must still record the assignment in the root scope.
        let ast = translate("x = 1", "test.rb");
        let model = VarSemanticModel::build(&ast);
        let scope = model.scope(ast.root()).expect("root scope");
        let x = scope
            .variables
            .iter()
            .find(|v| resolve_sym(&ast, v.name) == "x")
            .expect("x must be collected when the root node is the assignment");
        assert_eq!(x.assignments.len(), 1);
        assert!(
            !x.assignments[0].is_referenced,
            "unused root assignment should not be referenced"
        );
    }

    #[test]
    fn root_masgn_targets_are_collected() {
        // Root is the `Masgn` itself; both targets must be recorded.
        let ast = translate("a, b = 1, 2", "test.rb");
        let model = VarSemanticModel::build(&ast);
        let scope = model.scope(ast.root()).expect("root scope");
        for name in ["a", "b"] {
            let v = scope
                .variables
                .iter()
                .find(|v| resolve_sym(&ast, v.name) == name)
                .unwrap_or_else(|| panic!("`{name}` must be collected"));
            assert_eq!(v.assignments.len(), 1);
        }
    }

    #[test]
    fn unused_assignment_not_referenced() {
        let ast = translate("def foo; x = 1; end", "test.rb");
        let model = VarSemanticModel::build(&ast);
        let def_id =
            find_scope_node(&ast, ast.root(), |k| matches!(k, NodeKind::Def { .. })).unwrap();
        let scope = model.scope(def_id).unwrap();
        let x = scope
            .variables
            .iter()
            .find(|v| resolve_sym(&ast, v.name) == "x")
            .unwrap();
        assert!(
            !x.assignments[0].is_referenced,
            "unused x should not be referenced"
        );
    }

    #[test]
    fn used_assignment_is_referenced() {
        let ast = translate("def foo; x = 1; puts x; end", "test.rb");
        let model = VarSemanticModel::build(&ast);
        let def_id =
            find_scope_node(&ast, ast.root(), |k| matches!(k, NodeKind::Def { .. })).unwrap();
        let scope = model.scope(def_id).unwrap();
        let x = scope
            .variables
            .iter()
            .find(|v| resolve_sym(&ast, v.name) == "x")
            .unwrap();
        assert!(
            x.assignments[0].is_referenced,
            "used x should be referenced"
        );
    }

    #[test]
    fn exclusive_branches_both_referenced() {
        // Both branches assign x; after the if, x is read — both writes are referenced.
        let ast = translate(
            "def foo(c); if c; x = 1; else; x = 2; end; puts x; end",
            "test.rb",
        );
        let model = VarSemanticModel::build(&ast);
        let def_id =
            find_scope_node(&ast, ast.root(), |k| matches!(k, NodeKind::Def { .. })).unwrap();
        let scope = model.scope(def_id).unwrap();
        let x = scope
            .variables
            .iter()
            .find(|v| resolve_sym(&ast, v.name) == "x")
            .unwrap();
        assert!(
            x.assignments.iter().all(|a| a.is_referenced),
            "both branch assignments should be referenced"
        );
    }

    #[test]
    fn overwrite_before_read_not_referenced() {
        // x = 1; x = 2; puts x — first assignment is overwritten before read.
        let ast = translate("def foo; x = 1; x = 2; puts x; end", "test.rb");
        let model = VarSemanticModel::build(&ast);
        let def_id =
            find_scope_node(&ast, ast.root(), |k| matches!(k, NodeKind::Def { .. })).unwrap();
        let scope = model.scope(def_id).unwrap();
        let x = scope
            .variables
            .iter()
            .find(|v| resolve_sym(&ast, v.name) == "x")
            .unwrap();
        // First assignment (x = 1) should NOT be referenced (overwritten before read).
        assert!(
            !x.assignments[0].is_referenced,
            "first overwritten assignment should not be referenced"
        );
        // Second assignment (x = 2) SHOULD be referenced.
        assert!(
            x.assignments[1].is_referenced,
            "second assignment that is read should be referenced"
        );
    }

    #[test]
    fn case_in_branches_are_exclusive_barriers() {
        // x = 1 in the first `in` arm is overwritten by x = 2 in the second arm,
        // but they're exclusive, so both assignments should be is_referenced = true.
        let ast = translate(
            "def foo(v); case v; in 1; x = 1; in 2; x = 2; end; puts x; end",
            "test.rb",
        );
        let model = VarSemanticModel::build(&ast);
        let def_id =
            find_scope_node(&ast, ast.root(), |k| matches!(k, NodeKind::Def { .. })).unwrap();
        let scope = model.scope(def_id).unwrap();
        let x = scope
            .variables
            .iter()
            .find(|v| resolve_sym(&ast, v.name) == "x")
            .unwrap();
        assert!(
            x.assignments.iter().all(|a| a.is_referenced),
            "both in-pattern arms should be referenced (exclusive branches)"
        );
    }

    #[test]
    fn for_var_does_not_overwrite_outer_assignment_read_in_iter() {
        // `x = [1]; for x in x; end` — Ruby evaluates the iter `x` (reading
        // `x = [1]`) before binding the loop variable. The for-var write must
        // therefore end *after* the iter read, so `x = [1]` is observed and
        // not flagged as overwritten-before-read.
        let ast = translate("x = [1]\nfor x in x\nend\n", "test.rb");
        let model = VarSemanticModel::build(&ast);
        let scope = model.scope(ast.root()).expect("root scope");
        let x = scope
            .variables
            .iter()
            .find(|v| resolve_sym(&ast, v.name) == "x")
            .expect("x must be tracked");
        // First assignment is `x = [1]`; it is read by the iter expression.
        assert!(
            x.assignments[0].is_referenced,
            "`x = [1]` is read by the for-loop iter and must be referenced"
        );
    }

    #[test]
    fn for_mlhs_destructuring_tracked() {
        // `for a, b in [[1, 2]]` — both a and b should be declared in the scope.
        let ast = translate("for a, b in [[1, 2]]; puts a; end", "test.rb");
        let model = VarSemanticModel::build(&ast);
        let root_scope = model.scope(ast.root()).unwrap();
        let names: Vec<&str> = root_scope
            .variables
            .iter()
            .map(|v| ast.interner().resolve(v.name.0))
            .collect();
        assert!(names.contains(&"a"), "a should be tracked; got {names:?}");
        assert!(names.contains(&"b"), "b should be tracked; got {names:?}");
    }

    #[test]
    fn block_or_assignment_to_outer_local_is_referenced_by_later_read() {
        let ast = translate(
            "error = nil\naccounts.each do |account|\n  follow(account)\nrescue NotPermitted => e\n  error ||= e\nend\nraise error if error.present?\n",
            "test.rb",
        );
        let model = VarSemanticModel::build(&ast);
        let scope = model.scope(ast.root()).expect("root scope");
        let error = scope
            .variables
            .iter()
            .find(|v| resolve_sym(&ast, v.name) == "error")
            .expect("error must be tracked in the outer scope");

        assert_eq!(error.assignments.len(), 2);
        assert_eq!(error.references.len(), 3);
        assert!(
            error.assignments.iter().all(|a| a.is_referenced),
            "both error writes are observed by later reads"
        );
    }

    #[test]
    fn masgn_target_with_block_rhs_is_referenced_by_later_read() {
        let ast = translate(
            "_, pending, processed, async_refresh_key, threshold = redis.multi do |pipeline|\n  pipeline.hget(key, 'threshold')\nend\n\nif pending.zero? || processed >= (threshold || 1.0).to_f * (processed + pending)\n  cleanup\nend\n",
            "test.rb",
        );
        let model = VarSemanticModel::build(&ast);
        let scope = model.scope(ast.root()).expect("root scope");
        let threshold = scope
            .variables
            .iter()
            .find(|v| resolve_sym(&ast, v.name) == "threshold")
            .expect("threshold must be tracked");

        assert_eq!(threshold.assignments.len(), 1);
        assert_eq!(threshold.references.len(), 1);
        assert!(
            threshold.assignments[0].is_referenced,
            "threshold destructuring assignment is observed by the later condition"
        );
    }

    // ── captured-by-block writes (Mastodon FP fix) ──────────────────────────
    //
    // RuboCop's `Lint/UselessAssignment` never flags a write that is *physically
    // inside a block* to a variable captured from an enclosing scope — the block
    // may run zero or many times, so the write's value is indeterminate and
    // cannot be proven dead. Verified against standalone rubocop 1.87.0.

    /// Look up the outer-scope variable `name` in `def`'s scope and return it.
    fn captured_var<'a>(ast: &'a Ast, model: &'a VarSemanticModel, name: &str) -> &'a Variable {
        let def_id =
            find_scope_node(ast, ast.root(), |k| matches!(k, NodeKind::Def { .. })).unwrap();
        let scope = model.scope(def_id).unwrap();
        scope
            .variables
            .iter()
            .find(|v| resolve_sym(ast, v.name) == name)
            .unwrap_or_else(|| panic!("`{name}` must be tracked in the def scope"))
    }

    #[test]
    fn in_block_compound_write_to_captured_var_is_referenced() {
        // Target Mastodon FP: `n += 1` inside `each` writes the captured `n`.
        // The back-edge read `use(n)` is positionally before the write, so the
        // forward scan misses it — but the in-block write must still be
        // referenced (RuboCop: clean).
        let ast = translate(
            "def f(items)\n  n = 0\n  items.each do |x|\n    use(n)\n    n += 1\n  end\nend\n",
            "test.rb",
        );
        let model = VarSemanticModel::build(&ast);
        let n = captured_var(&ast, &model, "n");
        // Two writes: `n = 0` (outer) and `n += 1` (in block).
        assert_eq!(n.assignments.len(), 2);
        // The in-block `n += 1` must be referenced (no longer a false useless).
        assert!(
            n.assignments.iter().all(|a| a.is_referenced),
            "captured `n` writes (incl. the in-block `n += 1`) must be referenced"
        );
    }

    #[test]
    fn in_block_overwrite_of_captured_var_both_referenced() {
        // `n = 5; n = 6` both inside the block — even though `n = 6` lexically
        // dominates `n = 5`, neither in-block write is flagged. In-block writes
        // never dominate, and are themselves always referenced.
        //
        // (Accepted divergence from RuboCop, NOT a target FP: the *outer*
        // `n = 0` here — written only inside the block, never read — is still
        // flagged by Murphy. RuboCop treats the capture as a virtual read at
        // the block site and stays clean. This test only pins the in-block
        // writes; the outer-write divergence is documented and out of scope.)
        let ast = translate(
            "def f\n  n = 0\n  [1, 2].each do\n    n = 5\n    n = 6\n  end\nend\n",
            "test.rb",
        );
        let model = VarSemanticModel::build(&ast);
        let n = captured_var(&ast, &model, "n");
        // `n = 0` (outer) + `n = 5` + `n = 6` (both in block).
        assert_eq!(n.assignments.len(), 3);
        // Identify in-block writes by walking parents to a Block before the Def.
        let def_id =
            find_scope_node(&ast, ast.root(), |k| matches!(k, NodeKind::Def { .. })).unwrap();
        let is_in_block = |node: NodeId| -> bool {
            let mut cur = node;
            while let Some(p) = ast.parent(cur).get() {
                if p == def_id {
                    return false;
                }
                if matches!(ast.kind(p), NodeKind::Block { .. }) {
                    return true;
                }
                cur = p;
            }
            false
        };
        let in_block: Vec<&Assignment> = n
            .assignments
            .iter()
            .filter(|a| is_in_block(a.node_id))
            .collect();
        assert_eq!(in_block.len(), 2, "two in-block writes to captured `n`");
        assert!(
            in_block.iter().all(|a| a.is_referenced),
            "both in-block writes to captured `n` must be referenced"
        );
    }

    #[test]
    fn outer_dominated_write_still_flagged_despite_capture() {
        // Discriminator: `n = 0; n = 1; each { use(n) }` — `n = 0` is dominated
        // by the outer `n = 1` before any read, so it STAYS flagged even though
        // `n` is captured (RuboCop: `n = 0` flagged, `n = 1` clean). Capture
        // does NOT blanket-mark outer writes.
        let ast = translate(
            "def f\n  n = 0\n  n = 1\n  [1, 2].each { use(n) }\nend\n",
            "test.rb",
        );
        let model = VarSemanticModel::build(&ast);
        let n = captured_var(&ast, &model, "n");
        assert_eq!(n.assignments.len(), 2, "two outer writes: n=0, n=1");
        // n = 0 (first, dominated) must NOT be referenced.
        assert!(
            !n.assignments[0].is_referenced,
            "outer `n = 0` dominated by `n = 1` must stay flagged"
        );
        // n = 1 (read via block) must be referenced.
        assert!(
            n.assignments[1].is_referenced,
            "outer `n = 1` read by the block must be referenced"
        );
    }

    #[test]
    fn block_local_dead_write_still_flagged() {
        // Regression guard g1: `dead` is a *block-local* (not captured), assigned
        // and never read → still useless (RuboCop: flagged).
        let ast = translate("[1, 2].each { |x| dead = x }", "test.rb");
        let model = VarSemanticModel::build(&ast);
        let block_id =
            find_scope_node(&ast, ast.root(), |k| matches!(k, NodeKind::Block { .. })).unwrap();
        let scope = model.scope(block_id).expect("block scope");
        let dead = scope
            .variables
            .iter()
            .find(|v| resolve_sym(&ast, v.name) == "dead")
            .expect("`dead` is block-local");
        assert!(
            !dead.assignments[0].is_referenced,
            "block-local dead write must stay flagged (not captured)"
        );
    }

    #[test]
    fn assignment_in_block_call_arguments_is_not_captured() {
        // Regression guard g4: `n = 1` sits in the block's RECEIVER CALL
        // arguments (`items.each(n = 1) { … }`), which run in the parent scope
        // at the call site — not the deferred block body. With `n` never read it
        // must stay flagged (RuboCop: `n = 1` is useless); capture must not
        // force-mark a call-side assignment referenced.
        let ast = translate(
            "def f(items)\n  items.each(n = 1) { |x| x }\nend\n",
            "test.rb",
        );
        let model = VarSemanticModel::build(&ast);
        let n = captured_var(&ast, &model, "n");
        assert_eq!(n.assignments.len(), 1, "single write `n = 1`");
        assert!(
            !n.assignments[0].is_referenced,
            "call-argument `n = 1` (parent scope, never read) must stay flagged"
        );
    }
}
