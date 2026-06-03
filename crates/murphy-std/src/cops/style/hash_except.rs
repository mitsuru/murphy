//! `Style/HashExcept` — prefer `Hash#except` over `reject`/`select`/`filter` with key comparison.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/HashExcept
//! upstream_version_checked: 1.86.2
//! version_added: "1.7"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detects `reject`/`select`/`filter` with blocks that can be replaced by
//!   `except`. Requires Ruby 3.0+ (Hash#except added then).
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
//! # bad (reject with ==)
//! hash.reject { |k, v| k == :foo }
//! hash.reject { |k, v| :foo == k }
//! hash.reject { |k, v| k.eql?(:foo) }
//! hash.reject { |k, v| [:a, :b].include?(k) }
//!
//! # bad (select/filter with != / negated include)
//! hash.select { |k, v| k != :foo }
//! hash.filter { |k, v| k != :foo }
//! hash.select { |k, v| ![:a, :b].include?(k) }
//! hash.filter { |k, v| ![:a, :b].include?(k) }
//!
//! # good
//! hash.except(:foo)
//! hash.except(:a, :b)
//! hash.except(*excluded)
//! ```
//!
//! ## Autocorrect
//!
//! Replaces `reject { ... }` / `select { ... }` / `filter { ... }` with
//! `except(<key_source>)`. The offense range covers from the selector of
//! `reject`/`select`/`filter` through the closing brace/`end` of the block.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, OptNodeId, Range, Symbol, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct HashExcept;

const MSG: &str = "Use `%s` instead.";

#[cop(
    name = "Style/HashExcept",
    description = "Checks for `reject`/`select`/`filter` that can be replaced with `Hash#except`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
    safe_autocorrect = false,
)]
impl HashExcept {
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

    // Get block args -- must be exactly 2.
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

    // Determine if this is semantically an except (not a slice).
    if !is_semantically_except(method_name, cmp_method, negated) {
        return;
    }

    // For == / != / eql?: key must be a symbol or string literal.
    if matches!(cmp_method, "==" | "!=" | "eql?") && !is_sym_or_str_literal(key_expr, cx) {
        return;
    }

    // Build the replacement source.
    let key_source = except_key_source(key_expr, cx);
    let replacement = format!("except({})", key_source);
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
    if matches!(cx.kind(node), NodeKind::Send { .. })
        && cx.method_name(node) == Some("!") && cx.call_arguments(node).is_empty()
        && let Some(recv) = cx.call_receiver(node).get()
    {
        return (recv, true);
    }
    (node, false)
}

