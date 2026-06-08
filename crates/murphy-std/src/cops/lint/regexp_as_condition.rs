//! `Lint/RegexpAsCondition` — avoids regexp literals as implicit `$_` conditions.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/RegexpAsCondition
//! upstream_version_checked: master
//! status: partial
//! gap_issues: []
//! notes: >
//!   Initial v1 port covers regexp literals used directly as `if`, `unless`,
//!   `while`, and `until` AST conditions and autocorrects them to `/re/ =~ $_`.
//!   Negated conditions and RuboCop's exact `match-current-line` dispatch are
//!   documented v1 gaps.
//! ```

use murphy_plugin_api::{cop, Cx, NoOptions, NodeId, NodeKind};

const MSG: &str =
    "Do not use regexp literal as a condition. The regexp literal matches `$_` implicitly.";

#[derive(Default)]
pub struct RegexpAsCondition;

#[cop(
    name = "Lint/RegexpAsCondition",
    description = "Checks regexp literals used as implicit current-line conditions.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl RegexpAsCondition {
    #[on_node(kind = "if")]
    fn check_if(&self, node: NodeId, cx: &Cx<'_>) {
        check_cond(node, cx);
    }

    #[on_node(kind = "while")]
    fn check_while(&self, node: NodeId, cx: &Cx<'_>) {
        check_cond(node, cx);
    }

    #[on_node(kind = "until")]
    fn check_until(&self, node: NodeId, cx: &Cx<'_>) {
        check_cond(node, cx);
    }
}

fn check_cond(node: NodeId, cx: &Cx<'_>) {
    let cond = match *cx.kind(node) {
        NodeKind::If { cond, .. } | NodeKind::While { cond, .. } | NodeKind::Until { cond, .. } => {
            cond
        }
        _ => return,
    };
    if !matches!(cx.kind(cond), NodeKind::Regexp { .. } | NodeKind::Unknown) {
        return;
    }
    let range = cx.range(cond);
    let source = cx.raw_source(range);
    if !source.trim_start().starts_with('/') {
        return;
    }
    let replacement = format!("{source} =~ $_");
    cx.emit_offense(range, MSG, None);
    cx.emit_edit(range, &replacement);
}

murphy_plugin_api::submit_cop!(RegexpAsCondition);

#[cfg(test)]
mod tests {
    use super::RegexpAsCondition;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_corrects_regexp_if_condition() {
        test::<RegexpAsCondition>().expect_correction(
            indoc! {r#"
                if /foo/
                   ^^^^^ Do not use regexp literal as a condition. The regexp literal matches `$_` implicitly.
                  work
                end
            "#},
            "if /foo/ =~ $_\n  work\nend\n",
        );
    }

    #[test]
    fn flags_regexp_while_condition() {
        test::<RegexpAsCondition>().expect_offense(indoc! {r#"
            while /foo/
                  ^^^^^ Do not use regexp literal as a condition. The regexp literal matches `$_` implicitly.
              work
            end
        "#});
    }

    #[test]
    fn accepts_explicit_match() {
        test::<RegexpAsCondition>().expect_no_offenses("if /foo/ =~ line\n  work\nend\n");
    }

    #[test]
    fn accepts_heredoc_text_that_looks_like_condition() {
        test::<RegexpAsCondition>().expect_no_offenses("text = <<~RUBY\n  if /foo/\nRUBY\n");
    }
}
