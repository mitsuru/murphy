//! `Style/IfWithSemicolon` — flags `if cond; ...` (semicolon-opened `if`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/IfWithSemicolon
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags block-form `if`/`unless` where the body opener is a `;`
//!   (i.e. `if cond; body end`) rather than `then` or a newline.
//!
//!   Three message variants (mirrors RuboCop):
//!     - MSG_NEWLINE: when the branch(es) contain a `begin` node (multiple
//!       statements), or when a branch is a `return` with arguments.
//!     - MSG_IF_ELSE: when else-branch is an `if`/`begin` node, or any
//!       branch uses `masgn` or a block (`Block`/`Numblock`).
//!     - MSG_TERNARY: default (simple single-expression branches).
//!
//!   Guards:
//!     - modifier-form (`body if cond`) -> skip.
//!     - ternary (`cond ? a : b`) -> skip.
//!     - parent is `If` node -> skip (handles nested and elsif cases).
//!
//!   Detection: the token immediately after the condition end is checked; it
//!   must be an `Other` token with text `b";"`.
//!
//!   Autocorrect:
//!     - MSG_NEWLINE / masgn-branch cases: replace the `;` token with `\n`.
//!     - MSG_TERNARY case: replace the whole node with `cond ? then : else`.
//!       Murphy's AST stores `unless c; a else b` as `if c: then_=b, else_=a`
//!       (branches swapped by parser), so the AST order is used directly
//!       without additional reordering.
//!     - MSG_IF_ELSE case: no autocorrect (complex multi-line rewrite;
//!       RuboCop's `correct_elsif` is not yet implemented).
//!
//!   Gaps:
//!     - `correct_elsif` multiline correction for `if c; a elsif c2; b end`
//!       is not implemented.
//!     - `build_expression` parenthesization (RuboCop wraps unparenthesized
//!       method calls like `puts 1` to `(puts 1)` in ternary) is not
//!       implemented here. Simple expressions are emitted without wrapping.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! result = if some_condition; something else another_thing end
//!
//! # good
//! result = some_condition ? something : another_thing
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct IfWithSemicolon;

enum MessageTemplate {
    Newline,
    IfElse,
    Ternary,
}

#[cop(
    name = "Style/IfWithSemicolon",
    description = "Do not use if x; .... Use the ternary operator instead.",
    default_severity = "warning",
    default_enabled = true,
)]
impl IfWithSemicolon {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        // Skip modifier-form (`body if cond`) and ternaries.
        if cx.is_modifier_form(node) || cx.is_ternary(node) {
            return;
        }

        // Skip when the parent is an `If` node (handles elsif and nested ifs).
        if let Some(parent) = cx.parent(node).get()
            && matches!(cx.kind(parent), NodeKind::If { .. }) {
                return;
            }

        // Detect the semicolon: the token immediately after the condition end
        // must be an `Other` token with text `b";"`.
        let Some(cond_id) = cx.if_condition(node).get() else {
            return;
        };
        let cond_end = cx.range(cond_id).end;
        let semicolon_range = match find_semicolon_after(cond_end, cx) {
            Some(r) => r,
            None => return,
        };

        let keyword = if cx.is_unless(node) { "unless" } else { "if" };
        let expr = cx.raw_source(cx.range(cond_id));

        let message = match message_template(node, cx) {
            MessageTemplate::Newline => {
                format!("Do not use `{keyword} {expr};` - use a newline instead.")
            }
            MessageTemplate::IfElse => {
                format!("Do not use `{keyword} {expr};` - use `if/else` instead.")
            }
            MessageTemplate::Ternary => {
                format!("Do not use `{keyword} {expr};` - use a ternary operator instead.")
            }
        };

        cx.emit_offense(cx.range(node), &message, None);

        // Autocorrect.
        emit_correction(node, cond_id, semicolon_range, cx);
    }
}

// ---------------------------------------------------------------------------
// Token helper
// ---------------------------------------------------------------------------