/// Determines if this comparison is semantically `reject`/`select`/`filter` -> `except`.
///
/// RuboCop's `semantically_except_method?`:
/// - `reject` with `==`/`eql?`/non-negated-include? -> except
/// - `select`/`filter` with `!=`/negated-include? -> except
fn is_semantically_except(method_name: &str, cmp_method: &str, negated: bool) -> bool {
    match method_name {
        "reject" => match cmp_method {
            "==" | "eql?" => !negated,
            "!=" => false,
            "include?" => !negated,
            _ => false,
        },
        "select" | "filter" => match cmp_method {
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
/// or `None` if the pattern does not match.
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
            // `k.eql?(val)` -- receiver must be key lvar, one arg.
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
            // `collection.include?(k)` -- arg must be key lvar, one arg.
            if cmp_args.len() != 1 {
                return None;
            }
            if !is_lvar_matching(cmp_args[0], key_sym, cx) {
                return None;
            }
            let cmp_recv = cmp_recv_opt.get()?;
            // The receiver must not be a range or unknown (range_include? guard).
            // In Murphy, ranges in parentheses appear as Unknown -- skip them.
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
/// - `[:a, :b]` -> `":a, :b"` (join elements)
/// - `CONST` / `var` -> `"*CONST"` (splat for non-literals)
/// - `:foo` / `"foo"` -> `":foo"` / `"\"foo\""`
fn except_key_source(key: NodeId, cx: &Cx<'_>) -> String {
    match *cx.kind(key) {
        NodeKind::Array(elems_list) => {
            let elems = cx.list(elems_list);
            if elems.is_empty() {
                return String::new();
            }
            elems
                .iter()
                .map(|&e| match cx.kind(e) {
                    // `%i[a b]` elements have raw_source `a` (no colon);
                    // use the interned symbol value to always emit `:a`.
                    NodeKind::Sym(sid) => format!(":{}", cx.symbol_str(*sid)),
                    // `%w[a b]` elements have raw_source `a` (no quotes);
                    // wrap in double quotes using the interned string value.
                    NodeKind::Str(sid) => {
                        let raw = cx.raw_source(cx.range(e));
                        if raw.starts_with('"') || raw.starts_with('\'') {
                            raw.to_owned()
                        } else {
                            format!("\"{}\"", cx.string_str(*sid))
                        }
                    }
                    _ => cx.raw_source(cx.range(e)).to_owned(),
                })
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
    use super::HashExcept;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- reject + == ---

    #[test]
    fn flags_reject_key_eq_sym() {
        test::<HashExcept>().expect_offense(indoc! {r#"
            h.reject { |k, v| k == :foo }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:foo)` instead.
        "#});
    }

    #[test]
    fn flags_reject_sym_eq_key() {
        test::<HashExcept>().expect_offense(indoc! {r#"
            h.reject { |k, v| :foo == k }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:foo)` instead.
        "#});
    }

    #[test]
    fn corrects_reject_key_eq_sym() {
        test::<HashExcept>().expect_correction(
            indoc! {r#"
                h.reject { |k, v| k == :foo }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:foo)` instead.
            "#},
            "h.except(:foo)\n",
        );
    }

    // --- reject + eql? ---

    #[test]
    fn flags_reject_key_eql_sym() {
        test::<HashExcept>().expect_offense(indoc! {r#"
            h.reject { |k, v| k.eql?(:foo) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:foo)` instead.
        "#});
    }

    #[test]
    fn corrects_reject_key_eql_sym() {
        test::<HashExcept>().expect_correction(
            indoc! {r#"
                h.reject { |k, v| k.eql?(:foo) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:foo)` instead.
            "#},
            "h.except(:foo)\n",
        );
    }

    // --- select/filter + != ---

    #[test]
    fn flags_select_key_neq_sym() {
        test::<HashExcept>().expect_offense(indoc! {r#"
            h.select { |k, v| k != :foo }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:foo)` instead.
        "#});
    }

    #[test]
    fn corrects_select_key_neq_sym() {
        test::<HashExcept>().expect_correction(
            indoc! {r#"
                h.select { |k, v| k != :foo }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:foo)` instead.
            "#},
            "h.except(:foo)\n",
        );
    }

    #[test]
    fn flags_filter_key_neq_sym() {
        test::<HashExcept>().expect_offense(indoc! {r#"
            h.filter { |k, v| k != :foo }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:foo)` instead.
        "#});
    }

    // --- reject + include? ---

    #[test]
    fn flags_reject_include() {
        test::<HashExcept>().expect_offense(indoc! {r#"
            h.reject { |k, v| [:a, :b].include?(k) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:a, :b)` instead.
        "#});
    }

    #[test]
    fn corrects_reject_include() {
        test::<HashExcept>().expect_correction(
            indoc! {r#"
                h.reject { |k, v| [:a, :b].include?(k) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:a, :b)` instead.
            "#},
            "h.except(:a, :b)\n",
        );
    }

    #[test]
    fn flags_reject_include_variable() {
        test::<HashExcept>().expect_offense(indoc! {r#"
            h.reject { |k, v| excluded.include?(k) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(*excluded)` instead.
        "#});
    }

    #[test]
    fn corrects_reject_include_variable() {
        test::<HashExcept>().expect_correction(
            indoc! {r#"
                h.reject { |k, v| excluded.include?(k) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(*excluded)` instead.
            "#},
            "h.except(*excluded)\n",
        );
    }

    // --- select/filter + negated include? ---

    #[test]
    fn flags_select_negated_include() {
        test::<HashExcept>().expect_offense(indoc! {r#"
            h.select { |k, v| ![:a, :b].include?(k) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:a, :b)` instead.
        "#});
    }

    #[test]
    fn corrects_select_negated_include() {
        test::<HashExcept>().expect_correction(
            indoc! {r#"
                h.select { |k, v| ![:a, :b].include?(k) }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:a, :b)` instead.
            "#},
            "h.except(:a, :b)\n",
        );
    }

    #[test]
    fn flags_filter_negated_include() {
        test::<HashExcept>().expect_offense(indoc! {r#"
            h.filter { |k, v| ![:a, :b].include?(k) }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:a, :b)` instead.
        "#});
    }

    // --- string key ---

    #[test]
    fn flags_reject_key_eq_str() {
        test::<HashExcept>().expect_offense(indoc! {r#"
            h.reject { |k, v| k == "foo" }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except("foo")` instead.
        "#});
    }

    // --- No offense cases ---

    #[test]
    fn no_offense_select_eq_semantic() {
        // select + == is "slice", not "except"
        test::<HashExcept>().expect_no_offenses("h.select { |k, v| k == :foo }\n");
    }

    #[test]
    fn no_offense_reject_neq_semantic() {
        // reject + != is "slice", not "except"
        test::<HashExcept>().expect_no_offenses("h.reject { |k, v| k != :foo }\n");
    }

    #[test]
    fn no_offense_reject_with_non_literal_eq() {
        // key must be sym or str for ==; integer not allowed
        test::<HashExcept>().expect_no_offenses("h.reject { |k, v| k == 42 }\n");
    }

    #[test]
    fn no_offense_include_value_var() {
        // using value variable in include? should not flag
        test::<HashExcept>().expect_no_offenses("h.reject { |k, v| v.include?(k) }\n");
    }

    #[test]
    fn no_offense_range_include() {
        // range receiver of include? should not flag
        test::<HashExcept>().expect_no_offenses("h.reject { |k, v| (1..5).include?(k) }\n");
    }

    #[test]
    fn no_offense_no_block() {
        test::<HashExcept>().expect_no_offenses("h.reject\n");
    }

    #[test]
    fn no_offense_three_args() {
        test::<HashExcept>().expect_no_offenses("h.reject { |k, v, x| k == :foo }\n");
    }

    #[test]
    fn no_offense_one_arg() {
        test::<HashExcept>().expect_no_offenses("h.reject { |k| k == :foo }\n");
    }

    // ActiveSupport methods: not detected (documented v1 gap)

    #[test]
    fn no_offense_activesupport_in() {
        test::<HashExcept>().expect_no_offenses(
            "h.reject { |k, v| k.in?(%i[bar]) }\n",
        );
    }

    #[test]
    fn no_offense_activesupport_exclude() {
        test::<HashExcept>().expect_no_offenses(
            "h.select { |k, v| %i[bar].exclude?(k) }\n",
        );
    }

    // --- do/end block corrects same as brace block ---

    #[test]
    fn corrects_reject_do_end_block_singleline() {
        test::<HashExcept>().expect_correction(
            "h.reject do |k, v| k == :foo; end
  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `except(:foo)` instead.\n",
            "h.except(:foo)\n",
        );
    }
}

murphy_plugin_api::submit_cop!(HashExcept);
