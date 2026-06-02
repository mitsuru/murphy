//! `Style/NestedModifier` — flags nested use of `if`, `unless`, `while`,
//! and `until` in modifier form.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NestedModifier
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection is fully implemented for all four modifier kinds:
//!   if/unless/while/until. Fires when both the node and its parent are in
//!   modifier form. Grandparent guard prevents double-reporting in triple+
//!   chains (matching RuboCop's ignore_node behavior).
//!
//!   Autocorrect is partially implemented: only for same-keyword if/if
//!   and unless/unless cases. Mixed keyword (if/unless) and while/until
//!   modifier correction is not implemented.
//!
//!   Specifically not implemented vs RuboCop:
//!   - Mixed if/unless negation autocorrect
//!   - while/until modifier autocorrect
//!   - Full parenthesisation for or-conditions and comparison operators in
//!     right-hand operand
//!   - add_parentheses_to_method_arguments for unparenthesised method calls
//! ```
//!
//! ## Matched shapes
//!
//! A modifier-form `if`, `unless`, `while`, or `until` node whose parent is
//! also a modifier-form `if`, `unless`, `while`, or `until`, AND whose
//! grandparent is NOT a modifier-form node (to avoid double-reporting in
//! triple+ chains).
//!
//! ## Example
//!
//! ```ruby
//! # bad
//! something if a if b
//! something while a while b
//!
//! # good
//! something if b && a
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

const MSG: &str = "Avoid using nested modifiers.";

#[derive(Default)]
pub struct NestedModifier;

#[cop(
    name = "Style/NestedModifier",
    description = "Avoid using nested modifiers.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl NestedModifier {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Find the keyword token for a modifier-form node by scanning its tokens.
/// `cx.loc(node).keyword()` returns the token at `expression.start`, which
/// for modifier forms is the *body* token, not the keyword. So we must scan
/// the tokens within the node range and find the one whose text matches the
/// keyword (`if`, `unless`, `while`, `until`).
fn modifier_keyword_range(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);

    // For if/unless, cx.if_keyword_loc scans for the keyword correctly.
    if matches!(*cx.kind(node), NodeKind::If { .. }) {
        let loc = cx.if_keyword_loc(node);
        if loc != Range::ZERO {
            return loc;
        }
        return node_range;
    }

    // For while/until: scan tokens in the node range for the keyword token.
    let keyword_bytes: &[u8] = match *cx.kind(node) {
        NodeKind::While { .. } => b"while",
        NodeKind::Until { .. } => b"until",
        _ => return node_range,
    };
    let source = cx.source().as_bytes();
    for tok in cx.tokens_in(node_range) {
        if tok.kind != SourceTokenKind::Other {
            continue;
        }
        let tok_text = &source[tok.range.start as usize..tok.range.end as usize];
        if tok_text == keyword_bytes {
            return tok.range;
        }
    }
    node_range
}

fn check(node: NodeId, cx: &Cx<'_>) {
    // Node must be modifier-form.
    if !cx.is_modifier_form(node) {
        return;
    }

    // Parent must also be modifier-form.
    let Some(parent) = cx.parent(node).get() else {
        return;
    };
    if !cx.is_modifier_form(parent) {
        return;
    }

    // Grandparent guard: skip if grandparent is also modifier-form to avoid
    // double-reporting in triple+ chains (matches RuboCop's ignore_node behavior).
    if let Some(grandparent) = cx.parent(parent).get() {
        if cx.is_modifier_form(grandparent) {
            return;
        }
    }

    // Offense: keyword range of this (inner) modifier node.
    let offense_range = modifier_keyword_range(node, cx);

    cx.emit_offense(offense_range, MSG, None);

    // Autocorrect: only for if/if or unless/unless (same keyword).
    autocorrect(node, parent, cx);
}

