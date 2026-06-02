//! `Style/DoubleNegation` — flags double negation (`!!`).
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/DoubleNegation
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `!!expr` (two consecutive prefix-bang negations) as `(send (send _ :!) :!)`.
//!   Offense is placed on the outer `!` (RuboCop's `node.loc.selector`).
//!   Unsafe autocorrect: removes the outer `!` and appends `.nil?` — not safe when
//!   `expr` is `false` (`!!false` is `false` but `!false.nil?` is `true`).
//!
//!   `EnforcedStyle: allowed_in_returns` (default) — allows `!!` when used as:
//!     - the direct value of a `return`/`break`/`next` expression, or
//!     - the last expression in a `def`/`defs` body, or in a
//!       `define_method`/`define_singleton_method` block body.
//!   `EnforcedStyle: forbidden` — always flags.
//!
//!   Parity gap: RuboCop's `end_of_method_definition?` also walks conditional
//!   branches and rescue/ensure scoping. This implementation handles the
//!   common non-conditional case (direct last-child of def body). Conditional
//!   branches (`if/case` at end of method) are not tracked as allowed-in-returns;
//!   that is a minor gap vs RuboCop.
//! ```

use murphy_plugin_api::{
    CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, OptNodeId, Range, cop,
};

const MSG: &str = "Avoid the use of double negation (`!!`).";

#[derive(Default)]
pub struct DoubleNegation;

#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnforcedStyle {
    #[default]
    #[option(value = "allowed_in_returns")]
    AllowedInReturns,
    #[option(value = "forbidden")]
    Forbidden,
}

#[derive(CopOptions)]
pub struct DoubleNegationOptions {
    #[option(
        name = "EnforcedStyle",
        default = "allowed_in_returns",
        description = "When `allowed_in_returns`, allows `!!` in method return positions. When `forbidden`, always flags."
    )]
    pub enforced_style: EnforcedStyle,
}

#[cop(
    name = "Style/DoubleNegation",
    description = "Checks for uses of double negation (!!).",
    default_severity = "warning",
    default_enabled = true,
    options = DoubleNegationOptions,
)]
impl DoubleNegation {
    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Must be `(send (send _ :!) :!)` — outer `!` applied to inner `!`.
        let NodeKind::Send { receiver: outer_recv, method: outer_method, .. } = *cx.kind(node)
        else {
            return;
        };

        let outer_recv_id = match outer_recv.get() {
            Some(id) => id,
            None => return,
        };

        // Both sends must be prefix bang (not the `not` keyword).
        if !cx.is_prefix_bang(node) {
            return;
        }

        // Inner must also be a send with method `!`.
        let NodeKind::Send { method: inner_method, .. } = *cx.kind(outer_recv_id) else {
            return;
        };

        if cx.symbol_str(outer_method) != "!" || cx.symbol_str(inner_method) != "!" {
            return;
        }

        if !cx.is_prefix_bang(outer_recv_id) {
            return;
        }

        // Check style config.
        let opts = cx.options_or_default::<DoubleNegationOptions>();
        if opts.enforced_style == EnforcedStyle::AllowedInReturns
            && allowed_in_returns(node, cx)
        {
            return;
        }

        // Offense is on the outer `!` (loc.selector = loc.name for Send).
        let offense_range = cx.selector(node);
        cx.emit_offense(offense_range, MSG, None);

        // Unsafe autocorrect: remove outer `!`, append `.nil?` after whole node.
        let remove_range = Range {
            start: cx.range(node).start,
            end: cx.range(outer_recv_id).start,
        };
        cx.emit_edit(remove_range, "");
        cx.emit_edit(
            Range {
                start: cx.range(node).end,
                end: cx.range(node).end,
            },
            ".nil?",
        );
    }
}

/// Returns `true` if `node` is in an "allowed in returns" position:
/// - direct child of `return`/`break`/`next`
/// - last expression in a `def`/`defs` body (non-nil, non-conditional)
/// - last expression in a `define_method`/`define_singleton_method` block body
fn allowed_in_returns(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };

    // Parent is `return`, `break`, or `next`.
    if matches!(
        cx.kind(parent),
        NodeKind::Return(_) | NodeKind::Break(_) | NodeKind::Next(_)
    ) {
        return true;
    }

    // Last expression in a def/defs body.
    if is_last_in_def_body(node, parent, cx) {
        return true;
    }

    // Last expression in a define_method/define_singleton_method block body.
    if is_last_in_define_method_block(node, parent, cx) {
        return true;
    }

    false
}

/// Check if `node` is the last expression in a `def`/`defs` body.
fn is_last_in_def_body(node: NodeId, parent: NodeId, cx: &Cx<'_>) -> bool {
    // Direct parent is def/defs and node is its body.
    if let NodeKind::Def { body, .. } | NodeKind::Defs { body, .. } = cx.kind(parent)
        && body.get() == Some(node)
    {
        return true;
    }

    // Parent is a `begin` block that is the def body.
    if let NodeKind::Begin(list) = cx.kind(parent) {
        let items = cx.list(*list);
        if items.last() == Some(&node)
            && let Some(grandparent) = cx.parent(parent).get()
            && let NodeKind::Def { body, .. } | NodeKind::Defs { body, .. } =
                cx.kind(grandparent)
            && body.get() == Some(parent)
        {
            return true;
        }
    }

    false
}

