//! `Style/OrAssignment` — recommends usage of `||=` where applicable.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/OrAssignment
//! upstream_version_checked: 1.86.2
//! status: verified
//! gap_issues: []
//! notes: >
//!   Handles ternary assignment, if/else assignment, and unless-guard patterns
//!   for lvasgn, ivasgn, cvasgn, and gvasgn nodes.
//!   Autocorrect replaces the whole pattern with `var ||= default`.
//!   Ternary with nested if in else-branch is not flagged (matches RuboCop guard).
//!   Unless block: offense range covers the `unless <cond>` portion (first line).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad — ternary assignment
//! name = name ? name : 'Bozhidar'
//!
//! # bad — if/else assignment
//! name = if name
//!          name
//!        else
//!          'Bozhidar'
//!        end
//!
//! # bad — unless block
//! unless name
//!   name = 'Bozhidar'
//! end
//!
//! # bad — unless modifier
//! name = 'Bozhidar' unless name
//!
//! # good
//! name ||= 'Bozhidar'
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, Symbol, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct OrAssignment;

const MSG: &str = "Use the double pipe equals operator `||=` instead.";

#[cop(
    name = "Style/OrAssignment",
    description = "Recommends usage of `||=` where applicable.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl OrAssignment {
    /// Detect ternary and if/else assignment patterns:
    ///   `x = x ? x : y`  or  `x = if x; x; else; y; end`
    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_assignment(node, cx);
    }

    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_assignment(node, cx);
    }

    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_assignment(node, cx);
    }

    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_assignment(node, cx);
    }

    /// Detect unless-block / unless-modifier patterns:
    ///   `unless x; x = y; end`  or  `x = y unless x`
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check_unless_pattern(node, cx);
    }
}

// ---------------------------------------------------------------------------
// Assignment pattern: `x = x ? x : y` or `x = if x; x; else; y; end`
// ---------------------------------------------------------------------------

fn check_assignment(node: NodeId, cx: &Cx<'_>) {
    let Some((lhs_name, value_id)) = extract_name_and_value(node, cx) else {
        return;
    };

    let NodeKind::If { cond, then_, else_ } = *cx.kind(value_id) else {
        return;
    };

    // The condition must read the same variable as the LHS.
    if !cond_reads_same_var(cond, lhs_name, node, cx) {
        return;
    }

    // The then-branch must also read the same variable.
    let Some(then_id) = then_.get() else { return };
    if !var_reads_same(then_id, lhs_name, node, cx) {
        return;
    }

    // The else-branch is the default value.
    let Some(else_id) = else_.get() else { return };

    // Guard: do not flag when else-branch is itself an if node (matches RuboCop).
    if matches!(*cx.kind(else_id), NodeKind::If { .. }) {
        return;
    }

    cx.emit_offense(cx.range(node), MSG, None);
    emit_ternary_correction(node, else_id, cx);
}

// ---------------------------------------------------------------------------
// Unless pattern: `unless x; x = y; end`  or  `x = y unless x`
// ---------------------------------------------------------------------------

fn check_unless_pattern(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::If { cond, then_, else_ } = *cx.kind(node) else {
        return;
    };

    // `unless` in prism AST: `if cond { nil } else { body }` i.e. then_ is None,
    // else_ is the body.  Both modifier and block forms produce the same shape.
    let Some(body_id) = else_.get() else { return };
    if then_.get().is_some() {
        return; // regular if — has a then-branch
    }

    // Body must be a simple assignment of the same variable as the condition.
    let Some((asgn_name, _default_id)) = extract_name_and_value(body_id, cx) else {
        return;
    };

    if !cond_reads_same_var(cond, asgn_name, body_id, cx) {
        return;
    }

    // For block-form `unless`, narrow offense to `unless <cond>` (first line only).
    // For modifier-form, the whole node is single-line.
    let node_range = cx.range(node);
    let offense_range = unless_offense_range(node, cond, node_range, cx);

    cx.emit_offense(offense_range, MSG, None);
    // Autocorrect: replace the entire node (not just the offense range).
    emit_unless_correction(node_range, body_id, cx);
}

/// Compute the offense range for an `unless` node.
///
/// For block-form `unless name; ...; end` the node spans multiple lines.
/// We narrow the offense to `unless name` (from node start to end of cond).
/// For modifier-form `x = y unless name` the node is single-line — use full range.
fn unless_offense_range(node: NodeId, cond: NodeId, node_range: Range, cx: &Cx<'_>) -> Range {
    let kw = cx.loc(node).keyword();
    if kw == Range::ZERO {
        // modifier form: no keyword loc, return full range (single-line)
        return node_range;
    }
    // block form: `unless` keyword at kw.start, condition ends at cond.end
    let cond_end = cx.range(cond).end;
    Range {
        start: kw.start,
        end: cond_end,
    }
}

