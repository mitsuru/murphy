//! `Style/HashSlice` — prefer `Hash#slice` over `select`/`filter`/`reject` with key comparison.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashSlice
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `select`/`filter`/`reject` with blocks that can be replaced by
//!   `slice`. Requires Ruby 2.5+ (Hash#slice added then).
//!
//!   Covered comparison methods: `==`, `!=`, `eql?`, `include?`.
//!   Both send and csend are handled.
//!
//!   The block must have exactly two named block args (k, v). The first arg
//!   must be the key; the second is the value (never referenced in the body).
//!
//!   For `==`/`!=`/`eql?`: the offense key must be a symbol or string literal
//!   (RuboCop's `safe_to_register_offense?` check). For `include?`: any key
//!   expression is accepted (non-literals become `*expr` in the correction).
//!   When the receiver of `include?` is Unknown (e.g. a range literal), the
//!   offense is skipped to avoid false positives (range_include? guard).
//!   When the receiver of `include?` is the value lvar, the offense is also
//!   skipped (using_value_variable? guard).
//!
//!   ActiveSupport-only methods (`in?`, `exclude?`) are NOT covered; they
//!   require `AllCops.ActiveSupportExtensionsEnabled`, which the plugin API
//!   does not expose.
//!
//!   Gap: `in?` and `exclude?` (ActiveSupport) are not detected.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (select with ==)
//! hash.select { |k, v| k == :foo }
//! hash.select { |k, v| :foo == k }
//! hash.select { |k, v| k.eql?(:foo) }
//! hash.select { |k, v| [:a, :b].include?(k) }
//!
//! # bad (reject with != / negated include)
//! hash.reject { |k, v| k != :foo }
//! hash.reject { |k, v| ![:a, :b].include?(k) }
//!
//! # bad (filter is an alias of select)
//! hash.filter { |k, v| k == :foo }
//!
//! # good
//! hash.slice(:foo)
//! hash.slice(:a, :b)
//! hash.slice(*allowed)
//! ```
//!
//! ## Autocorrect
//!
//! Replaces `select { ... }` / `reject { ... }` / `filter { ... }` with
//! `slice(<key_source>)`. The offense range covers from the selector of
//! `select`/`reject`/`filter` through the closing brace/`end` of the block.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, Symbol, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct HashSlice;

const MSG: &str = "Use `%s` instead.";

