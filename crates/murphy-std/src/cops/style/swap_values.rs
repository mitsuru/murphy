//! `Style/SwapValues` — enforces shorthand-style swapping of 2 variables.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SwapValues
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects the three-assignment verbose swap pattern and corrects it to
//!   parallel assignment (`x, y = y, x`). Handles local, instance, class,
//!   global, and constant variables including namespaced constants.
//!   The cop fires on the first (tmp) assignment when three consecutive
//!   siblings in a Begin block form the swap pattern.
//!   Autocorrect is marked `SafeAutoCorrect: false` (unsafe) because the
//!   temporary variable may be referenced elsewhere after the swap.
//!   Skipped when the assignment's parent is an Mlhs (multiple-lhs) node or a
//!   shorthand assignment (op_asgn / or_asgn / and_asgn).
//! ```
//!
//! ## Matched shape
//!
//! ```ruby
//! # bad
//! tmp = x
//! x = y
//! y = tmp
//!
//! # good
//! x, y = y, x
//! ```
//!
//! All five simple assignment types (`lvasgn`, `ivasgn`, `cvasgn`, `gvasgn`,
//! `casgn`) are supported, and mixed-type swaps (e.g. `@x` <-> `$y`) are
//! detected correctly.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct SwapValues;

const MSG: &str =
    "Replace this and assignments at lines %<x_line>d and %<y_line>d with `%<replacement>s`.";

#[cop(
    name = "Style/SwapValues",
    description = "Enforces the use of shorthand-style swapping of 2 variables.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SwapValues {
    /// Walk `begin` sequences looking for consecutive 3-statement swap triples.
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Begin(list) = *cx.kind(node) else {
            return;
        };
        let stmts = cx.list(list);

        for window in stmts.windows(3) {
            let (tmp_node, x_node, y_node) = (window[0], window[1], window[2]);

            // Each of the three must be a simple assignment.
            if !is_simple_assignment(tmp_node, cx)
                || !is_simple_assignment(x_node, cx)
                || !is_simple_assignment(y_node, cx)
            {
                continue;
            }

            // Skip if parent is mlhs or shorthand assignment.
            if is_inside_mlhs(tmp_node, cx) || cx.is_shorthand_asgn(tmp_node) {
                continue;
            }

            if !swapping_values(tmp_node, x_node, y_node, cx) {
                continue;
            }

            let x_line = line_number(cx, x_node);
            let y_line = line_number(cx, y_node);
            let replacement = build_replacement(x_node, cx);

            let message = MSG
                .replace("%<x_line>d", &x_line.to_string())
                .replace("%<y_line>d", &y_line.to_string())
                .replace("%<replacement>s", &replacement);

            let offense_range = cx.range(tmp_node);
            cx.emit_offense(offense_range, &message, None);

            // Autocorrect: replace the three-statement span (including
            // surrounding whole lines / comments) with the parallel assignment.
            let correction_range = correction_range(cx, tmp_node, y_node);
            let corrected = format!("{}\n", replacement);
            cx.emit_edit(correction_range, &corrected);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `true` iff `node` is one of the five simple assignment kinds.
fn is_simple_assignment(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Lvasgn { .. }
            | NodeKind::Ivasgn { .. }
            | NodeKind::Cvasgn { .. }
            | NodeKind::Gvasgn { .. }
            | NodeKind::Casgn { .. }
    )
}

/// Returns `true` iff the parent of `node` is an `Mlhs` node.
fn is_inside_mlhs(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.parent(node).get().map(|p| cx.kind(p)),
        Some(NodeKind::Mlhs(..))
    )
}

/// Checks whether three consecutive assignments form the swap pattern:
///
/// ```text
/// tmp = x
/// x   = y
/// y   = tmp
/// ```
fn swapping_values(tmp: NodeId, x: NodeId, y: NodeId, cx: &Cx<'_>) -> bool {
    let Some(tmp_lhs) = asgn_lhs(tmp, cx) else {
        return false;
    };
    let Some(tmp_rhs) = asgn_rhs_ref(tmp, cx) else {
        return false;
    };
    let Some(x_lhs) = asgn_lhs(x, cx) else {
        return false;
    };
    let Some(x_rhs) = asgn_rhs_ref(x, cx) else {
        return false;
    };
    let Some(y_lhs) = asgn_lhs(y, cx) else {
        return false;
    };
    let Some(y_rhs) = asgn_rhs_ref(y, cx) else {
        return false;
    };

    // lhs(x) == rhs(tmp): x is assigned what tmp originally held
    // lhs(y) == rhs(x):   y is assigned what x originally held
    // rhs(y) == lhs(tmp): tmp's original value goes to y (completing the cycle)
    x_lhs == tmp_rhs && y_lhs == x_rhs && y_rhs == tmp_lhs
}

/// A typed variable reference for equality comparison.
#[derive(Debug, PartialEq, Eq)]
enum VarRef {
    Local(String),
    Instance(String),
    Global(String),
    Classvar(String),
    Const(String),
}

