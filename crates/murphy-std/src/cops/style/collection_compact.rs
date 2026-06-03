//! `Style/CollectionCompact` — use `compact`/`compact!` instead of custom
//! logic that rejects `nil` values from a collection.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/CollectionCompact
//! upstream_version_checked: 1.86.2
//! version_added: "1.2"
//! safe: false
//! supports_autocorrect: true
//! status: partial
//! gap_issues: []
//! notes: >
//!   Safe: false (same as RuboCop) — receiver may not respond to `compact`.
//!
//!   Covered patterns:
//!     - `reject(&:nil?)` / `reject!(&:nil?)` -> `compact` / `compact!`
//!     - `reject { |e [, ...]| e_last.nil? }` block form (last param must be the nil?-tested lvar)
//!     - `reject { _1.nil? }` (numblock) / `reject { it.nil? }` (itblock)
//!     - `select`/`select!`/`filter`/`filter!` block/numblock/itblock with `!e.nil?`
//!     - `grep_v(nil)` / `grep_v(NilClass)` / `grep_v(::NilClass)`
//!
//!   Known v1 limitations (no corresponding beads issues filed):
//!     - AllowedReceivers: implemented as a simple source-string match against
//!       the immediate receiver. RuboCop walks chained receivers to find the
//!       "root" receiver name; Murphy does not chain-walk. This difference only
//!       matters when AllowedReceivers is non-empty, which is opt-in and
//!       uncommon. Default (empty) is fully equivalent.
//!     - TargetRubyVersion gates: Murphy has no version gate. `filter`/`filter!`
//!       (Ruby >= 2.6) and `to_enum`/`lazy` receiver exclusion (Ruby <= 3.0) are
//!       always active. Net effect is "latest Ruby" behavior.
//!     - Safe navigation in block body (`e&.nil?`, `!e&.nil?`) -- these shapes
//!       use `Csend` in the block body. Currently not matched; RuboCop handles
//!       them via its generic matcher. In Murphy they fall through silently.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad
//! array.reject { |e| e.nil? }
//! array.reject! { |e| e.nil? }
//! array.reject(&:nil?)
//! array.reject!(&:nil?)
//! array.select { |e| !e.nil? }
//! array.filter { |e| !e.nil? }
//! array.select { _1.nil? }   # numblock
//! array.reject { it.nil? }   # itblock
//! array.grep_v(nil)
//! array.grep_v(NilClass)
//! array.grep_v(::NilClass)
//!
//! # good
//! array.compact
//! array.compact!
//! ```
//!
//! ## Why this shape
//!
//! We dispatch on the `Send`/`Csend` (the `reject`/`select`/`grep_v` call node).
//! For block-form patterns the parent node is the block, which we inspect.
//! For block-pass and grep_v forms no block is involved.
//!
//! ## Autocorrect
//!
//! Replaces the entire offense range with `compact` or `compact!`.
//! Whole-range replacement is appropriate because the block/block-pass is
//! collapsed into a single method call.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct CollectionCompact;

#[derive(CopOptions)]
pub struct CollectionCompactOptions {
    #[option(
        name = "AllowedReceivers",
        default = [],
        description = "Receiver source strings exempted from this cop."
    )]
    pub allowed_receivers: Vec<String>,
}

const MSG: &str = "Use `%<good>s` instead of `%<bad>s`.";

#[cop(
    name = "Style/CollectionCompact",
    description = "Use `compact`/`compact!` instead of custom nil-rejection logic.",
    default_severity = "warning",
    default_enabled = true,
    options = CollectionCompactOptions,
    safe_autocorrect = false,
)]
impl CollectionCompact {
    /// Triggered on reject, reject!, select, select!, filter, filter!, grep_v sends.
    #[on_node(kind = "send", methods = ["reject", "reject!", "select", "select!", "filter", "filter!", "grep_v"])]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    /// Also handle csend (safe-navigation) versions: `array&.reject { ... }`.
    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if matches!(
            cx.method_name(node),
            Some("reject" | "reject!" | "select" | "select!" | "filter" | "filter!" | "grep_v")
        ) {
            check(node, cx);
        }
    }
}

