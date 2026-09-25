//! `Style/SelectByKind` — prefer `grep`/`grep_v` to `select`/`reject`/`filter`/
//! `find_all` with a class-type check.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SelectByKind
//! upstream_version_checked: 1.87.0
//! version_added: "1.85"
//! safe: true
//! supports_autocorrect: true
//! safe_autocorrect: false
//! status: verified
//! gap_issues: []
//! notes: >
//!   Looks for a subset of an Enumerable calculated via a class-type check
//!   (`is_a?`/`kind_of?`) inside a `select`/`filter`/`find_all`/`reject` block,
//!   and suggests `grep`/`grep_v`.
//!
//!   Handles regular blocks, numblocks (`_1`) and itblocks (`it`), plus the
//!   negated form (`!x.is_a?(Foo)`). The block must take exactly one parameter
//!   and its body must be a single `is_a?`/`kind_of?` send whose receiver is
//!   that parameter (RuboCop's `(send (lvar _) %CLASS_CHECK_METHODS _)`).
//!
//!   Replacement mapping (verified against rubocop 1.87.0):
//!     select/filter/find_all -> grep   (negated -> grep_v)
//!     reject                 -> grep_v (negated -> grep)
//!
//!   Receiver guard (RuboCop's `receiver_allowed?`): skipped when the receiver
//!   is a hash literal, `Hash.new` (call or block form), `Hash[]`, a `to_h`/
//!   `to_hash` call (send or csend), or the `ENV` constant. The `Hash` constant
//!   matches any scope; `ENV` matches only nil/cbase scope, mirroring RuboCop's
//!   `(const _ :Hash)` vs `(const {nil? cbase} :ENV)` patterns.
//!
//!   A parenthesized body `{ |x| (x.is_a?(Foo)) }` parses as `(begin ...)` and
//!   is skipped, matching RuboCop's `return if block_node.body&.begin_type?`.
//!
//!   Offense spans the whole block node; autocorrect replaces selector-begin..
//!   block-end with `grep(Klass)` / `grep_v(Klass)`. Autocorrect is unsafe
//!   (the receiver may not actually be an array), matching RuboCop's
//!   `SafeAutoCorrect: false`.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (select / find_all / filter)
//! array.select { |x| x.is_a?(Foo) }
//! array.select { |x| x.kind_of?(Foo) }
//!
//! # bad (reject)
//! array.reject { |x| x.is_a?(Foo) }
//!
//! # bad (negative form)
//! array.reject { |x| !x.is_a?(Foo) }
//!
//! # good
//! array.grep(Foo)
//! array.grep_v(Foo)
//! ```

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop, def_node_matcher};

// RuboCop parity: `Style/SelectByKind` receiver guards are
// `env_const?` = `(const {nil? cbase} :ENV)` and the `Hash` head of
// `creates_hash?` = `(call (const _ :Hash) {:new :[]} ...)` (plus the `to_h`/
// `to_hash` arms and the `(block (call (const _ :Hash) :new ...) ...)` arm).
// In Murphy `::ENV` / `::Hash` collapse to `Const{scope:None}`: `nil?` covers
// bare + `::` for `ENV` (namespaced `Foo::ENV` still flags — pinned by
// `boundary_flags_namespaced_env`), while `_` covers any scope for `Hash`
// (namespaced `Foo::Hash` still suppresses, unlike `ENV`). `call` covers
// both `send` and `csend`, matching the `Send`/`Csend` arms below (pinned by
// `boundary_accepts_csend_hash_new`); the `Block` arm stays hand-rolled
// (normal `block` only, Numblock/Itblock bail).
def_node_matcher!(env_const, "(const nil? :ENV)");
def_node_matcher!(hash_new_or_index, "(call (const _ :Hash) {:new :[]} ...)");

#[derive(Default)]
pub struct SelectByKind;

/// `is_a?` / `kind_of?` — RuboCop's `CLASS_CHECK_METHODS`. Note: `instance_of?`
/// is intentionally NOT included.
const CLASS_CHECK_METHODS: [&str; 2] = ["is_a?", "kind_of?"];

/// `select` / `filter` / `find_all` — RuboCop's `SELECT_METHODS`. Plus `reject`
/// they form `RESTRICT_ON_SEND`.
const SELECT_METHODS: [&str; 3] = ["select", "filter", "find_all"];
const RESTRICT_ON_SEND: [&str; 4] = ["select", "filter", "find_all", "reject"];

