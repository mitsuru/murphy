//! `Style/InverseMethods` — use the inverse method instead of inverting a
//! method call with `!`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/InverseMethods
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Implemented:
//!     - Flags `!foo.method` when `method` has a known inverse, e.g. `!x.any?`
//!       -> `x.none?`, `!x.even?` -> `x.odd?`.
//!     - Flags `!foo.any? { ... }` (negated block call) and renames the
//!       selector to its inverse.
//!     - Autocorrect: removes the `!` prefix and renames the selector to the
//!       inverse method name (two surgical edits).
//!     - `SAFE_NAVIGATION_INCOMPATIBLE_METHODS` (`any?`, `none?`, `<`, `<=`,
//!       `>`, `>=`): never flag when safe-navigation (`&.`) is used.
//!     - Double-negation guard: skip when the outer `!` node's parent is also
//!       a `!` call (e.g. `!!x.any?` is not flagged).
//!   Deferred (gap):
//!     - `InverseBlocks` — `select { !f.even? }` -> `reject { f.even? }` and
//!       `reject { f != 7 }` -> `select { f == 7 }`. Requires traversal of
//!       block bodies.
//!     - User-configurable `InverseMethods` / `InverseBlocks` maps in
//!       `.murphy.yml` (requires hand-rolled `CopOptions` with nested map
//!       decoding). The default built-in map is hardcoded here.
//!     - Parenthesized begin nodes (e.g. `!(a == b)`, `!(Foo < Numeric)`)
//!       appear as `Unknown` in Murphy's arena AST; they are silently skipped.
//!       This means `!(a == b)` is not flagged (Murphy ABI gap).
//! ```
//!
//! ## Matched shapes
//!
//! `Send` nodes with method `!` where the receiver is a `Send`/`Csend` or
//! `Block` node whose call selector is a key in the inverse method table.
//!
//! ## Examples
//!
//! ```ruby
//! # bad
//! !foo.none?
//! !foo.any? { |f| f.even? }
//!
//! # good
//! foo.any?
//! foo.none? { |f| f.even? }
//! ```
//!
//! ## Autocorrect
//!
//! Two surgical edits:
//! 1. Delete the `!` negation prefix (from the outer node start to the inner
//!    node start).
//! 2. Rename the inner method selector to its inverse.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};



/// Methods that cannot be called with safe-navigation (`&.`) and still be
/// inverted safely. Matches RuboCop's `SAFE_NAVIGATION_INCOMPATIBLE_METHODS`.
const SAFE_NAVIGATION_INCOMPATIBLE: &[&str] = &["any?", "none?", "<", "<=", ">", ">="];

/// The built-in bidirectional inverse method table.
///
/// Encodes each pair once — the lookup function builds the reverse mapping
/// at runtime.  Matches RuboCop's default `InverseMethods` config (from
/// `default.yml`), bidirectionally merged:
///
/// ```yaml
/// InverseMethods:
///   :any?: :none?
///   :even?: :odd?
///   :==: :!=
///   :=~: :!~
///   :<: :>=
///   :>: :<=
/// ```
const INVERSE_PAIRS: &[(&str, &str)] = &[
    ("any?", "none?"),
    ("even?", "odd?"),
    ("==", "!="),
    ("=~", "!~"),
    ("<", ">="),
    (">", "<="),
];

/// Returns the inverse of `method` if it is in the built-in table, or `None`.
fn inverse_of(method: &str) -> Option<&'static str> {
    for &(a, b) in INVERSE_PAIRS {
        if a == method {
            return Some(b);
        }
        if b == method {
            return Some(a);
        }
    }
    None
}

/// Stateless unit struct.
#[derive(Default)]
pub struct InverseMethods;

#[cop(
    name = "Style/InverseMethods",
    description = "Use the inverse method instead of inverting a method call.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl InverseMethods {
    /// Check `!receiver.method` patterns.
    #[on_node(kind = "send", methods = ["!"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        // `node` is the outer `!` call.  The receiver is the inner call.
        let inner = match cx.call_receiver(node).get() {
            Some(id) => id,
            None => return,
        };

        // Skip double-negation: `!!foo.any?` — the parent of `node` is itself
        // a `!` call.
        if cx.parent(node).get().is_some_and(|p| cx.is_negation_method(p)) {
            return;
        }

        // Inner node must be a Send, Csend, or Block (method call on a receiver).
        let (inner_method, is_csend, selector_node) = match cx.kind(inner) {
            NodeKind::Send { .. } => {
                let m = cx.method_name(inner).unwrap_or("");
                (m, false, inner)
            }
            NodeKind::Csend { .. } => {
                let m = cx.method_name(inner).unwrap_or("");
                (m, true, inner)
            }
            // Block node: `!foo.any? { ... }` — the outer `!` wraps a Block
            // whose `call` field is the method call.
            NodeKind::Block { call, .. } => {
                let call_id = *call;
                let m = cx.method_name(call_id).unwrap_or("");
                let csend = cx.is_safe_navigation(call_id);
                (m, csend, call_id)
            }
            // Unknown (e.g. parenthesized begin `!(a == b)`) — skip silently.
            _ => return,
        };

        // Check if an inverse exists.
        let inverse = match inverse_of(inner_method) {
            Some(inv) => inv,
            None => return,
        };

        // Safe-navigation incompatible: don't flag `!x&.any?`.
        if is_csend && SAFE_NAVIGATION_INCOMPATIBLE.contains(&inner_method) {
            return;
        }

        // Emit the offense on the full outer `!` node range.
        let msg = format!("Use `{inverse}` instead of inverting `{inner_method}`.");
        cx.emit_offense(cx.range(node), &msg, None);

        // Autocorrect: two surgical edits.
        // Edit 1: delete the `!` negation (from outer start to inner start).
        let negation_prefix = Range {
            start: cx.range(node).start,
            end: cx.range(inner).start,
        };
        cx.emit_edit(negation_prefix, "");

        // Edit 2: rename the inner method selector to its inverse.
        cx.emit_edit(cx.loc(selector_node).name, inverse);
    }
}