/// Returns the `Range` of the `;` token that immediately follows `cond_end`,
/// skipping whitespace. Returns `None` if the first non-whitespace token is
/// not a semicolon.
fn find_semicolon_after(cond_end: u32, cx: &Cx<'_>) -> Option<Range> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < cond_end);
    let tok = toks.get(idx)?;
    if tok.kind == SourceTokenKind::Other
        && &source[tok.range.start as usize..tok.range.end as usize] == b";"
    {
        Some(tok.range)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Message selection
// ---------------------------------------------------------------------------

fn message_template(node: NodeId, cx: &Cx<'_>) -> MessageTemplate {
    if require_newline(node, cx) {
        MessageTemplate::Newline
    } else if else_is_if_or_begin(node, cx) || branches_have_masgn_or_block(node, cx) {
        MessageTemplate::IfElse
    } else {
        MessageTemplate::Ternary
    }
}

/// `require_newline?` from RuboCop: any compacted branch is a `Begin` node,
/// or any branch is a `return` with arguments.
fn require_newline(node: NodeId, cx: &Cx<'_>) -> bool {
    branches(node, cx)
        .into_iter()
        .flatten()
        .any(|b| matches!(cx.kind(b), NodeKind::Begin(_)) || is_return_with_argument(b, cx))
}

/// Returns `true` if the else-branch is an `If` or `Begin` node.
fn else_is_if_or_begin(node: NodeId, cx: &Cx<'_>) -> bool {
    if let Some(else_b) = cx.if_else_branch(node).get() {
        matches!(cx.kind(else_b), NodeKind::If { .. } | NodeKind::Begin(_))
    } else {
        false
    }
}

/// Returns `true` if any branch uses `Masgn` or a block (`Block`/`Numblock`).
fn branches_have_masgn_or_block(node: NodeId, cx: &Cx<'_>) -> bool {
    branches(node, cx).into_iter().flatten().any(|b| {
        matches!(
            cx.kind(b),
            NodeKind::Masgn { .. } | NodeKind::Block { .. } | NodeKind::Numblock { .. }
        )
    })
}

/// Returns `true` if `node` is a `Return` node with at least one argument.
fn is_return_with_argument(node: NodeId, cx: &Cx<'_>) -> bool {
    if let NodeKind::Return(opt) = cx.kind(node) {
        opt.get().is_some()
    } else {
        false
    }
}

/// Returns both branches of the `if` node as a fixed-size array.
/// Each element is `Some(branch_id)` or `None` if the branch is absent.
fn branches(node: NodeId, cx: &Cx<'_>) -> [Option<NodeId>; 2] {
    [cx.if_then_branch(node).get(), cx.if_else_branch(node).get()]
}

// ---------------------------------------------------------------------------
// Autocorrect
// ---------------------------------------------------------------------------

fn emit_correction(
    node: NodeId,
    cond_id: NodeId,
    semicolon_range: Range,
    cx: &Cx<'_>,
) {
    if require_newline(node, cx) || branches_have_masgn_or_block(node, cx) {
        // Replace the `;` with a newline.
        cx.emit_edit(semicolon_range, "\n");
    } else if !else_is_if_or_begin(node, cx) {
        // Ternary correction: `cond ? then : else`
        emit_ternary_correction(node, cond_id, cx);
    }
    // MSG_IF_ELSE (else is if/begin): no autocorrect — correct_elsif is a gap.
}

fn emit_ternary_correction(node: NodeId, cond_id: NodeId, cx: &Cx<'_>) {
    let cond_src = cx.raw_source(cx.range(cond_id));
    let then_src = cx
        .if_then_branch(node)
        .get()
        .map(|t| cx.raw_source(cx.range(t)))
        .unwrap_or("nil");
    let else_src = cx
        .if_else_branch(node)
        .get()
        .map(|e| cx.raw_source(cx.range(e)))
        .unwrap_or("nil");

    // Murphy's AST stores `unless c; a else b` as `if c: then_=b, else_=a`
    // (branches are already swapped by the parser). Using `then_src : else_src`
    // directly produces the correct ternary for both `if` and `unless` forms.
    let replacement = format!("{cond_src} ? {then_src} : {else_src}");
    cx.emit_edit(cx.range(node), &replacement);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::IfWithSemicolon;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Basic offense detection ---

    #[test]
    fn flags_if_with_semicolon_ternary_case() {
        test::<IfWithSemicolon>().expect_offense(indoc! {r#"
            result = if some_condition; something else another_thing end
                     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Do not use `if some_condition;` - use a ternary operator instead.
        "#});
    }

    #[test]
    fn flags_if_with_semicolon_no_else() {
        test::<IfWithSemicolon>().expect_offense(indoc! {r#"
            if x; y end
            ^^^^^^^^^^^ Do not use `if x;` - use a ternary operator instead.
        "#});
    }

    #[test]
    fn flags_unless_with_semicolon() {
        test::<IfWithSemicolon>().expect_offense(indoc! {r#"
            unless x; y end
            ^^^^^^^^^^^^^^^ Do not use `unless x;` - use a ternary operator instead.
        "#});
    }

    // --- Newline message when begin node (multiple stmts) ---

    #[test]
    fn flags_if_with_begin_branch_newline_msg() {
        test::<IfWithSemicolon>().expect_offense(indoc! {r#"
            if x; a; b end
            ^^^^^^^^^^^^^^ Do not use `if x;` - use a newline instead.
        "#});
    }

    // --- Newline message when return with argument ---

    #[test]
    fn flags_if_with_return_arg_newline_msg() {
        test::<IfWithSemicolon>().expect_offense(indoc! {r#"
            if x; return 1 end
            ^^^^^^^^^^^^^^^^^^ Do not use `if x;` - use a newline instead.
        "#});
    }

    // --- If/else message ---

    #[test]
    fn flags_if_with_else_if_message() {
        test::<IfWithSemicolon>().expect_offense(indoc! {r#"
            if x; a else b end
            ^^^^^^^^^^^^^^^^^^ Do not use `if x;` - use a ternary operator instead.
        "#});
    }

    // --- Accepted forms ---

    #[test]
    fn accepts_if_then_form() {
        test::<IfWithSemicolon>().expect_no_offenses("if x then y end\n");
    }

    #[test]
    fn accepts_multiline_if() {
        test::<IfWithSemicolon>().expect_no_offenses(indoc! {"
            if x
              y
            end
        "});
    }

    #[test]
    fn accepts_modifier_if() {
        test::<IfWithSemicolon>().expect_no_offenses("y if x\n");
    }

    #[test]
    fn accepts_ternary() {
        test::<IfWithSemicolon>().expect_no_offenses("x ? y : z\n");
    }

    // --- Autocorrect: ternary ---

    #[test]
    fn corrects_simple_if_to_ternary() {
        test::<IfWithSemicolon>().expect_correction(
            indoc! {r#"
                if x; y end
                ^^^^^^^^^^^ Do not use `if x;` - use a ternary operator instead.
            "#},
            "x ? y : nil\n",
        );
    }

    #[test]
    fn corrects_if_else_to_ternary() {
        test::<IfWithSemicolon>().expect_correction(
            indoc! {r#"
                if x; a else b end
                ^^^^^^^^^^^^^^^^^^ Do not use `if x;` - use a ternary operator instead.
            "#},
            "x ? a : b\n",
        );
    }

    #[test]
    fn corrects_unless_else_to_ternary() {
        // `unless x; a else b end` -> AST: `if x: then_=b, else_=a`
        // Ternary: `x ? b : a` (the "unless" body `a` appears after `:`)
        test::<IfWithSemicolon>().expect_correction(
            indoc! {r#"
                unless x; a else b end
                ^^^^^^^^^^^^^^^^^^^^^^ Do not use `unless x;` - use a ternary operator instead.
            "#},
            "x ? b : a\n",
        );
    }

    // --- Autocorrect: newline ---

    #[test]
    fn corrects_begin_branch_to_newline() {
        test::<IfWithSemicolon>().expect_correction(
            indoc! {r#"
                if x; a; b end
                ^^^^^^^^^^^^^^ Do not use `if x;` - use a newline instead.
            "#},
            "if x\n a; b end\n",
        );
    }
}

murphy_plugin_api::submit_cop!(IfWithSemicolon);
