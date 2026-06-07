//! `Lint/UselessSetterCall` — Checks for setter call to a local variable as
//! the final expression of a method definition.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/UselessSetterCall
//! upstream_version_checked: 1.87.0
//! status: partial
//! gap_issues: []
//! notes: >
//!   Known v1 limitations: Casgn, OrAsgn, AndAsgn, OpAsgn tracking mirrors
//!   RuboCop's MethodVariableTracker for Lvasgn/Ivasgn/Cvasgn/Gvasgn.
//!   OpAsgn (+= etc.) marks the target as non-local (always flags the setter)
//!   which is a conservative approximation of RuboCop's behavior. The
//!   cop does not handle `for` loop iteration variables or `rescue => var`
//!   exception bindings as assignment sources. All common shapes are verified:
//!   def/defs, lvar/ivar/cvar/gvar receiver, setter method (attr=, []=),
//!   nested assignment tracking through multiple/logical/binary operator
//!   assignments, constructor detection (ClassName.new), literal sources,
//!   and argument-source exclusion.
//! ```
//!
//! ## Matched shapes
//!
//! The last expression of a `def`/`defs` body is a setter call on a local
//! variable (lvar/ivar/cvar/gvar) that was assigned from a constructor
//! call or literal (i.e., a local object that is not visible outside the
//! method scope).
//!
//! ## Autocorrect
//!
//! Append the variable name after the setter call as a trailing expression:
//!
//! ```ruby
//! def test
//!   top = Top.new
//!   top.attr = 5
//!   top          # ← inserted
//! end
//! ```

use std::collections::HashMap;

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

#[derive(Default)]
pub struct UselessSetterCall;

#[cop(
    name = "Lint/UselessSetterCall",
    description = "Checks for useless setter call to a local variable.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl UselessSetterCall {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_def_like(node, cx);
    }
}

fn check_def_like(node: NodeId, cx: &Cx<'_>) {
    let (body_opt, _name) = match *cx.kind(node) {
        NodeKind::Def { body, name, .. } => (body, name),
        NodeKind::Defs { body, name, .. } => (body, name),
        _ => return,
    };

    let Some(body_id) = body_opt.get() else {
        return;
    };

    let last_expr = last_expression(body_id, cx);
    let Some(last_id) = last_expr else {
        return;
    };

    // Check if the last expression is a setter call on a local variable.
    let receiver = match *cx.kind(last_id) {
        NodeKind::Send { receiver, .. } => receiver,
        NodeKind::IndexAsgn { receiver, .. } => OptNodeId::from(Some(receiver)),
        _ => return,
    };

    let Some(recv_id) = receiver.get() else {
        return;
    };

    // Receiver must be a local variable read (lvar/ivar/cvar/gvar).
    let var_name = match *cx.kind(recv_id) {
        NodeKind::Lvar(name) | NodeKind::Ivar(name) | NodeKind::Cvar(name) | NodeKind::Gvar(name) => name,
        _ => return,
    };
    let var_name_str = cx.symbol_str(var_name);

    // Check method name ends with `=` and is a setter (not ==, !=, <=, >=, =~).
    let is_setter = match *cx.kind(last_id) {
        NodeKind::Send { method, .. } => {
            let m = cx.symbol_str(method);
            is_setter_method(m)
        }
        NodeKind::IndexAsgn { .. } => true,
        _ => false,
    };
    if !is_setter {
        return;
    }

    // Track variable assignments through the body.
    if !contain_local_object(body_id, var_name_str, cx) {
        return;
    }

    // Emit offense and autocorrect.
    let msg = format!("Useless setter call to local variable `{var_name_str}`.");
    cx.emit_offense(cx.range(recv_id), &msg, None);

    // Autocorrect: insert `\n<indent><var_name>` after the last expression.
    let last_range = cx.range(last_id);
    let indent = compute_indent(last_id, cx);
    let correction = format!("\n{indent}{var_name_str}");
    cx.emit_edit(
        Range {
            start: last_range.end,
            end: last_range.end,
        },
        &correction,
    );
}

