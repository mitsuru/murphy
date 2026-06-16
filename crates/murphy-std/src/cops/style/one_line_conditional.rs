//! `Style/OneLineConditional` — flags single-line `if/then/else/end` constructs.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/OneLineConditional
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags single-line `if/unless` with both `then` and `else` branches
//!   present (not `elsif`). By default, autocorrects to ternary operator.
//!   With `AlwaysCorrectToMultiline: true`, autocorrects to multi-line form.
//!
//!   Message is parameterized with the keyword (`if` or `unless`):
//!     "Favor the ternary operator (`?:`) over single-line
//!      `%<keyword>s/then/else/end` constructs."
//!   or with AlwaysCorrectToMultiline:
//!     "Favor multi-line `%<keyword>s` over single-line
//!      `%<keyword>s/then/else/end` constructs."
//!
//!   Guards (fire only when ALL hold):
//!     - node is single-line (`!is_multiline`)
//!     - else branch is present
//!     - node is not `elsif`
//!     - then branch is not a `begin` node (multiple statements)
//!
//!   In Murphy's AST, `unless c then a else b` is encoded as
//!   `if c; b; a` (branches swapped). The ternary correction always uses
//!   `cond ? then_ : else_` (AST children order), which produces
//!   the correct result for both `if` and `unless`.
//!
//!   Parenthesization for ternary (mirrors RuboCop's `requires_parentheses?`):
//!     - `and`/`or`/`if` nodes → always wrap
//!     - assignment nodes → always wrap
//!     - Send with unparenthesized non-operator arguments → wrap
//!     - `return`/`next`/`break`/`yield` with argument → wrap
//!
//!   Outer parenthesization (wraps the whole `a ? b : c`):
//!     - parent is an `and`/`or` operator keyword → wrap
//!     - parent is an operator-method Send → wrap
//!
//!   `cannot_replace_to_ternary?`: falls back to multiline when:
//!     - else branch is an `elsif` node
//!     - else branch is a `begin` node with 2+ non-nil statements
//!
//!   Multiline correction uses the current-line leading whitespace plus 2
//!   spaces for body lines. Handles the elsif chain by walking it recursively.
//!
//!   Gaps:
//!     - `AlwaysCorrectToMultiline: true` is implemented (forces multiline even
//!       when ternary would be possible).
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const MSG_TERNARY: &str =
    "Favor the ternary operator (`?:`) over single-line `%<keyword>s/then/else/end` constructs.";
const MSG_MULTILINE: &str =
    "Favor multi-line `%<keyword>s` over single-line `%<keyword>s/then/else/end` constructs.";

#[derive(Default)]
pub struct OneLineConditional;

#[derive(CopOptions)]
pub struct Options {
    #[option(
        name = "AlwaysCorrectToMultiline",
        default = false,
        description = "When `true`, always correct to multi-line form instead of ternary."
    )]
    pub always_correct_to_multiline: bool,
}

#[cop(
    name = "Style/OneLineConditional",
    description = "Favor the ternary operator or multi-line constructs over single-line `if/then/else/end` constructs.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl OneLineConditional {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        check(node, cx, opts.always_correct_to_multiline);
    }
}

fn check(node: NodeId, cx: &Cx<'_>, always_multiline: bool) {
    // Skip modifier form (`x if cond`) and ternaries (`cond ? a : b`).
    if cx.is_modifier_form(node) || cx.is_ternary(node) {
        return;
    }

    // Skip elsif.
    if cx.is_elsif(node) {
        return;
    }

    // Must be single-line.
    if cx.is_multiline(node) {
        return;
    }

    // Must have an else branch.
    let else_branch = match cx.if_else_branch(node).get() {
        Some(b) => b,
        None => return,
    };

    // Skip if then branch is a `begin` node (multiple statements).
    if let Some(then_) = cx.if_then_branch(node).get()
        && matches!(cx.kind(then_), NodeKind::Begin(_)) {
            return;
        }

    // Determine correction mode.
    let use_multiline = always_multiline || cannot_replace_to_ternary(else_branch, cx);

    let keyword = if cx.is_unless(node) { "unless" } else { "if" };
    let msg_template = if use_multiline {
        MSG_MULTILINE
    } else {
        MSG_TERNARY
    };
    let message = msg_template.replace("%<keyword>s", keyword);

    cx.emit_offense(cx.range(node), &message, None);

    // Skip nested conditionals that have an ancestor being corrected.
    // (RuboCop's `ignore_node/part_of_ignored_node?` pattern.)
    let has_corrected_ancestor = cx
        .ancestors(node)
        .any(|anc| is_flaggable_one_liner(anc, cx));
    if has_corrected_ancestor {
        return;
    }

    if use_multiline {
        emit_multiline_correction(node, cx);
    } else {
        emit_ternary_correction(node, cx);
    }
}

