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
    /// Placeholder for Task 2 branch analysis; always `false` here.
    pub is_referenced: bool,
}

/// One read site for a local variable.
pub struct Reference {
    /// The `Lvar` node that reads the variable.
    pub node_id: NodeId,
    /// Byte position of the reference start.
    pub pos: u32,
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

        // Seed the work stack with the root's children, all owned by root.
        // Push in reverse order so that `pop()` yields source-order nodes.
        let mut stack: Vec<WorkItem> = ast
            .children(root)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|node| WorkItem { node, scope: root })
            .collect();

        while let Some(WorkItem { node, scope }) = stack.pop() {
            match *ast.kind(node) {
                // ── Scope boundaries ────────────────────────────────────────
                NodeKind::Def { .. }
                | NodeKind::Defs { .. }
                | NodeKind::Block { .. }
                | NodeKind::Lambda
                | NodeKind::Class { .. }
                | NodeKind::Module { .. }
                | NodeKind::Sclass { .. }
                | NodeKind::Numblock { .. }
                | NodeKind::Itblock { .. } => {
                    // Create a new scope keyed by this boundary node.
                    scopes.insert(
                        node,
                        ScopeInfo {
                            parent_scope: Some(scope),
                            variables: Vec::new(),
                        },
                    );
                    // Children of this boundary belong to the NEW scope.
                    let children: Vec<NodeId> = ast.children(node).collect();
                    for child in children.into_iter().rev() {
                        stack.push(WorkItem { node: child, scope: node });
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

                NodeKind::Optarg { name, default } => {
                    if !Self::is_underscore_prefix(name, ast) {
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                        Self::find_or_declare_arg(scope_info, name, node);
                    }
                    // Recurse into the default expression.
                    stack.push(WorkItem { node: default, scope });
                }

                NodeKind::Kwoptarg { name, default } => {
                    if !Self::is_underscore_prefix(name, ast) {
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                        Self::find_or_declare_arg(scope_info, name, node);
                    }
                    stack.push(WorkItem { node: default, scope });
                }

                // ── Plain assignment: `x = expr` ────────────────────────────
                NodeKind::Lvasgn { name, value } => {
                    // Only a full assignment (with value) registers an Assignment.
                    // A value-less Lvasgn is a target placeholder inside
                    // OpAsgn/OrAsgn/AndAsgn/Masgn; it is handled by those arms.
                    if let Some(val_id) = value.get() {
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
                        // Recurse into the value expression.
                        stack.push(WorkItem { node: val_id, scope });
                    }
                    // Value-less targets have no children to recurse into.
                }

                // ── Compound assignment: `x op= expr` ───────────────────────
                NodeKind::OpAsgn { target, value, .. } => {
                    // Target is always a value-less write node; for Lvasgn:
                    // push a Reference (read side) + an Assignment (write side).
                    if let NodeKind::Lvasgn { name, .. } = *ast.kind(target) && !Self::is_underscore_prefix(name, ast) {
                        let target_range = ast.range(target);
                        let asgn_end = ast.range(node).end;
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
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
                    }
                    // Recurse into the RHS value (target has no sub-children for Lvasgn).
                    stack.push(WorkItem { node: value, scope });
                }

                // ── ||= / &&= ────────────────────────────────────────────────
                NodeKind::OrAsgn { target, value } | NodeKind::AndAsgn { target, value } => {
                    if let NodeKind::Lvasgn { name, .. } = *ast.kind(target) && !Self::is_underscore_prefix(name, ast) {
                        let target_range = ast.range(target);
                        let asgn_end = ast.range(node).end;
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
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
                    let all_children: Vec<NodeId> = ast.children(node).collect();
                    for child in all_children.into_iter().rev() {
                        if var.get() == Some(child) {
                            // Skip the var: already classified above.
                            continue;
                        }
                        stack.push(WorkItem { node: child, scope });
                    }
                }

                // ── `for x in iter; body; end` ──────────────────────────────
                NodeKind::For { var, iter, body } => {
                    if let NodeKind::Lvasgn { name, .. } = *ast.kind(var) && !Self::is_underscore_prefix(name, ast) {
                        let end = ast.range(var).end;
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                        let v = Self::find_or_declare_local(scope_info, name, var);
                        v.assignments.push(Assignment {
                            node_id: var,
                            end,
                            is_referenced: false,
                        });
                    }
                    // Recurse into iter and body (var itself has no sub-children).
                    stack.push(WorkItem { node: iter, scope });
                    if let Some(b) = body.get() {
                        stack.push(WorkItem { node: b, scope });
                    }
                }

                // ── Variable read ────────────────────────────────────────────
                NodeKind::Lvar(name) => {
                    if !Self::is_underscore_prefix(name, ast) {
                        let pos = ast.range(node).start;
                        let scope_info = scopes.get_mut(&scope).expect("scope must exist");
                        let var = Self::find_or_declare_local(scope_info, name, node);
                        var.references.push(Reference { node_id: node, pos });
                    }
                    // Leaf node; no children.
                }

                // ── All other nodes: classify children under the same scope ──
                _ => {
                    let children: Vec<NodeId> = ast.children(node).collect();
                    for child in children.into_iter().rev() {
                        stack.push(WorkItem { node: child, scope });
                    }
                }
            }
        }

        VarSemanticModel { scopes }
    }

    /// Retrieve the `ScopeInfo` keyed by `boundary_node`.
    pub fn scope(&self, boundary_node: NodeId) -> Option<&ScopeInfo> {
        self.scopes.get(&boundary_node)
    }

    /// Iterate over all scopes: `(boundary_node_id, &ScopeInfo)`.
    pub fn scopes(&self) -> impl Iterator<Item = (NodeId, &ScopeInfo)> {
        self.scopes.iter().map(|(&id, s)| (id, s))
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Returns `true` if the symbol starts with `_` (intentionally-unused marker).
    fn is_underscore_prefix(name: Symbol, ast: &Ast) -> bool {
        ast.interner().resolve(name.0).starts_with('_')
    }

    /// Find or insert an argument variable in `scope_info`.
    fn find_or_declare_arg(scope_info: &mut ScopeInfo, name: Symbol, node: NodeId) -> &mut Variable {
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
    fn find_or_declare_local(scope_info: &mut ScopeInfo, name: Symbol, node: NodeId) -> &mut Variable {
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
                    let scope_info = scopes.get_mut(&scope).expect("scope must exist");
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
            let children: Vec<NodeId> = ast.children(id).collect();
            for c in children.into_iter().rev() {
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
        let def_id = find_node(&ast, |k| matches!(k, NodeKind::Def { .. }))
            .expect("def node");

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

        let def_id = find_node(&ast, |k| matches!(k, NodeKind::Def { .. }))
            .expect("def node");

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
        let def_id = find_node(&ast, |k| matches!(k, NodeKind::Def { .. }))
            .expect("def node");
        let block_id = find_node(&ast, |k| matches!(k, NodeKind::Block { .. }))
            .expect("block node");

        let block_scope = model.scope(block_id).expect("block scope");
        assert_eq!(
            block_scope.parent_scope,
            Some(def_id),
            "block scope parent should be the def scope"
        );
    }
}
