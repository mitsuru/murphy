//! `Style/SelectByRange` — prefer `grep`/`grep_v` over
//! `select`/`reject`/`find`/`detect` with a range check.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SelectByRange
//! upstream_version_checked: 1.87.0
//! status: verified
//! gap_issues: []
//! notes: >
//!   Flags `select`/`filter`/`find_all`/`reject`/`find`/`detect` blocks whose
//!   body is a range check on the sole block argument, and autocorrects to
//!   `grep`/`grep_v` (with a trailing `.first` for `find`/`detect`).
//!
//!   Replacement table (matches upstream `replacement`):
//!     - select/filter/find_all: non-negated → `grep`,  negated → `grep_v`
//!     - reject:                 non-negated → `grep_v`, negated → `grep`
//!     - find/detect:            non-negated → `grep(...).first`,
//!                               negated     → `grep_v(...).first`
//!
//!   Covered block body patterns (and their `!(...)` / `!x...` negations):
//!     - `x.between?(min, max)`         → range literal is `min..max`
//!     - `(min..max).cover?(x)`         → range literal is the receiver source
//!     - `(min..max).include?(x)`       → range literal is the receiver source
//!   across normal blocks, numblocks (`_1`), and itblocks (`it`).
//!
//!   Hash exclusions (matching upstream): hash literal receiver, `Hash.new`,
//!   `Hash[]`, `to_h`/`to_hash` chain, and the top-level `ENV` constant
//!   (`ENV`/`::ENV` only, per upstream `(const {nil? cbase} :ENV)`) are all
//!   suppressed.
//!
//!   Bodies that are `begin`/multi-statement (`{ |x| x.between?(1,10); true }`)
//!   are skipped, matching upstream's `return if block_node.body&.begin_type?`.
//!
//!   Autocorrect is marked unsafe (matching upstream `SafeAutoCorrect: false`):
//!   the cop cannot prove statically that the receiver is an array, so the
//!   grep rewrite may not be equivalent.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (→ grep)
//! array.select { |x| x.between?(1, 10) }
//! array.select { |x| (1..10).cover?(x) }
//!
//! # bad (→ grep_v)
//! array.reject { |x| x.between?(1, 10) }
//!
//! # bad (→ grep(...).first)
//! array.find { |x| x.between?(1, 10) }
//!
//! # good
//! array.grep(1..10)
//! array.grep_v(1..10)
//! array.grep(1..10).first
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, Symbol, cop};

const MSG: &str = "Prefer `%<replacement>s` to `%<original_method>s` with a range check.";

/// Methods that resolve to `grep` (non-negated) / `grep_v` (negated).
const SELECT_METHODS: &[&str] = &["select", "filter", "find_all"];
/// Methods that append `.first` to the grep replacement.
const FIND_METHODS: &[&str] = &["find", "detect"];
const REJECT_METHOD: &str = "reject";

/// Sentinel symbol for numblock/itblock implicit parameters; resolved against
/// the lvar name (`_1` for numblock, `it` for itblock) at match time.
const IMPLICIT_ARG: Symbol = Symbol(0);

/// Stateless unit struct.
#[derive(Default)]
pub struct SelectByRange;

#[cop(
    name = "Style/SelectByRange",
    description = "Prefer `grep`/`grep_v` to `select`/`reject`/`find`/`detect` with a range check.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl SelectByRange {
    /// `array.select { |x| ... }`
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    /// `array.select { _1.between?(1, 10) }`
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    /// `array.select { it.between?(1, 10) }`
    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(block_node: NodeId, cx: &Cx<'_>) {
    let call = match *cx.kind(block_node) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } | NodeKind::Itblock { send, .. } => send,
        _ => return,
    };

    let original_method = match cx.method_name(call) {
        Some(m) if is_restricted_method(m) => m,
        _ => return,
    };

    // Receiver hash-exclusion checks (matches upstream `receiver_allowed?`).
    if is_hash_like_receiver(call, cx) {
        return;
    }

    // Extract the (block_arg_symbol, body_node) for the block. For
    // numblock/itblock the symbol is `IMPLICIT_ARG` and resolved by name.
    let Some((block_arg_sym, body_node)) = match_block_shape(block_node, cx) else {
        return;
    };

    // Skip multi-statement / parenthesized bodies (upstream: skip begin_type?).
    if matches!(*cx.kind(body_node), NodeKind::Begin(_)) {
        return;
    }

    // Match the body against the range-check patterns.
    let Some(found) = match_range_check(body_node, block_arg_sym, cx) else {
        return;
    };

    let replacement = replacement_method(original_method, found.is_negated);
    let msg = MSG
        .replace("%<replacement>s", replacement.display)
        .replace("%<original_method>s", original_method);

    cx.emit_offense(cx.range(block_node), &msg, None);

    autocorrect(block_node, call, &replacement, found.range_literal, cx);
}