fn check(node: NodeId, cx: &Cx<'_>) {
    let method_name = cx.method_name(node).unwrap_or("");

    // AllowedReceivers check: suppress if the immediate receiver source matches.
    let opts = cx.options_or_default::<CollectionCompactOptions>();
    if !opts.allowed_receivers.is_empty()
        && let Some(recv) = cx.call_receiver(node).get()
        && opts.allowed_receivers.iter().any(|a| a == cx.raw_source(cx.range(recv)))
    {
        return;
    }

    // Determine offense range and autocorrect replacement.
    let Some((offense_range, good)) = offense(node, method_name, cx) else {
        return;
    };

    let bad = cx.raw_source(offense_range);
    let msg = MSG
        .replace("%<good>s", good)
        .replace("%<bad>s", bad);

    cx.emit_offense(offense_range, &msg, None);
    cx.emit_edit(offense_range, good);
}

/// Returns `Some((offense_range, good_method))` when the send node is an
/// offense, or `None` to skip.
fn offense(node: NodeId, method_name: &str, cx: &Cx<'_>) -> Option<(Range, &'static str)> {
    let good = good_method_name(method_name);

    // Shape 1: reject/reject!(&:nil?) block-pass form.
    if matches!(method_name, "reject" | "reject!") && is_block_pass_nil(node, cx) {
        // Receiver must be present (mirrors RuboCop's `!nil?` guard on receiver).
        cx.call_receiver(node).get()?;
        let range = selector_to_end(node, cx);
        return Some((range, good));
    }

    // Shape 2: grep_v(nil) / grep_v(NilClass) / grep_v(::NilClass).
    if method_name == "grep_v" && is_grep_v_nil(node, cx) {
        let range = selector_to_end(node, cx);
        return Some((range, "compact"));
    }

    // Shapes 3-6: block/numblock/itblock forms.
    // The parent of the send must be the block wrapping it.
    let parent = cx.parent(node).get()?;

    match *cx.kind(parent) {
        NodeKind::Block { call, .. } if call == node => {
            // Receiver of the inner call must be present (not a bare `reject { }`).
            cx.call_receiver(node).get()?;
            if block_matches(parent, method_name, cx) {
                let range = selector_to_block_end(node, parent, cx);
                return Some((range, good));
            }
        }
        NodeKind::Numblock { send, .. } if send == node => {
            cx.call_receiver(node).get()?;
            if numblock_matches(parent, method_name, cx) {
                let range = selector_to_block_end(node, parent, cx);
                return Some((range, good));
            }
        }
        NodeKind::Itblock { send, .. } if send == node => {
            cx.call_receiver(node).get()?;
            if itblock_matches(parent, method_name, cx) {
                let range = selector_to_block_end(node, parent, cx);
                return Some((range, good));
            }
        }
        _ => {}
    }

    None
}

// ---------------------------------------------------------------------------
// Block-pass form: reject(&:nil?)
// ---------------------------------------------------------------------------

/// Returns `true` when the send has exactly one argument that is `BlockPass(Sym("nil?"))`.
fn is_block_pass_nil(node: NodeId, cx: &Cx<'_>) -> bool {
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return false;
    }
    let NodeKind::BlockPass(inner) = *cx.kind(args[0]) else {
        return false;
    };
    let Some(sym_node) = inner.get() else {
        return false;
    };
    let NodeKind::Sym(sym) = *cx.kind(sym_node) else {
        return false;
    };
    cx.symbol_str(sym) == "nil?"
}

// ---------------------------------------------------------------------------
// grep_v form: grep_v(nil) / grep_v(NilClass) / grep_v(::NilClass)
// ---------------------------------------------------------------------------

