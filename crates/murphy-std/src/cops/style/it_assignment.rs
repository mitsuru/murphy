//! `Style/ItAssignment` — flags local variables and method parameters named `it`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ItAssignment
//! upstream_version_checked: 1.70.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - Local variable assignments (`lvasgn`): `it = 5`.
//!     - Method parameters: `arg`, `optarg`, `restarg`, `kwarg`, `kwoptarg`,
//!       `kwrestarg`, `blockarg` — all parameter forms that can be named `it`.
//!     - Block parameters (pipe-style `|it|`) are also `arg` nodes, so they
//!       are caught by the same handler.
//!   Range notes:
//!     - `lvasgn`: Prism does not populate `loc.name` for local-variable write
//!       nodes; the name range is computed from the expression start.
//!     - `kwarg`/`kwoptarg`: Prism's `name_loc` covers `name:` (including the
//!       colon), so the offense range is shortened to just the identifier.
//!   No autocorrect — RuboCop upstream does not include one.
//!   No configurable options.
//! ```
//!
//! ## Matched shapes
//!
//! Any assignment to or parameter named `it`:
//!
//! ```ruby
//! # bad
//! it = 5
//! def foo(it); end
//! def foo(it = 5); end
//! def foo(*it); end
//! def foo(it:); end
//! def foo(it: 5); end
//! def foo(**it); end
//! def foo(&it); end
//! [1].each { |it| }
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

const MSG: &str = "`it` is the default block parameter; consider another name.";

/// Stateless unit struct.
#[derive(Default)]
pub struct ItAssignment;

#[cop(
    name = "Style/ItAssignment",
    description = "Checks for local variables and method parameters named `it`.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl ItAssignment {
    #[on_node(kind = "lvasgn")]
    fn check_lvasgn(&self, node: NodeId, cx: &Cx<'_>) {
        check_name(node, cx);
    }

    #[on_node(kind = "arg")]
    fn check_arg(&self, node: NodeId, cx: &Cx<'_>) {
        check_name(node, cx);
    }

    #[on_node(kind = "optarg")]
    fn check_optarg(&self, node: NodeId, cx: &Cx<'_>) {
        check_name(node, cx);
    }

    #[on_node(kind = "restarg")]
    fn check_restarg(&self, node: NodeId, cx: &Cx<'_>) {
        check_name(node, cx);
    }

    #[on_node(kind = "kwarg")]
    fn check_kwarg(&self, node: NodeId, cx: &Cx<'_>) {
        check_name(node, cx);
    }

    #[on_node(kind = "kwoptarg")]
    fn check_kwoptarg(&self, node: NodeId, cx: &Cx<'_>) {
        check_name(node, cx);
    }

    #[on_node(kind = "kwrestarg")]
    fn check_kwrestarg(&self, node: NodeId, cx: &Cx<'_>) {
        check_name(node, cx);
    }

    #[on_node(kind = "blockarg")]
    fn check_blockarg(&self, node: NodeId, cx: &Cx<'_>) {
        check_name(node, cx);
    }
}

/// Check whether the node's name is `it` and emit an offense covering just the identifier.
fn check_name(node: NodeId, cx: &Cx<'_>) {
    let Some((name_sym, name_range)) = identifier_name_range(node, cx) else {
        return;
    };
    if cx.symbol_str(name_sym) == "it" {
        cx.emit_offense(name_range, MSG, None);
    }
}

/// Extract the symbol and source range covering just the identifier.
///
/// Returns `None` for unrecognised node kinds (should not happen given dispatch).
fn identifier_name_range(node: NodeId, cx: &Cx<'_>) -> Option<(murphy_plugin_api::Symbol, Range)> {
    match *cx.kind(node) {
        NodeKind::Lvasgn { name, .. } => {
            // Prism does not set loc.name for lvasgn; compute it from expression start.
            let start = cx.range(node).start;
            let end = start + cx.symbol_str(name).len() as u32;
            Some((name, Range { start, end }))
        }
        NodeKind::Arg(s)
        | NodeKind::Restarg(s)
        | NodeKind::Kwrestarg(s)
        | NodeKind::Blockarg(s) => {
            // loc.name covers the identifier (without prefix sigil * / ** / &).
            Some((s, cx.node(node).loc.name))
        }
        NodeKind::Kwarg(s) => {
            // Prism name_loc includes the trailing colon; strip it to cover just the identifier.
            let start = cx.node(node).loc.name.start;
            let end = start + cx.symbol_str(s).len() as u32;
            Some((s, Range { start, end }))
        }
        NodeKind::Optarg { name, .. } => {
            // loc.name covers the identifier only (no `= default`).
            Some((name, cx.node(node).loc.name))
        }
        NodeKind::Kwoptarg { name, .. } => {
            // Prism name_loc includes the trailing colon; strip it to cover just the identifier.
            let start = cx.node(node).loc.name.start;
            let end = start + cx.symbol_str(name).len() as u32;
            Some((name, Range { start, end }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::ItAssignment;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- lvasgn -----

    #[test]
    fn flags_lvasgn_it() {
        test::<ItAssignment>().expect_offense(indoc! {"
            it = 5
            ^^ `it` is the default block parameter; consider another name.
        "});
    }

    #[test]
    fn accepts_lvasgn_other_name() {
        test::<ItAssignment>().expect_no_offenses("var = 5\n");
    }

    // ----- arg (required positional) -----

    #[test]
    fn flags_arg_it() {
        test::<ItAssignment>().expect_offense(indoc! {"
            def foo(it)
                    ^^ `it` is the default block parameter; consider another name.
            end
        "});
    }

    #[test]
    fn accepts_arg_other_name() {
        test::<ItAssignment>().expect_no_offenses(indoc! {"
            def foo(arg)
            end
        "});
    }

    // ----- optarg (optional positional) -----

    #[test]
    fn flags_optarg_it() {
        test::<ItAssignment>().expect_offense(indoc! {"
            def foo(it = 5)
                    ^^ `it` is the default block parameter; consider another name.
            end
        "});
    }

    // ----- restarg (*) -----

    #[test]
    fn flags_restarg_it() {
        test::<ItAssignment>().expect_offense(indoc! {"
            def foo(*it)
                     ^^ `it` is the default block parameter; consider another name.
            end
        "});
    }

    // ----- kwarg (required keyword) -----

    #[test]
    fn flags_kwarg_it() {
        test::<ItAssignment>().expect_offense(indoc! {"
            def foo(it:)
                    ^^ `it` is the default block parameter; consider another name.
            end
        "});
    }

    // ----- kwoptarg (optional keyword) -----

    #[test]
    fn flags_kwoptarg_it() {
        test::<ItAssignment>().expect_offense(indoc! {"
            def foo(it: 5)
                    ^^ `it` is the default block parameter; consider another name.
            end
        "});
    }

    // ----- kwrestarg (**) -----

    #[test]
    fn flags_kwrestarg_it() {
        test::<ItAssignment>().expect_offense(indoc! {"
            def foo(**it)
                      ^^ `it` is the default block parameter; consider another name.
            end
        "});
    }

    // ----- blockarg (&) -----

    #[test]
    fn flags_blockarg_it() {
        test::<ItAssignment>().expect_offense(indoc! {"
            def foo(&it)
                     ^^ `it` is the default block parameter; consider another name.
            end
        "});
    }

    // ----- block param (pipe-style) -----

    #[test]
    fn flags_block_param_it() {
        test::<ItAssignment>().expect_offense(indoc! {"
            [1].each { |it| }
                        ^^ `it` is the default block parameter; consider another name.
        "});
    }
}
murphy_plugin_api::submit_cop!(ItAssignment);