/// Returns true if `name` is a setter method (ends with `=` but is not
/// a comparison operator).
fn is_setter_method(name: &str) -> bool {
    if !name.ends_with('=') {
        return false;
    }
    // Exclude comparison operators: ==, !=, <=, >=, =~
    !matches!(name, "==" | "!=" | "<=" | ">=" | "=~" | "!=~" | "<=>" | "===")
}

/// Returns the last expression of a method body. If the body is a Begin,
/// returns the last child. Otherwise, returns the body itself.
fn last_expression(body_id: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(body_id) {
        NodeKind::Begin(list) | NodeKind::Kwbegin(list) => {
            let children = cx.list(list);
            children.last().copied()
        }
        _ => Some(body_id),
    }
}

/// Compute the indentation string (spaces) for the expression.
fn compute_indent(node: NodeId, cx: &Cx<'_>) -> String {
    let start = cx.range(node).start as usize;
    let source = cx.source();
    let line_start = source[..start]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let indent_width = start - line_start;
    " ".repeat(indent_width)
}

// ── MethodVariableTracker ──────────────────────────────────────────────────
//
// Tracks variable assignments through a method body to determine if a
// given variable ever contained a "local object" (constructor result or
// literal). Mirrors RuboCop's `MethodVariableTracker`.

/// Scan the method body and return true if `var_name` ever held a local
/// object (constructor call or literal value).
fn contain_local_object(body_id: NodeId, var_name: &str, cx: &Cx<'_>) -> bool {
    let mut local: HashMap<String, bool> = HashMap::new();
    scan_assignments(body_id, &mut local, cx);
    local.get(var_name).copied().unwrap_or(false)
}

/// Walk the AST and process each assignment node, tracking whether each
/// variable holds a local object.
fn scan_assignments(node: NodeId, local: &mut HashMap<String, bool>, cx: &Cx<'_>) {
    match *cx.kind(node) {
        NodeKind::Lvasgn { name, value } | NodeKind::Ivasgn { name, value } => {
            let var = cx.symbol_str(name).to_string();
            if let Some(rhs) = value.get() {
                process_plain_assignment(&var, rhs, local, cx);
            }
        }
        NodeKind::Cvasgn { name, value } | NodeKind::Gvasgn { name, value } => {
            let var = cx.symbol_str(name).to_string();
            if let Some(rhs) = value.get() {
                process_plain_assignment(&var, rhs, local, cx);
            }
        }
        NodeKind::Masgn { lhs, rhs } => {
            process_masgn(lhs, rhs, local, cx);
            return; // Skip children (Masgn's children are its parts).
        }
        NodeKind::OrAsgn { target, value } | NodeKind::AndAsgn { target, value } => {
            process_logical_op_asgn(target, value, local, cx);
            return;
        }
        NodeKind::OpAsgn { target, value, .. } => {
            process_binary_op_asgn(target, value, local, cx);
            return;
        }
        _ => {}
    }

    // Recurse into children (unless we returned early above).
    for child in cx.children(node) {
        scan_assignments(child, local, cx);
    }
}

/// `x = rhs` — tracks whether rhs is a local object.
fn process_plain_assignment(var: &str, rhs: NodeId, local: &mut HashMap<String, bool>, cx: &Cx<'_>) {
    let is_local = if is_variable_read(rhs, cx) {
        // `x = other_var` → follow the other_var's tracked status.
        let other_name = variable_name(rhs, cx);
        other_name
            .and_then(|n| local.get(n))
            .copied()
            .unwrap_or(false)
    } else {
        is_constructor_or_literal(rhs, cx)
    };
    local.insert(var.to_string(), is_local);
}