#[cop(
    name = "Style/HashSlice",
    description = "Checks for `select`/`filter`/`reject` that can be replaced with `Hash#slice`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
    safe_autocorrect = false,
)]
impl HashSlice {
    #[on_node(kind = "send", methods = ["select", "filter", "reject"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if matches!(cx.method_name(node), Some("select" | "filter" | "reject")) {
            check(node, cx);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Main check: the `send` node is a `select`/`filter`/`reject` call.
fn check(send_node: NodeId, cx: &Cx<'_>) {
    // The block wrapping this send call.
    let block_node = match cx.block_node(send_node).get() {
        Some(b) => b,
        None => return,
    };

    let method_name = cx.method_name(send_node).unwrap_or("");

    // Get block args — must be exactly 2.
    let args_node = match cx.block_arguments(block_node).get() {
        Some(a) => a,
        None => return,
    };
    let NodeKind::Args(args_list) = *cx.kind(args_node) else {
        return;
    };
    let args = cx.list(args_list);
    if args.len() != 2 {
        return;
    }
    let key_arg = args[0];
    let value_arg = args[1];

    // Both block args must be plain `arg` nodes (not mlhs, etc.).
    let (NodeKind::Arg(key_sym), NodeKind::Arg(value_sym)) =
        (*cx.kind(key_arg), *cx.kind(value_arg))
    else {
        return;
    };

    // Block must have a body.
    let body = match cx.block_body(block_node).get() {
        Some(b) => b,
        None => return,
    };

    // Extract the comparison expression, handling possible negation.
    let (comparison, negated) = strip_negation(body, cx);

    // The comparison must be a send node.
    if !matches!(cx.kind(comparison), NodeKind::Send { .. }) {
        return;
    }
    let cmp_method = cx.method_name(comparison).unwrap_or("");
    let cmp_recv_opt = cx.call_receiver(comparison);
    let cmp_args = cx.call_arguments(comparison);

    // Extract key expression from the comparison.
    let key_expr = match extract_key_expr(
        cmp_recv_opt, cmp_method, cmp_args, key_sym, value_sym, cx,
    ) {
        Some(k) => k,
        None => return,
    };

    // Determine if this is semantically a slice (not an except).
    if !is_semantically_slice(method_name, cmp_method, negated) {
        return;
    }

    // For == / != / eql?: key must be a symbol or string literal.
    if matches!(cmp_method, "==" | "!=" | "eql?") && !is_sym_or_str_literal(key_expr, cx) {
        return;
    }

    // Build the replacement source.
    let key_source = except_key_source(key_expr, cx);
    let replacement = format!("slice({})", key_source);
    let msg = MSG.replacen("%s", &replacement, 1);

    // Offense range: from selector start to block closing `}` / `end`.
    let offense_range = Range {
        start: cx.selector(send_node).start,
        end: cx.range(block_node).end,
    };

    cx.emit_offense(offense_range, &msg, None);
    cx.emit_edit(offense_range, &replacement);
}

/// Strip a leading `!` (send with method `!`) and return `(inner, negated)`.
fn strip_negation(node: NodeId, cx: &Cx<'_>) -> (NodeId, bool) {
    if matches!(cx.kind(node), NodeKind::Send { .. }) {
        if cx.method_name(node) == Some("!") && cx.call_arguments(node).is_empty() {
            if let Some(recv) = cx.call_receiver(node).get() {
                return (recv, true);
            }
        }
    }
    (node, false)
}

/// Determines if this comparison is semantically `select`/`filter`/`reject` → `slice`.
///
/// RuboCop's `semantically_slice_method? = !semantically_except_method?`:
/// - `select`/`filter` with `==`/`eql?`/non-negated-include? → slice
/// - `reject` with `!=`/negated-include? → slice
fn is_semantically_slice(method_name: &str, cmp_method: &str, negated: bool) -> bool {
    match method_name {
        "select" | "filter" => match cmp_method {
            "==" | "eql?" => !negated,
            "!=" => false,
            "include?" => !negated,
            _ => false,
        },
        "reject" => match cmp_method {
            "==" | "eql?" => false,
            "!=" => !negated,
            "include?" => negated,
            _ => false,
        },
        _ => false,
    }
}

/// Extract the key expression from the comparison.
///
/// Returns the key `NodeId` (the thing being compared against the block's first arg),
/// or `None` if the pattern doesn't match.
fn extract_key_expr(
    cmp_recv_opt: OptNodeId,
    cmp_method: &str,
    cmp_args: &[NodeId],
    key_sym: Symbol,
    value_sym: Symbol,
    cx: &Cx<'_>,
) -> Option<NodeId> {
    match cmp_method {
        "==" | "!=" => {
            // Exactly one arg required.
            if cmp_args.len() != 1 {
                return None;
            }
            let cmp_recv = cmp_recv_opt.get()?;
            let rhs = cmp_args[0];
            // Shapes: `k == val` or `val == k`
            if is_lvar_matching(cmp_recv, key_sym, cx) {
                Some(rhs)
            } else if is_lvar_matching(rhs, key_sym, cx) {
                Some(cmp_recv)
            } else {
                None
            }
        }
        "eql?" => {
            // `k.eql?(val)` — receiver must be key lvar, one arg.
            if cmp_args.len() != 1 {
                return None;
            }
            let cmp_recv = cmp_recv_opt.get()?;
            if !is_lvar_matching(cmp_recv, key_sym, cx) {
                return None;
            }
            Some(cmp_args[0])
        }
        "include?" => {
            // `collection.include?(k)` — arg must be key lvar, one arg.
            if cmp_args.len() != 1 {
                return None;
            }
            if !is_lvar_matching(cmp_args[0], key_sym, cx) {
                return None;
            }
            let cmp_recv = cmp_recv_opt.get()?;
            // The receiver must not be a range or unknown (range_include? guard).
            // In Murphy, ranges in parentheses appear as Unknown — skip them.
            if matches!(cx.kind(cmp_recv), NodeKind::Unknown | NodeKind::RangeExpr { .. }) {
                return None;
            }
            // The receiver must not be the value lvar (using_value_variable? guard).
            if is_lvar_matching(cmp_recv, value_sym, cx) {
                return None;
            }
            Some(cmp_recv)
        }
        _ => None,
    }
}

/// Check if a node is an `lvar` with the given symbol name.
fn is_lvar_matching(node: NodeId, sym: Symbol, cx: &Cx<'_>) -> bool {
    matches!(cx.kind(node), NodeKind::Lvar(s) if *s == sym)
}

/// Check if a node is a symbol or string literal.
fn is_sym_or_str_literal(node: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        cx.kind(node),
        NodeKind::Sym(_) | NodeKind::Str(_) | NodeKind::Dsym(_) | NodeKind::Dstr(_)
    )
}

