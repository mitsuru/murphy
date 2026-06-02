//! `Style/NumericPredicate` — prefer predicate methods or comparison operators for
//! numeric zero/positive/negative checks.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/NumericPredicate
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Both EnforcedStyle values (`predicate` and `comparison`) are implemented.
//!   Predicate style: flags `x == 0`, `x > 0`, `x < 0`, and inverted forms
//!   (`0 == x`, `0 < x`, `0 > x`), excluding gvar receivers.
//!   Comparison style: flags `x.zero?`, `x.positive?`, `x.negative?`, including
//!   the negated `!x.zero?` -> `(x != 0)`. For `!x.positive?` / `!x.negative?`
//!   the offense is flagged but no autocorrect is emitted, because `!x.positive?`
//!   is not equivalent to `x <= 0` for NaN values. RuboCop emits `(x <= 0)`
//!   unconditionally; Murphy conservatively omits the autocorrect for these cases.
//!   AllowedMethods (Vec<String>): flags the node and skips when the method name
//!   or any ancestor send/block method name is in AllowedMethods. Defaults to [].
//!   AllowedPatterns (regex): not supported -- derive only covers Vec<String>.
//!   This is a v1 gap; users can work around it with AllowedMethods.
//!   target_ruby_version guard for `>` and `<` (Ruby >= 2.3): not enforced.
//!   Murphy v1 has no per-file Ruby version tracking; benign gap.
//!   `!= 0` and `nonzero?` are deliberately excluded from RESTRICT_ON_SEND,
//!   matching RuboCop: `nonzero?` is truthy/falsey (not true/false), and
//!   `x != 0` is not in RuboCop's RESTRICT_ON_SEND either.
//!   @safety: this cop is marked unsafe in RuboCop (no sandbox); Murphy has no
//!   cop-level safety metadata knob in v1.
//!   Global variables are excluded from the predicate direction (gvar receiver).
//! ```
//!
//! ## Matched shapes
//!
//! Predicate style (`predicate`, default):
//! - `x == 0` → `x.zero?`
//! - `x > 0` → `x.positive?`
//! - `x < 0` → `x.negative?`
//! - `0 == x` → `x.zero?` (inverted)
//! - `0 < x` → `x.positive?` (inverted, operator flipped)
//! - `0 > x` → `x.negative?` (inverted, operator flipped)
//!
//! Comparison style (`comparison`):
//! - `x.zero?` → `x == 0`
//! - `x.positive?` → `x > 0`
//! - `x.negative?` → `x < 0`
//! - `!x.zero?` -> `(x != 0)` (negated comparison; autocorrect applied)
//! - `!x.positive?` / `!x.negative?` -> flagged, no autocorrect (NaN-safe gap)
//!
//! ## Autocorrect
//!
//! Whole-node replacement (`cx.emit_edit(node_range, replacement)`).
//! In predicate style, binary-operation receivers are wrapped in parens:
//! `a + b > 0` → `(a + b).positive?`.

use murphy_plugin_api::{CopOptionEnum, CopOptions, Cx, NodeId, NodeKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct NumericPredicate;

/// Enforced style.
#[derive(CopOptionEnum, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NumericPredicateStyle {
    #[default]
    #[option(value = "predicate")]
    Predicate,
    #[option(value = "comparison")]
    Comparison,
}

#[derive(CopOptions)]
pub struct NumericPredicateOptions {
    #[option(
        name = "EnforcedStyle",
        default = "predicate",
        description = "Whether to prefer predicate methods or comparison operators."
    )]
    pub enforced_style: NumericPredicateStyle,

    #[option(
        default = [],
        description = "Method names that are always allowed (not flagged)."
    )]
    pub allowed_methods: Vec<String>,
}

const MSG: &str = "Use `{prefer}` instead of `{current}`.";

fn fmt_msg(prefer: &str, current: &str) -> String {
    MSG.replace("{prefer}", prefer).replace("{current}", current)
}

/// Returns `true` when the node is a gvar (global variable).
fn is_gvar(id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(id), NodeKind::Gvar(_))
}