#[cop(
    name = "Style/SelectByKind",
    description = "Prefer `grep`/`grep_v` to `select`/`reject`/`filter`/`find_all` with a kind check.",
    default_severity = "warning",
    default_enabled = false,
    safe_autocorrect = false,
    options = murphy_plugin_api::NoOptions
)]
impl SelectByKind {
    /// Regular block: `array.select { |x| x.is_a?(Foo) }`.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Block { call, args, body } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        // The block must take exactly one named parameter.
        let arg_children = cx.children(args);
        let [arg_id] = arg_children.as_slice() else {
            return;
        };
        let NodeKind::Arg(param_sym) = *cx.kind(*arg_id) else {
            return;
        };
        check_select_block(node, call, body_id, cx.symbol_str(param_sym), cx);
    }

    /// Numbered-parameter block: `array.select { _1.is_a?(Foo) }`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Numblock { send, max_n, body } = *cx.kind(node) else {
            return;
        };
        if max_n != 1 {
            return;
        }
        let Some(body_id) = body.get() else {
            return;
        };
        check_select_block(node, send, body_id, "_1", cx);
    }

    /// `it`-parameter block: `array.select { it.is_a?(Foo) }`.
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        let NodeKind::Itblock { send, body } = *cx.kind(node) else {
            return;
        };
        let Some(body_id) = body.get() else {
            return;
        };
        check_select_block(node, send, body_id, "it", cx);
    }
}

/// Shared check for all three block kinds.
///
/// `block_node` is the whole `Block`/`Numblock`/`Itblock` node, `call` its
/// underlying `select`/`reject`/`filter`/`find_all` call, `body` its single
/// body expression, and `param` the block's parameter name (`x`, `_1`, `it`).
fn check_select_block(block_node: NodeId, call: NodeId, body: NodeId, param: &str, cx: &Cx<'_>) {
    // The call must be `select`/`filter`/`find_all`/`reject`.
    let Some(method) = cx.method_name(call) else {
        return;
    };
    if !RESTRICT_ON_SEND.contains(&method) {
        return;
    }
    // RuboCop's `block_node.body&.begin_type?` guard: a multi-statement body or
    // a parenthesized single body (`(begin ...)`) is skipped.
    if matches!(cx.kind(body), NodeKind::Begin(_)) {
        return;
    }
    // Receiver guard: hashes and `ENV` behave differently under `grep`.
    if receiver_allowed(cx.call_receiver(call).get(), cx) {
        return;
    }

    // Strip a leading `!`, tracking negation. The kind-check arg must be
    // extracted from the *inner* (unwrapped) send.
    let (kind_send, negated) = strip_negation(body, cx);

    // The kind-check must be `<param>.is_a?(Klass)` / `<param>.kind_of?(Klass)`:
    // a plain `Send` (not `Csend`) whose receiver is the block parameter lvar
    // and which has exactly one argument.
    let NodeKind::Send {
        receiver: kind_recv,
        method: kind_method,
        args: kind_args,
    } = *cx.kind(kind_send)
    else {
        return;
    };
    if !CLASS_CHECK_METHODS.contains(&cx.symbol_str(kind_method)) {
        return;
    }
    let Some(recv_id) = kind_recv.get() else {
        return;
    };
    if !crate::cops::util::is_block_parameter_read(recv_id, param, cx) {
        return;
    }
    // Exactly one argument — the class to pass to grep/grep_v.
    let [klass] = cx.list(kind_args) else {
        return;
    };

    let replacement = replacement(method, negated);
    let klass_src = cx.raw_source(cx.range(*klass));
    let message = format!("Prefer `{replacement}` to `{method}` with a kind check.");

    // Offense spans the whole block node (matches RuboCop's `add_offense(block_node)`).
    cx.emit_offense(cx.range(block_node), &message, None);

    // Autocorrect replaces selector-begin..block-end with `grep(Klass)`,
    // preserving the receiver and dot that sit before the selector.
    let edit_range = Range {
        start: cx.selector(block_node).start,
        end: cx.range(block_node).end,
    };
    cx.emit_edit(edit_range, &format!("{replacement}({klass_src})"));
}

