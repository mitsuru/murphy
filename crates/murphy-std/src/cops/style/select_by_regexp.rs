//! `Style/SelectByRegexp` — prefer `grep`/`grep_v` over `select`/`reject` with
//! a regexp match.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SelectByRegexp
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `select`/`filter`/`find_all`/`reject` blocks whose body is a
//!   regexp match on the sole block argument. Autocorrects to `grep`/`grep_v`.
//!
//!   Covered block body patterns:
//!     - `x.match?(/re/)` or `/re/.match?(x)` → non-negated
//!     - `x =~ /re/` or `/re/ =~ x` → non-negated
//!     - `x.match(/re/)` → non-negated (deprecated but common)
//!     - `!x.match?(/re/)` or `!/re/.match?(x)` → negated
//!     - `x !~ /re/` or `/re/ !~ x` → negated via `!~`
//!     - numblock `_1.match?(/re/)` / `_1 =~ /re/` etc.
//!
//!   Gaps vs. upstream:
//!     - `!(x =~ /re/)` and `!(/re/ =~ x)` produce `Unknown` nodes in the
//!       current murphy-translate; these are not flagged (conservative v1 gap).
//!     - itblock (`it` param, Ruby 3.4) not handled — cop dispatch lacks itblock.
//!     - `x =~ lvar` (non-literal regexp in =~ position) is supported.
//!     - Hash exclusions: hash literal receiver, Hash.new, ENV constant,
//!       to_h/to_hash chain — all suppressed (matching upstream).
//!   Autocorrect is marked unsafe (matching upstream) because `MatchData`
//!   will not be created by `grep`, but may have previously been relied upon.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (→ grep)
//! array.select { |x| x.match? /regexp/ }
//! array.select { |x| x =~ /regexp/ }
//!
//! # bad (→ grep_v)
//! array.select { |x| !x.match?(/regexp/) }
//! array.select { |x| x !~ /regexp/ }
//!
//! # good
//! array.grep(/regexp/)
//! array.grep_v(/regexp/)
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Symbol, cop};

const MSG: &str = "Prefer `%<replacement>s` to `%<original_method>s` with a regexp match.";

/// The set of send methods that trigger this cop.
const SELECT_METHODS: &[&str] = &["select", "filter", "find_all"];
const REJECT_METHOD: &str = "reject";

/// Stateless unit struct.
#[derive(Default)]
pub struct SelectByRegexp;

#[cop(
    name = "Style/SelectByRegexp",
    description = "Prefer `grep` or `grep_v` to `select`/`reject` with a regexp match.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl SelectByRegexp {
    /// Check `block` nodes: `array.select { |x| ... }`.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }

    /// Check `numblock` nodes: `array.select { _1.match?(/re/) }`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(block_node: NodeId, cx: &Cx<'_>) {
    // The block's call node must be `send` or `csend` with a select/reject method.
    // `block_call` covers Block; for Numblock we extract `send` directly.
    let call = match *cx.kind(block_node) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } => send,
        _ => return,
    };
    let original_method = match cx.method_name(call) {
        Some(m) if is_select_or_reject(m) => m,
        _ => return,
    };

    // Receiver hash-exclusion checks.
    if is_hash_like_receiver(call, cx) {
        return;
    }

    // Match the block shape: exactly one argument (or numblock with max_n=1).
    let (block_arg_sym, body_node) = match match_block_shape(block_node, cx) {
        Some(v) => v,
        None => return,
    };

    // Match the body: a regexp match expression involving the block arg.
    let (regexp_node, is_negated) = match match_regexp_body(body_node, block_arg_sym, cx) {
        Some(v) => v,
        None => return,
    };

    // Determine replacement method name.
    let replacement = replacement_method(original_method, is_negated);

    let msg = MSG
        .replace("%<replacement>s", replacement)
        .replace("%<original_method>s", original_method);

    cx.emit_offense(cx.range(block_node), &msg, None);

    // Autocorrect: replace the whole block expression.
    autocorrect(block_node, call, original_method, replacement, regexp_node, cx);
}

/// Returns true if the method name is one that triggers this cop.
fn is_select_or_reject(method: &str) -> bool {
    SELECT_METHODS.contains(&method) || method == REJECT_METHOD
}

/// Returns the replacement method name given the original method and negation.
fn replacement_method(original: &str, is_negated: bool) -> &'static str {
    let is_select = SELECT_METHODS.contains(&original);
    match (is_select, is_negated) {
        (true, false) => "grep",
        (true, true) => "grep_v",
        (false, false) => "grep_v",  // reject + non-negated → grep_v
        (false, true) => "grep",     // reject + negated → grep
    }
}

