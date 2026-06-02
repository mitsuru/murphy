//! `Style/NonNilCheck` — flags redundant non-nil checks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NonNilCheck
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues:
//!   - murphy-y3h2
//! notes: >
//!   Covered:
//!     - `x != nil` → flags with `Prefer \`!(x).nil?\` over \`x != nil\`.` message,
//!       autocorrects to `!(x).nil?`. (Default `IncludeSemanticChanges: false` path.)
//!       The receiver is wrapped in parentheses unconditionally to preserve semantics
//!       for compound receivers (e.g. `a + b != nil` → `!(a + b).nil?`). This is
//!       safer than RuboCop's unparenthesized form and produces correct Ruby.
//!     - Predicate method exemption: any `x != nil` that is a descendant of the
//!       final expression of a predicate method body (`def foo?`) is NOT flagged.
//!       This covers not just direct final expressions but also final `if`/`case`
//!       branches within the last expression subtree.
//!   Gap (IncludeSemanticChanges: true paths — not implemented):
//!     - `!x.nil?` offense is not reported (requires `IncludeSemanticChanges: true`).
//!     - `unless x.nil?` offense is not reported (requires `IncludeSemanticChanges: true`).
//!     - Autocorrect to `x` (stripping the non-nil check entirely) is not implemented.
//!   Gap:
//!     - Cross-cop guard (`nil_comparison_style == 'comparison'`) is not implemented;
//!       Murphy's NilComparison has no `comparison` EnforcedStyle, so it never triggers.
//!   Note:
//!     - `IncludeSemanticChanges` option is not exposed; it would only be needed for
//!       the unimplemented `!x.nil?`/`unless` paths (documented gap above).
//! ```
//!
//! ## Matched shapes (default `IncludeSemanticChanges: false`)
//!
//! `Send` nodes with method `!=` whose single argument is `nil` and whose receiver
//! is non-absent — i.e. `x != nil`.
//!
//! ## Predicate method exemption
//!
//! `x != nil` is NOT flagged when it is a descendant of the final expression of
//! a predicate method body (`def foo?`) or singleton predicate (`def self.foo?`).
//! This covers `x != nil` directly as the body, as the last `begin` child, or
//! nested inside a final `if`/`case` expression within the last subtree.
//!
//! ## Autocorrect
//!
//! `recv != nil` → `!(recv).nil?` (receiver wrapped in parens for safe precedence
//! with compound expressions like `a + b != nil` → `!(a + b).nil?`).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, cop};

const MSG_FOR_REPLACEMENT: &str = "Prefer `%<prefer>s` over `%<current>s`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct NonNilCheck;

#[cop(
    name = "Style/NonNilCheck",
    description = "Checks for redundant nil checks.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl NonNilCheck {
    #[on_node(kind = "send", methods = ["!="])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check_neq_nil(node, cx);
    }
}

/// Check `x != nil` — the default (IncludeSemanticChanges: false) path.
fn check_neq_nil(node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Send {
        receiver,
        args,
        ..
    } = *cx.kind(node)
    else {
        return;
    };

    // Receiver must be present.
    let Some(recv_id) = receiver.get() else {
        return;
    };

    // Exactly one argument: `nil`.
    let arg_list = cx.list(args);
    if arg_list.len() != 1 {
        return;
    }
    if !matches!(cx.kind(arg_list[0]), NodeKind::Nil) {
        return;
    }

    // Predicate method exemption: do not flag when this node is anywhere within
    // the final expression subtree of a predicate method body.
    if is_in_predicate_method_final_expr(node, cx) {
        return;
    }

    let recv_src = cx.raw_source(cx.range(recv_id));
    let node_src = cx.raw_source(cx.range(node));
    // Wrap receiver in parentheses unconditionally for safe precedence with
    // compound receivers (e.g. `a + b != nil` → `!(a + b).nil?` rather than
    // `!a + b.nil?` which parses as `(!a) + (b.nil?)`).
    let prefer = format!("!({recv_src}).nil?");
    let msg = MSG_FOR_REPLACEMENT
        .replace("%<prefer>s", &prefer)
        .replace("%<current>s", node_src);

    let node_range = cx.range(node);
    cx.emit_offense(node_range, &msg, None);
    cx.emit_edit(node_range, &prefer);
}

/// Returns `true` when `node` is anywhere within the final expression subtree
/// of a predicate method body.
///
/// This mirrors RuboCop's `on_def`/`ignore_node` logic. RuboCop ignores the
/// entire last expression of the body (including all its descendants), so
/// `x != nil` inside a final `if`/`case` is also exempt.
fn is_in_predicate_method_final_expr(node: NodeId, cx: &Cx<'_>) -> bool {
    // Walk up to find a `def`/`defs` ancestor (stopping at scope boundaries).
    for ancestor in cx.ancestors(node) {
        match cx.kind(ancestor) {
            NodeKind::Def { name, body, .. } | NodeKind::Defs { name, body, .. } => {
                // Must be a predicate method (name ends in `?`).
                if !cx.symbol_str(*name).ends_with('?') {
                    return false;
                }
                let Some(body_id) = body.get() else {
                    return false;
                };
                return is_in_final_expression(node, body_id, cx);
            }
            // Stop traversal at any other scope-creating node.
            NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Class { .. }
            | NodeKind::Module { .. } => return false,
            _ => {}
        }
    }
    false
}