/// Returns `true` if `node` itself would be flagged as a one-liner.
fn is_flaggable_one_liner(node: NodeId, cx: &Cx<'_>) -> bool {
    if !matches!(cx.kind(node), NodeKind::If { .. }) {
        return false;
    }
    if cx.is_modifier_form(node) || cx.is_ternary(node) || cx.is_elsif(node) {
        return false;
    }
    if cx.is_multiline(node) {
        return false;
    }
    if cx.if_else_branch(node).get().is_none() {
        return false;
    }
    if let Some(then_) = cx.if_then_branch(node).get()
        && matches!(cx.kind(then_), NodeKind::Begin(_)) {
            return false;
        }
    true
}

/// Returns `true` if the node cannot be safely converted to a ternary.
fn cannot_replace_to_ternary(else_branch: NodeId, cx: &Cx<'_>) -> bool {
    // Else branch is an `elsif` — needs multiline form.
    if cx.is_elsif(else_branch) {
        return true;
    }
    // Else branch has multiple statements.
    if let NodeKind::Begin(stmts) = cx.kind(else_branch) {
        let list = cx.list(*stmts);
        if list.len() >= 2 {
            return true;
        }
    }
    false
}

/// Emits the ternary correction: `cond ? then_ : else_`.
fn emit_ternary_correction(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::If { cond, then_, else_ } = *cx.kind(node) else {
        return;
    };

    let cond_src = expr_replacement(cond, cx);
    let then_src = opt_node_replacement(then_.get(), cx);
    let else_src = opt_node_replacement(else_.get(), cx);

    let ternary = format!("{cond_src} ? {then_src} : {else_src}");

    // Wrap the whole ternary in parens if the parent requires it.
    let replacement = if needs_outer_parens(node, cx) {
        format!("({ternary})")
    } else {
        ternary
    };

    cx.emit_edit(cx.range(node), &replacement);
}

/// Returns `true` if the whole ternary expression needs outer parentheses.
fn needs_outer_parens(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };
    if cx.is_operator_keyword(parent) {
        return true;
    }
    if let NodeKind::Send { method, .. } = cx.kind(parent) {
        return is_operator_method(cx.symbol_str(*method));
    }
    false
}

/// Returns the source replacement for an optional branch node (nil → "nil").
fn opt_node_replacement(node: Option<NodeId>, cx: &Cx<'_>) -> String {
    match node {
        Some(n) => expr_replacement(n, cx),
        None => "nil".to_owned(),
    }
}

/// Returns the source of `node`, wrapped in parens if needed.
fn expr_replacement(node: NodeId, cx: &Cx<'_>) -> String {
    let src = cx.raw_source(cx.range(node));
    if requires_parentheses(node, cx) {
        format!("({src})")
    } else {
        src.to_owned()
    }
}

/// Mirrors RuboCop's `requires_parentheses?`.
fn requires_parentheses(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::And { .. } | NodeKind::Or { .. } | NodeKind::If { .. } => return true,
        _ => {}
    }
    if cx.is_assignment(node) {
        return true;
    }
    if let NodeKind::Send { method, args, .. } | NodeKind::Csend { method, args, .. } = cx.kind(node)
        && !cx.list(*args).is_empty()
            && !cx.is_parenthesized(node)
            && !is_operator_method(cx.symbol_str(*method))
        {
            return true;
        }
    keyword_with_changed_precedence(node, cx)
}

