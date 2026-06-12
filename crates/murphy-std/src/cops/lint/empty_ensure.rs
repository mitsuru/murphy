//! `Lint/EmptyEnsure` — flag an empty `ensure` block.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EmptyEnsure
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's on_ensure: flags when the ensure clause has no body
//!   (`node.branch` is nil). Offense range and autocorrect both target the
//!   `ensure` keyword token, which is located by scanning the tokens after
//!   the protected body so a nested `begin/ensure` inside the body cannot be
//!   matched first. Message text and autocorrect (remove the keyword) match
//!   RuboCop verbatim.
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, cop};

#[derive(Default)]
pub struct EmptyEnsure;

#[cop(
    name = "Lint/EmptyEnsure",
    description = "Flag empty ensure blocks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyEnsure {
    #[on_node(kind = "ensure")]
    fn check_ensure(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Ensure { body, ensure_ } = *cx.kind(node) else {
            return;
        };
        // RuboCop: `return if node.branch` — `branch` is the ensure clause
        // body. A present body means the ensure is not empty.
        if ensure_.get().is_some() {
            return;
        }
        let Some(keyword) = ensure_keyword_range(node, body, cx) else {
            return;
        };
        cx.emit_offense(keyword, "Empty `ensure` block detected.", None);
        // RuboCop's `corrector.remove(node.loc.keyword)`.
        cx.emit_edit(keyword, "");
    }
}

/// The `ensure` keyword token range. The `Ensure` node's range starts at the
/// protected body, so the keyword sits after it. Scanning from the end of the
/// protected body (when present) avoids matching a nested `begin/ensure`'s
/// keyword inside the body.
fn ensure_keyword_range(node: NodeId, body: OptNodeId, cx: &Cx<'_>) -> Option<Range> {
    let node_range = cx.range(node);
    let search_start = body.get().map_or(node_range.start, |b| cx.range(b).end);
    let search_range = Range {
        start: search_start,
        end: node_range.end,
    };
    cx.tokens_in(search_range)
        .iter()
        .find(|&&tok| cx.token_text(tok) == "ensure")
        .map(|tok| tok.range)
}

murphy_plugin_api::submit_cop!(EmptyEnsure);

#[cfg(test)]
mod tests {
    use super::EmptyEnsure;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_empty_ensure() {
        test::<EmptyEnsure>().expect_offense(indoc! {r#"
            def foo
              something
            ensure
            ^^^^^^ Empty `ensure` block detected.
            end
        "#});
    }

    #[test]
    fn accepts_ensure_with_body() {
        test::<EmptyEnsure>().expect_no_offenses(indoc! {r#"
            def foo
              something
            ensure
              cleanup
            end
        "#});
    }

    #[test]
    fn flags_empty_ensure_in_begin_block() {
        test::<EmptyEnsure>().expect_offense(indoc! {r#"
            begin
              something
            ensure
            ^^^^^^ Empty `ensure` block detected.
            end
        "#});
    }

    #[test]
    fn does_not_match_nested_ensure_keyword_in_body() {
        // The outer ensure has a body (the inner begin/ensure), so it is not
        // flagged; the inner ensure is empty and is the only offense, with the
        // keyword located on the inner `ensure`, not the outer.
        test::<EmptyEnsure>().expect_offense(indoc! {r#"
            begin
              begin
                work
              ensure
              ^^^^^^ Empty `ensure` block detected.
              end
            ensure
              cleanup
            end
        "#});
    }

    #[test]
    fn autocorrect_removes_empty_ensure_keyword() {
        test::<EmptyEnsure>().expect_correction(
            indoc! {r#"
                def foo
                  something
                ensure
                ^^^^^^ Empty `ensure` block detected.
                end
            "#},
            indoc! {r#"
                def foo
                  something

                end
            "#},
        );
    }
}