/// Produce the key source string for the replacement.
///
/// - `[:a, :b]` → `":a, :b"` (join elements)
/// - `CONST` / `var` → `"*CONST"` (splat for non-literals)
/// - `:foo` / `"foo"` → `":foo"` / `"\"foo\""`
fn except_key_source(key: NodeId, cx: &Cx<'_>) -> String {
    match *cx.kind(key) {
        NodeKind::Array(elems_list) => {
            let elems = cx.list(elems_list);
            if elems.is_empty() {
                return String::new();
            }
            elems
                .iter()
                .map(|&e| cx.raw_source(cx.range(e)).to_owned())
                .collect::<Vec<_>>()
                .join(", ")
        }
        // Literals are used directly.
        NodeKind::Sym(_) | NodeKind::Str(_) | NodeKind::Dsym(_) | NodeKind::Dstr(_) => {
            cx.raw_source(cx.range(key)).to_owned()
        }
        // Non-literals get a splat.
        _ => format!("*{}", cx.raw_source(cx.range(key))),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::HashSlice;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- select + == ---

    #[test]
    fn flags_select_key_eq_sym() {
        test::<HashSlice>().expect_offense(indoc! {r#"
            h.select { |k, v| k == :foo }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:foo)` instead.
        "#});
    }

    #[test]
    fn flags_select_sym_eq_key() {
        test::<HashSlice>().expect_offense(indoc! {r#"
            h.select { |k, v| :foo == k }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:foo)` instead.
        "#});
    }

    #[test]
    fn corrects_select_key_eq_sym() {
        test::<HashSlice>().expect_correction(
            indoc! {r#"
                h.select { |k, v| k == :foo }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:foo)` instead.
            "#},
            "h.slice(:foo)\n",
        );
    }

    // --- select + eql? ---

    #[test]
    fn flags_select_key_eql_sym() {
        test::<HashSlice>().expect_offense(indoc! {r#"
            h.select { |k, v| k.eql?(:foo) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:foo)` instead.
        "#});
    }

    #[test]
    fn corrects_select_key_eql_sym() {
        test::<HashSlice>().expect_correction(
            indoc! {r#"
                h.select { |k, v| k.eql?(:foo) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:foo)` instead.
            "#},
            "h.slice(:foo)\n",
        );
    }

    // --- reject + != ---

    #[test]
    fn flags_reject_key_neq_sym() {
        test::<HashSlice>().expect_offense(indoc! {r#"
            h.reject { |k, v| k != :foo }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:foo)` instead.
        "#});
    }

    #[test]
    fn corrects_reject_key_neq_sym() {
        test::<HashSlice>().expect_correction(
            indoc! {r#"
                h.reject { |k, v| k != :foo }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:foo)` instead.
            "#},
            "h.slice(:foo)\n",
        );
    }

    // --- select + include? ---

    #[test]
    fn flags_select_include() {
        test::<HashSlice>().expect_offense(indoc! {r#"
            h.select { |k, v| [:a, :b].include?(k) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:a, :b)` instead.
        "#});
    }

    #[test]
    fn corrects_select_include() {
        test::<HashSlice>().expect_correction(
            indoc! {r#"
                h.select { |k, v| [:a, :b].include?(k) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:a, :b)` instead.
            "#},
            "h.slice(:a, :b)\n",
        );
    }

    #[test]
    fn flags_select_include_variable() {
        test::<HashSlice>().expect_offense(indoc! {r#"
            h.select { |k, v| allowed.include?(k) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(*allowed)` instead.
        "#});
    }

    #[test]
    fn corrects_select_include_variable() {
        test::<HashSlice>().expect_correction(
            indoc! {r#"
                h.select { |k, v| allowed.include?(k) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(*allowed)` instead.
            "#},
            "h.slice(*allowed)\n",
        );
    }

    // --- reject + negated include? ---

    #[test]
    fn flags_reject_negated_include() {
        test::<HashSlice>().expect_offense(indoc! {r#"
            h.reject { |k, v| ![:a, :b].include?(k) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:a, :b)` instead.
        "#});
    }

    #[test]
    fn corrects_reject_negated_include() {
        test::<HashSlice>().expect_correction(
            indoc! {r#"
                h.reject { |k, v| ![:a, :b].include?(k) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:a, :b)` instead.
            "#},
            "h.slice(:a, :b)\n",
        );
    }

    // --- filter alias ---

    #[test]
    fn flags_filter_key_eq_sym() {
        test::<HashSlice>().expect_offense(indoc! {r#"
            h.filter { |k, v| k == :foo }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:foo)` instead.
        "#});
    }

    // --- string key ---

    #[test]
    fn flags_select_key_eq_str() {
        test::<HashSlice>().expect_offense(indoc! {r#"
            h.select { |k, v| k == "foo" }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice("foo")` instead.
        "#});
    }

    // --- No offense cases ---

    #[test]
    fn no_offense_select_with_non_literal_eq() {
        // key must be sym or str for ==; integer not allowed
        test::<HashSlice>().expect_no_offenses("h.select { |k, v| k == 42 }\n");
    }

    #[test]
    fn no_offense_reject_eq_semantic() {
        // reject + == is "except", not "slice"
        test::<HashSlice>().expect_no_offenses("h.reject { |k, v| k == :foo }\n");
    }

    #[test]
    fn no_offense_select_neq_semantic() {
        // select + != is "except", not "slice"
        test::<HashSlice>().expect_no_offenses("h.select { |k, v| k != :foo }\n");
    }

    #[test]
    fn no_offense_include_value_var() {
        // using value variable in include? should not flag
        test::<HashSlice>().expect_no_offenses("h.select { |k, v| v.include?(k) }\n");
    }

    #[test]
    fn no_offense_range_include() {
        // range receiver of include? should not flag
        test::<HashSlice>().expect_no_offenses("h.select { |k, v| (1..5).include?(k) }\n");
    }

    #[test]
    fn no_offense_no_block() {
        test::<HashSlice>().expect_no_offenses("h.select\n");
    }

    #[test]
    fn no_offense_three_args() {
        test::<HashSlice>().expect_no_offenses("h.select { |k, v, x| k == :foo }\n");
    }

    #[test]
    fn no_offense_one_arg() {
        test::<HashSlice>().expect_no_offenses("h.select { |k| k == :foo }\n");
    }

    // --- do/end block corrects same as brace block ---

    #[test]
    fn corrects_select_do_end_block_singleline() {
        // Single-line do/end to keep offense on one line.
        test::<HashSlice>().expect_correction(
            "h.select do |k, v| k == :foo; end
  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `slice(:foo)` instead.\n",
            "h.slice(:foo)\n",
        );
    }
}

murphy_plugin_api::submit_cop!(HashSlice);