fn autocorrect(inner: NodeId, outer: NodeId, cx: &Cx<'_>) {
    // Only handle if-type nodes (if or unless) for now.
    if !matches!(*cx.kind(inner), NodeKind::If { .. })
        || !matches!(*cx.kind(outer), NodeKind::If { .. })
    {
        return;
    }

    let inner_kw = cx.if_keyword(inner);
    let outer_kw = cx.if_keyword(outer);

    // Only handle same-keyword case in v1.
    if inner_kw != outer_kw {
        return;
    }

    let operator = if inner_kw == "if" { "&&" } else { "||" };

    let NodeKind::If { cond: outer_cond_id, .. } = *cx.kind(outer) else {
        return;
    };
    let NodeKind::If { cond: inner_cond_id, .. } = *cx.kind(inner) else {
        return;
    };

    // For `something if a if b`:
    //   AST: (if b (if a something nil) nil)
    //   outer = `if b`, outer_cond = b
    //   inner = `if a`, inner_cond = a, body = something
    // We want: `something if b && a`

    let outer_cond_src = cx.raw_source(cx.range(outer_cond_id));
    let inner_cond_src = cx.raw_source(cx.range(inner_cond_id));

    // Find body: for `if`, body is the then-branch. For `unless`, body is else-branch.
    let body_opt = if outer_kw == "unless" {
        cx.if_else_branch(outer)
    } else {
        cx.if_then_branch(outer)
    };
    let Some(body_via_outer) = body_opt.get() else {
        return;
    };
    // body_via_outer should be the inner node.
    if body_via_outer != inner {
        return;
    }

    // Find the actual body (statement) of the inner node.
    let actual_body_opt = if inner_kw == "unless" {
        cx.if_else_branch(inner)
    } else {
        cx.if_then_branch(inner)
    };
    let Some(actual_body) = actual_body_opt.get() else {
        return;
    };

    let body_src = cx.raw_source(cx.range(actual_body));
    let outer_range = cx.range(outer);

    // Build replacement: `body if outer_cond && inner_cond`
    let replacement = format!(
        "{body_src} {outer_kw} {outer_cond_src} {operator} {inner_cond_src}"
    );
    cx.emit_edit(outer_range, &replacement);
}

#[cfg(test)]
mod tests {
    use super::NestedModifier;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- basic detection ---

    #[test]
    fn flags_nested_if_modifier() {
        test::<NestedModifier>().expect_offense(indoc! {"
            something if a if b
                      ^^ Avoid using nested modifiers.
        "});
    }

    #[test]
    fn flags_nested_unless_modifier() {
        test::<NestedModifier>().expect_offense(indoc! {"
            something unless a unless b
                      ^^^^^^ Avoid using nested modifiers.
        "});
    }

    #[test]
    fn flags_nested_while_modifier() {
        test::<NestedModifier>().expect_offense(indoc! {"
            something while a while b
                      ^^^^^ Avoid using nested modifiers.
        "});
    }

    #[test]
    fn flags_nested_until_modifier() {
        test::<NestedModifier>().expect_offense(indoc! {"
            something until a until b
                      ^^^^^ Avoid using nested modifiers.
        "});
    }

    #[test]
    fn flags_nested_if_unless_mix() {
        test::<NestedModifier>().expect_offense(indoc! {"
            something if a unless b
                      ^^ Avoid using nested modifiers.
        "});
    }

    // --- autocorrect ---

    #[test]
    fn autocorrects_if_if() {
        test::<NestedModifier>().expect_correction(
            indoc! {"
                something if a if b
                          ^^ Avoid using nested modifiers.
            "},
            "something if b && a\n",
        );
    }

    #[test]
    fn autocorrects_unless_unless() {
        test::<NestedModifier>().expect_correction(
            indoc! {"
                something unless a unless b
                          ^^^^^^ Avoid using nested modifiers.
            "},
            "something unless b || a\n",
        );
    }

    // --- triple+ chain: only one offense ---

    #[test]
    fn triple_chain_emits_one_offense() {
        // `s if a if b if c` → only the middle `if b` fires.
        // Inner-most `if a` has grandparent `if c` (modifier) → skipped.
        // Middle `if b` has parent `if c` (modifier) and grandparent = root → fires.
        // Outer `if c` has parent = root (not modifier) → skipped.
        test::<NestedModifier>().expect_offense(indoc! {"
            something if a if b if c
                           ^^ Avoid using nested modifiers.
        "});
    }

    // --- no offense cases ---

    #[test]
    fn accepts_single_modifier() {
        test::<NestedModifier>().expect_no_offenses("something if condition\n");
    }

    #[test]
    fn accepts_block_form_if() {
        test::<NestedModifier>().expect_no_offenses(indoc! {"
            if condition
              something
            end
        "});
    }

    #[test]
    fn accepts_block_form_while() {
        test::<NestedModifier>().expect_no_offenses(indoc! {"
            while condition
              something
            end
        "});
    }

    #[test]
    fn accepts_modifier_inside_block_if() {
        test::<NestedModifier>().expect_no_offenses(indoc! {"
            if condition_a
              something if condition_b
            end
        "});
    }
}

murphy_plugin_api::submit_cop!(NestedModifier);