/// Strip a leading `!` (a send with method `!`, no args, with a receiver) and
/// return `(inner, negated)`. Mirrors RuboCop's `unwrap_negation` + `negated?`.
fn strip_negation(node: NodeId, cx: &Cx<'_>) -> (NodeId, bool) {
    if matches!(cx.kind(node), NodeKind::Send { .. })
        && cx.method_name(node) == Some("!")
        && cx.call_arguments(node).is_empty()
        && let Some(recv) = cx.call_receiver(node).get()
    {
        return (recv, true);
    }
    (node, false)
}

/// RuboCop's `replacement`: select-family maps to `grep` (negated -> `grep_v`);
/// `reject` maps to `grep_v` (negated -> `grep`).
fn replacement(method: &str, negated: bool) -> &'static str {
    if SELECT_METHODS.contains(&method) {
        if negated { "grep_v" } else { "grep" }
    } else if negated {
        // reject
        "grep"
    } else {
        "grep_v"
    }
}

/// RuboCop's `receiver_allowed?`: the receiver is a hash literal, `Hash.new`
/// (call or block form), `Hash[]`, a `to_h`/`to_hash` call, or `ENV`.
fn receiver_allowed(receiver: Option<NodeId>, cx: &Cx<'_>) -> bool {
    let Some(recv) = receiver else {
        return false;
    };
    matches!(cx.kind(recv), NodeKind::Hash(_)) || creates_hash(recv, cx) || env_const(recv, cx)
}

/// RuboCop's `creates_hash?`: `Hash.new(...)` / `Hash.new { ... }` / `Hash[...]`
/// / `x.to_h` / `x.to_hash`.
fn creates_hash(node: NodeId, cx: &Cx<'_>) -> bool {
    // `Hash.new { ... }` block form: unwrap to the underlying `Hash.new` call.
    // RuboCop's `creates_hash?` is `(block (call (const _ :Hash) :new ...) ...)`
    // — a normal `block` only. A numblock/itblock `Hash.new` default proc does
    // NOT match and so does not suppress the offense (verified against rubocop
    // 1.87.0). Bail explicitly: `cx.method_name`/`cx.call_receiver` delegate
    // through Numblock/Itblock to the inner send, so falling to `_ => node`
    // would incorrectly resolve `Hash.new` and suppress.
    let call = match *cx.kind(node) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { .. } | NodeKind::Itblock { .. } => return false,
        _ => node,
    };

    let Some(method) = cx.method_name(call) else {
        return false;
    };

    // `to_h` / `to_hash` — any receiver, send or csend.
    if matches!(method, "to_h" | "to_hash") {
        return true;
    }

    // `(call (const _ :Hash) {:new :[]} ...)` — `Hash.new` / `Hash[]`
    // (any scope); `call` covers `send` + `csend`.
    // `to_h` / `to_hash` (any receiver) stays hand-rolled above.
    hash_new_or_index(call, cx)
}

#[cfg(test)]
mod tests {
    use super::SelectByKind;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- select / filter / find_all -> grep ---

    #[test]
    fn flags_select_is_a() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array.select { |x| x.is_a?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a kind check.
        "});
    }

    #[test]
    fn flags_select_kind_of() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array.select { |x| x.kind_of?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a kind check.
        "});
    }

    #[test]
    fn flags_filter_is_a() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array.filter { |x| x.is_a?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `filter` with a kind check.
        "});
    }

    #[test]
    fn flags_find_all_is_a() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array.find_all { |x| x.is_a?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `find_all` with a kind check.
        "});
    }

    // --- reject -> grep_v ---

    #[test]
    fn flags_reject_is_a() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array.reject { |x| x.is_a?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `reject` with a kind check.
        "});
    }

    // --- negated forms ---

    #[test]
    fn flags_negated_reject_is_a() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array.reject { |x| !x.is_a?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `reject` with a kind check.
        "});
    }

    #[test]
    fn flags_negated_select_is_a() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array.select { |x| !x.is_a?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `select` with a kind check.
        "});
    }

    // --- namespaced constant ---

    #[test]
    fn flags_namespaced_class() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array.find_all { |x| x.kind_of?(Bar::Baz) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `find_all` with a kind check.
        "});
    }

    // --- numblock / itblock ---