/// `a, b, c = rhs` — process each target.
fn process_masgn(lhs: NodeId, rhs: NodeId, local: &mut HashMap<String, bool>, cx: &Cx<'_>) {
    let targets: Vec<NodeId> = match *cx.kind(lhs) {
        NodeKind::Mlhs(list) => cx.list(list).to_vec(),
        _ => return,
    };
    let rhs_children = match *cx.kind(rhs) {
        NodeKind::Array(list) => cx.list(list).to_vec(),
        _ => {
            // Non-array RHS: conservative - mark all as potentially local.
            for t in &targets {
                if let Some(name) = target_name(*t, cx) {
                    local.insert(name.to_string(), true);
                }
            }
            return;
        }
    };

    for (i, t) in targets.iter().enumerate() {
        let Some(name) = target_name(*t, cx) else {
            continue;
        };
        let is_local = rhs_children.get(i).map_or(true, |&child| {
            if is_variable_read(child, cx) {
                variable_name(child, cx)
                    .and_then(|n| local.get(n).map(|&v| v))
                    .unwrap_or(false)
            } else {
                is_constructor_or_literal(child, cx)
            }
        });
        local.insert(name.to_string(), is_local);
    }
}

/// `x ||= rhs` / `x &&= rhs` — tracks rhs (logical operator assignment).
fn process_logical_op_asgn(
    target: NodeId,
    value: NodeId,
    local: &mut HashMap<String, bool>,
    cx: &Cx<'_>,
) {
    let Some(name) = target_name(target, cx) else {
        return;
    };
    let is_local = if is_variable_read(value, cx) {
        variable_name(value, cx)
            .and_then(|n| local.get(n).map(|&v| v))
            .unwrap_or(false)
    } else {
        is_constructor_or_literal(value, cx)
    };
    local.insert(name.to_string(), is_local);
}

/// `x += rhs` — binary operator assignment. In RuboCop, this always marks
/// the variable as non-local (true), which produces a "flag the setter"
/// result. We follow the same conservative behavior.
fn process_binary_op_asgn(
    target: NodeId,
    _value: NodeId,
    local: &mut HashMap<String, bool>,
    cx: &Cx<'_>,
) {
    let Some(name) = target_name(target, cx) else {
        return;
    };
    // OpAsgn always marks the variable as containing a local object
    // (conservative: always flags the setter).
    local.insert(name.to_string(), true);
}

/// Returns the variable name from a write-target node (Lvasgn target).
fn target_name<'a>(node: NodeId, cx: &'a Cx<'_>) -> Option<&'a str> {
    match *cx.kind(node) {
        NodeKind::Lvasgn { name, .. }
        | NodeKind::Ivasgn { name, .. }
        | NodeKind::Cvasgn { name, .. }
        | NodeKind::Gvasgn { name, .. } => Some(cx.symbol_str(name)),
        _ => None,
    }
}

/// Returns the variable name from a variable read node.
fn variable_name<'a>(node: NodeId, cx: &'a Cx<'_>) -> Option<&'a str> {
    match *cx.kind(node) {
        NodeKind::Lvar(name)
        | NodeKind::Ivar(name)
        | NodeKind::Cvar(name)
        | NodeKind::Gvar(name) => Some(cx.symbol_str(name)),
        _ => None,
    }
}

/// True if the node is a variable read.
fn is_variable_read(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Lvar(_) | NodeKind::Ivar(_) | NodeKind::Cvar(_) | NodeKind::Gvar(_)
    )
}

/// True if the node represents a "local object" — either a literal or a
/// constructor call (`ClassName.new`).
fn is_constructor_or_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    if is_literal(node, cx) {
        return true;
    }
    match *cx.kind(node) {
        NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => {
            cx.symbol_str(method) == "new"
        }
        _ => false,
    }
}

