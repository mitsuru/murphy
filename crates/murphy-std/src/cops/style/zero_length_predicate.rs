//! `Style/ZeroLengthPredicate` — prefer `empty?` over `length == 0` comparisons.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ZeroLengthPredicate
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Covers all six zero-length comparison forms (==, <, > with 0/1) and the
//!   `.zero?` predicate form, plus the `!empty?` non-zero forms.
//!   `csend` forms (`x&.size.zero?`) are supported for the EMPTY direction only,
//!   matching RuboCop's `on_csend` omitting nonzero checks.
//!   The cop is marked `Safe: false` in the default config (unsafe autocorrect)
//!   because `empty?` may not be defined in terms of `length` on all receivers.
//!   Murphy emits autocorrect unconditionally (no unsafe-autocorrect flag in the
//!   ABI at time of authoring); the safety annotation is preserved in default.yml.
//!   Non-polymorphic exclusions for `File`, `Tempfile`, `StringIO` match RuboCop.
//! ```
//!
//! ## Matched shapes — EMPTY (`→ empty?`)
//!
//! - `x.size.zero?`   / `x.length.zero?`        (predicate form)
//! - `x.size == 0`    / `0 == x.size`
//! - `x.size < 1`     / `1 > x.size`
//!
//! ## Matched shapes — NOT-EMPTY (`→ !empty?`)
//!
//! - `x.size != 0`    / `0 != x.size`
//! - `x.size > 0`     / `0 < x.size`
//!
//! ## Non-polymorphic exclusions
//!
//! Skips when the receiver of `size`/`length` is any of:
//! - `File.stat(…).size`
//! - `{File,Tempfile,StringIO}.{new,open}(…).size`
//!
//! ## Autocorrect
//!
//! - Predicate form: replace `size.zero?` span with `empty?`.
//! - Comparison form: replace whole comparison node with `recv.empty?` or
//!   `!recv.empty?` using whole-node interpolation (structural rewrite).

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct ZeroLengthPredicate;

const ZERO_MSG: &str = "Use `empty?` instead of `%s`.";
const NONZERO_MSG: &str = "Use `!empty?` instead of `%s`.";

#[cop(
    name = "Style/ZeroLengthPredicate",
    description = "Use #empty? when testing for objects of length 0.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ZeroLengthPredicate {
    /// Single `send` handler covering both `zero?` predicate form and all
    /// comparison operators. The macro does not allow two `kind = "send"` entries,
    /// so we dispatch internally based on the method name.
    #[on_node(kind = "send", methods = ["zero?", "==", "!=", "<", ">"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        let Some(method) = cx.method_name(node) else {
            return;
        };
        match method {
            "zero?" => check_predicate_form(node, cx),
            "==" | "!=" | "<" | ">" => check_comparison_form(node, cx),
            _ => {}
        }
    }

    /// Fires for any csend node; handles `x&.size.zero?` patterns only.
    /// RuboCop's on_csend only calls check_zero_length_predicate (EMPTY direction),
    /// not comparison checks.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if cx.method_name(node) == Some("zero?") {
            check_predicate_form(node, cx);
        }
    }
}

// --------------------------------------------------------------------------
// Predicate form: `x.size.zero?` / `x.length.zero?`
// --------------------------------------------------------------------------

fn check_predicate_form(outer_node: NodeId, cx: &Cx<'_>) {
    // Outer node receiver must be a `size`/`length` send with its own receiver.
    let Some(inner_id) = cx.call_receiver(outer_node).get() else {
        return;
    };

    let Some(inner_method) = cx.method_name(inner_id) else {
        return;
    };
    if !is_length_method(inner_method) {
        return;
    }

    // The `size`/`length` call must have a receiver.
    let Some(length_recv_id) = cx.call_receiver(inner_id).get() else {
        return;
    };

    if is_non_polymorphic(length_recv_id, cx) {
        return;
    }

    // No args on the length call.
    if !cx.call_arguments(inner_id).is_empty() {
        return;
    }

    // Offense range: from start of inner method name (e.g. `size`) to end of
    // outer node (e.g. end of `zero?`). Mirrors RuboCop's
    // `node.loc.selector.join(node.parent.source_range.end)`.
    let inner_name_start = cx.node(inner_id).loc.name.start;
    let outer_end = cx.range(outer_node).end;
    let offense_range = Range {
        start: inner_name_start,
        end: outer_end,
    };
    let offense_src = cx.raw_source(offense_range);
    let message = ZERO_MSG.replace("%s", offense_src);
    cx.emit_offense(offense_range, &message, None);

    // Autocorrect: replace `size.zero?` span with `empty?`.
    cx.emit_edit(offense_range, "empty?");
}

