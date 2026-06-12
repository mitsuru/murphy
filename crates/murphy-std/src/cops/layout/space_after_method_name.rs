//! `Layout/SpaceAfterMethodName` — forbid a space between a method name and the
//! opening parenthesis of its parameter list in a method definition.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Layout/SpaceAfterMethodName
//! upstream_version_checked: master
//! status: verified
//! gap_issues: []
//! notes: >
//!   Port of RuboCop's `on_def`/`on_defs`. Fires on `def foo (x)` (a space
//!   between the method name and the parenthesized parameter list) and removes
//!   the gap. Only parenthesized parameter lists are considered, mirroring
//!   `args.parenthesized_call?`; `def foo x` (no parens) and `def foo` (no
//!   args) are left untouched. The whole gap between the name and `(` is
//!   removed in one pass (RuboCop removes a single char and relies on fixpoint
//!   re-runs; the result is identical for the common single-space case and
//!   more robust for `def foo  (x)`).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct SpaceAfterMethodName;

#[cop(
    name = "Layout/SpaceAfterMethodName",
    description = "Do not put a space between a method name and the opening parenthesis in a method definition.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SpaceAfterMethodName {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let Some(name_range) = find_method_name_range(node, cx) else {
        return;
    };

    // The parameter-list opener is the first token after the method name. If it
    // is not `(`, the parameter list is not parenthesized (`def foo x`, `def
    // foo`) — nothing to flag. This mirrors `args.parenthesized_call?`.
    let node_end = cx.range(node).end;
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < name_range.end);
    let Some(open_paren) = toks.get(idx).filter(|t| t.range.start < node_end) else {
        return;
    };
    if open_paren.kind != SourceTokenKind::LeftParen {
        return;
    }

    // The gap between the method name and `(`. RuboCop only fires when this gap
    // begins with a space; a newline-separated `(` cannot occur in a method
    // definition header, so any non-empty gap here is inline whitespace.
    let gap = Range {
        start: name_range.end,
        end: open_paren.range.start,
    };
    if gap.start >= gap.end {
        return;
    }
    if !cx.raw_source(gap).starts_with(' ') {
        return;
    }

    cx.emit_offense(
        gap,
        "Do not put a space between a method name and the opening parenthesis.",
        None,
    );
    cx.emit_edit(gap, "");
}

/// Find the method name token range by searching for the `Other` token whose
/// bytes equal the method name `Symbol`. Mirrors the helper in
/// `style/method_def_parentheses.rs`.
fn find_method_name_range(node: NodeId, cx: &Cx<'_>) -> Option<Range> {
    let name_sym = match cx.kind(node) {
        NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => *name,
        _ => return None,
    };
    let name_str = cx.symbol_str(name_sym);
    let node_range = cx.range(node);

    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < node_range.start);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| t.kind == SourceTokenKind::Other && cx.raw_source(t.range) == name_str)
        .map(|t| t.range)
}

murphy_plugin_api::submit_cop!(SpaceAfterMethodName);

#[cfg(test)]
mod tests {
    use super::SpaceAfterMethodName;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_space_before_paren_in_def() {
        test::<SpaceAfterMethodName>().expect_correction(
            indoc! {r#"
                def foo (x)
                       ^ Do not put a space between a method name and the opening parenthesis.
                end
            "#},
            "def foo(x)\nend\n",
        );
    }

    #[test]
    fn accepts_no_space_before_paren() {
        test::<SpaceAfterMethodName>().expect_no_offenses("def foo(x)\nend\n");
    }

    #[test]
    fn ignores_unparenthesized_args() {
        test::<SpaceAfterMethodName>().expect_no_offenses("def foo x\nend\n");
    }

    #[test]
    fn ignores_def_without_args() {
        test::<SpaceAfterMethodName>().expect_no_offenses("def foo\nend\n");
    }

    #[test]
    fn flags_space_before_paren_in_defs() {
        test::<SpaceAfterMethodName>().expect_correction(
            indoc! {r#"
                def self.foo (x)
                            ^ Do not put a space between a method name and the opening parenthesis.
                end
            "#},
            "def self.foo(x)\nend\n",
        );
    }

    #[test]
    fn flags_space_before_empty_paren() {
        test::<SpaceAfterMethodName>().expect_correction(
            indoc! {r#"
                def foo ()
                       ^ Do not put a space between a method name and the opening parenthesis.
                end
            "#},
            "def foo()\nend\n",
        );
    }
}
