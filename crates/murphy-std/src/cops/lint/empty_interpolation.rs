//! `Lint/EmptyInterpolation` — flag interpolation with no meaningful expression.
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Lint/EmptyInterpolation
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors RuboCop's empty interpolation detection for dstr/dsym/xstr/regexp
//!   parts and removes the empty interpolation. Percent literal arrays are skipped.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

#[derive(Default)]
pub struct EmptyInterpolation;

#[cop(
    name = "Lint/EmptyInterpolation",
    description = "Flag empty string interpolation.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions
)]
impl EmptyInterpolation {
    #[on_node(kind = "begin")]
    fn check_begin(&self, node: NodeId, cx: &Cx<'_>) {
        if !is_interpolation_part(node, cx) || in_percent_literal_array(node, cx) {
            return;
        }
        let NodeKind::Begin(children) = *cx.kind(node) else { return; };
        let children = cx.list(children);
        if children.iter().all(|&child| is_empty_interpolation_child(child, cx)) {
            cx.emit_offense(cx.range(node), "Empty interpolation detected.", None);
            cx.emit_edit(cx.range(node), "");
        }
    }
}

fn is_interpolation_part(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else { return false; };
    matches!(cx.kind(parent), NodeKind::Dstr(_) | NodeKind::Dsym(_) | NodeKind::Xstr(_) | NodeKind::Regexp { .. })
}

fn is_empty_interpolation_child(node: NodeId, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Nil => true,
        NodeKind::Str(id) => cx.string_str(id).is_empty(),
        _ => false,
    }
}

fn in_percent_literal_array(mut node: NodeId, cx: &Cx<'_>) -> bool {
    while let Some(parent) = cx.parent(node).get() {
        if matches!(cx.kind(parent), NodeKind::Array(_)) && cx.is_percent_literal(parent) {
            return true;
        }
        node = parent;
    }
    false
}

murphy_plugin_api::submit_cop!(EmptyInterpolation);

#[cfg(test)]
mod tests {
    use super::EmptyInterpolation;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_and_removes_empty_interpolation() {
        test::<EmptyInterpolation>().expect_correction(
            indoc! {r#"
                "result is #{}"
                           ^^^ Empty interpolation detected.
            "#},
            "\"result is \"\n",
        );
    }

    #[test]
    fn flags_nil_and_empty_string_interpolation() {
        test::<EmptyInterpolation>()
            .expect_offense(indoc! {r#"
                "result is #{nil}"
                           ^^^^^^ Empty interpolation detected.
            "#})
            .expect_offense(indoc! {r#"
                "result is #{''}"
                           ^^^^^ Empty interpolation detected.
            "#});
    }

    #[test]
    fn accepts_non_empty_interpolation_and_percent_arrays() {
        test::<EmptyInterpolation>()
            .expect_no_offenses("\"result is #{value}\"\n")
            .expect_no_offenses("%W[#{''} one two]\n");
    }
}