// --------------------------------------------------------------------------
// Comparison forms
// --------------------------------------------------------------------------

fn check_comparison_form(node: NodeId, cx: &Cx<'_>) {
    let Some(lhs_id) = cx.call_receiver(node).get() else {
        return;
    };
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return;
    }
    let rhs_id = args[0];
    let Some(op) = cx.method_name(node) else {
        return;
    };

    // Try `lhs OP rhs` then `rhs (flipped OP) lhs`.
    let (length_call, direction) = if let Some(d) = check_lhs_op_rhs(lhs_id, op, rhs_id, cx) {
        (lhs_id, d)
    } else if let Some(d) = check_lhs_op_rhs(rhs_id, flip_op(op), lhs_id, cx) {
        (rhs_id, d)
    } else {
        return;
    };

    // The length call must have a non-absent receiver.
    let Some(length_recv) = cx.call_receiver(length_call).get() else {
        return;
    };

    if is_non_polymorphic(length_recv, cx) {
        return;
    }

    // No arguments on the length call.
    if !cx.call_arguments(length_call).is_empty() {
        return;
    }

    // Message: "<lhs> <op> <rhs>".
    let lhs_src = cx.raw_source(cx.range(lhs_id));
    let rhs_src = cx.raw_source(cx.range(rhs_id));
    let current = format!("{} {} {}", lhs_src, op, rhs_src);
    let message = if direction {
        ZERO_MSG.replace("%s", &current)
    } else {
        NONZERO_MSG.replace("%s", &current)
    };

    let outer_range = cx.range(node);
    cx.emit_offense(outer_range, &message, None);

    // Autocorrect: `recv<dot>empty?` or `!recv<dot>empty?`.
    let dot_src = cx.raw_source(cx.loc(length_call).dot());
    let recv_src = cx.raw_source(cx.range(length_recv));
    let replacement = if direction {
        format!("{}{}empty?", recv_src, dot_src)
    } else {
        format!("!{}{}empty?", recv_src, dot_src)
    };
    cx.emit_edit(outer_range, &replacement);
}

/// Check if `lhs OP rhs` is a matching zero/one-length pattern.
/// Returns `Some(true)` for empty direction, `Some(false)` for non-empty.
///
/// Only plain `send` nodes are matched: safe-navigation (`csend`) comparison
/// forms are semantically different when the receiver is `nil` — RuboCop's
/// `on_csend` only invokes the predicate check, not comparison checks.
fn check_lhs_op_rhs(lhs: NodeId, op: &str, rhs: NodeId, cx: &Cx<'_>) -> Option<bool> {
    // Exclude safe-navigation calls from comparison forms.
    if cx.is_safe_navigation(lhs) {
        return None;
    }
    let lhs_method = cx.method_name(lhs)?;
    if !is_length_method(lhs_method) {
        return None;
    }

    let NodeKind::Int(int_val) = *cx.kind(rhs) else {
        return None;
    };

    match (op, int_val) {
        ("==", 0) => Some(true),
        ("<", 1) => Some(true),
        ("!=", 0) => Some(false),
        (">", 0) => Some(false),
        _ => None,
    }
}

fn flip_op(op: &str) -> &str {
    match op {
        "<" => ">",
        ">" => "<",
        other => other,
    }
}

fn is_length_method(name: &str) -> bool {
    name == "size" || name == "length"
}

// --------------------------------------------------------------------------
// Non-polymorphic exclusion
// --------------------------------------------------------------------------