/// Returns true if the method name triggers this cop.
fn is_restricted_method(method: &str) -> bool {
    SELECT_METHODS.contains(&method) || FIND_METHODS.contains(&method) || method == REJECT_METHOD
}

/// Resolved replacement: the message text and the grep method + `.first` flag.
struct Replacement {
    /// Text shown in the message, e.g. `grep`, `grep_v`, `grep(...).first`.
    display: &'static str,
    /// `grep` or `grep_v`.
    grep_method: &'static str,
    /// Whether to append `.first` in the autocorrect.
    first_suffix: bool,
}

/// Resolve the replacement from the original method and negation, matching
/// upstream's `replacement`.
fn replacement_method(original: &str, is_negated: bool) -> Replacement {
    if SELECT_METHODS.contains(&original) {
        if is_negated {
            Replacement { display: "grep_v", grep_method: "grep_v", first_suffix: false }
        } else {
            Replacement { display: "grep", grep_method: "grep", first_suffix: false }
        }
    } else if FIND_METHODS.contains(&original) {
        if is_negated {
            Replacement {
                display: "grep_v(...).first",
                grep_method: "grep_v",
                first_suffix: true,
            }
        } else {
            Replacement { display: "grep(...).first", grep_method: "grep", first_suffix: true }
        }
    } else {
        // reject
        if is_negated {
            Replacement { display: "grep", grep_method: "grep", first_suffix: false }
        } else {
            Replacement { display: "grep_v", grep_method: "grep_v", first_suffix: false }
        }
    }
}

/// Whether the block receiver is hash-like (suppresses the offense).
///
/// Hash-like: hash literal `{}`, `Hash.new`, `Hash[]`, `to_h`/`to_hash`
/// chain, or `ENV` constant.
fn is_hash_like_receiver(call: NodeId, cx: &Cx<'_>) -> bool {
    let Some(receiver) = cx.call_receiver(call).get() else {
        return false;
    };
    if cx.is_global_const(receiver, "ENV") {
        // Upstream `env_const?` is `(const {nil? cbase} :ENV)` — only top-level
        // `ENV` / `::ENV`, not a namespaced `Foo::ENV`.
        return true;
    }
    match *cx.kind(receiver) {
        NodeKind::Hash(_) => true,
        NodeKind::Send { receiver: inner, method, .. } => {
            is_hash_chain(cx.symbol_str(method), inner.get(), cx)
        }
        NodeKind::Csend { receiver: inner, method, .. } => {
            is_hash_chain(cx.symbol_str(method), Some(inner), cx)
        }
        // `Hash.new { ... }.select { ... }` — the receiver is a block whose
        // call is `Hash.new` (upstream `creates_hash?` block arm).
        NodeKind::Block { call, .. } => {
            cx.method_name(call) == Some("new")
                && cx.call_receiver(call).get().is_some_and(|r| is_const_named(r, "Hash", cx))
        }
        _ => false,
    }
}

/// Whether a send/csend with `method` and `inner` receiver is a hash-producing
/// chain: `to_h`/`to_hash` (any receiver) or `Hash.new`/`Hash[]`.
fn is_hash_chain(method: &str, inner: Option<NodeId>, cx: &Cx<'_>) -> bool {
    if matches!(method, "to_h" | "to_hash") {
        return true;
    }
    matches!(method, "new" | "[]") && inner.is_some_and(|r| is_const_named(r, "Hash", cx))
}

fn is_const_named(node: NodeId, name: &str, cx: &Cx<'_>) -> bool {
    matches!(*cx.kind(node), NodeKind::Const { name: n, .. } if cx.symbol_str(n) == name)
}