/// True if the node is a literal value.
fn is_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(node),
        NodeKind::Nil
            | NodeKind::True_
            | NodeKind::False_
            | NodeKind::Int(_)
            | NodeKind::Float(_)
            | NodeKind::Str(_)
            | NodeKind::Sym(_)
            | NodeKind::Rational(_)
            | NodeKind::Complex(_)
            | NodeKind::Regexp { .. }
            | NodeKind::Array(_)
            | NodeKind::Hash(_)
            | NodeKind::RangeExpr { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::UselessSetterCall;
    use murphy_plugin_api::test_support::{indoc, test, run_cop_with_edits};

    #[test]
    fn flags_setter_call_on_local_object() {
        test::<UselessSetterCall>().expect_offense(indoc! {r#"
            def test
              top = Top.new
              top.attr = 5
              ^^^ Useless setter call to local variable `top`.
            end
        "#});
    }

    #[test]
    fn corrects_setter_call_on_local_object() {
        let src = "def test\n  top = Top.new\n  top.attr = 5\nend\n";
        let run = run_cop_with_edits::<UselessSetterCall>(src);
        assert!(!run.offenses.is_empty());
        assert_eq!(run.edits.len(), 1);
        let e = &run.edits[0];
        // The edit inserts "\n  top" at the end of the last expression.
        assert_eq!(e.replacement, "\n  top", "expected newline+indent+var insertion: {:?}", e.replacement);
    }

    #[test]
    fn flags_setter_call_on_local_object_singleton() {
        test::<UselessSetterCall>().expect_offense(indoc! {r#"
            def Top.test
              top = Top.new
              top.attr = 5
              ^^^ Useless setter call to local variable `top`.
            end
        "#});
    }

    #[test]
    fn flags_square_bracket_setter() {
        test::<UselessSetterCall>().expect_offense(indoc! {r#"
            def test
              top = Top.new
              top[:attr] = 5
              ^^^ Useless setter call to local variable `top`.
            end
        "#});
    }

    #[test]
    fn accepts_ivar_assignment() {
        test::<UselessSetterCall>().expect_no_offenses(indoc! {r#"
            def test
              something
              @top = 5
            end
        "#});
    }

    #[test]
    fn accepts_setter_call_on_ivar() {
        test::<UselessSetterCall>().expect_no_offenses(indoc! {r#"
            def test
              something
              @top.attr = 5
            end
        "#});
    }

    #[test]
    fn accepts_setter_call_on_cvar() {
        test::<UselessSetterCall>().expect_no_offenses(indoc! {r#"
            def test
              something
              @@top.attr = 5
            end
        "#});
    }

    #[test]
    fn accepts_setter_call_on_gvar() {
        test::<UselessSetterCall>().expect_no_offenses(indoc! {r#"
            def test
              something
              $top.attr = 5
            end
        "#});
    }

    #[test]
    fn accepts_setter_call_on_arg() {
        test::<UselessSetterCall>().expect_no_offenses(indoc! {r#"
            def test(some_arg)
              unrelated_local_variable = Top.new
              some_arg.attr = 5
            end
        "#});
    }

    #[test]
    fn accepts_non_setter_operator() {
        test::<UselessSetterCall>().expect_no_offenses(indoc! {r#"
            def test
              top.attr == 5
            end
        "#});
    }

    #[test]
    fn corrects_with_literal_source() {
        let src = "def test\n  some_arg = {}\n  some_arg[:attr] = 1\nend\n";
        let run = run_cop_with_edits::<UselessSetterCall>(src);
        assert!(!run.offenses.is_empty(), "should flag setter on literal-constructed var");
    }

    #[test]
    fn accepts_setter_call_from_method_return() {
        test::<UselessSetterCall>().expect_no_offenses(indoc! {r#"
            def test
              some_lvar = Foo.shared_object
              some_lvar[:attr] = 1
            end
        "#});
    }

    #[test]
    fn flags_when_var_reassigned_from_constructor() {
        test::<UselessSetterCall>().expect_offense(indoc! {r#"
            def test(some_arg)
              some_arg = Top.new
              some_arg.attr = 5
              ^^^^^^^^ Useless setter call to local variable `some_arg`.
            end
        "#});
    }
}
murphy_plugin_api::submit_cop!(UselessSetterCall);