/// Returns `true` when `grep_v` has exactly one argument that is `nil` or
/// `NilClass` / `::NilClass` (both translate to `Const { scope: None, name: "NilClass" }`).
fn is_grep_v_nil(node: NodeId, cx: &Cx<'_>) -> bool {
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return false;
    }
    let arg = args[0];
    match *cx.kind(arg) {
        NodeKind::Nil => true,
        NodeKind::Const { scope, name } => {
            // Match `NilClass` and `::NilClass` (both have scope=None in Murphy's AST;
            // `Foo::NilClass` has a Const scope and is intentionally skipped).
            scope.get().is_none() && cx.symbol_str(name) == "NilClass"
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Block form matching (regular Block node)
// ---------------------------------------------------------------------------

/// Returns `true` when the Block node matches the reject/select nil pattern.
///
/// reject/reject! form: body is `lvar_last.nil?`
///   where `lvar_last` is the lvar of the last block arg.
/// select/select!/filter/filter! form: body is `!lvar_last.nil?`
fn block_matches(block_node: NodeId, method_name: &str, cx: &Cx<'_>) -> bool {
    let NodeKind::Block { args, body, .. } = *cx.kind(block_node) else {
        return false;
    };
    let Some(body_node) = body.get() else {
        return false;
    };

    // Extract the last block arg name.
    let Some(last_arg) = last_arg_name(args, cx) else {
        return false;
    };

    match method_name {
        "reject" | "reject!" => is_nil_call_on_lvar(body_node, &last_arg, cx),
        "select" | "select!" | "filter" | "filter!" => {
            is_negated_nil_call_on_lvar(body_node, &last_arg, cx)
        }
        _ => false,
    }
}

/// Returns `true` when the Numblock node matches the nil pattern.
/// Numblock uses `_1` as the implicit parameter.
fn numblock_matches(block_node: NodeId, method_name: &str, cx: &Cx<'_>) -> bool {
    let NodeKind::Numblock { body, .. } = *cx.kind(block_node) else {
        return false;
    };
    let Some(body_node) = body.get() else {
        return false;
    };

    match method_name {
        "reject" | "reject!" => is_nil_call_on_lvar(body_node, "_1", cx),
        "select" | "select!" | "filter" | "filter!" => {
            is_negated_nil_call_on_lvar(body_node, "_1", cx)
        }
        _ => false,
    }
}

/// Returns `true` when the Itblock node matches the nil pattern.
/// Itblock uses `it` as the implicit parameter.
fn itblock_matches(block_node: NodeId, method_name: &str, cx: &Cx<'_>) -> bool {
    let NodeKind::Itblock { body, .. } = *cx.kind(block_node) else {
        return false;
    };
    let Some(body_node) = body.get() else {
        return false;
    };

    match method_name {
        "reject" | "reject!" => is_nil_call_on_lvar(body_node, "it", cx),
        "select" | "select!" | "filter" | "filter!" => {
            is_negated_nil_call_on_lvar(body_node, "it", cx)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Body pattern predicates
// ---------------------------------------------------------------------------

/// Returns `true` when `node` is `lvar_name.nil?` (Send or Csend, no args).
fn is_nil_call_on_lvar(node: NodeId, lvar_name: &str, cx: &Cx<'_>) -> bool {
    let (recv, method, args_list) = match *cx.kind(node) {
        NodeKind::Send { receiver, method, args } => (receiver.get(), method, cx.list(args)),
        NodeKind::Csend { receiver, method, args } => (Some(receiver), method, cx.list(args)),
        _ => return false,
    };
    if !args_list.is_empty() {
        return false;
    }
    if cx.symbol_str(method) != "nil?" {
        return false;
    }
    let Some(recv_node) = recv else {
        return false;
    };
    is_lvar_named(recv_node, lvar_name, cx)
}

/// Returns `true` when `node` is `!lvar_name.nil?`.
fn is_negated_nil_call_on_lvar(node: NodeId, lvar_name: &str, cx: &Cx<'_>) -> bool {
    // Outer call must be `.!` with no extra args.
    let (recv, method, args_list) = match *cx.kind(node) {
        NodeKind::Send { receiver, method, args } => (receiver.get(), method, cx.list(args)),
        NodeKind::Csend { receiver, method, args } => (Some(receiver), method, cx.list(args)),
        _ => return false,
    };
    if !args_list.is_empty() {
        return false;
    }
    if cx.symbol_str(method) != "!" {
        return false;
    }
    let Some(recv_node) = recv else {
        return false;
    };
    // Inner call must be `lvar_name.nil?`.
    is_nil_call_on_lvar(recv_node, lvar_name, cx)
}

/// Returns the name of the last `Arg` node in the args list, or `None` if
/// there are no args or the last arg is not an `Arg`.
fn last_arg_name(args_node: NodeId, cx: &Cx<'_>) -> Option<String> {
    let NodeKind::Args(list) = *cx.kind(args_node) else {
        return None;
    };
    let args = cx.list(list);
    let last = *args.last()?;
    match *cx.kind(last) {
        NodeKind::Arg(sym) => Some(cx.symbol_str(sym).to_string()),
        _ => None,
    }
}

/// Returns `true` when `node` is `lvar(lvar_name)`.
fn is_lvar_named(node: NodeId, lvar_name: &str, cx: &Cx<'_>) -> bool {
    match *cx.kind(node) {
        NodeKind::Lvar(sym) => cx.symbol_str(sym) == lvar_name,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Range helpers
// ---------------------------------------------------------------------------

/// Offense range covering just the send itself: from selector start to send end.
fn selector_to_end(node: NodeId, cx: &Cx<'_>) -> Range {
    Range {
        start: cx.selector(node).start,
        end: cx.range(node).end,
    }
}

/// Offense range from selector start of the send to end of the enclosing block.
fn selector_to_block_end(send: NodeId, block: NodeId, cx: &Cx<'_>) -> Range {
    Range {
        start: cx.selector(send).start,
        end: cx.range(block).end,
    }
}

// ---------------------------------------------------------------------------
// Good method name
// ---------------------------------------------------------------------------

fn good_method_name(method_name: &str) -> &'static str {
    match method_name {
        "reject!" | "select!" | "filter!" => "compact!",
        _ => "compact",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{CollectionCompact, CollectionCompactOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    // reject(&:nil?) -> compact

    #[test]
    fn flags_reject_block_pass_nil() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.reject(&:nil?)
                  ^^^^^^^^^^^^^^ Use `compact` instead of `reject(&:nil?)`.
        "#});
    }

    #[test]
    fn corrects_reject_block_pass_nil() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array.reject(&:nil?)
                      ^^^^^^^^^^^^^^ Use `compact` instead of `reject(&:nil?)`.
            "#},
            "array.compact\n",
        );
    }

    // reject!(&:nil?) -> compact!

    #[test]
    fn flags_reject_bang_block_pass_nil() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.reject!(&:nil?)
                  ^^^^^^^^^^^^^^^ Use `compact!` instead of `reject!(&:nil?)`.
        "#});
    }

    #[test]
    fn corrects_reject_bang_block_pass_nil() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array.reject!(&:nil?)
                      ^^^^^^^^^^^^^^^ Use `compact!` instead of `reject!(&:nil?)`.
            "#},
            "array.compact!\n",
        );
    }

    // reject { |e| e.nil? } -> compact

    #[test]
    fn flags_reject_block_nil() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.reject { |e| e.nil? }
                  ^^^^^^^^^^^^^^^^^^^^^ Use `compact` instead of `reject { |e| e.nil? }`.
        "#});
    }

    #[test]
    fn corrects_reject_block_nil() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array.reject { |e| e.nil? }
                      ^^^^^^^^^^^^^^^^^^^^^ Use `compact` instead of `reject { |e| e.nil? }`.
            "#},
            "array.compact\n",
        );
    }

    // reject! { |e| e.nil? } -> compact!

    #[test]
    fn flags_reject_bang_block_nil() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.reject! { |e| e.nil? }
                  ^^^^^^^^^^^^^^^^^^^^^^ Use `compact!` instead of `reject! { |e| e.nil? }`.
        "#});
    }

    #[test]
    fn corrects_reject_bang_block_nil() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array.reject! { |e| e.nil? }
                      ^^^^^^^^^^^^^^^^^^^^^^ Use `compact!` instead of `reject! { |e| e.nil? }`.
            "#},
            "array.compact!\n",
        );
    }

    // select { |e| !e.nil? } -> compact

    #[test]
    fn flags_select_block_not_nil() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.select { |e| !e.nil? }
                  ^^^^^^^^^^^^^^^^^^^^^^ Use `compact` instead of `select { |e| !e.nil? }`.
        "#});
    }

    #[test]
    fn corrects_select_block_not_nil() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array.select { |e| !e.nil? }
                      ^^^^^^^^^^^^^^^^^^^^^^ Use `compact` instead of `select { |e| !e.nil? }`.
            "#},
            "array.compact\n",
        );
    }

    // select! { |e| !e.nil? } -> compact!

    #[test]
    fn flags_select_bang_block_not_nil() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.select! { |e| !e.nil? }
                  ^^^^^^^^^^^^^^^^^^^^^^^ Use `compact!` instead of `select! { |e| !e.nil? }`.
        "#});
    }

    #[test]
    fn corrects_select_bang_block_not_nil() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array.select! { |e| !e.nil? }
                      ^^^^^^^^^^^^^^^^^^^^^^^ Use `compact!` instead of `select! { |e| !e.nil? }`.
            "#},
            "array.compact!\n",
        );
    }

    // filter { |e| !e.nil? } -> compact

    #[test]
    fn flags_filter_block_not_nil() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.filter { |e| !e.nil? }
                  ^^^^^^^^^^^^^^^^^^^^^^ Use `compact` instead of `filter { |e| !e.nil? }`.
        "#});
    }

    #[test]
    fn corrects_filter_block_not_nil() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array.filter { |e| !e.nil? }
                      ^^^^^^^^^^^^^^^^^^^^^^ Use `compact` instead of `filter { |e| !e.nil? }`.
            "#},
            "array.compact\n",
        );
    }

    // filter! { |e| !e.nil? } -> compact!

    #[test]
    fn flags_filter_bang_block_not_nil() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.filter! { |e| !e.nil? }
                  ^^^^^^^^^^^^^^^^^^^^^^^ Use `compact!` instead of `filter! { |e| !e.nil? }`.
        "#});
    }

    // multi-param block: select! { |k, v| !v.nil? }

    #[test]
    fn flags_select_multi_param_last_tested() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.select! { |k, v| !v.nil? }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^ Use `compact!` instead of `select! { |k, v| !v.nil? }`.
        "#});
    }

    // numblock: reject { _1.nil? }

    #[test]
    fn flags_reject_numblock() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.reject { _1.nil? }
                  ^^^^^^^^^^^^^^^^^^ Use `compact` instead of `reject { _1.nil? }`.
        "#});
    }

    #[test]
    fn corrects_reject_numblock() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array.reject { _1.nil? }
                      ^^^^^^^^^^^^^^^^^^ Use `compact` instead of `reject { _1.nil? }`.
            "#},
            "array.compact\n",
        );
    }

    // numblock: select { !_1.nil? }

    #[test]
    fn flags_select_numblock() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.select { !_1.nil? }
                  ^^^^^^^^^^^^^^^^^^^ Use `compact` instead of `select { !_1.nil? }`.
        "#});
    }

    // itblock: reject { it.nil? }

    #[test]
    fn flags_reject_itblock() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.reject { it.nil? }
                  ^^^^^^^^^^^^^^^^^^ Use `compact` instead of `reject { it.nil? }`.
        "#});
    }

    #[test]
    fn corrects_reject_itblock() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array.reject { it.nil? }
                      ^^^^^^^^^^^^^^^^^^ Use `compact` instead of `reject { it.nil? }`.
            "#},
            "array.compact\n",
        );
    }

    // itblock: select { !it.nil? }

    #[test]
    fn flags_select_itblock() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.select { !it.nil? }
                  ^^^^^^^^^^^^^^^^^^^ Use `compact` instead of `select { !it.nil? }`.
        "#});
    }

    // grep_v(nil) -> compact

    #[test]
    fn flags_grep_v_nil() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.grep_v(nil)
                  ^^^^^^^^^^^ Use `compact` instead of `grep_v(nil)`.
        "#});
    }

    #[test]
    fn corrects_grep_v_nil() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array.grep_v(nil)
                      ^^^^^^^^^^^ Use `compact` instead of `grep_v(nil)`.
            "#},
            "array.compact\n",
        );
    }

    // grep_v(NilClass) -> compact

    #[test]
    fn flags_grep_v_nil_class() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.grep_v(NilClass)
                  ^^^^^^^^^^^^^^^^ Use `compact` instead of `grep_v(NilClass)`.
        "#});
    }

    #[test]
    fn corrects_grep_v_nil_class() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array.grep_v(NilClass)
                      ^^^^^^^^^^^^^^^^ Use `compact` instead of `grep_v(NilClass)`.
            "#},
            "array.compact\n",
        );
    }

    // grep_v(::NilClass) -> compact

    #[test]
    fn flags_grep_v_root_nil_class() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array.grep_v(::NilClass)
                  ^^^^^^^^^^^^^^^^^^ Use `compact` instead of `grep_v(::NilClass)`.
        "#});
    }

    // csend receiver: array&.reject { |e| e.nil? }

    #[test]
    fn flags_csend_reject_block() {
        test::<CollectionCompact>().expect_offense(indoc! {r#"
            array&.reject { |e| e.nil? }
                   ^^^^^^^^^^^^^^^^^^^^^ Use `compact` instead of `reject { |e| e.nil? }`.
        "#});
    }

    #[test]
    fn corrects_csend_reject_block() {
        test::<CollectionCompact>().expect_correction(
            indoc! {r#"
                array&.reject { |e| e.nil? }
                       ^^^^^^^^^^^^^^^^^^^^^ Use `compact` instead of `reject { |e| e.nil? }`.
            "#},
            "array&.compact\n",
        );
    }

    // negative: bare call without receiver

    #[test]
    fn accepts_reject_without_receiver() {
        test::<CollectionCompact>().expect_no_offenses("reject { |e| e.nil? }\n");
    }

    // negative: non-nil? block body

    #[test]
    fn accepts_reject_non_nil_predicate() {
        test::<CollectionCompact>().expect_no_offenses("array.reject { |e| e.odd? }\n");
    }

    #[test]
    fn accepts_select_non_nil_predicate() {
        test::<CollectionCompact>().expect_no_offenses("array.select { |e| !e.odd? }\n");
    }

    // negative: block-pass other than &:nil?

    #[test]
    fn accepts_reject_block_pass_other() {
        test::<CollectionCompact>().expect_no_offenses("array.reject(&:blank?)\n");
    }

    // negative: wrong lvar in block body (first param tested, not last)

    #[test]
    fn accepts_reject_multi_param_wrong_tested() {
        // |v, k| v.nil? -- first param tested, not last
        test::<CollectionCompact>().expect_no_offenses("array.reject { |v, k| v.nil? }\n");
    }

    // negative: select with nil? not negated

    #[test]
    fn accepts_select_non_negated_nil() {
        // select { |e| e.nil? } keeps nils -- not what we flag
        test::<CollectionCompact>().expect_no_offenses("array.select { |e| e.nil? }\n");
    }

    // negative: grep_v with non-nil argument

    #[test]
    fn accepts_grep_v_non_nil_arg() {
        test::<CollectionCompact>().expect_no_offenses("array.grep_v(/pattern/)\n");
    }

    #[test]
    fn accepts_grep_v_other_const() {
        test::<CollectionCompact>().expect_no_offenses("array.grep_v(Foo::NilClass)\n");
    }

    // negative: grep_v without argument

    #[test]
    fn accepts_grep_v_no_args() {
        test::<CollectionCompact>().expect_no_offenses("array.grep_v\n");
    }

    // AllowedReceivers

    #[test]
    fn allows_configured_receiver() {
        test::<CollectionCompact>()
            .with_options(&CollectionCompactOptions {
                allowed_receivers: vec!["params".to_string()],
            })
            .expect_no_offenses("params.reject { |e| e.nil? }\n");
    }

    #[test]
    fn flags_non_allowed_receiver() {
        test::<CollectionCompact>()
            .with_options(&CollectionCompactOptions {
                allowed_receivers: vec!["params".to_string()],
            })
            .expect_offense(indoc! {r#"
                array.reject { |e| e.nil? }
                      ^^^^^^^^^^^^^^^^^^^^^ Use `compact` instead of `reject { |e| e.nil? }`.
            "#});
    }
}

murphy_plugin_api::submit_cop!(CollectionCompact);
