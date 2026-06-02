//! `Style/NonNilCheck` — flags redundant non-nil checks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NonNilCheck
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covered:
//!     - `x != nil` → flags with `Prefer \`!(x).nil?\` over \`x != nil\`.` message,
//!       autocorrects to `!(x).nil?`. (Default `IncludeSemanticChanges: false` path.)
//!       The receiver is wrapped in parentheses unconditionally to preserve semantics
//!       for compound receivers (e.g. `a + b != nil` → `!(a + b).nil?`). This is
//!       safer than RuboCop's unparenthesized form and produces correct Ruby.
//!     - Predicate method exemption: the final expression of a predicate method body
//!       (`def foo?`) is NOT flagged (matches RuboCop's `on_def`/`ignore_node` logic).
//!   Gap (IncludeSemanticChanges: true paths — not implemented):
//!     - `!x.nil?` offense is not reported (requires `IncludeSemanticChanges: true`).
//!     - `unless x.nil?` offense is not reported (requires `IncludeSemanticChanges: true`).
//!     - Autocorrect to `x` (stripping the non-nil check entirely) is not implemented.
//!   Gap:
//!     - Cross-cop guard (`nil_comparison_style == 'comparison'`) is not implemented;
//!       Murphy's NilComparison has no `comparison` EnforcedStyle, so it never triggers.
//! ```
//!
//! ## Matched shapes (default `IncludeSemanticChanges: false`)
//!
//! `Send` nodes with method `!=` whose single argument is `nil` and whose receiver
//! is non-absent — i.e. `x != nil`.
//!
//! ## Predicate method exemption
//!
//! `x != nil` is NOT flagged when it is the final expression of a predicate
//! method body (`def foo?`) or singleton predicate (`def self.foo?`).
//!
//! ## Autocorrect
//!
//! `recv != nil` → `!(recv).nil?` (receiver wrapped in parens for safe precedence
//! with compound expressions like `a + b != nil` → `!(a + b).nil?`).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

const MSG_FOR_REPLACEMENT: &str = "Prefer `%<prefer>s` over `%<current>s`.";

/// Stateless unit struct.
#[derive(Default)]
pub struct NonNilCheck;

/// Options for `Style/NonNilCheck`.
#[derive(CopOptions, Debug)]
pub struct Options {
    #[option(
        name = "IncludeSemanticChanges",
        default = false,
        description = "When true, also flags `!x.nil?` and autocorrects to just `x`."
    )]
    pub include_semantic_changes: bool,
}

#[cop(
    name = "Style/NonNilCheck",
    description = "Checks for redundant nil checks.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
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

    // Predicate method exemption: do not flag when this node is the final
    // expression of a predicate method body (`def foo?` / `def self.foo?`).
    if is_last_in_predicate_method(node, cx) {
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

/// Returns `true` when `node` is the final expression of a predicate method body.
///
/// This mirrors RuboCop's `on_def` / `ignore_node` logic: any `!=` send that is
/// the last child of the body of a `def`/`defs` whose name ends in `?` is exempt.
fn is_last_in_predicate_method(node: NodeId, cx: &Cx<'_>) -> bool {
    // Walk up to find a direct `def`/`defs` ancestor.
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
                return is_last_expression(node, body_id, cx);
            }
            // Stop traversal at any other scope-creating node to avoid
            // accidentally matching outer predicate methods.
            NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Class { .. }
            | NodeKind::Module { .. } => return false,
            _ => {}
        }
    }
    false
}

/// Returns `true` when `target` is the last expression of the `body` node.
///
/// - If `body` is a `begin` (sequence), check that `target` is the last child.
/// - Otherwise, check that `target` is `body` itself.
fn is_last_expression(target: NodeId, body: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(body) {
        NodeKind::Begin(children) => {
            let list = cx.list(*children);
            list.last().copied() == Some(target)
        }
        _ => body == target,
    }
}

#[cfg(test)]
mod tests {
    use super::{NonNilCheck, Options};
    use murphy_plugin_api::test_support::{indoc, test};

    fn default_opts() -> Options {
        Options { include_semantic_changes: false }
    }

    // ----- offense cases -----

    #[test]
    fn flags_neq_nil() {
        test::<NonNilCheck>()
            .with_options(&default_opts())
            .expect_correction(
                indoc! {"
                    x != nil
                    ^^^^^^^^ Prefer `!(x).nil?` over `x != nil`.
                "},
                "!(x).nil?\n",
            );
    }

    #[test]
    fn flags_neq_nil_with_method_receiver() {
        test::<NonNilCheck>()
            .with_options(&default_opts())
            .expect_correction(
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
        test::<NonNilCheck>()
            .with_options(&default_opts())
            .expect_correction(
                indoc! {"
                    a + b != nil
                    ^^^^^^^^^^^^ Prefer `!(a + b).nil?` over `a + b != nil`.
                "},
                "!(a + b).nil?\n",
            );
    }

    #[test]
    fn flags_neq_nil_in_if_condition() {
        test::<NonNilCheck>()
            .with_options(&default_opts())
            .expect_correction(
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
        test::<NonNilCheck>()
            .with_options(&default_opts())
            .expect_no_offenses(indoc! {"
                def signed_in?
                  current_user != nil
                end
            "});
    }

    #[test]
    fn no_offense_in_predicate_method_last_expr() {
        // `x != nil` is the final expression of a multi-statement body — exempt.
        test::<NonNilCheck>()
            .with_options(&default_opts())
            .expect_no_offenses(indoc! {"
                def authenticated?
                  setup
                  current_user != nil
                end
            "});
    }

    #[test]
    fn flags_neq_nil_in_non_predicate_method() {
        // Non-predicate method — NOT exempt.
        test::<NonNilCheck>()
            .with_options(&default_opts())
            .expect_offense(indoc! {"
                def foo
                  x != nil
                  ^^^^^^^^ Prefer `!(x).nil?` over `x != nil`.
                end
            "});
    }

    #[test]
    fn flags_neq_nil_non_last_in_predicate_method() {
        // Not the last expression — NOT exempt.
        test::<NonNilCheck>()
            .with_options(&default_opts())
            .expect_offense(indoc! {"
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
        test::<NonNilCheck>()
            .with_options(&default_opts())
            .expect_no_offenses("x.nil?\n");
    }

    #[test]
    fn accepts_eq_nil() {
        // `== nil` is Style/NilComparison's territory.
        test::<NonNilCheck>()
            .with_options(&default_opts())
            .expect_no_offenses("x == nil\n");
    }

    #[test]
    fn accepts_neq_non_nil() {
        test::<NonNilCheck>()
            .with_options(&default_opts())
            .expect_no_offenses("x != 1\n");
    }

    // ----- options parsing -----

    #[test]
    fn options_parse_error_not_an_object() {
        use murphy_plugin_api::{ConfigErrorKind, CopOptions};
        let err = <Options as CopOptions>::from_config_json(b"[]")
            .expect_err("array root should be invalid");
        assert_eq!(err.kind(), &ConfigErrorKind::NotAnObject);
    }
}
murphy_plugin_api::submit_cop!(NonNilCheck);
