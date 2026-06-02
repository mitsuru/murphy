//! `Style/FetchEnvVar` — suggest `ENV.fetch` for the replacement of `ENV[]`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/FetchEnvVar
//! upstream_version_checked: 1.86.2
//! version_added: "1.28"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   `ENV['X']` silently returns nil on a missing key; `ENV.fetch` raises KeyError
//!   or returns an explicit default.
//!
//!   Options:
//!     - DefaultToNil (bool, default true): when true → `ENV.fetch('X', nil)`;
//!       when false → `ENV.fetch('X')`.
//!     - AllowedVars (Vec<String>): env-var names exempt from this rule.
//!
//!   Allowed (no offense):
//!     - Used as a boolean condition: `if ENV['X']`, `while ENV['X']`, `until ENV['X']`.
//!       Detected as: parent is an If/While/Until node AND this node is the cond child.
//!     - Used as a predicate/negation or receiver of a dot-chain: parent is a Send where
//!       ENV[] is the receiver (covers `!ENV['X']`, `ENV['X'].nil?`, comparisons, etc.).
//!     - Left-hand side of an `or` expression: `ENV['X'] || default`.
//!     - `||=` / `&&=` on ENV: these parse as `Unknown` in Murphy (ABI gap) and
//!       are therefore never dispatched to this cop; documented as a gap.
//!
//!   Scope: ENV must be nil-scoped only (::ENV is NOT matched, matching RuboCop).
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! ENV['X']
//! x = ENV['X']
//!
//! # good (allowed)
//! ENV.fetch('X', nil)
//! ENV.fetch('X')
//! if ENV['X']         # flag-use
//! !ENV['X']           # negation
//! ENV['X'].nil?       # method chain
//! ENV['X'] || 'def'   # or-lhs
//! ```
//!
//! ## Autocorrect
//!
//! Replaces `ENV['X']` with `ENV.fetch('X', nil)` (DefaultToNil: true)
//! or `ENV.fetch('X')` (DefaultToNil: false).

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct FetchEnvVar;

#[derive(CopOptions)]
pub struct FetchEnvVarOptions {
    #[option(
        name = "DefaultToNil",
        default = true,
        description = "When true, autocorrects to `ENV.fetch('X', nil)`. When false, to `ENV.fetch('X')`."
    )]
    pub default_to_nil: bool,

    #[option(
        name = "AllowedVars",
        default = [],
        description = "Environment variable names exempt from this rule."
    )]
    pub allowed_vars: Vec<String>,
}

#[cop(
    name = "Style/FetchEnvVar",
    description = "Suggest `ENV.fetch` for the replacement of `ENV[]`.",
    default_severity = "warning",
    default_enabled = false,
    options = FetchEnvVarOptions,
)]
impl FetchEnvVar {
    #[on_node(kind = "send", methods = ["[]"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // Receiver must be `ENV` (nil-scope only, not ::ENV).
        let Some(recv) = cx.call_receiver(node).get() else {
            return;
        };
        if !is_env_const(recv, cx) {
            return;
        }

        // Must have exactly one argument (the key).
        let args = cx.call_arguments(node);
        if args.len() != 1 {
            return;
        }
        let key_node = args[0];

        // Check AllowedVars.
        let opts = cx.options_or_default::<FetchEnvVarOptions>();
        if let NodeKind::Str(string_id) = *cx.kind(key_node) {
            let key_str = cx.string_str(string_id);
            if opts.allowed_vars.iter().any(|v| v == key_str) {
                return;
            }
        }

        // Check allowable uses.
        if is_allowable_use(node, cx) {
            return;
        }

        let key_src = cx.raw_source(cx.range(key_node));
        let (replacement, message) = if opts.default_to_nil {
            (
                format!("ENV.fetch({key_src}, nil)"),
                format!("Use `ENV.fetch({key_src}, nil)` instead of `ENV[{key_src}]`."),
            )
        } else {
            (
                format!("ENV.fetch({key_src})"),
                format!("Use `ENV.fetch({key_src})` instead of `ENV[{key_src}]`."),
            )
        };

        cx.emit_offense(cx.range(node), &message, None);
        cx.emit_edit(cx.range(node), &replacement);
    }
}

/// Returns true if the node is `ENV` (nil-scoped only).
fn is_env_const(node: NodeId, cx: &Cx<'_>) -> bool {
    let NodeKind::Const { name, scope } = *cx.kind(node) else {
        return false;
    };
    cx.symbol_str(name) == "ENV" && scope.get().is_none()
}