fn is_non_polymorphic(recv: NodeId, cx: &Cx<'_>) -> bool {
    let Some(const_id) = cx.call_receiver(recv).get() else {
        return false;
    };
    let Some(method_name) = cx.method_name(recv) else {
        return false;
    };

    if method_name == "stat" && cx.is_global_const(const_id, "File") {
        return true;
    }
    if matches!(method_name, "new" | "open")
        && (cx.is_global_const(const_id, "File")
            || cx.is_global_const(const_id, "Tempfile")
            || cx.is_global_const(const_id, "StringIO"))
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::ZeroLengthPredicate;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- Predicate form: size.zero? / length.zero? → empty? -----

    #[test]
    fn flags_size_zero_predicate() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                x.size.zero?
                  ^^^^^^^^^^ Use `empty?` instead of `size.zero?`.
            "},
            "x.empty?\n",
        );
    }

    #[test]
    fn flags_length_zero_predicate() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                x.length.zero?
                  ^^^^^^^^^^^^ Use `empty?` instead of `length.zero?`.
            "},
            "x.empty?\n",
        );
    }

    // ----- Comparison forms: → empty? -----

    #[test]
    fn flags_length_eq_zero() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {r#"
                [1, 2, 3].length == 0
                ^^^^^^^^^^^^^^^^^^^^^ Use `empty?` instead of `[1, 2, 3].length == 0`.
            "#},
            "[1, 2, 3].empty?\n",
        );
    }

    #[test]
    fn flags_zero_eq_length() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {r#"
                0 == "foobar".length
                ^^^^^^^^^^^^^^^^^^^^ Use `empty?` instead of `0 == "foobar".length`.
            "#},
            "\"foobar\".empty?\n",
        );
    }

    #[test]
    fn flags_size_eq_zero() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                hash.size == 0
                ^^^^^^^^^^^^^^ Use `empty?` instead of `hash.size == 0`.
            "},
            "hash.empty?\n",
        );
    }

    #[test]
    fn flags_length_lt_one() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                array.length < 1
                ^^^^^^^^^^^^^^^^ Use `empty?` instead of `array.length < 1`.
            "},
            "array.empty?\n",
        );
    }

    #[test]
    fn flags_one_gt_length() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                1 > array.length
                ^^^^^^^^^^^^^^^^ Use `empty?` instead of `1 > array.length`.
            "},
            "array.empty?\n",
        );
    }

    // ----- Comparison forms: → !empty? -----

    #[test]
    fn flags_length_neq_zero() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                {a: 1, b: 2}.length != 0
                ^^^^^^^^^^^^^^^^^^^^^^^^ Use `!empty?` instead of `{a: 1, b: 2}.length != 0`.
            "},
            "!{a: 1, b: 2}.empty?\n",
        );
    }

    #[test]
    fn flags_length_gt_zero() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                string.length > 0
                ^^^^^^^^^^^^^^^^^ Use `!empty?` instead of `string.length > 0`.
            "},
            "!string.empty?\n",
        );
    }

    #[test]
    fn flags_size_gt_zero() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                hash.size > 0
                ^^^^^^^^^^^^^ Use `!empty?` instead of `hash.size > 0`.
            "},
            "!hash.empty?\n",
        );
    }

    #[test]
    fn flags_zero_lt_size() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                0 < hash.size
                ^^^^^^^^^^^^^ Use `!empty?` instead of `0 < hash.size`.
            "},
            "!hash.empty?\n",
        );
    }

    #[test]
    fn flags_zero_neq_size() {
        test::<ZeroLengthPredicate>().expect_correction(
            indoc! {"
                0 != string.size
                ^^^^^^^^^^^^^^^^ Use `!empty?` instead of `0 != string.size`.
            "},
            "!string.empty?\n",
        );
    }

    // ----- No-offense cases -----

    #[test]
    fn accepts_empty_predicate() {
        test::<ZeroLengthPredicate>().expect_no_offenses("[1, 2, 3].empty?\n");
    }

    #[test]
    fn accepts_non_zero_comparison() {
        test::<ZeroLengthPredicate>().expect_no_offenses("x.size == 1\n");
    }

    #[test]
    fn accepts_non_size_method() {
        test::<ZeroLengthPredicate>().expect_no_offenses("x.count_words == 0\n");
    }

    // ----- Non-polymorphic exclusions -----

    #[test]
    fn accepts_file_stat_size() {
        test::<ZeroLengthPredicate>().expect_no_offenses("File.stat(f).size == 0\n");
    }

    #[test]
    fn accepts_file_new_size() {
        test::<ZeroLengthPredicate>().expect_no_offenses("File.new(f).size == 0\n");
    }

    #[test]
    fn accepts_stringio_new_length() {
        test::<ZeroLengthPredicate>().expect_no_offenses("StringIO.new(f).length == 0\n");
    }

    #[test]
    fn accepts_tempfile_open_size() {
        test::<ZeroLengthPredicate>().expect_no_offenses("Tempfile.open(f).size == 0\n");
    }

    #[test]
    fn accepts_file_open_size_zero_predicate() {
        test::<ZeroLengthPredicate>().expect_no_offenses("File.open(f).size.zero?\n");
    }

    // ----- Safe-navigation (csend) comparison forms are NOT flagged -----
    // x&.size == 0 is not equivalent to x&.empty? when x is nil:
    // x&.size evaluates to nil (safe-navigation short-circuits), and nil == 0
    // is false; while x&.empty? returns nil which is falsy but not false.
    // RuboCop's on_csend only handles the predicate form.

    #[test]
    fn accepts_csend_size_eq_zero() {
        test::<ZeroLengthPredicate>().expect_no_offenses("x&.size == 0\n");
    }

    #[test]
    fn accepts_csend_size_gt_zero() {
        test::<ZeroLengthPredicate>().expect_no_offenses("x&.size > 0\n");
    }
}