/// Check whether the block receiver is hash-like (suppresses the offense).
///
/// Hash-like receivers: hash literal `{}`, `Hash.new`, `Hash[]`,
/// `to_h`/`to_hash` chain, `ENV` constant.
fn is_hash_like_receiver(call: NodeId, cx: &Cx<'_>) -> bool {
    let receiver = match cx.call_receiver(call).get() {
        Some(r) => r,
        None => return false,
    };
    match *cx.kind(receiver) {
        // Hash literal `{}`
        NodeKind::Hash(_) => true,
        // ENV constant
        NodeKind::Const { name, .. } => cx.symbol_str(name) == "ENV",
        // Send: to_h / to_hash chain, or Hash.new
        NodeKind::Send { receiver: inner_recv, method, .. } => {
            let mname = cx.symbol_str(method);
            if matches!(mname, "to_h" | "to_hash") {
                return true;
            }
            if mname == "new"
                && inner_recv.get().is_some_and(|inner_r| {
                    matches!(*cx.kind(inner_r), NodeKind::Const { name, .. } if cx.symbol_str(name) == "Hash")
                })
            {
                return true;
            }
            false
        }
        // Csend: same checks (receiver is always NodeId for csend)
        NodeKind::Csend { receiver: inner_recv, method, .. } => {
            let mname = cx.symbol_str(method);
            if matches!(mname, "to_h" | "to_hash") {
                return true;
            }
            if mname == "new" && matches!(*cx.kind(inner_recv), NodeKind::Const { name, .. } if cx.symbol_str(name) == "Hash") {
                return true;
            }
            false
        }
        _ => false,
    }
}

/// Extract the (block_arg_symbol, body_node_id) from the block.
///
/// For a normal block: requires exactly one `Arg` in the args list.
/// For a numblock: requires max_n == 1.
/// Returns None if the block shape doesn't match.
fn match_block_shape(block_node: NodeId, cx: &Cx<'_>) -> Option<(Symbol, NodeId)> {
    match *cx.kind(block_node) {
        NodeKind::Block { args, body, .. } => {
            let body_id = body.get()?;
            // The args must have exactly one plain `Arg`.
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
            let body_id = body.get()?;
            // Numblock: the parameter is implicitly `_1`.
            // Use a sentinel Symbol that we recognize in is_block_arg_lvar.
            // We find it by looking for Lvar nodes with name "_1".
            // Pass a dummy symbol value; is_block_arg_numarg handles this case.
            let sym = Symbol(0); // sentinel for numblock
            Some((sym, body_id))
        }
        _ => None,
    }
}