/// Extract `(block_arg_symbol, body_node)` from the block. Returns `None` if
/// the block shape does not have exactly one parameter (or a body).
fn match_block_shape(block_node: NodeId, cx: &Cx<'_>) -> Option<(Symbol, NodeId)> {
    match *cx.kind(block_node) {
        NodeKind::Block { args, body, .. } => {
            let body_id = body.get()?;
            let NodeKind::Args(list) = *cx.kind(args) else {
                return None;
            };
            let arg_children = cx.list(list);
            if arg_children.len() != 1 {
                return None;
            }
            let NodeKind::Arg(arg_sym) = *cx.kind(arg_children[0]) else {
                return None;
            };
            Some((arg_sym, body_id))
        }
        NodeKind::Numblock { body, max_n, .. } => {
            if max_n != 1 {
                return None;
            }
            Some((IMPLICIT_ARG, body.get()?))
        }
        NodeKind::Itblock { body, .. } => Some((IMPLICIT_ARG, body.get()?)),
        _ => None,
    }
}

/// A matched range check: its source-range literal and whether it was negated.
struct RangeCheck {
    /// The source text for the range literal (e.g. `1..10`).
    range_literal: String,
    is_negated: bool,
}

/// Match the block body against the range-check patterns. The block arg is
/// `block_arg_sym` (or `IMPLICIT_ARG` for numblock/itblock).
fn match_range_check(body: NodeId, block_arg_sym: Symbol, cx: &Cx<'_>) -> Option<RangeCheck> {
    // A leading `!` negates: `!x.between?(...)` or `!(x.between?(...))`.
    if cx.method_name(body) == Some("!") {
        let inner = cx.call_receiver(body).get()?;
        let inner = unwrap_begin_single(inner, cx);
        let literal = match_inner_range_send(inner, block_arg_sym, cx)?;
        return Some(RangeCheck { range_literal: literal, is_negated: true });
    }

    // Non-negated: the body itself must be the range-check send.
    let literal = match_inner_range_send(body, block_arg_sym, cx)?;
    Some(RangeCheck { range_literal: literal, is_negated: false })
}

/// Match a (non-negated) range-check send node: `x.between?(min, max)` or
/// `(min..max).cover?(x)` / `(min..max).include?(x)`. Returns the range
/// literal source string.
fn match_inner_range_send(node: NodeId, block_arg_sym: Symbol, cx: &Cx<'_>) -> Option<String> {
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
        return None;
    };
    let method_str = cx.symbol_str(method);
    let recv = receiver.get()?;
    let arg_list = cx.list(args);

    match method_str {
        "between?" => {
            // x.between?(min, max) — receiver is the block arg, two args.
            if !is_block_arg_lvar(recv, block_arg_sym, cx) {
                return None;
            }
            if arg_list.len() != 2 {
                return None;
            }
            let min = cx.raw_source(cx.range(arg_list[0]));
            let max = cx.raw_source(cx.range(arg_list[1]));
            Some(format!("{min}..{max}"))
        }
        "cover?" | "include?" => {
            // (min..max).cover?(x) — receiver is a range (maybe parenthesized),
            // single arg is the block arg.
            if arg_list.len() != 1 || !is_block_arg_lvar(arg_list[0], block_arg_sym, cx) {
                return None;
            }
            let range_node = unwrap_begin_single(recv, cx);
            if !matches!(*cx.kind(range_node), NodeKind::RangeExpr { .. }) {
                return None;
            }
            Some(cx.raw_source(cx.range(range_node)).to_owned())
        }
        _ => None,
    }
}

/// Unwrap a single-element `begin` (parenthesized expression) one level.
fn unwrap_begin_single(node: NodeId, cx: &Cx<'_>) -> NodeId {
    if let NodeKind::Begin(list) = *cx.kind(node)
        && let [single] = cx.list(list)
    {
        return *single;
    }
    node
}