    #[test]
    fn flags_numblock() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array.filter { _1.is_a?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `filter` with a kind check.
        "});
    }

    #[test]
    fn flags_itblock() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array.select { it.is_a?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a kind check.
        "});
    }

    // --- safe navigation (csend) ---

    #[test]
    fn flags_safe_navigation() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array&.select { |x| x.is_a?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a kind check.
        "});
    }

    #[test]
    fn corrects_safe_navigation() {
        test::<SelectByKind>().expect_correction(
            indoc! {"
                array&.select { |x| x.is_a?(Foo) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a kind check.
            "},
            "array&.grep(Foo)\n",
        );
    }

    // --- find / detect are NOT covered (return one element, not a subset) ---

    #[test]
    fn accepts_find() {
        test::<SelectByKind>().expect_no_offenses("array.find { |x| x.is_a?(Foo) }\n");
    }

    #[test]
    fn accepts_detect() {
        test::<SelectByKind>().expect_no_offenses("array.detect { |x| x.is_a?(Foo) }\n");
    }

    // --- multiline block ---

    #[test]
    fn flags_multiline_block() {
        test::<SelectByKind>().expect_offense(indoc! {"
            array.select do |x|; x.is_a?(Foo); end
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a kind check.
        "});
    }

    // --- corrections ---

    #[test]
    fn corrects_select_to_grep() {
        test::<SelectByKind>().expect_correction(
            indoc! {"
                array.select { |x| x.is_a?(Foo) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a kind check.
            "},
            "array.grep(Foo)\n",
        );
    }

    #[test]
    fn corrects_reject_to_grep_v() {
        test::<SelectByKind>().expect_correction(
            indoc! {"
                array.reject { |x| x.is_a?(Foo) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `reject` with a kind check.
            "},
            "array.grep_v(Foo)\n",
        );
    }

    #[test]
    fn corrects_negated_reject_to_grep() {
        test::<SelectByKind>().expect_correction(
            indoc! {"
                array.reject { |x| !x.is_a?(Foo) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `reject` with a kind check.
            "},
            "array.grep(Foo)\n",
        );
    }

    #[test]
    fn corrects_namespaced_class() {
        test::<SelectByKind>().expect_correction(
            indoc! {"
                array.find_all { |x| x.kind_of?(Bar::Baz) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `find_all` with a kind check.
            "},
            "array.grep(Bar::Baz)\n",
        );
    }

    #[test]
    fn corrects_numblock() {
        test::<SelectByKind>().expect_correction(
            indoc! {"
                array.filter { _1.is_a?(Foo) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `filter` with a kind check.
            "},
            "array.grep(Foo)\n",
        );
    }

    // --- receiver guards (no offense) ---

    #[test]
    fn accepts_hash_literal_receiver() {
        test::<SelectByKind>().expect_no_offenses("{a: 1}.select { |x| x.is_a?(Foo) }\n");
    }

    #[test]
    fn accepts_hash_new_receiver() {
        test::<SelectByKind>().expect_no_offenses("Hash.new.select { |x| x.is_a?(Foo) }\n");
    }

    #[test]
    fn accepts_hash_new_block_receiver() {
        test::<SelectByKind>()
            .expect_no_offenses("Hash.new { |h, k| h[k] = 0 }.select { |x| x.is_a?(Foo) }\n");
    }

    #[test]
    fn flags_hash_new_numblock_receiver() {
        // RuboCop's `creates_hash?` is `(block (call (const _ :Hash) :new ...) ...)`
        // — it matches a normal `block` only, NOT a numblock. So a `Hash.new`
        // numblock default proc does not suppress the offense (verified against
        // rubocop 1.87.0, which fires here).
        test::<SelectByKind>().expect_offense(indoc! {r#"
            Hash.new { _1 }.select { |x| x.is_a?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a kind check.
        "#});
    }

    #[test]
    fn accepts_hash_index_receiver() {
        test::<SelectByKind>().expect_no_offenses("Hash[pairs].select { |x| x.is_a?(Foo) }\n");
    }

    #[test]
    fn accepts_to_h_receiver() {
        test::<SelectByKind>().expect_no_offenses("foo.to_h.select { |x| x.is_a?(Foo) }\n");
    }

    #[test]
    fn accepts_to_hash_receiver() {
        test::<SelectByKind>().expect_no_offenses("foo.to_hash.select { |x| x.is_a?(Foo) }\n");
    }

    #[test]
    fn accepts_env_receiver() {
        test::<SelectByKind>().expect_no_offenses("ENV.select { |x| x.is_a?(Foo) }\n");
    }

    // --- Boundary characterization (murphy-ft88.5): pin the exact node set
    // the hand-rolled `ENV` / `Hash` guards match, so the verbatim
    // `(const nil? :ENV)` / `(call (const _ :Hash) {:new :[]} ...)` refactor
    // can be proven equivalent. `::ENV` / `::Hash` collapse to
    // `Const{scope:None}` in Murphy: `nil?` covers bare + `::` for `ENV`
    // (namespaced `Foo::ENV` still flags), while `_` covers any scope for
    // `Hash` (namespaced `Foo::Hash` still suppresses, unlike `ENV`). `call`
    // covers both `send` and `csend`, matching the `Send`/`Csend` arms
    // (the `Block` arm stays hand-rolled, normal `block` only).

    #[test]
    fn boundary_accepts_cbase_env() {
        // `::ENV` collapses to scope-less `Const`, so `nil?` matches.
        test::<SelectByKind>().expect_no_offenses("::ENV.select { |x| x.is_a?(Foo) }\n");
    }

    #[test]
    fn boundary_flags_namespaced_env() {
        // Upstream `(const {nil? cbase} :ENV)` matches top-level only, so
        // `Foo::ENV` still flags (unlike `Hash`, where namespaced suppresses).
        test::<SelectByKind>().expect_offense(indoc! {r#"
            Foo::ENV.select { |x| x.is_a?(Foo) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a kind check.
        "#});
    }

    #[test]
    fn boundary_accepts_cbase_hash_new() {
        // `::Hash` collapses to scope-less `Const`, so wildcard `_` matches.
        test::<SelectByKind>().expect_no_offenses("::Hash.new.select { |x| x.is_a?(Foo) }\n");
    }

    #[test]
    fn boundary_accepts_namespaced_hash_new() {
        // Upstream `(const _ :Hash)` matches any scope, so `Foo::Hash` still
        // suppresses (unlike `ENV`, where namespaced flags).
        test::<SelectByKind>().expect_no_offenses("Foo::Hash.new.select { |x| x.is_a?(Foo) }\n");
    }

    #[test]
    fn boundary_accepts_csend_hash_new() {
        // `&.` is a `csend` node; `call` covers both dispatch kinds and both
        // `Send`/`Csend` arms suppress, so it stays suppressed.
        test::<SelectByKind>().expect_no_offenses("Hash&.new.select { |x| x.is_a?(Foo) }\n");
    }

    // --- shape guards (no offense) ---

    #[test]
    fn accepts_parenthesized_body() {
        // `(x.is_a?(Foo))` parses as `(begin ...)` and is skipped.
        test::<SelectByKind>().expect_no_offenses("array.select { |x| (x.is_a?(Foo)) }\n");
    }

    #[test]
    fn accepts_external_variable() {
        test::<SelectByKind>().expect_no_offenses("array.select { |x| y.is_a?(Foo) }\n");
    }

    #[test]
    fn accepts_instance_of() {
        // `instance_of?` is intentionally NOT covered.
        test::<SelectByKind>().expect_no_offenses("array.select { |x| x.instance_of?(Foo) }\n");
    }

    #[test]
    fn accepts_multiple_expressions() {
        test::<SelectByKind>().expect_no_offenses(indoc! {"
            array.select do |x|
              next if x.nil?
              x.is_a?(Foo)
            end
        "});
    }

    #[test]
    fn accepts_two_block_args() {
        test::<SelectByKind>().expect_no_offenses("array.select { |x, y| x.is_a?(Foo) }\n");
    }

    #[test]
    fn accepts_non_kind_block() {
        test::<SelectByKind>().expect_no_offenses("array.select { |x| x.even? }\n");
    }

    #[test]
    fn accepts_unrelated_method() {
        test::<SelectByKind>().expect_no_offenses("array.map { |x| x.is_a?(Foo) }\n");
    }

    #[test]
    fn accepts_no_block() {
        test::<SelectByKind>().expect_no_offenses("array.select(Foo)\n");
    }
}

murphy_plugin_api::submit_cop!(SelectByKind);