/// Returns `true` when `name` starts with a non-alphanumeric, non-underscore char
/// (i.e., it's an operator method like `+`, `-`, `==`, `>`, etc.).
fn is_operator_method(name: &str) -> bool {
    name.chars()
        .next()
        .map_or(false, |c| !c.is_alphanumeric() && c != '_')
}

/// Returns `true` when the node should be wrapped in parentheses when used as
/// a method receiver. This mirrors RuboCop's `require_parentheses?`:
/// `node.send_type? && node.binary_operation? && !node.parenthesized?`.
fn requires_parens(id: NodeId, cx: &Cx<'_>) -> bool {
    match cx.kind(id) {
        NodeKind::Send { .. } | NodeKind::Csend { .. } => {
            if let Some(name) = cx.method_name(id) {
                // Binary operation: operator method, and the call is not
                // already parenthesized (e.g., `(a + b)` has unknown node).
                return is_operator_method(name) && !cx.is_parenthesized(id);
            }
            false
        }
        _ => false,
    }
}

/// Parenthesize the source if needed.
fn parenthesized_source<'a>(id: NodeId, cx: &'a Cx<'_>) -> String {
    let src = cx.raw_source(cx.range(id));
    if requires_parens(id, cx) {
        format!("({src})")
    } else {
        src.to_string()
    }
}

/// Returns `true` when the given node is a negated call (parent is `!`).
fn is_negated(id: NodeId, cx: &Cx<'_>) -> bool {
    if let Some(parent) = cx.parent(id).get() {
        if let NodeKind::Send { method, args, receiver, .. } = cx.kind(parent) {
            let method_name = cx.symbol_str(*method);
            if method_name == "!" {
                if let Some(recv_id) = receiver.get() {
                    return recv_id == id && cx.list(*args).is_empty();
                }
            }
        }
    }
    false
}