/// Extract the LHS variable identity from a simple assignment node.
fn asgn_lhs(node: NodeId, cx: &Cx<'_>) -> Option<VarRef> {
    match *cx.kind(node) {
        NodeKind::Lvasgn { name, .. } => Some(VarRef::Local(cx.symbol_str(name).to_owned())),
        NodeKind::Ivasgn { name, .. } => Some(VarRef::Instance(cx.symbol_str(name).to_owned())),
        NodeKind::Gvasgn { name, .. } => Some(VarRef::Global(cx.symbol_str(name).to_owned())),
        NodeKind::Cvasgn { name, .. } => Some(VarRef::Classvar(cx.symbol_str(name).to_owned())),
        NodeKind::Casgn { .. } => cx.const_name(node).map(VarRef::Const),
        _ => None,
    }
}

/// Extract the RHS value node and interpret it as a variable reference.
///
/// Returns `None` when the value is absent, is not a simple variable read,
/// or is a call with arguments (not a vcall-style send).
fn asgn_rhs_ref(node: NodeId, cx: &Cx<'_>) -> Option<VarRef> {
    let value_id = match *cx.kind(node) {
        NodeKind::Lvasgn { value, .. }
        | NodeKind::Ivasgn { value, .. }
        | NodeKind::Gvasgn { value, .. }
        | NodeKind::Cvasgn { value, .. } => value.get()?,
        NodeKind::Casgn { value, .. } => value.get()?,
        _ => return None,
    };

    match *cx.kind(value_id) {
        NodeKind::Lvar(sym) => Some(VarRef::Local(cx.symbol_str(sym).to_owned())),
        NodeKind::Ivar(sym) => Some(VarRef::Instance(cx.symbol_str(sym).to_owned())),
        NodeKind::Gvar(sym) => Some(VarRef::Global(cx.symbol_str(sym).to_owned())),
        NodeKind::Cvar(sym) => Some(VarRef::Classvar(cx.symbol_str(sym).to_owned())),
        // vcall: `(send :x nil)` — receiver-less send with no args
        NodeKind::Send {
            receiver,
            method,
            args,
        } => {
            if receiver == OptNodeId::NONE && cx.list(args).is_empty() {
                Some(VarRef::Local(cx.symbol_str(method).to_owned()))
            } else {
                None
            }
        }
        NodeKind::Const { .. } => cx.const_name(value_id).map(VarRef::Const),
        _ => None,
    }
}

/// Get the source representation of the LHS name for the replacement string.
///
/// Uses raw source to faithfully reproduce `::X`, `Foo::Y`, `@x`, `$y`, etc.
fn asgn_lhs_source(node: NodeId, cx: &Cx<'_>) -> String {
    match *cx.kind(node) {
        NodeKind::Lvasgn { name, .. } => cx.symbol_str(name).to_owned(),
        NodeKind::Ivasgn { name, .. } => cx.symbol_str(name).to_owned(),
        NodeKind::Gvasgn { name, .. } => cx.symbol_str(name).to_owned(),
        NodeKind::Cvasgn { name, .. } => cx.symbol_str(name).to_owned(),
        NodeKind::Casgn { .. } => {
            // Raw source of the whole node up to the `=` gives us `::X` / `Foo::X`.
            let node_range = cx.range(node);
            let source = cx.source();
            let bytes = source.as_bytes();
            // Find the `=` token that is the actual assignment operator.
            let toks = cx.sorted_tokens();
            let idx = toks.partition_point(|t| t.range.start < node_range.start);
            let eq_tok = toks[idx..]
                .iter()
                .take_while(|t| t.range.start < node_range.end)
                .find(|t| {
                    t.kind == SourceTokenKind::Other
                        && &bytes[t.range.start as usize..t.range.end as usize] == b"="
                });
            if let Some(eq) = eq_tok {
                let lhs_bytes = &bytes[node_range.start as usize..eq.range.start as usize];
                String::from_utf8_lossy(lhs_bytes).trim().to_owned()
            } else {
                cx.const_name(node).unwrap_or_default()
            }
        }
        _ => String::new(),
    }
}

/// Get the raw source of the RHS (value) of an assignment.
fn asgn_rhs_source(node: NodeId, cx: &Cx<'_>) -> String {
    let value_id = match *cx.kind(node) {
        NodeKind::Lvasgn { value, .. }
        | NodeKind::Ivasgn { value, .. }
        | NodeKind::Gvasgn { value, .. }
        | NodeKind::Cvasgn { value, .. } => match value.get() {
            Some(v) => v,
            None => return String::new(),
        },
        NodeKind::Casgn { value, .. } => match value.get() {
            Some(v) => v,
            None => return String::new(),
        },
        _ => return String::new(),
    };
    cx.raw_source(cx.range(value_id)).to_owned()
}