/// Match the block body against known regexp-match patterns.
///
/// Returns `Some((regexp_node_id, is_negated))` if the body matches, else None.
/// The `block_arg_sym` is the symbol of the block's single parameter.
fn match_regexp_body(
    body: NodeId,
    block_arg_sym: Symbol,
    cx: &Cx<'_>,
) -> Option<(NodeId, bool)> {
    match *cx.kind(body) {
        // `x.match?(/re/)`, `/re/.match?(x)`, `x =~ /re/`, `/re/ =~ x`,
        // `x.match(/re/)`, `x !~ /re/`, `/re/ !~ x`
        NodeKind::Send { receiver, method, args } => {
            let method_str = cx.symbol_str(method);
            match method_str {
                "match?" | "=~" | "match" => {
                    // x.match?(/re/) → receiver is x (block arg), args[0] is regexp
                    // /re/.match?(x) → receiver is regexp, args[0] is x (block arg)
                    let recv_id = receiver.get()?;
                    let arg_list = cx.list(args);
                    if arg_list.len() != 1 {
                        return None;
                    }
                    let arg_id = arg_list[0];

                    if is_block_arg_lvar(recv_id, block_arg_sym, cx) {
                        // x.match?(/re/) — arg must be regexp-like
                        let re = regexp_or_lvar(arg_id, cx)?;
                        Some((re, false))
                    } else if is_block_arg_lvar(arg_id, block_arg_sym, cx) {
                        // /re/.match?(x) — receiver must be regexp-like
                        let re = regexp_or_lvar(recv_id, cx)?;
                        Some((re, false))
                    } else {
                        None
                    }
                }
                "!~" => {
                    // x !~ /re/ → receiver is x, args[0] is regexp (negated)
                    // /re/ !~ x → receiver is regexp, args[0] is x (negated)
                    let recv_id = receiver.get()?;
                    let arg_list = cx.list(args);
                    if arg_list.len() != 1 {
                        return None;
                    }
                    let arg_id = arg_list[0];

                    if is_block_arg_lvar(recv_id, block_arg_sym, cx) {
                        let re = regexp_or_lvar(arg_id, cx)?;
                        Some((re, true))
                    } else if is_block_arg_lvar(arg_id, block_arg_sym, cx) {
                        let re = regexp_or_lvar(recv_id, cx)?;
                        Some((re, true))
                    } else {
                        None
                    }
                }
                "!" => {
                    // `!x.match?(/re/)` — receiver is `x.match?(/re/)` send
                    let inner = receiver.get()?;
                    let (re, _) = match_non_negated_send(inner, block_arg_sym, cx)?;
                    Some((re, true))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Match a non-negated regexp send node (match?, =~, match, !~).
/// Returns (regexp_node, is_negated=false) for direct patterns.
fn match_non_negated_send(
    node: NodeId,
    block_arg_sym: Symbol,
    cx: &Cx<'_>,
) -> Option<(NodeId, bool)> {
    let NodeKind::Send { receiver, method, args } = *cx.kind(node) else {
        return None;
    };
    let method_str = cx.symbol_str(method);
    match method_str {
        "match?" | "=~" | "match" => {
            let recv_id = receiver.get()?;
            let arg_list = cx.list(args);
            if arg_list.len() != 1 {
                return None;
            }
            let arg_id = arg_list[0];
            if is_block_arg_lvar(recv_id, block_arg_sym, cx) {
                let re = regexp_or_lvar(arg_id, cx)?;
                Some((re, false))
            } else if is_block_arg_lvar(arg_id, block_arg_sym, cx) {
                let re = regexp_or_lvar(recv_id, cx)?;
                Some((re, false))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Returns true if `node` is an `Lvar` with the given symbol.
/// For numblock (sym == Symbol(0) sentinel), matches any `Lvar(_1)`.
fn is_block_arg_lvar(node: NodeId, sym: Symbol, cx: &Cx<'_>) -> bool {
    let NodeKind::Lvar(s) = *cx.kind(node) else {
        return false;
    };
    if sym == Symbol(0) {
        // Numblock: the block arg is _1, _2, etc. Only _1 is valid here.
        cx.symbol_str(s) == "_1"
    } else {
        s == sym
    }
}

/// Returns the node ID if it's a `Regexp` node or a plain `Lvar` (variable
/// holding a regexp). Returns None for other kinds (e.g. method calls).
fn regexp_or_lvar(node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    match *cx.kind(node) {
        NodeKind::Regexp { .. } => Some(node),
        NodeKind::Lvar(_) => Some(node),
        _ => None,
    }
}

/// Apply autocorrect: replace the block expression with `receiver.grep(/re/)`.
fn autocorrect(
    block_node: NodeId,
    call: NodeId,
    _original_method: &str,
    replacement: &str,
    regexp_node: NodeId,
    cx: &Cx<'_>,
) {
    // Reconstruct: `receiver_src.grep(regexp_src)` or `receiver_src&.grep(regexp_src)`
    // For implicit receiver (no dot), just `grep(regexp_src)`.
    let receiver = cx.call_receiver(call).get();
    let is_safe_nav = matches!(*cx.kind(call), NodeKind::Csend { .. });
    let regexp_src = cx.raw_source(cx.range(regexp_node));
    let block_range = cx.range(block_node);

    let replacement_src = if let Some(recv) = receiver {
        let recv_src = cx.raw_source(cx.range(recv));
        let dot = if is_safe_nav { "&." } else { "." };
        format!("{recv_src}{dot}{replacement}({regexp_src})")
    } else {
        format!("{replacement}({regexp_src})")
    };

    cx.emit_edit(block_range, &replacement_src);
}

#[cfg(test)]
mod tests {
    use super::SelectByRegexp;
    use murphy_plugin_api::test_support::{indoc, test};

    // ----- select → grep -----

    #[test]
    fn flags_select_with_match_predicate() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.select { |x| x.match?(/regexp/) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a regexp match.
        "#});
    }

    #[test]
    fn flags_select_with_tilde_match() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.select { |x| x =~ /regexp/ }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a regexp match.
        "#});
    }

    #[test]
    fn flags_select_with_reversed_match() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.select { |x| /regexp/.match? x }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a regexp match.
        "#});
    }

    #[test]
    fn flags_select_with_reversed_tilde() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.select { |x| /regexp/ =~ x }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a regexp match.
        "#});
    }

    #[test]
    fn flags_filter_method() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.filter { |x| x.match?(/regexp/) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `filter` with a regexp match.
        "#});
    }

    #[test]
    fn flags_find_all_method() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.find_all { |x| x.match?(/regexp/) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `find_all` with a regexp match.
        "#});
    }

    // ----- reject → grep_v -----

    #[test]
    fn flags_reject_non_negated() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.reject { |x| x.match?(/regexp/) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `reject` with a regexp match.
        "#});
    }

    // ----- select → grep_v (negated) -----

    #[test]
    fn flags_select_with_negated_match() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.select { |x| !x.match?(/regexp/) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `select` with a regexp match.
        "#});
    }

    #[test]
    fn flags_select_with_not_tilde() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.select { |x| x !~ /regexp/ }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `select` with a regexp match.
        "#});
    }

    #[test]
    fn flags_select_with_negated_reversed_match() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.select { |x| !/regexp/.match?(x) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `select` with a regexp match.
        "#});
    }

    // ----- reject → grep (negated) -----

    #[test]
    fn flags_reject_with_not_tilde() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.reject { |x| x !~ /regexp/ }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `reject` with a regexp match.
        "#});
    }

    // ----- numblock -----

    #[test]
    fn flags_numblock_select() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.select { _1.match?(/regexp/) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a regexp match.
        "#});
    }

    #[test]
    fn flags_numblock_with_tilde() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array.select { _1 =~ /regexp/ }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a regexp match.
        "#});
    }

    // ----- safe navigation -----

    #[test]
    fn flags_safe_nav_select() {
        test::<SelectByRegexp>().expect_offense(indoc! {r#"
            array&.select { |x| x.match?(/regexp/) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a regexp match.
        "#});
    }

    // ----- accepted cases -----

    #[test]
    fn accepts_non_regexp_block() {
        test::<SelectByRegexp>().expect_no_offenses("array.select { |x| x.even? }\n");
    }

    #[test]
    fn accepts_multiple_block_args() {
        test::<SelectByRegexp>()
            .expect_no_offenses("obj.select { |x, y| y.match? /regexp/ }\n");
    }

    #[test]
    fn accepts_external_variable_in_match() {
        test::<SelectByRegexp>()
            .expect_no_offenses("array.select { |x| y.match? /regexp/ }\n");
    }

    #[test]
    fn accepts_hash_literal_receiver() {
        test::<SelectByRegexp>()
            .expect_no_offenses("{}.select { |x| x.match? /regexp/ }\n");
    }

    #[test]
    fn accepts_hash_new_receiver() {
        test::<SelectByRegexp>()
            .expect_no_offenses("Hash.new.select { |x| x.match? /regexp/ }\n");
    }

    #[test]
    fn accepts_to_h_chain() {
        test::<SelectByRegexp>()
            .expect_no_offenses("foo.to_h.select { |x| x.match? /regexp/ }\n");
    }

    #[test]
    fn accepts_env_constant() {
        test::<SelectByRegexp>()
            .expect_no_offenses("ENV.select { |x| x.match? /regexp/ }\n");
    }

    #[test]
    fn accepts_empty_block() {
        test::<SelectByRegexp>().expect_no_offenses("array.select { }\n");
    }

    #[test]
    fn accepts_proc_argument() {
        test::<SelectByRegexp>().expect_no_offenses("array.select(&:even?)\n");
    }

    // ----- autocorrect -----

    #[test]
    fn corrects_select_to_grep() {
        test::<SelectByRegexp>().expect_correction(
            indoc! {r#"
                array.select { |x| x.match?(/regexp/) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a regexp match.
            "#},
            "array.grep(/regexp/)\n",
        );
    }

    #[test]
    fn corrects_select_tilde_to_grep() {
        test::<SelectByRegexp>().expect_correction(
            indoc! {r#"
                array.select { |x| x =~ /regexp/ }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a regexp match.
            "#},
            "array.grep(/regexp/)\n",
        );
    }

    #[test]
    fn corrects_negated_select_to_grep_v() {
        test::<SelectByRegexp>().expect_correction(
            indoc! {r#"
                array.select { |x| !x.match?(/regexp/) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `select` with a regexp match.
            "#},
            "array.grep_v(/regexp/)\n",
        );
    }

    #[test]
    fn corrects_reject_to_grep_v() {
        test::<SelectByRegexp>().expect_correction(
            indoc! {r#"
                array.reject { |x| x.match?(/regexp/) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep_v` to `reject` with a regexp match.
            "#},
            "array.grep_v(/regexp/)\n",
        );
    }

    #[test]
    fn corrects_safe_nav_select() {
        test::<SelectByRegexp>().expect_correction(
            indoc! {r#"
                array&.select { |x| x.match?(/regexp/) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `grep` to `select` with a regexp match.
            "#},
            "array&.grep(/regexp/)\n",
        );
    }
}

murphy_plugin_api::submit_cop!(SelectByRegexp);