/// Returns true if `node` is an `Lvar` matching the block parameter. For
/// `IMPLICIT_ARG`, matches the numblock/itblock implicit name (`_1` or `it`).
fn is_block_arg_lvar(node: NodeId, sym: Symbol, cx: &Cx<'_>) -> bool {
    let NodeKind::Lvar(s) = *cx.kind(node) else {
        return false;
    };
    if sym == IMPLICIT_ARG {
        let name = cx.symbol_str(s);
        name == "_1" || name == "it"
    } else {
        s == sym
    }
}

/// Apply autocorrect: replace the call selector through the block end with
/// `grep(<range>)` / `grep_v(<range>)` and an optional trailing `.first`,
/// preserving the receiver byte-for-byte.
fn autocorrect(
    block_node: NodeId,
    call: NodeId,
    replacement: &Replacement,
    range_literal: String,
    cx: &Cx<'_>,
) {
    let selector_start = cx.node(call).loc.name.start;
    let block_end = cx.range(block_node).end;
    if selector_start == 0 {
        // loc.name unset — skip the autocorrect rather than corrupt source.
        return;
    }
    let edit_range = Range { start: selector_start, end: block_end };
    let suffix = if replacement.first_suffix { ".first" } else { "" };
    let new_src = format!("{}({range_literal}){suffix}", replacement.grep_method);
    cx.emit_edit(edit_range, &new_src);
}

#[cfg(test)]
mod tests {
    use super::SelectByRange;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- select / filter / find_all → grep -----

    #[test]
    fn flags_select_with_between() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.select { |x| x.between?(1, 10) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a range check.
        "#});
    }

    #[test]
    fn flags_select_with_cover() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.select { |x| (1..10).cover?(x) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a range check.
        "#});
    }

    #[test]
    fn flags_select_with_include() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.select { |x| (1..10).include?(x) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a range check.
        "#});
    }

    #[test]
    fn flags_filter_method() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.filter { |x| x.between?(1, 10) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `filter` with a range check.
        "#});
    }

    #[test]
    fn flags_find_all_method() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.find_all { |x| x.between?(1, 10) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `find_all` with a range check.
        "#});
    }

    // ----- reject → grep_v -----

    #[test]
    fn flags_reject_non_negated() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.reject { |x| x.between?(1, 10) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `reject` with a range check.
        "#});
    }

    #[test]
    fn flags_reject_negated() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.reject { |x| !x.between?(1, 10) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `reject` with a range check.
        "#});
    }

    // ----- find / detect → grep(...).first -----

    #[test]
    fn flags_find_method() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.find { |x| x.between?(1, 10) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep(...).first` to `find` with a range check.
        "#});
    }

    #[test]
    fn flags_detect_with_cover() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.detect { |x| (1..10).cover?(x) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep(...).first` to `detect` with a range check.
        "#});
    }

    #[test]
    fn flags_find_negated() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.find { |x| !x.between?(1, 10) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v(...).first` to `find` with a range check.
        "#});
    }

    // ----- select negated → grep_v -----

    #[test]
    fn flags_select_negated() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.select { |x| !x.between?(1, 10) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `select` with a range check.
        "#});
    }

    #[test]
    fn flags_select_negated_cover() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.find { |x| !(1..10).cover?(x) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v(...).first` to `find` with a range check.
        "#});
    }

    // ----- parenthesized negation `!(...)` -----

    #[test]
    fn flags_parenthesized_negation() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.reject { |x| !(x.between?(1, 10)) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `reject` with a range check.
        "#});
    }

    // ----- numblock / itblock -----

    #[test]
    fn flags_numblock() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.select { _1.between?(1, 10) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a range check.
        "#});
    }