#[cfg(test)]
mod tests {
    use super::InverseMethods;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- any? / none? ---

    #[test]
    fn flags_negated_none() {
        test::<InverseMethods>().expect_offense(indoc! {"
            !foo.none?
            ^^^^^^^^^^ Use `any?` instead of inverting `none?`.
        "});
    }

    #[test]
    fn flags_negated_any() {
        test::<InverseMethods>().expect_offense(indoc! {"
            !foo.any?
            ^^^^^^^^^ Use `none?` instead of inverting `any?`.
        "});
    }

    #[test]
    fn corrects_negated_none_to_any() {
        test::<InverseMethods>().expect_correction(
            "!foo.none?\n^^^^^^^^^^ Use `any?` instead of inverting `none?`.\n",
            "foo.any?\n",
        );
    }

    #[test]
    fn corrects_negated_any_to_none() {
        test::<InverseMethods>().expect_correction(
            "!foo.any?\n^^^^^^^^^ Use `none?` instead of inverting `any?`.\n",
            "foo.none?\n",
        );
    }

    // --- even? / odd? ---

    #[test]
    fn flags_negated_even() {
        test::<InverseMethods>().expect_offense(indoc! {"
            !x.even?
            ^^^^^^^^ Use `odd?` instead of inverting `even?`.
        "});
    }

    #[test]
    fn corrects_negated_even_to_odd() {
        test::<InverseMethods>().expect_correction(
            "!x.even?\n^^^^^^^^ Use `odd?` instead of inverting `even?`.\n",
            "x.odd?\n",
        );
    }

    // --- any? with block ---

    #[test]
    fn flags_negated_any_with_block() {
        test::<InverseMethods>().expect_offense(indoc! {"
            !foo.any? { |f| f.even? }
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Use `none?` instead of inverting `any?`.
        "});
    }

    #[test]
    fn corrects_negated_any_with_block() {
        test::<InverseMethods>().expect_correction(
            "!foo.any? { |f| f.even? }\n^^^^^^^^^^^^^^^^^^^^^^^^^ Use `none?` instead of inverting `any?`.\n",
            "foo.none? { |f| f.even? }\n",
        );
    }

    // --- double negation (excluded) ---

    #[test]
    fn accepts_double_negation() {
        // `!!foo.any?` — double-negation should not be flagged.
        test::<InverseMethods>().expect_no_offenses("!!foo.any?\n");
    }

    // --- safe navigation (excluded for incompatible methods) ---

    #[test]
    fn accepts_safe_navigation_any() {
        test::<InverseMethods>().expect_no_offenses("!foo&.any?\n");
    }

    #[test]
    fn accepts_safe_navigation_none() {
        test::<InverseMethods>().expect_no_offenses("!foo&.none?\n");
    }

    // --- methods not in the table ---

    #[test]
    fn accepts_method_without_inverse() {
        test::<InverseMethods>().expect_no_offenses("!foo.blank?\n");
    }

    #[test]
    fn accepts_simple_negation() {
        test::<InverseMethods>().expect_no_offenses("!foo\n");
    }

    // --- parenthesized begin (unknown in Murphy AST) ---

    #[test]
    fn accepts_parenthesized_begin_no_false_positive() {
        // `!(a < b)` parses with a parenthesized begin node that appears as
        // Unknown in Murphy's arena AST; the cop must not produce a false
        // positive or panic.
        test::<InverseMethods>().expect_no_offenses("!(a < b)\n");
    }

    // --- < method-call form (direct, not parenthesized) ---

    #[test]
    fn flags_negated_lt_method_form() {
        // `!a.<(b)` is the explicit operator-method-call form.
        test::<InverseMethods>().expect_offense(
            "!a.<(b)\n^^^^^^^ Use `>=` instead of inverting `<`.\n",
        );
    }

    #[test]
    fn accepts_safe_navigation_lt() {
        test::<InverseMethods>().expect_no_offenses("!foo&.<(bar)\n");
    }
}

murphy_plugin_api::submit_cop!(InverseMethods);