// ---------------------------------------------------------------------------
// Autocorrect helpers
// ---------------------------------------------------------------------------

/// Emit `var ||= default` for the ternary/if-else pattern.
fn emit_ternary_correction(asgn: NodeId, default_id: NodeId, cx: &Cx<'_>) {
    let var_src = asgn_var_source(asgn, cx);
    let default_src = cx.raw_source(cx.range(default_id)).to_owned();
    let replacement = format!("{} ||= {}", var_src, default_src);
    cx.emit_edit(cx.range(asgn), &replacement);
}

/// Emit `var ||= default` for the unless pattern, replacing the whole node.
fn emit_unless_correction(unless_range: Range, asgn_id: NodeId, cx: &Cx<'_>) {
    let var_src = asgn_var_source(asgn_id, cx);
    let Some((_, default_id)) = extract_name_and_value(asgn_id, cx) else {
        return;
    };
    let default_src = cx.raw_source(cx.range(default_id)).to_owned();
    let replacement = format!("{} ||= {}", var_src, default_src);
    cx.emit_edit(unless_range, &replacement);
}

// ---------------------------------------------------------------------------
// Small utilities
// ---------------------------------------------------------------------------

/// Extract `(name, value_id)` from an assignment node.
fn extract_name_and_value(node: NodeId, cx: &Cx<'_>) -> Option<(Symbol, NodeId)> {
    match *cx.kind(node) {
        NodeKind::Lvasgn { name, value }
        | NodeKind::Ivasgn { name, value }
        | NodeKind::Cvasgn { name, value }
        | NodeKind::Gvasgn { name, value } => {
            let v = value.get()?;
            Some((name, v))
        }
        _ => None,
    }
}

/// Get the variable name string from an assignment node.
/// `loc.name` is `Range::ZERO` for these node kinds; use the Symbol.
fn asgn_var_source(asgn_id: NodeId, cx: &Cx<'_>) -> String {
    match *cx.kind(asgn_id) {
        NodeKind::Lvasgn { name, .. }
        | NodeKind::Ivasgn { name, .. }
        | NodeKind::Cvasgn { name, .. }
        | NodeKind::Gvasgn { name, .. } => cx.symbol_str(name).to_owned(),
        _ => String::new(),
    }
}

/// Return `true` when `cond_id` reads the same variable that is written by `asgn_id`.
fn cond_reads_same_var(cond_id: NodeId, asgn_name: Symbol, asgn_id: NodeId, cx: &Cx<'_>) -> bool {
    match (cx.kind(asgn_id), cx.kind(cond_id)) {
        (NodeKind::Lvasgn { .. }, NodeKind::Lvar(sym)) => *sym == asgn_name,
        (NodeKind::Ivasgn { .. }, NodeKind::Ivar(sym)) => *sym == asgn_name,
        (NodeKind::Cvasgn { .. }, NodeKind::Cvar(sym)) => *sym == asgn_name,
        (NodeKind::Gvasgn { .. }, NodeKind::Gvar(sym)) => *sym == asgn_name,
        // `unless name` where `name` is parsed as vcall `(send :name nil)` — matches lvasgn
        (NodeKind::Lvasgn { .. }, NodeKind::Send { receiver, method, args })
            if *receiver == OptNodeId::NONE && cx.list(*args).is_empty() =>
        {
            cx.symbol_str(*method) == cx.symbol_str(asgn_name)
        }
        _ => false,
    }
}