    #[test]
    fn flags_itblock() {
        test::<SelectByRange>().expect_offense(indoc! {r#"
            array.select { it.between?(1, 10) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a range check.
        "#});
    }

    // ----- accepted cases -----

    #[test]
    fn accepts_non_range_block() {
        test::<SelectByRange>().expect_no_offenses("array.select { |x| x.even? }\n");
    }

    #[test]
    fn accepts_external_variable() {
        test::<SelectByRange>().expect_no_offenses("array.select { |x| y.between?(1, 10) }\n");
    }

    #[test]
    fn accepts_multiple_block_args() {
        test::<SelectByRange>().expect_no_offenses("obj.select { |x, y| x.between?(1, 10) }\n");
    }

    #[test]
    fn accepts_begin_body() {
        test::<SelectByRange>()
            .expect_no_offenses("array.select { |x| x.between?(1, 10); true }\n");
    }

    #[test]
    fn accepts_hash_literal_receiver() {
        test::<SelectByRange>().expect_no_offenses("{}.select { |x| x.between?(1, 10) }\n");
    }

    #[test]
    fn accepts_hash_new_receiver() {
        test::<SelectByRange>().expect_no_offenses("Hash.new.select { |x| x.between?(1, 10) }\n");
    }

    #[test]
    fn accepts_hash_new_block_receiver() {
        test::<SelectByRange>().expect_no_offenses(
            "Hash.new { |h, k| h[k] = 0 }.select { |x| x.between?(1, 10) }\n",
        );
    }

    #[test]
    fn accepts_hash_index_receiver() {
        test::<SelectByRange>()
            .expect_no_offenses("Hash[pairs].select { |x| x.between?(1, 10) }\n");
    }

    #[test]
    fn accepts_to_h_chain() {
        test::<SelectByRange>().expect_no_offenses("foo.to_h.select { |x| x.between?(1, 10) }\n");
    }

    #[test]
    fn accepts_env_constant() {
        test::<SelectByRange>().expect_no_offenses("ENV.select { |x| x.between?(1, 10) }\n");
    }

    #[test]
    fn accepts_toplevel_env_constant() {
        // `::ENV` matches upstream `env_const?` (`(const {nil? cbase} :ENV)`).
        test::<SelectByRange>().expect_no_offenses("::ENV.select { |x| x.between?(1, 10) }\n");
    }

    #[test]
    fn flags_namespaced_env_constant() {
        // `Foo::ENV` is NOT the top-level ENV, so upstream's `env_const?` does
        // not match and the offense fires (verified against rubocop 1.87.0).
        test::<SelectByRange>().expect_offense(indoc! {r#"
            Foo::ENV.select { |x| x.between?(1, 10) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a range check.
        "#});
    }

    #[test]
    fn accepts_empty_block() {
        test::<SelectByRange>().expect_no_offenses("array.select { }\n");
    }

    #[test]
    fn accepts_proc_argument() {
        test::<SelectByRange>().expect_no_offenses("array.select(&:even?)\n");
    }

    // ----- autocorrect -----

    #[test]
    fn corrects_select_to_grep() {
        test::<SelectByRange>().expect_correction(
            indoc! {r#"
                array.select { |x| x.between?(1, 10) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a range check.
            "#},
            "array.grep(1..10)\n",
        );
    }

    #[test]
    fn corrects_cover_to_grep() {
        test::<SelectByRange>().expect_correction(
            indoc! {r#"
                array.select { |x| (1..10).cover?(x) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a range check.
            "#},
            "array.grep(1..10)\n",
        );
    }

    #[test]
    fn corrects_reject_to_grep_v() {
        test::<SelectByRange>().expect_correction(
            indoc! {r#"
                array.reject { |x| x.between?(1, 10) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `reject` with a range check.
            "#},
            "array.grep_v(1..10)\n",
        );
    }

    #[test]
    fn corrects_find_to_grep_first() {
        test::<SelectByRange>().expect_correction(
            indoc! {r#"
                array.find { |x| x.between?(1, 10) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep(...).first` to `find` with a range check.
            "#},
            "array.grep(1..10).first\n",
        );
    }

    #[test]
    fn corrects_negated_find_to_grep_v_first() {
        test::<SelectByRange>().expect_correction(
            indoc! {r#"
                array.find { |x| !x.between?(1, 10) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v(...).first` to `find` with a range check.
            "#},
            "array.grep_v(1..10).first\n",
        );
    }

    #[test]
    fn corrects_numblock() {
        test::<SelectByRange>().expect_correction(
            indoc! {r#"
                array.select { _1.between?(1, 10) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a range check.
            "#},
            "array.grep(1..10)\n",
        );
    }

    #[test]
    fn corrects_safe_nav() {
        test::<SelectByRange>().expect_correction(
            indoc! {r#"
                array&.select { |x| x.between?(1, 10) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a range check.
            "#},
            "array&.grep(1..10)\n",
        );
    }
}

murphy_plugin_api::submit_cop!(SelectByRange);