/// Check if `node` is the last expression in a `define_method` or
/// `define_singleton_method` block body.
fn is_last_in_define_method_block(node: NodeId, parent: NodeId, cx: &Cx<'_>) -> bool {
    // Direct parent is a Block and node is its body.
    let block_id = if let NodeKind::Block { body, call, .. } = cx.kind(parent) {
        if body.get() == Some(node) && is_define_method_call(call, cx) {
            return true;
        }
        parent
    } else if let NodeKind::Begin(list) = cx.kind(parent) {
        let items = cx.list(*list);
        if items.last() != Some(&node) {
            return false;
        }
        // Check if the parent Begin is the body of a define_method block.
        let Some(gp) = cx.parent(parent).get() else {
            return false;
        };
        gp
    } else {
        return false;
    };

    if let NodeKind::Block { body, call, .. } = cx.kind(block_id)
        && body.get() == Some(parent) && is_define_method_call(call, cx)
    {
        return true;
    }

    false
}

/// Returns `true` if `call` is `define_method` or `define_singleton_method`.
fn is_define_method_call(call: &NodeId, cx: &Cx<'_>) -> bool {
    if let NodeKind::Send { method, .. } = cx.kind(*call) {
        let name = cx.symbol_str(*method);
        return name == "define_method" || name == "define_singleton_method";
    }
    false
}

// Keep OptNodeId in scope so the `use` is load-bearing.
const _: () = {
    let _ = std::mem::size_of::<OptNodeId>();
};

#[cfg(test)]
mod tests {
    use super::{DoubleNegation, DoubleNegationOptions, EnforcedStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_double_negation() {
        test::<DoubleNegation>().expect_offense(indoc! {r#"
            !!something
            ^ Avoid the use of double negation (`!!`).
        "#});
    }

    #[test]
    fn corrects_double_negation() {
        test::<DoubleNegation>().expect_correction(
            indoc! {r#"
                !!something
                ^ Avoid the use of double negation (`!!`).
            "#},
            "!something.nil?\n",
        );
    }

    #[test]
    fn accepts_single_negation() {
        test::<DoubleNegation>().expect_no_offenses("!something\n");
    }

    #[test]
    fn accepts_double_negation_in_return_allowed_in_returns() {
        test::<DoubleNegation>().expect_no_offenses("def foo?\n  return !!x\nend\n");
    }

    #[test]
    fn accepts_double_negation_as_method_body_allowed_in_returns() {
        test::<DoubleNegation>().expect_no_offenses("def foo?\n  !!return_value\nend\n");
    }

    #[test]
    fn flags_double_negation_in_method_body_forbidden() {
        test::<DoubleNegation>()
            .with_options(&DoubleNegationOptions {
                enforced_style: EnforcedStyle::Forbidden,
            })
            .expect_offense(indoc! {r#"
                def foo?
                  !!return_value
                  ^ Avoid the use of double negation (`!!`).
                end
            "#});
    }

    #[test]
    fn flags_double_negation_in_plain_context() {
        test::<DoubleNegation>().expect_offense(indoc! {r#"
            x = !!foo
                ^ Avoid the use of double negation (`!!`).
        "#});
    }

    #[test]
    fn accepts_define_method_block_allowed_in_returns() {
        test::<DoubleNegation>().expect_no_offenses(indoc! {r#"
            define_method :foo? do
              !!return_value
            end
        "#});
    }

    #[test]
    fn flags_define_method_block_forbidden() {
        test::<DoubleNegation>()
            .with_options(&DoubleNegationOptions {
                enforced_style: EnforcedStyle::Forbidden,
            })
            .expect_offense(indoc! {r#"
                define_method :foo? do
                  !!return_value
                  ^ Avoid the use of double negation (`!!`).
                end
            "#});
    }

    #[test]
    fn default_style_is_allowed_in_returns() {
        let opts = DoubleNegationOptions::default();
        assert_eq!(opts.enforced_style, EnforcedStyle::AllowedInReturns);
    }

    #[test]
    fn config_json_forbidden() {
        use murphy_plugin_api::CopOptions;
        let opts =
            DoubleNegationOptions::from_config_json(br#"{"EnforcedStyle": "forbidden"}"#)
                .expect("valid");
        assert_eq!(opts.enforced_style, EnforcedStyle::Forbidden);
    }

    #[test]
    fn config_json_allowed_in_returns() {
        use murphy_plugin_api::CopOptions;
        let opts =
            DoubleNegationOptions::from_config_json(br#"{"EnforcedStyle": "allowed_in_returns"}"#)
                .expect("valid");
        assert_eq!(opts.enforced_style, EnforcedStyle::AllowedInReturns);
    }
}

murphy_plugin_api::submit_cop!(DoubleNegation);
