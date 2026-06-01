//! `Style/RedundantSelfAssignment` — flags redundant assignments where the
//! method already modifies its receiver in place.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/RedundantSelfAssignment
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Implements the variable-assignment path (lvasgn/ivasgn/cvasgn/gvasgn).
//!   casgn (constant) is excluded (not in RuboCop's type map either).
//!   The setter-method path (`self.x = self.x.merge!(...)` via on_send/on_csend)
//!   is a documented gap — that path requires matching a complex node-pattern
//!   against `self.x.METHOD(self.x, ...)` shapes.
//!   Block-method forms (`x = x.map! { ... }`) are supported:
//!   the method and receiver are extracted from the Block's inner Send.
//!   This cop is unsafe (SafeAutoCorrect: false) — user-defined methods with
//!   matching names but non-in-place semantics will produce false positives.
//!   Offense is on the `=` operator token; autocorrect replaces the whole
//!   node with the RHS source.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! args = args.concat(ary)
//! hash = hash.merge!(other)
//! @items = @items.push(item)
//!
//! # good
//! args.concat(ary)
//! hash.merge!(other)
//! @items.push(item)
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct RedundantSelfAssignment;

const MSG: &str =
    "Redundant self assignment detected. Method `%<method_name>s` modifies its receiver in place.";

/// Methods that modify their receiver in place and return `self`.
///
/// Matches RuboCop's `METHODS_RETURNING_SELF` set.
const METHODS_RETURNING_SELF: &[&str] = &[
    "append",
    "clear",
    "collect!",
    "compare_by_identity",
    "concat",
    "delete_if",
    "fill",
    "initialize_copy",
    "insert",
    "keep_if",
    "map!",
    "merge!",
    "prepend",
    "push",
    "rehash",
    "replace",
    "reverse!",
    "rotate!",
    "shuffle!",
    "sort!",
    "sort_by!",
    "transform_keys!",
    "transform_values!",
    "unshift",
    "update",
];

#[cop(
    name = "Style/RedundantSelfAssignment",
    description = "Checks for places where redundant assignments are made for in place modification methods.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RedundantSelfAssignment {
    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_var_asgn(node, cx);
    }

    #[on_node(kind = "ivasgn")]
    fn check_ivasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_var_asgn(node, cx);
    }

    #[on_node(kind = "cvasgn")]
    fn check_cvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_var_asgn(node, cx);
    }

    #[on_node(kind = "gvasgn")]
    fn check_gvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_var_asgn(node, cx);
    }
}

// ---------------------------------------------------------------------------
// Core check
// ---------------------------------------------------------------------------

fn check_var_asgn(node: NodeId, cx: &Cx<'_>) {
    // Extract lhs name and value for all four assignment types.
    let (lhs_name, value_opt) = match *cx.kind(node) {
        NodeKind::Lvasgn { name, value } => (name, value),
        NodeKind::Ivasgn { name, value } => (name, value),
        NodeKind::Cvasgn { name, value } => (name, value),
        NodeKind::Gvasgn { name, value } => (name, value),
        _ => return,
    };

    let Some(rhs) = value_opt.get() else {
        return;
    };

    // Determine the expected receiver kind and the inner send node.
    // rhs may be a plain Send or a Block (for block-taking methods).
    let (inner_send, method_name) = match *cx.kind(rhs) {
        NodeKind::Send { method, .. } => (rhs, cx.symbol_str(method).to_owned()),
        NodeKind::Block { call, .. } => {
            let NodeKind::Send { method, .. } = *cx.kind(call) else {
                return;
            };
            (call, cx.symbol_str(method).to_owned())
        }
        _ => return,
    };

    // The method must be one that returns self.
    if !METHODS_RETURNING_SELF.contains(&method_name.as_str()) {
        return;
    }

    // The receiver of the method call must be the same variable.
    let recv_opt = match *cx.kind(inner_send) {
        NodeKind::Send { receiver, .. } => receiver,
        _ => return,
    };

    let Some(recv) = recv_opt.get() else {
        return;
    };

    // Receiver must be the same variable type and name as the lhs.
    let recv_matches = match *cx.kind(node) {
        NodeKind::Lvasgn { .. } => matches!(cx.kind(recv), NodeKind::Lvar(sym) if *sym == lhs_name),
        NodeKind::Ivasgn { .. } => matches!(cx.kind(recv), NodeKind::Ivar(sym) if *sym == lhs_name),
        NodeKind::Cvasgn { .. } => matches!(cx.kind(recv), NodeKind::Cvar(sym) if *sym == lhs_name),
        NodeKind::Gvasgn { .. } => matches!(cx.kind(recv), NodeKind::Gvar(sym) if *sym == lhs_name),
        _ => false,
    };

    if !recv_matches {
        return;
    }

    // Find the `=` operator token.
    let eq_range = find_asgn_eq_range(node, cx);
    if eq_range == Range::ZERO {
        return;
    }

    let msg = MSG.replace("%<method_name>s", &method_name);
    cx.emit_offense(eq_range, &msg, None);

    // Autocorrect: replace the whole assignment node with just the RHS source.
    let rhs_source = cx.raw_source(cx.range(rhs)).to_owned();
    cx.emit_edit(cx.range(node), &rhs_source);
}

