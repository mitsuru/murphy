//! `Rails/RedundantTravelBack` — redundant `travel_back` in teardown/after.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/RedundantTravelBack
//! upstream_version_checked: 2.35.0
//! version_added: "2.12"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0: RESTRICT_ON_SEND [:travel_back] with a
//!   `def teardown` or `after` block ancestor, whole-node offense with
//!   whole-line removal autocorrect, and `minimum_target_rails_version 5.2`
//!   gating. Upstream Include (`**/spec/**/*.rb`, `**/test/**/*.rb`) is
//!   enforced via the murphy-rails pack default.yml (engine
//!   `cop_applies_to_file` gate, verified vs rubocop-rails 2.35.0
//!   default.yml).
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct RedundantTravelBack;

#[cop(
    name = "Rails/RedundantTravelBack",
    description = "Checks for redundant `travel_back` calls.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl RedundantTravelBack {
    // Mirrors upstream `RESTRICT_ON_SEND`.
    #[on_node(kind = "send", methods = ["travel_back"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    if !cx.rails_version_at_least(5, 2) {
        return;
    }
    if !matches!(*cx.kind(node), NodeKind::Send { .. }) {
        return;
    }
    if !in_teardown_or_after(cx, node) {
        return;
    }
    cx.emit_offense(
        cx.range(node),
        "Redundant `travel_back` detected.",
        None,
    );
    let whole = cx.range_by_whole_lines(cx.range(node), true);
    cx.emit_edit(whole, "");
}

fn in_teardown_or_after(cx: &Cx<'_>, node: NodeId) -> bool {
    for anc in cx.ancestors(node) {
        match *cx.kind(anc) {
            NodeKind::Def { name, .. } => {
                if cx.symbol_str(name) == "teardown" {
                    return true;
                }
            }
            NodeKind::Block { call, .. } => {
                if cx.method_name(call) == Some("after") {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::RedundantTravelBack;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_in_teardown() {
        test::<RedundantTravelBack>().expect_offense(indoc! {r#"
            def teardown
              do_something
              travel_back
              ^^^^^^^^^^^ Redundant `travel_back` detected.
            end
        "#});
    }

    #[test]
    fn corrects_in_teardown() {
        test::<RedundantTravelBack>().expect_correction(
            indoc! {r#"
                def teardown
                  do_something
                  travel_back
                  ^^^^^^^^^^^ Redundant `travel_back` detected.
                end
            "#},
            "def teardown\n  do_something\nend\n",
        );
    }

    #[test]
    fn flags_in_after_block() {
        test::<RedundantTravelBack>().expect_offense(indoc! {r#"
            after do
              do_something
              travel_back
              ^^^^^^^^^^^ Redundant `travel_back` detected.
            end
        "#});
    }

    #[test]
    fn corrects_in_after_block() {
        test::<RedundantTravelBack>().expect_correction(
            indoc! {r#"
                after do
                  do_something
                  travel_back
                  ^^^^^^^^^^^ Redundant `travel_back` detected.
                end
            "#},
            "after do\n  do_something\nend\n",
        );
    }

    #[test]
    fn allows_outside_teardown() {
        test::<RedundantTravelBack>().expect_no_offenses(indoc! {r#"
            def do_something
              travel_back
            end
        "#});
    }

    #[test]
    fn allows_outside_after() {
        test::<RedundantTravelBack>().expect_no_offenses(indoc! {r#"
            do_something do
              travel_back
            end
        "#});
    }

    #[test]
    fn gated_below_rails_52() {
        test::<RedundantTravelBack>()
            .with_target_rails_version(5, 1)
            .expect_no_offenses(indoc! {r#"
                def teardown
                  do_something
                  travel_back
                end
            "#});
    }
}
murphy_plugin_api::submit_cop!(RedundantTravelBack);