/// Return `true` when `node_id` reads the same variable as written by `asgn_id`.
fn var_reads_same(node_id: NodeId, asgn_name: Symbol, asgn_id: NodeId, cx: &Cx<'_>) -> bool {
    match (cx.kind(asgn_id), cx.kind(node_id)) {
        (NodeKind::Lvasgn { .. }, NodeKind::Lvar(sym)) => *sym == asgn_name,
        (NodeKind::Ivasgn { .. }, NodeKind::Ivar(sym)) => *sym == asgn_name,
        (NodeKind::Cvasgn { .. }, NodeKind::Cvar(sym)) => *sym == asgn_name,
        (NodeKind::Gvasgn { .. }, NodeKind::Gvar(sym)) => *sym == asgn_name,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::OrAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- ternary patterns ---

    #[test]
    fn flags_ternary_local() {
        test::<OrAssignment>().expect_offense(indoc! {"
            name = name ? name : 'Bozhidar'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
        "});
    }

    #[test]
    fn corrects_ternary_local() {
        test::<OrAssignment>().expect_correction(
            indoc! {"
                name = name ? name : 'Bozhidar'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
            "},
            "name ||= 'Bozhidar'\n",
        );
    }

    #[test]
    fn flags_ternary_instance() {
        test::<OrAssignment>().expect_offense(indoc! {"
            @name = @name ? @name : 'Bozhidar'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
        "});
    }

    #[test]
    fn corrects_ternary_instance() {
        test::<OrAssignment>().expect_correction(
            indoc! {"
                @name = @name ? @name : 'Bozhidar'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
            "},
            "@name ||= 'Bozhidar'\n",
        );
    }

    #[test]
    fn flags_ternary_class_var() {
        test::<OrAssignment>().expect_offense(indoc! {"
            @@name = @@name ? @@name : 'Bozhidar'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
        "});
    }

    #[test]
    fn corrects_ternary_class_var() {
        test::<OrAssignment>().expect_correction(
            indoc! {"
                @@name = @@name ? @@name : 'Bozhidar'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
            "},
            "@@name ||= 'Bozhidar'\n",
        );
    }

    #[test]
    fn flags_ternary_global() {
        test::<OrAssignment>().expect_offense(indoc! {"
            $name = $name ? $name : 'Bozhidar'
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
        "});
    }

    #[test]
    fn corrects_ternary_global() {
        test::<OrAssignment>().expect_correction(
            indoc! {"
                $name = $name ? $name : 'Bozhidar'
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
            "},
            "$name ||= 'Bozhidar'\n",
        );
    }

    // --- if/else patterns ---

    #[test]
    fn flags_if_else_local() {
        test::<OrAssignment>().expect_offense(indoc! {"
            name = if name; name; else; 'Bozhidar'; end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
        "});
    }

    #[test]
    fn corrects_if_else_local() {
        test::<OrAssignment>().expect_correction(
            indoc! {"
                name = if name; name; else; 'Bozhidar'; end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
            "},
            "name ||= 'Bozhidar'\n",
        );
    }

    // --- unless block (offense is narrowed to `unless <cond>` first line) ---

    #[test]
    fn flags_unless_block_local() {
        test::<OrAssignment>().expect_offense(indoc! {"
            unless name
            ^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
              name = 'Bozhidar'
            end
        "});
    }

    #[test]
    fn corrects_unless_block_local() {
        test::<OrAssignment>().expect_correction(
            indoc! {"
                unless name
                ^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
                  name = 'Bozhidar'
                end
            "},
            "name ||= 'Bozhidar'\n",
        );
    }

    #[test]
    fn flags_unless_block_instance() {
        test::<OrAssignment>().expect_offense(indoc! {"
            unless @name
            ^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
              @name = 'Bozhidar'
            end
        "});
    }

    #[test]
    fn corrects_unless_block_instance() {
        test::<OrAssignment>().expect_correction(
            indoc! {"
                unless @name
                ^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
                  @name = 'Bozhidar'
                end
            "},
            "@name ||= 'Bozhidar'\n",
        );
    }

    // --- unless modifier ---

    #[test]
    fn flags_unless_modifier_local() {
        test::<OrAssignment>().expect_offense(indoc! {"
            name = 'Bozhidar' unless name
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
        "});
    }

    #[test]
    fn corrects_unless_modifier_local() {
        test::<OrAssignment>().expect_correction(
            indoc! {"
                name = 'Bozhidar' unless name
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use the double pipe equals operator `||=` instead.
            "},
            "name ||= 'Bozhidar'\n",
        );
    }

    // --- no-offense cases ---

    #[test]
    fn accepts_already_or_asgn() {
        test::<OrAssignment>().expect_no_offenses("name ||= 'Bozhidar'\n");
    }

    #[test]
    fn accepts_different_var_in_ternary_cond() {
        test::<OrAssignment>().expect_no_offenses("name = other ? name : 'Bozhidar'\n");
    }

    #[test]
    fn accepts_different_var_in_ternary_then() {
        test::<OrAssignment>().expect_no_offenses("name = name ? other : 'Bozhidar'\n");
    }

    #[test]
    fn accepts_ternary_with_nested_if_in_else() {
        // else branch is itself an if — guard matches RuboCop behavior
        test::<OrAssignment>().expect_no_offenses(
            "name = name ? name : if other; a; else; b; end\n",
        );
    }

    #[test]
    fn accepts_regular_if_without_else() {
        test::<OrAssignment>().expect_no_offenses(indoc! {"
            if name
              do_something
            end
        "});
    }

    #[test]
    fn accepts_unless_different_var() {
        test::<OrAssignment>().expect_no_offenses(indoc! {"
            unless other
              name = 'Bozhidar'
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(OrAssignment);