/// Check if an allowed method name applies to this node (node itself or any ancestor).
fn is_allowed(id: NodeId, cx: &Cx<'_>, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return false;
    }
    // Check the node's own method name.
    if let Some(name) = cx.method_name(id) {
        if allowed.iter().any(|m| m == name) {
            return true;
        }
    }
    // Walk ancestors: any send or block ancestor with an allowed method name.
    for ancestor in cx.ancestors(id) {
        match cx.kind(ancestor) {
            NodeKind::Send { .. }
            | NodeKind::Csend { .. }
            | NodeKind::Block { .. }
            | NodeKind::Numblock { .. }
            | NodeKind::Itblock { .. } => {
                if let Some(name) = cx.method_name(ancestor) {
                    if allowed.iter().any(|m| m == name) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Predicate style handlers
// ---------------------------------------------------------------------------

/// `x == 0` → `x.zero?`
/// `x > 0`  → `x.positive?`
/// `x < 0`  → `x.negative?`
///
/// Returns `(receiver, predicate_name)` or `None`.
fn match_direct_comparison(node: NodeId, cx: &Cx<'_>) -> Option<(NodeId, &'static str)> {
    let NodeKind::Send { receiver, method, args } = cx.kind(node) else {
        return None;
    };
    let recv_id = receiver.get()?;
    // Exclude gvar receivers.
    if is_gvar(recv_id, cx) {
        return None;
    }
    let arg_list = cx.list(*args);
    if arg_list.len() != 1 {
        return None;
    }
    // Argument must be `(int 0)`.
    if !matches!(cx.kind(arg_list[0]), NodeKind::Int(0)) {
        return None;
    }
    let pred = match cx.symbol_str(*method) {
        "==" => "zero?",
        ">" => "positive?",
        "<" => "negative?",
        _ => return None,
    };
    Some((recv_id, pred))
}

/// `0 == x` → `x.zero?`
/// `0 < x`  → `x.positive?` (operator flipped: `<` becomes `>`)
/// `0 > x`  → `x.negative?` (operator flipped: `>` becomes `<`)
///
/// Returns `(receiver, predicate_name)` or `None`.
fn match_inverted_comparison(node: NodeId, cx: &Cx<'_>) -> Option<(NodeId, &'static str)> {
    let NodeKind::Send { receiver, method, args } = cx.kind(node) else {
        return None;
    };
    // Receiver must be `(int 0)`.
    let recv_id = receiver.get()?;
    if !matches!(cx.kind(recv_id), NodeKind::Int(0)) {
        return None;
    }
    let arg_list = cx.list(*args);
    if arg_list.len() != 1 {
        return None;
    }
    let numeric = arg_list[0];
    // Argument must not be a gvar.
    if is_gvar(numeric, cx) {
        return None;
    }
    // Flip the operator: `0 < x` means `x > 0` → `x.positive?`
    let pred = match cx.symbol_str(*method) {
        "==" => "zero?",
        "<" => "positive?",  // 0 < x  ⟺  x > 0
        ">" => "negative?",  // 0 > x  ⟺  x < 0
        _ => return None,
    };
    Some((numeric, pred))
}

// ---------------------------------------------------------------------------
// Comparison style handlers
// ---------------------------------------------------------------------------

/// `x.zero?` → `x == 0`
/// `x.positive?` → `x > 0`
/// `x.negative?` → `x < 0`
///
/// Returns `(receiver, comparison_str)` or `None`.
fn match_predicate(node: NodeId, cx: &Cx<'_>) -> Option<(NodeId, &'static str)> {
    let NodeKind::Send { receiver, method, args } = cx.kind(node) else {
        return None;
    };
    let recv_id = receiver.get()?;
    if !cx.list(*args).is_empty() {
        return None;
    }
    let op = match cx.symbol_str(*method) {
        "zero?" => "==",
        "positive?" => ">",
        "negative?" => "<",
        _ => return None,
    };
    Some((recv_id, op))
}

// ---------------------------------------------------------------------------
// Cop implementation
// ---------------------------------------------------------------------------

#[cop(
    name = "Style/NumericPredicate",
    description = "Prefer predicate methods or comparison operators for numeric checks.",
    default_severity = "warning",
    default_enabled = true,
    options = NumericPredicateOptions,
)]
impl NumericPredicate {
    /// Handles `==`, `>`, `<` (predicate style) and `zero?`, `positive?`, `negative?`
    /// (comparison style).
    #[on_node(kind = "send", methods = ["==", ">", "<", "zero?", "positive?", "negative?"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<NumericPredicateOptions>();
        let allowed = &opts.allowed_methods;

        match opts.enforced_style {
            NumericPredicateStyle::Predicate => {
                // Try direct comparison: `x OP 0`.
                if let Some((numeric, pred)) = match_direct_comparison(node, cx)
                    .or_else(|| match_inverted_comparison(node, cx))
                {
                    if is_allowed(node, cx, allowed) {
                        return;
                    }
                    let recv_src = parenthesized_source(numeric, cx);
                    let replacement = format!("{recv_src}.{pred}");
                    let node_src = cx.raw_source(cx.range(node));
                    let node_range = cx.range(node);
                    cx.emit_offense(node_range, &fmt_msg(&replacement, node_src), None);
                    cx.emit_edit(node_range, &replacement);
                }
            }
            NumericPredicateStyle::Comparison => {
                // Try predicate: `x.zero?` / `x.positive?` / `x.negative?`.
                if let Some((recv_id, op)) = match_predicate(node, cx) {
                    if is_allowed(node, cx, allowed) {
                        return;
                    }
                    let recv_src = cx.raw_source(cx.range(recv_id));
                    let node_range = cx.range(node);
                    let node_src = cx.raw_source(node_range);

                    // Check if negated: `!x.zero?` -> `(x != 0)`.
                    // Negated autocorrect is safe only for `zero?` (== -> !=).
                    // For `positive?`/`negative?`, negation interacts with NaN
                    // semantics: `!x.positive?` is not equivalent to `x <= 0`
                    // for NaN values. We flag the offense but do not autocorrect
                    // `!x.positive?` / `!x.negative?` forms.
                    if is_negated(node, cx) {
                        if op == "==" {
                            // `!x.zero?` -> `(x != 0)` - safe for all numeric types.
                            let parent_id = cx.parent(node).get().unwrap();
                            let parent_range = cx.range(parent_id);
                            let parent_src = cx.raw_source(parent_range);
                            let repl = format!("({recv_src} != 0)");
                            cx.emit_offense(parent_range, &fmt_msg(&repl, parent_src), None);
                            cx.emit_edit(parent_range, &repl);
                        } else {
                            // `!x.positive?` / `!x.negative?`: flag offense on the inner
                            // predicate node only (no autocorrect: `!x.positive?` != `x <= 0`
                            // for NaN values).
                            let not_op = match op { ">" => "<=", "<" => ">=", _ => return };
                            let msg_prefer = format!("{recv_src} {not_op} 0");
                            cx.emit_offense(node_range, &fmt_msg(&msg_prefer, node_src), None);
                            // No emit_edit: autocorrect omitted for NaN safety.
                        }
                        return;
                    }

                    let replacement = format!("{recv_src} {op} 0");
                    cx.emit_offense(node_range, &fmt_msg(&replacement, node_src), None);
                    cx.emit_edit(node_range, &replacement);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NumericPredicate, NumericPredicateOptions, NumericPredicateStyle};
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- EnforcedStyle: predicate (default) -----

    #[test]
    fn flags_eq_zero() {
        test::<NumericPredicate>().expect_correction(
            indoc! {"
                foo == 0
                ^^^^^^^^ Use `foo.zero?` instead of `foo == 0`.
            "},
            "foo.zero?\n",
        );
    }

    #[test]
    fn flags_gt_zero() {
        test::<NumericPredicate>().expect_correction(
            indoc! {"
                bar.baz > 0
                ^^^^^^^^^^^ Use `bar.baz.positive?` instead of `bar.baz > 0`.
            "},
            "bar.baz.positive?\n",
        );
    }

    #[test]
    fn flags_lt_zero() {
        test::<NumericPredicate>().expect_correction(
            indoc! {"
                foo < 0
                ^^^^^^^ Use `foo.negative?` instead of `foo < 0`.
            "},
            "foo.negative?\n",
        );
    }

    #[test]
    fn flags_inverted_eq_zero() {
        // `0 == foo` → `foo.zero?`
        test::<NumericPredicate>().expect_correction(
            indoc! {"
                0 == foo
                ^^^^^^^^ Use `foo.zero?` instead of `0 == foo`.
            "},
            "foo.zero?\n",
        );
    }

    #[test]
    fn flags_inverted_lt_zero_becomes_positive() {
        // `0 < foo` means `foo > 0` → `foo.positive?`
        test::<NumericPredicate>().expect_correction(
            indoc! {"
                0 < foo
                ^^^^^^^ Use `foo.positive?` instead of `0 < foo`.
            "},
            "foo.positive?\n",
        );
    }

    #[test]
    fn flags_inverted_gt_zero_becomes_negative() {
        // `0 > foo` means `foo < 0` → `foo.negative?`
        test::<NumericPredicate>().expect_correction(
            indoc! {"
                0 > foo
                ^^^^^^^ Use `foo.negative?` instead of `0 > foo`.
            "},
            "foo.negative?\n",
        );
    }

    #[test]
    fn wraps_binary_receiver_in_parens() {
        test::<NumericPredicate>().expect_correction(
            indoc! {"
                a + b > 0
                ^^^^^^^^^ Use `(a + b).positive?` instead of `a + b > 0`.
            "},
            "(a + b).positive?\n",
        );
    }

    #[test]
    fn does_not_flag_gvar_receiver() {
        test::<NumericPredicate>().expect_no_offenses("$foo == 0\n");
    }

    #[test]
    fn does_not_flag_gvar_inverted() {
        // `0 == $foo` — gvar on the argument side (inverted form)
        test::<NumericPredicate>().expect_no_offenses("0 == $foo\n");
    }

    #[test]
    fn does_not_flag_non_zero() {
        test::<NumericPredicate>().expect_no_offenses("foo == 1\n");
    }

    #[test]
    fn does_not_flag_without_receiver() {
        // Bare `== 0` with no receiver doesn't match
        test::<NumericPredicate>().expect_no_offenses("foo.zero?\n");
    }

    // ----- EnforcedStyle: comparison -----

    #[test]
    fn flags_zero_predicate_comparison_style() {
        test::<NumericPredicate>()
            .with_options(&NumericPredicateOptions {
                enforced_style: NumericPredicateStyle::Comparison,
                allowed_methods: vec![],
            })
            .expect_correction(
                indoc! {"
                    foo.zero?
                    ^^^^^^^^^ Use `foo == 0` instead of `foo.zero?`.
                "},
                "foo == 0\n",
            );
    }

    #[test]
    fn flags_positive_predicate_comparison_style() {
        test::<NumericPredicate>()
            .with_options(&NumericPredicateOptions {
                enforced_style: NumericPredicateStyle::Comparison,
                allowed_methods: vec![],
            })
            .expect_correction(
                indoc! {"
                    bar.baz.positive?
                    ^^^^^^^^^^^^^^^^^ Use `bar.baz > 0` instead of `bar.baz.positive?`.
                "},
                "bar.baz > 0\n",
            );
    }

    #[test]
    fn flags_negative_predicate_comparison_style() {
        test::<NumericPredicate>()
            .with_options(&NumericPredicateOptions {
                enforced_style: NumericPredicateStyle::Comparison,
                allowed_methods: vec![],
            })
            .expect_correction(
                indoc! {"
                    foo.negative?
                    ^^^^^^^^^^^^^ Use `foo < 0` instead of `foo.negative?`.
                "},
                "foo < 0\n",
            );
    }

    #[test]
    fn flags_negated_zero_predicate_comparison_style() {
        // `!foo.zero?` → `(foo != 0)`
        test::<NumericPredicate>()
            .with_options(&NumericPredicateOptions {
                enforced_style: NumericPredicateStyle::Comparison,
                allowed_methods: vec![],
            })
            .expect_correction(
                indoc! {"
                    !foo.zero?
                    ^^^^^^^^^^ Use `(foo != 0)` instead of `!foo.zero?`.
                "},
                "(foo != 0)\n",
            );
    }

    #[test]
    fn flags_negated_positive_predicate_no_autocorrect() {
        // `!foo.positive?` is flagged but no autocorrect (NaN safety).
        test::<NumericPredicate>()
            .with_options(&NumericPredicateOptions {
                enforced_style: NumericPredicateStyle::Comparison,
                allowed_methods: vec![],
            })
            .expect_no_corrections("!foo.positive?\n");
    }

    #[test]
    fn flags_negated_positive_offense_only() {
        test::<NumericPredicate>()
            .with_options(&NumericPredicateOptions {
                enforced_style: NumericPredicateStyle::Comparison,
                allowed_methods: vec![],
            })
            .expect_offense(indoc! {"
                !foo.positive?
                 ^^^^^^^^^^^^^ Use `foo <= 0` instead of `foo.positive?`.
            "});
    }

    #[test]
    fn accepts_predicate_in_predicate_style() {
        test::<NumericPredicate>().expect_no_offenses("foo.zero?\n");
    }

    #[test]
    fn accepts_comparison_in_comparison_style() {
        test::<NumericPredicate>()
            .with_options(&NumericPredicateOptions {
                enforced_style: NumericPredicateStyle::Comparison,
                allowed_methods: vec![],
            })
            .expect_no_offenses("foo == 0\n");
    }

    // ----- AllowedMethods -----

    #[test]
    fn allowed_methods_skips_flagged_node() {
        test::<NumericPredicate>()
            .with_options(&NumericPredicateOptions {
                enforced_style: NumericPredicateStyle::Predicate,
                allowed_methods: vec!["==".to_string()],
            })
            .expect_no_offenses("foo == 0\n");
    }
}

murphy_plugin_api::submit_cop!(NumericPredicate);