// ---------------------------------------------------------------------------
// Token helpers
// ---------------------------------------------------------------------------

/// Find the `=` assignment operator token in the gap between the lhs name
/// end and the rhs start.
fn find_asgn_eq_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);
    let name_end = cx.node(node).loc.name.end;
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < name_end);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == b"="
        })
        .map(|t| t.range)
        .unwrap_or(Range::ZERO)
}

#[cfg(test)]
mod tests {
    use super::RedundantSelfAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Basic offense cases ---

    #[test]
    fn flags_lvar_concat() {
        test::<RedundantSelfAssignment>().expect_offense(indoc! {"
            args = args.concat(ary)
                 ^ Redundant self assignment detected. Method `concat` modifies its receiver in place.
        "});
    }

    #[test]
    fn flags_lvar_merge_bang() {
        test::<RedundantSelfAssignment>().expect_offense(indoc! {"
            hash = hash.merge!(other)
                 ^ Redundant self assignment detected. Method `merge!` modifies its receiver in place.
        "});
    }

    #[test]
    fn flags_ivar_push() {
        test::<RedundantSelfAssignment>().expect_offense(indoc! {"
            @items = @items.push(item)
                   ^ Redundant self assignment detected. Method `push` modifies its receiver in place.
        "});
    }

    #[test]
    fn flags_cvar_append() {
        test::<RedundantSelfAssignment>().expect_offense(indoc! {"
            @@list = @@list.append(val)
                   ^ Redundant self assignment detected. Method `append` modifies its receiver in place.
        "});
    }

    #[test]
    fn flags_gvar_sort_bang() {
        test::<RedundantSelfAssignment>().expect_offense(indoc! {"
            $arr = $arr.sort!(  )
                 ^ Redundant self assignment detected. Method `sort!` modifies its receiver in place.
        "});
    }

    // --- Block method forms ---

    #[test]
    fn flags_block_map_bang() {
        test::<RedundantSelfAssignment>().expect_offense(indoc! {"
            x = x.map! { |e| e.to_s }
              ^ Redundant self assignment detected. Method `map!` modifies its receiver in place.
        "});
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_lvar_concat() {
        test::<RedundantSelfAssignment>().expect_correction(
            indoc! {"
                args = args.concat(ary)
                     ^ Redundant self assignment detected. Method `concat` modifies its receiver in place.
            "},
            "args.concat(ary)\n",
        );
    }

    #[test]
    fn corrects_ivar_push() {
        test::<RedundantSelfAssignment>().expect_correction(
            indoc! {"
                @items = @items.push(item)
                       ^ Redundant self assignment detected. Method `push` modifies its receiver in place.
            "},
            "@items.push(item)\n",
        );
    }

    #[test]
    fn corrects_block_sort_by_bang() {
        test::<RedundantSelfAssignment>().expect_correction(
            indoc! {"
                x = x.sort_by! { |e| e.length }
                  ^ Redundant self assignment detected. Method `sort_by!` modifies its receiver in place.
            "},
            "x.sort_by! { |e| e.length }\n",
        );
    }

    // --- No-offense cases ---

    #[test]
    fn no_offense_different_receiver() {
        test::<RedundantSelfAssignment>().expect_no_offenses("args = other.concat(ary)\n");
    }

    #[test]
    fn no_offense_non_in_place_method() {
        test::<RedundantSelfAssignment>().expect_no_offenses("args = args.concat_all(ary)\n");
    }

    #[test]
    fn no_offense_plain_assignment() {
        test::<RedundantSelfAssignment>().expect_no_offenses("args = ary\n");
    }

    #[test]
    fn no_offense_different_names() {
        test::<RedundantSelfAssignment>().expect_no_offenses("foo.concat(ary)\n");
    }

    #[test]
    fn no_offense_concat_different_receiver_var() {
        test::<RedundantSelfAssignment>().expect_no_offenses("a = b.concat(ary)\n");
    }
}
murphy_plugin_api::submit_cop!(RedundantSelfAssignment);