/// Build the replacement string `"x, y = y, x"` based on the x-assignment node.
fn build_replacement(x_node: NodeId, cx: &Cx<'_>) -> String {
    let x = asgn_lhs_source(x_node, cx);
    let y = asgn_rhs_source(x_node, cx);
    format!("{}, {} = {}, {}", x, y, y, x)
}

/// 1-based line number of the first line of `node`.
fn line_number(cx: &Cx<'_>, node: NodeId) -> usize {
    let offset = cx.range(node).start as usize;
    let source = cx.source();
    source[..offset].bytes().filter(|&b| b == b'\n').count() + 1
}

/// The range covering the whole-line span from `tmp_node` to `y_node`,
/// including any interleaved comments, expanded to full line boundaries
/// and including the trailing newline.
fn correction_range(cx: &Cx<'_>, tmp_node: NodeId, y_node: NodeId) -> Range {
    let start = cx.range(tmp_node).start;
    let end = cx.range(y_node).end;
    let inner = Range { start, end };
    cx.range_by_whole_lines(inner, true)
}

#[cfg(test)]
mod tests {
    use super::SwapValues;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Basic offense cases ---

    #[test]
    fn flags_swap_local_vars() {
        test::<SwapValues>().expect_offense(indoc! {"
            tmp = x
            ^^^^^^^ Replace this and assignments at lines 2 and 3 with `x, y = y, x`.
            x = y
            y = tmp
        "});
    }

    #[test]
    fn corrects_swap_local_vars() {
        test::<SwapValues>().expect_correction(
            indoc! {"
                tmp = x
                ^^^^^^^ Replace this and assignments at lines 2 and 3 with `x, y = y, x`.
                x = y
                y = tmp
            "},
            "x, y = y, x\n",
        );
    }

    #[test]
    fn flags_swap_global_vars() {
        test::<SwapValues>().expect_offense(indoc! {"
            tmp = $x
            ^^^^^^^^ Replace this and assignments at lines 2 and 3 with `$x, $y = $y, $x`.
            $x = $y
            $y = tmp
        "});
    }

    #[test]
    fn corrects_swap_global_vars() {
        test::<SwapValues>().expect_correction(
            indoc! {"
                tmp = $x
                ^^^^^^^^ Replace this and assignments at lines 2 and 3 with `$x, $y = $y, $x`.
                $x = $y
                $y = tmp
            "},
            "$x, $y = $y, $x\n",
        );
    }

    #[test]
    fn flags_swap_instance_vars() {
        test::<SwapValues>().expect_offense(indoc! {"
            tmp = @x
            ^^^^^^^^ Replace this and assignments at lines 2 and 3 with `@x, @y = @y, @x`.
            @x = @y
            @y = tmp
        "});
    }

    #[test]
    fn corrects_swap_instance_vars() {
        test::<SwapValues>().expect_correction(
            indoc! {"
                tmp = @x
                ^^^^^^^^ Replace this and assignments at lines 2 and 3 with `@x, @y = @y, @x`.
                @x = @y
                @y = tmp
            "},
            "@x, @y = @y, @x\n",
        );
    }

    #[test]
    fn flags_swap_class_vars() {
        test::<SwapValues>().expect_offense(indoc! {"
            tmp = @@x
            ^^^^^^^^^ Replace this and assignments at lines 2 and 3 with `@@x, @@y = @@y, @@x`.
            @@x = @@y
            @@y = tmp
        "});
    }

    #[test]
    fn flags_swap_constants() {
        test::<SwapValues>().expect_offense(indoc! {"
            tmp = X
            ^^^^^^^ Replace this and assignments at lines 2 and 3 with `X, Y = Y, X`.
            X = Y
            Y = tmp
        "});
    }

    #[test]
    fn flags_swap_mixed_types() {
        test::<SwapValues>().expect_offense(indoc! {"
            tmp = @x
            ^^^^^^^^ Replace this and assignments at lines 2 and 3 with `@x, $y = $y, @x`.
            @x = $y
            $y = tmp
        "});
    }

    // --- No-offense cases ---

    #[test]
    fn accepts_idiomatic_parallel_assign() {
        test::<SwapValues>().expect_no_offenses("x, y = y, x\n");
    }

    #[test]
    fn accepts_almost_swap_wrong_rhs() {
        test::<SwapValues>().expect_no_offenses(indoc! {"
            tmp = x
            x = y
            y = not_a_tmp
        "});
    }

    #[test]
    fn accepts_only_two_assignments() {
        test::<SwapValues>().expect_no_offenses(indoc! {"
            tmp = x
            x = tmp
        "});
    }

    // --- Comment removal in autocorrect ---

    #[test]
    fn corrects_removing_interleaved_comments() {
        test::<SwapValues>().expect_correction(
            indoc! {"
                tmp = x # comment 1
                ^^^^^^^ Replace this and assignments at lines 3 and 4 with `x, y = y, x`.
                # comment 2
                x = y
                y = tmp # comment 3
            "},
            "x, y = y, x\n",
        );
    }
}
murphy_plugin_api::submit_cop!(SwapValues);