/// Returns true if this `ENV['X']` use is allowable (no offense).
fn is_allowable_use(node: NodeId, cx: &Cx<'_>) -> bool {
    let Some(parent) = cx.parent(node).get() else {
        return false;
    };

    match *cx.kind(parent) {
        // Used as the condition of if/unless — flag-use.
        NodeKind::If { cond, .. } if cond == node => true,
        // Used as the condition of while/until — flag-use.
        NodeKind::While { cond, .. } | NodeKind::Until { cond, .. } if cond == node => true,
        // Left-hand side of `or` expression: `ENV['X'] || default`.
        // Also allow if parent `or` is itself an lhs (nested `||` chains).
        NodeKind::Or { lhs, .. } if lhs == node => true,
        NodeKind::Or { .. } => is_or_lhs_chain(parent, node, cx),
        // Receiving a method with dot-syntax: `ENV['X'].nil?`, `!ENV['X']`, etc.
        NodeKind::Send { receiver, .. } if receiver.get() == Some(node) => true,
        // Safe-nav: `ENV['X']&.some_method` — Csend.receiver is NodeId (always present).
        NodeKind::Csend { receiver, .. } if receiver == node => true,
        _ => false,
    }
}

/// Returns true when `node` (`ENV['X']`) is nested in an `or` lhs chain at
/// some depth — e.g. `ENV['X'] || a || b` where the inner `or` holds `node`.
fn is_or_lhs_chain(or_node: NodeId, env_node: NodeId, cx: &Cx<'_>) -> bool {
    // Check whether `or_node` itself is the lhs of a grandparent `or`.
    let Some(grandparent) = cx.parent(or_node).get() else {
        return false;
    };
    matches!(*cx.kind(grandparent), NodeKind::Or { lhs, .. } if lhs == or_node)
        && matches!(*cx.kind(or_node), NodeKind::Or { lhs, .. } if lhs == env_node)
}

#[cfg(test)]
mod tests {
    use super::{FetchEnvVar, FetchEnvVarOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // --- offense + autocorrect (DefaultToNil: true, default) ---

    #[test]
    fn flags_env_bracket_access() {
        test::<FetchEnvVar>().expect_correction(
            indoc! {r#"
                ENV['X']
                ^^^^^^^^ Use `ENV.fetch('X', nil)` instead of `ENV['X']`.
            "#},
            "ENV.fetch('X', nil)\n",
        );
    }

    #[test]
    fn flags_env_bracket_in_assignment() {
        test::<FetchEnvVar>().expect_correction(
            indoc! {r#"
                x = ENV['X']
                    ^^^^^^^^ Use `ENV.fetch('X', nil)` instead of `ENV['X']`.
            "#},
            "x = ENV.fetch('X', nil)\n",
        );
    }

    // --- DefaultToNil: false ---

    #[test]
    fn flags_without_nil_default() {
        test::<FetchEnvVar>()
            .with_options(&FetchEnvVarOptions {
                default_to_nil: false,
                allowed_vars: vec![],
            })
            .expect_correction(
                indoc! {r#"
                    ENV['X']
                    ^^^^^^^^ Use `ENV.fetch('X')` instead of `ENV['X']`.
                "#},
                "ENV.fetch('X')\n",
            );
    }

    // --- AllowedVars ---

    #[test]
    fn accepts_allowed_var() {
        test::<FetchEnvVar>()
            .with_options(&FetchEnvVarOptions {
                default_to_nil: true,
                allowed_vars: vec!["ALLOWED".to_string()],
            })
            .expect_no_offenses("ENV['ALLOWED']\n");
    }

    #[test]
    fn flags_non_allowed_var() {
        test::<FetchEnvVar>()
            .with_options(&FetchEnvVarOptions {
                default_to_nil: true,
                allowed_vars: vec!["ALLOWED".to_string()],
            })
            .expect_offense(indoc! {r#"
                ENV['OTHER']
                ^^^^^^^^^^^^ Use `ENV.fetch('OTHER', nil)` instead of `ENV['OTHER']`.
            "#});
    }

    // --- allowed uses ---

    #[test]
    fn accepts_env_used_in_if_condition() {
        test::<FetchEnvVar>().expect_no_offenses("if ENV['X']\n  puts 1\nend\n");
    }

    #[test]
    fn accepts_env_negated() {
        test::<FetchEnvVar>().expect_no_offenses("!ENV['X']\n");
    }

    #[test]
    fn accepts_env_method_chain() {
        test::<FetchEnvVar>().expect_no_offenses("ENV['X'].nil?\n");
    }

    #[test]
    fn accepts_env_or_lhs() {
        test::<FetchEnvVar>().expect_no_offenses("y = ENV['X'] || 'default'\n");
    }

    #[test]
    fn accepts_env_fetch() {
        test::<FetchEnvVar>().expect_no_offenses("ENV.fetch('X', nil)\n");
    }
}
murphy_plugin_api::submit_cop!(FetchEnvVar);