/// Mirrors RuboCop's `keyword_with_changed_precedence?`.
fn keyword_with_changed_precedence(node: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(node) {
        NodeKind::Return(v) | NodeKind::Next(v) | NodeKind::Break(v) => v.get().is_some(),
        NodeKind::Yield(list) => !cx.list(*list).is_empty(),
        _ => false,
    }
}

/// Returns `true` if `name` is an operator method (mirrors ternary_parentheses.rs).
fn is_operator_method(name: &str) -> bool {
    if name == "[]" {
        return false;
    }
    let bytes = name.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    matches!(
        bytes[0],
        b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>' | b'=' | b'!' | b'~' | b'&' | b'|'
            | b'^'
    )
}

/// Emits the multiline correction.
///
/// For `if cond then a else b end` (single level):
/// ```ruby
/// if cond
///   a
/// else
///   b
/// end
/// ```
///
/// For `if cond then a elsif cond2 then b end` (with elsif chain):
/// ```ruby
/// if cond
///   a
/// elsif cond2
///   b
/// end
/// ```
fn emit_multiline_correction(node: NodeId, cx: &Cx<'_>) {
    let source = cx.source().as_bytes();
    let node_start = cx.range(node).start as usize;

    // Compute base indentation from the current line.
    let line_start = source[..node_start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    let leading_ws_len = source[line_start..node_start]
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .count();
    let base_indent =
        std::str::from_utf8(&source[line_start..line_start + leading_ws_len]).unwrap_or("");
    let body_indent = format!("{base_indent}  ");

    let replacement = build_multiline(node, base_indent, &body_indent, cx);
    cx.emit_edit(cx.range(node), &replacement);
}

/// Recursively builds the multiline replacement string for an if/elsif/else chain.
fn build_multiline(node: NodeId, base_indent: &str, body_indent: &str, cx: &Cx<'_>) -> String {
    let NodeKind::If { cond, then_, else_ } = *cx.kind(node) else {
        return cx.raw_source(cx.range(node)).to_owned();
    };

    let keyword = if cx.is_elsif(node) {
        "elsif"
    } else if cx.is_unless(node) {
        "unless"
    } else {
        "if"
    };

    let cond_src = cx.raw_source(cx.range(cond));
    let then_src = match then_.get() {
        Some(t) => cx.raw_source(cx.range(t)).to_owned(),
        None => "nil".to_owned(),
    };

    let mut result = format!("{keyword} {cond_src}\n{body_indent}{then_src}");

    // Walk the else chain.
    match else_.get() {
        None => {
            // No else — close with end.
            result.push_str(&format!("\n{base_indent}end"));
        }
        Some(else_node) if cx.is_elsif(else_node) => {
            // elsif: recurse and prepend the `elsif` keyword.
            let elsif_part = build_multiline_elsif(else_node, base_indent, body_indent, cx);
            result.push_str(&format!("\n{base_indent}{elsif_part}"));
        }
        Some(else_node) => {
            // Plain else branch.
            let else_src = cx.raw_source(cx.range(else_node));
            result.push_str(&format!(
                "\n{base_indent}else\n{body_indent}{else_src}\n{base_indent}end"
            ));
        }
    }

    result
}

/// Builds the continuation of an elsif chain (used by `build_multiline`).
fn build_multiline_elsif(
    node: NodeId,
    base_indent: &str,
    body_indent: &str,
    cx: &Cx<'_>,
) -> String {
    let NodeKind::If { cond, then_, else_ } = *cx.kind(node) else {
        return cx.raw_source(cx.range(node)).to_owned();
    };

    let cond_src = cx.raw_source(cx.range(cond));
    let then_src = match then_.get() {
        Some(t) => cx.raw_source(cx.range(t)).to_owned(),
        None => "nil".to_owned(),
    };

    let mut result = format!("elsif {cond_src}\n{body_indent}{then_src}");

    match else_.get() {
        None => {
            result.push_str(&format!("\n{base_indent}end"));
        }
        Some(else_node) if cx.is_elsif(else_node) => {
            let elsif_part = build_multiline_elsif(else_node, base_indent, body_indent, cx);
            result.push_str(&format!("\n{base_indent}{elsif_part}"));
        }
        Some(else_node) => {
            let else_src = cx.raw_source(cx.range(else_node));
            result.push_str(&format!(
                "\n{base_indent}else\n{body_indent}{else_src}\n{base_indent}end"
            ));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::OneLineConditional;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_if_then_else() {
        test::<OneLineConditional>().expect_correction(
            indoc! {r#"
                if cond then run else dont end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor the ternary operator (`?:`) over single-line `if/then/else/end` constructs.
            "#},
            "cond ? run : dont\n",
        );
    }

    #[test]
    fn flags_and_corrects_unless_then_else() {
        test::<OneLineConditional>().expect_correction(
            indoc! {r#"
                unless cond then run else dont end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor the ternary operator (`?:`) over single-line `unless/then/else/end` constructs.
            "#},
            "cond ? dont : run\n",
        );
    }

    #[test]
    fn flags_empty_then_branch() {
        test::<OneLineConditional>().expect_correction(
            indoc! {r#"
                if cond then else dont end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor the ternary operator (`?:`) over single-line `if/then/else/end` constructs.
            "#},
            "cond ? nil : dont\n",
        );
    }

    #[test]
    fn accepts_if_without_else() {
        test::<OneLineConditional>().expect_no_offenses(indoc! {"
            if cond then run end
        "});
    }

    #[test]
    fn accepts_multiline_if() {
        test::<OneLineConditional>().expect_no_offenses(indoc! {"
            if cond
              run
            else
              dont
            end
        "});
    }

    #[test]
    fn accepts_modifier_if() {
        test::<OneLineConditional>().expect_no_offenses(indoc! {"
            run if cond
        "});
    }

    #[test]
    fn accepts_ternary() {
        test::<OneLineConditional>().expect_no_offenses(indoc! {"
            cond ? run : dont
        "});
    }

    #[test]
    fn accepts_if_with_multiple_statements_in_then() {
        test::<OneLineConditional>().expect_no_offenses(indoc! {"
            if cond then x; y else z end
        "});
    }

    #[test]
    fn parenthesizes_send_with_unparenthesized_args() {
        test::<OneLineConditional>().expect_correction(
            indoc! {r#"
                if cond then puts 1 else puts 2 end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor the ternary operator (`?:`) over single-line `if/then/else/end` constructs.
            "#},
            "cond ? (puts 1) : (puts 2)\n",
        );
    }

    #[test]
    fn parenthesizes_and_or_branches() {
        test::<OneLineConditional>().expect_correction(
            indoc! {r#"
                if c then a and b else d or e end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor the ternary operator (`?:`) over single-line `if/then/else/end` constructs.
            "#},
            "c ? (a and b) : (d or e)\n",
        );
    }

    #[test]
    fn wraps_outer_when_parent_is_operator_keyword() {
        test::<OneLineConditional>().expect_correction(
            indoc! {r#"
                a and if cond then run else dont end
                      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor the ternary operator (`?:`) over single-line `if/then/else/end` constructs.
            "#},
            "a and (cond ? run : dont)\n",
        );
    }

    #[test]
    fn falls_back_to_multiline_for_elsif() {
        test::<OneLineConditional>().expect_correction(
            indoc! {r#"
                if cond then run elsif cond2 then dont end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor multi-line `if` over single-line `if/then/else/end` constructs.
            "#},
            "if cond\n  run\nelsif cond2\n  dont\nend\n",
        );
    }

    #[test]
    fn operator_method_does_not_need_parens() {
        test::<OneLineConditional>().expect_correction(
            indoc! {r#"
                if c then 1 + 1 else 2 + 2 end
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor the ternary operator (`?:`) over single-line `if/then/else/end` constructs.
            "#},
            "c ? 1 + 1 : 2 + 2\n",
        );
    }

    #[test]
    fn wraps_outer_when_parent_is_operator_send() {
        test::<OneLineConditional>().expect_correction(
            indoc! {r#"
                0 + if cond then run else dont end
                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Favor the ternary operator (`?:`) over single-line `if/then/else/end` constructs.
            "#},
            "0 + (cond ? run : dont)\n",
        );
    }
}

murphy_plugin_api::submit_cop!(OneLineConditional);