/// Returns `true` when `target` is anywhere within the final expression subtree
/// of `body`.
///
/// - If `body` is a `begin` (sequence): the final expression is its last child;
///   `target` is in-scope if it equals that last child or is a descendant of it.
/// - Otherwise `body` itself is the only expression; `target` is in-scope if it
///   equals `body` or is a descendant of `body`.
fn is_in_final_expression(target: NodeId, body: NodeId, cx: &Cx<'_>) -> bool {
    let final_expr = match cx.kind(body) {
        NodeKind::Begin(children) => {
            let list = cx.list(*children);
            match list.last().copied() {
                Some(last) => last,
                None => return false,
            }
        }
        _ => body,
    };
    // `target` must be the final expression itself or one of its descendants.
    target == final_expr || cx.ancestors(target).any(|a| a == final_expr)
}

#[cfg(test)]
mod tests {
    use super::NonNilCheck;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- offense cases -----

    #[test]
    fn flags_neq_nil() {
        test::<NonNilCheck>().expect_correction(
            indoc! {"
                x != nil
                ^^^^^^^^ Prefer `!(x).nil?` over `x != nil`.
            "},
            "!(x).nil?\n",
        );
    }

    #[test]
    fn flags_neq_nil_with_method_receiver() {
        test::<NonNilCheck>().expect_correction(
            indoc! {"
                foo.bar != nil
                ^^^^^^^^^^^^^^ Prefer `!(foo.bar).nil?` over `foo.bar != nil`.
            "},
            "!(foo.bar).nil?\n",
        );
    }

    #[test]
    fn flags_neq_nil_with_operator_receiver() {
        // Complex receiver (operator call) — parens preserve precedence.
        // `!a + b.nil?` → wrong; `!(a + b).nil?` → correct.
        test::<NonNilCheck>().expect_correction(
            indoc! {"
                a + b != nil
                ^^^^^^^^^^^^ Prefer `!(a + b).nil?` over `a + b != nil`.
            "},
            "!(a + b).nil?\n",
        );
    }

    #[test]
    fn flags_neq_nil_in_if_condition() {
        test::<NonNilCheck>().expect_correction(
            indoc! {"
                if x != nil
                   ^^^^^^^^ Prefer `!(x).nil?` over `x != nil`.
                  y
                end
            "},
            indoc! {"
                if !(x).nil?
                  y
                end
            "},
        );
    }

    // ----- predicate method exemption -----

    #[test]
    fn no_offense_in_predicate_method_single_expr() {
        // `x != nil` is the sole body — exempt.
        test::<NonNilCheck>().expect_no_offenses(indoc! {"
            def signed_in?
              current_user != nil
            end
        "});
    }

    #[test]
    fn no_offense_in_predicate_method_last_expr() {
        // `x != nil` is the final expression of a multi-statement body — exempt.
        test::<NonNilCheck>().expect_no_offenses(indoc! {"
            def authenticated?
              setup
              current_user != nil
            end
        "});
    }

    #[test]
    fn no_offense_in_predicate_method_final_if() {
        // `x != nil` is nested inside the final `if` expression — exempt.
        test::<NonNilCheck>().expect_no_offenses(indoc! {"
            def present?
              if cond
                x != nil
              else
                y != nil
              end
            end
        "});
    }

    #[test]
    fn flags_neq_nil_in_non_predicate_method() {
        // Non-predicate method — NOT exempt.
        test::<NonNilCheck>().expect_offense(indoc! {"
            def foo
              x != nil
              ^^^^^^^^ Prefer `!(x).nil?` over `x != nil`.
            end
        "});
    }

    #[test]
    fn flags_neq_nil_non_last_in_predicate_method() {
        // Not the last expression — NOT exempt.
        test::<NonNilCheck>().expect_offense(indoc! {"
            def foo?
              y = x != nil
                  ^^^^^^^^ Prefer `!(x).nil?` over `x != nil`.
              y
            end
        "});
    }

    // ----- negative cases -----

    #[test]
    fn accepts_nil_predicate() {
        test::<NonNilCheck>().expect_no_offenses("x.nil?\n");
    }

    #[test]
    fn accepts_eq_nil() {
        // `== nil` is Style/NilComparison's territory.
        test::<NonNilCheck>().expect_no_offenses("x == nil\n");
    }

    #[test]
    fn accepts_neq_non_nil() {
        test::<NonNilCheck>().expect_no_offenses("x != 1\n");
    }
}
murphy_plugin_api::submit_cop!(NonNilCheck);
