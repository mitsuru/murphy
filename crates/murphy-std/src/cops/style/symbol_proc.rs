//! `Style/SymbolProc` — use symbols as procs instead of blocks when possible.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SymbolProc
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags blocks (and numblocks) that call a single argument-less method on
//!   their sole block parameter. Autocorrects to `&:method_name` syntax.
//!
//!   Covered:
//!     - Normal Block: `map { |s| s.upcase }` -> `map(&:upcase)`
//!     - Numblock (max_n == 1): `map { _1.upcase }` -> `map(&:upcase)`
//!     - Lambda (->): `->(x) { x.method }` -> `lambda(&:method)`
//!     - proc/Proc.new blocks: `proc { |x| x.method }` -> `proc(&:method)`
//!     - Blocks on calls with arguments:
//!       `do_something(foo) { |o| o.bar }` -> `do_something(foo, &:bar)`
//!     - AllowedMethods: default ["define_method"]
//!     - AllowComments: skip if block has inline comments (default false)
//!     - AllowMethodsWithArguments: skip if call has args (default false)
//!     - Unsafe hash: skip .reject/.select on hash literal receiver
//!     - Unsafe array: skip .min/.max on array literal receiver
//!
//!   Gaps:
//!     - AllowedPatterns (regex): not supported — derive only covers Vec<String>.
//!     - AllCops::ActiveSupportExtensionsEnabled: no AllCops config infra;
//!       Murphy always uses the false/default path (lambda/proc/Proc.new
//!       blocks ARE flagged, matching the default RuboCop behavior).
//!     - itblock (Ruby 3.4 `it` param): not handled — on_node lacks itblock.
//! ```

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, Symbol, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct SymbolProc;

const MSG: &str = "Pass `&:%<method>s` as an argument to `%<block_method>s` instead of a block.";

/// Cop options for `Style/SymbolProc`.
#[derive(CopOptions)]
pub struct Options {
    #[option(
        default = false,
        description = "When true, allows blocks on methods that have arguments."
    )]
    pub allow_methods_with_arguments: bool,

    #[option(
        default = false,
        description = "When true, allows blocks that contain comments."
    )]
    pub allow_comments: bool,

    #[option(
        default = ["define_method"],
        description = "Method names that are always allowed (not flagged)."
    )]
    pub allowed_methods: Vec<String>,
}

#[cop(
    name = "Style/SymbolProc",
    description = "Use symbols as procs instead of blocks when possible.",
    default_severity = "warning",
    default_enabled = true,
    options = Options,
)]
impl SymbolProc {
    /// Handles normal `Block` nodes: `method { |x| x.foo }`.
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        check_any_block(node, cx, &opts);
    }

    /// Handles numbered-parameter `Numblock` nodes: `method { _1.foo }`.
    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        let opts = cx.options_or_default::<Options>();
        check_any_block(node, cx, &opts);
    }
}

// ---------------------------------------------------------------------------
// Main check
// ---------------------------------------------------------------------------

fn check_any_block(node: NodeId, cx: &Cx<'_>, opts: &Options) {
    // Extract (dispatch_call, method_name_in_body) from the block pattern.
    let Some((call, method_name)) = extract_symbol_proc_pattern(node, cx) else {
        return;
    };

    // Get the dispatch method name for the message and exclusion checks.
    // Lambda `->` blocks have `NodeKind::Lambda` as call, not a Send; use "lambda".
    let block_method = if cx.is_lambda_literal(node) {
        "lambda"
    } else if let Some(m) = cx.method_name(call) {
        m
    } else {
        return;
    };

    // Unsafe hash usage: hash.reject / hash.select
    if is_unsafe_hash_usage(call, block_method, cx) {
        return;
    }

    // Unsafe array usage: array.min / array.max
    if is_unsafe_array_usage(call, block_method, cx) {
        return;
    }

    // Allowed methods check
    if opts.allowed_methods.iter().any(|m| m == block_method) {
        return;
    }

    // AllowMethodsWithArguments: skip if the call has arguments and option is true.
    if opts.allow_methods_with_arguments && !cx.call_arguments(call).is_empty() {
        return;
    }

    // AllowComments: skip if the block contains comments.
    if opts.allow_comments && block_has_comments(node, cx) {
        return;
    }

    // Destructuring block argument: `{ |x,| }` — skip.
    if is_destructuring_block_arg(node, cx) {
        return;
    }

    // Compute offense range: from block opener (`{` or `do`) to block closer.
    let offense_range = block_opener_to_closer(node, cx);

    let message = MSG
        .replace("%<method>s", method_name)
        .replace("%<block_method>s", block_method);

    cx.emit_offense(offense_range, &message, None);

    // Autocorrect
    autocorrect(node, call, method_name, cx);
}

// ---------------------------------------------------------------------------
// Pattern extraction
// ---------------------------------------------------------------------------

/// For a Block or Numblock node, extract `(call_node_id, body_method_name)`.
fn extract_symbol_proc_pattern<'a>(node: NodeId, cx: &'a Cx<'_>) -> Option<(NodeId, &'a str)> {
    match *cx.kind(node) {
        NodeKind::Block { call, args, body } => {
            let body_id = body.get()?;

            // The args node must have exactly one plain `Arg`.
            let args_children = match *cx.kind(args) {
                NodeKind::Args(list) => cx.list(list),
                _ => return None,
            };
            if args_children.len() != 1 {
                return None;
            }
            let param = args_children[0];
            let NodeKind::Arg(param_sym) = *cx.kind(param) else {
                return None;
            };

            // Body must be a send with Lvar(param_sym) as receiver and no args.
            extract_body_send(body_id, param_sym, cx).map(|m| (call, m))
        }
        NodeKind::Numblock { send, max_n, body } => {
            // Only max_n == 1 is convertible.
            if max_n != 1 {
                return None;
            }
            let body_id = body.get()?;
            extract_body_send_numblock(body_id, cx).map(|m| (send, m))
        }
        _ => None,
    }
}

/// Extract method name from a Send body where receiver is `Lvar(param_sym)`.
fn extract_body_send<'a>(body_id: NodeId, param_sym: Symbol, cx: &'a Cx<'_>) -> Option<&'a str> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(body_id)
    else {
        return None;
    };
    let recv_id = receiver.get()?;
    let NodeKind::Lvar(lvar_sym) = *cx.kind(recv_id) else {
        return None;
    };
    if lvar_sym != param_sym {
        return None;
    }
    if !cx.list(args).is_empty() {
        return None;
    }
    Some(cx.symbol_str(method))
}

/// Extract method name from a Numblock body where receiver is `Lvar(_1)`.
fn extract_body_send_numblock<'a>(body_id: NodeId, cx: &'a Cx<'_>) -> Option<&'a str> {
    let NodeKind::Send {
        receiver,
        method,
        args,
    } = *cx.kind(body_id)
    else {
        return None;
    };
    let recv_id = receiver.get()?;
    let NodeKind::Lvar(sym) = *cx.kind(recv_id) else {
        return None;
    };
    if cx.symbol_str(sym) != "_1" {
        return None;
    }
    if !cx.list(args).is_empty() {
        return None;
    }
    Some(cx.symbol_str(method))
}

// ---------------------------------------------------------------------------
// Exclusion helpers
// ---------------------------------------------------------------------------

fn is_unsafe_hash_usage(call: NodeId, method_name: &str, cx: &Cx<'_>) -> bool {
    if !matches!(method_name, "reject" | "select") {
        return false;
    }
    let Some(recv) = cx.call_receiver(call).get() else {
        return false;
    };
    matches!(cx.kind(recv), NodeKind::Hash(..))
}

fn is_unsafe_array_usage(call: NodeId, method_name: &str, cx: &Cx<'_>) -> bool {
    if !matches!(method_name, "min" | "max") {
        return false;
    }
    let Some(recv) = cx.call_receiver(call).get() else {
        return false;
    };
    matches!(cx.kind(recv), NodeKind::Array(..))
}

fn block_has_comments(node: NodeId, cx: &Cx<'_>) -> bool {
    let range = cx.range(node);
    !cx.comments_in_range(range).is_empty()
}

/// Returns `true` if the block argument list has exactly one argument whose
/// source text contains a comma (RuboCop's destructuring check: `{ |x,| }`).
fn is_destructuring_block_arg(node: NodeId, cx: &Cx<'_>) -> bool {
    let args_id = match *cx.kind(node) {
        NodeKind::Block { args, .. } => args,
        _ => return false,
    };
    let args_children = match *cx.kind(args_id) {
        NodeKind::Args(list) => cx.list(list),
        _ => return false,
    };
    if args_children.len() == 1 {
        let source = cx.raw_source(cx.range(args_id));
        source.contains(',')
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Offense range
// ---------------------------------------------------------------------------

/// Find block opener (`{` or `do`) and return the range to end of block.
///
/// IMPORTANT: The `call` inside `Block { call }` has its range set to the
/// full prism `CallNode` range (which includes the block). So we cannot use
/// `cx.range(call).end` as the search start. Instead we use the selector end
/// or the paren-close end, whichever is later.
///
/// For lambda `->` blocks, the `call` is `NodeKind::Lambda` and has no
/// selector. We use the `args` node range end as the search start instead.
fn block_opener_to_closer(node: NodeId, cx: &Cx<'_>) -> Range {
    let node_range = cx.range(node);

    let search_from = match *cx.kind(node) {
        NodeKind::Block { call, .. } => {
            if matches!(cx.kind(call), NodeKind::Lambda) {
                // Lambda `->`: cx.range(call) = just the `->` token.
                // Searching from its end finds the first token AFTER `->`,
                // skipping the parameter list `(x)` and reaching `{`.
                cx.range(call).end
            } else {
                // Regular call: selector end or paren-close end, whichever
                // is later. (cx.range(call).end is the full CallNode range
                // including block — cannot use it directly.)
                let selector_end = cx.selector(call).end;
                let paren_close_end = cx.loc(call).end().end;
                selector_end.max(paren_close_end)
            }
        }
        NodeKind::Numblock { send, .. } => {
            let selector_end = cx.selector(send).end;
            let paren_close_end = cx.loc(send).end().end;
            selector_end.max(paren_close_end)
        }
        _ => return node_range,
    };

    let opener_start = find_block_opener(search_from, node_range.end, cx).unwrap_or(search_from);

    Range {
        start: opener_start,
        end: node_range.end,
    }
}

/// Find the start position of the block opener token (`{` or `do`).
///
/// Note: Lambda `-> {` uses `PM_TOKEN_LAMBDA_BEGIN` which Murphy tokenizes as
/// `SourceTokenKind::Other` with text `{`, not `LeftBrace`. Regular brace
/// blocks use `LeftBrace`. Both are matched here.
fn find_block_opener(search_from: u32, search_until: u32, cx: &Cx<'_>) -> Option<u32> {
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();
    let idx = toks.partition_point(|t| t.range.start < search_from);
    toks[idx..]
        .iter()
        .take_while(|t| t.range.start < search_until)
        .find(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other
                    && matches!(
                        &source[t.range.start as usize..t.range.end as usize],
                        b"{" | b"do"
                    ))
        })
        .map(|t| t.range.start)
}

// ---------------------------------------------------------------------------
// Autocorrect
// ---------------------------------------------------------------------------

fn autocorrect(node: NodeId, call: NodeId, method_name: &str, cx: &Cx<'_>) {
    let args = cx.call_arguments(call);
    if args.is_empty() {
        autocorrect_without_args(node, call, method_name, cx);
    } else {
        autocorrect_with_args(node, call, method_name, cx);
    }
}

/// Autocorrect for calls with no arguments.
fn autocorrect_without_args(node: NodeId, call: NodeId, method_name: &str, cx: &Cx<'_>) {
    // Lambda `->` case: replace whole block with `lambda(&:method)`.
    if cx.is_lambda_literal(node) {
        let whole_range = cx.range(node);
        cx.emit_edit(whole_range, &format!("lambda(&:{})", method_name));
        return;
    }

    let node_range = cx.range(node);
    let loc = cx.loc(call);

    // If call has empty parens `foo()`, replace from `(` to end of block.
    let has_empty_parens = loc.begin() != Range::ZERO;
    if has_empty_parens {
        let replacement_start = loc.begin().start;
        cx.emit_edit(
            Range {
                start: replacement_start,
                end: node_range.end,
            },
            &format!("(&:{})", method_name),
        );
        return;
    }

    // No parens: replace from just after the selector (method name) to end of block.
    // This handles `coll.map { ... }` -> `coll.map(&:upcase)`.
    let selector_end = cx.selector(call).end;
    cx.emit_edit(
        Range {
            start: selector_end,
            end: node_range.end,
        },
        &format!("(&:{})", method_name),
    );
}

/// Autocorrect for calls with arguments: append `&:method` to args, remove block.
fn autocorrect_with_args(node: NodeId, call: NodeId, method_name: &str, cx: &Cx<'_>) {
    let args = cx.call_arguments(call);
    let last_arg = *args.last().expect("has args");
    let last_arg_range = cx.range(last_arg);

    // Insert `, &:method` after the last argument.
    cx.emit_edit(
        Range {
            start: last_arg_range.end,
            end: last_arg_range.end,
        },
        &format!(", &:{}", method_name),
    );

    // Remove the block: from after the call's closing `)` to end of block node.
    let call_loc = cx.loc(call);
    let call_end_paren = call_loc.end();
    let node_range = cx.range(node);

    if call_end_paren != Range::ZERO {
        // `foo(a, b) { ... }` → keep `foo(a, b, &:m)`, so just remove ` { ... }`.
        cx.emit_edit(
            Range {
                start: call_end_paren.end,
                end: node_range.end,
            },
            "",
        );
    } else {
        // No parens command style: search from selector end.
        let selector_end = cx.selector(call).end;
        cx.emit_edit(
            Range {
                start: selector_end,
                end: node_range.end,
            },
            "",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::SymbolProc;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- Basic offense cases ---

    #[test]
    fn flags_block_with_method_call_on_param() {
        test::<SymbolProc>().expect_offense(indoc! {"
            coll.map { |e| e.upcase }
                     ^^^^^^^^^^^^^^^^ Pass `&:upcase` as an argument to `map` instead of a block.
        "});
    }

    #[test]
    fn corrects_basic_block() {
        test::<SymbolProc>().expect_correction(
            indoc! {"
                coll.map { |e| e.upcase }
                         ^^^^^^^^^^^^^^^^ Pass `&:upcase` as an argument to `map` instead of a block.
            "},
            "coll.map(&:upcase)\n",
        );
    }

    #[test]
    fn flags_block_no_space_before_brace() {
        test::<SymbolProc>().expect_offense(indoc! {"
            foo.map{ |a| a.nil? }
                   ^^^^^^^^^^^^^^ Pass `&:nil?` as an argument to `map` instead of a block.
        "});
    }

    #[test]
    fn corrects_block_no_space_before_brace() {
        test::<SymbolProc>().expect_correction(
            indoc! {"
                foo.map{ |a| a.nil? }
                       ^^^^^^^^^^^^^^ Pass `&:nil?` as an argument to `map` instead of a block.
            "},
            "foo.map(&:nil?)\n",
        );
    }

    // --- Numblock ---

    #[test]
    fn flags_numblock() {
        test::<SymbolProc>().expect_offense(indoc! {"
            something { _1.foo }
                      ^^^^^^^^^^ Pass `&:foo` as an argument to `something` instead of a block.
        "});
    }

    #[test]
    fn corrects_numblock() {
        test::<SymbolProc>().expect_correction(
            indoc! {"
                something { _1.foo }
                          ^^^^^^^^^^ Pass `&:foo` as an argument to `something` instead of a block.
            "},
            "something(&:foo)\n",
        );
    }

    #[test]
    fn accepts_numblock_with_max_n_gt_1() {
        test::<SymbolProc>().expect_no_offenses("something { _1 + _2 }\n");
    }

    // --- No-offense cases ---

    #[test]
    fn accepts_block_with_more_than_one_param() {
        test::<SymbolProc>().expect_no_offenses("something { |x, y| x.method }\n");
    }

    #[test]
    fn accepts_empty_block_body() {
        test::<SymbolProc>().expect_no_offenses("something { |x| }\n");
    }

    #[test]
    fn accepts_block_not_called_on_param() {
        test::<SymbolProc>().expect_no_offenses("something { |x| y.method }\n");
    }

    #[test]
    fn accepts_block_body_with_args() {
        test::<SymbolProc>().expect_no_offenses("something { |x| x.foo(bar) }\n");
    }

    #[test]
    fn accepts_block_with_no_param() {
        test::<SymbolProc>().expect_no_offenses("something { x.method }\n");
    }

    #[test]
    fn accepts_block_with_splat_param() {
        test::<SymbolProc>().expect_no_offenses("something { |*x| x.first }\n");
    }

    #[test]
    fn accepts_block_with_blockarg_param() {
        test::<SymbolProc>().expect_no_offenses("something { |&x| x.call }\n");
    }

    #[test]
    fn accepts_block_with_destructuring_comma_arg() {
        test::<SymbolProc>().expect_no_offenses("something { |x,| x.first }\n");
    }

    // --- Allowed methods ---

    #[test]
    fn accepts_define_method_block() {
        test::<SymbolProc>().expect_no_offenses("define_method(:foo) { |foo| foo.bar }\n");
    }

    // --- Unsafe hash/array usage ---

    #[test]
    fn accepts_hash_reject() {
        test::<SymbolProc>().expect_no_offenses("{a: 1}.reject { |x| x.foo }\n");
    }

    #[test]
    fn accepts_hash_select() {
        test::<SymbolProc>().expect_no_offenses("{a: 1}.select { |x| x.foo }\n");
    }

    #[test]
    fn accepts_array_min() {
        test::<SymbolProc>().expect_no_offenses("[1, 2].min { |x| x.foo }\n");
    }

    #[test]
    fn accepts_array_max() {
        test::<SymbolProc>().expect_no_offenses("[1, 2].max { |x| x.foo }\n");
    }

    // --- Non-hash reject/select and non-array min/max are still flagged ---

    #[test]
    fn flags_non_hash_reject() {
        test::<SymbolProc>().expect_offense(indoc! {"
            [1, 2, 3].reject { |x| x.odd? }
                             ^^^^^^^^^^^^^^^ Pass `&:odd?` as an argument to `reject` instead of a block.
        "});
    }

    // --- Call with arguments ---

    #[test]
    fn flags_block_when_call_has_args() {
        test::<SymbolProc>().expect_offense(indoc! {"
            method(one, 2) { |x| x.test }
                           ^^^^^^^^^^^^^^ Pass `&:test` as an argument to `method` instead of a block.
        "});
    }

    #[test]
    fn corrects_block_when_call_has_args() {
        test::<SymbolProc>().expect_correction(
            indoc! {"
                method(one, 2) { |x| x.test }
                               ^^^^^^^^^^^^^^ Pass `&:test` as an argument to `method` instead of a block.
            "},
            "method(one, 2, &:test)\n",
        );
    }

    // --- Lambda ---

    #[test]
    fn flags_lambda_arrow() {
        test::<SymbolProc>().expect_offense(indoc! {r#"
            ->(x) { x.method }
                  ^^^^^^^^^^^^ Pass `&:method` as an argument to `lambda` instead of a block.
        "#});
    }

    #[test]
    fn corrects_lambda_arrow() {
        test::<SymbolProc>().expect_correction(
            indoc! {r#"
                ->(x) { x.method }
                      ^^^^^^^^^^^^ Pass `&:method` as an argument to `lambda` instead of a block.
            "#},
            "lambda(&:method)\n",
        );
    }

    // --- proc/Proc.new ---

    #[test]
    fn flags_proc_block() {
        test::<SymbolProc>().expect_offense(indoc! {"
            proc { |x| x.method }
                 ^^^^^^^^^^^^^^^^ Pass `&:method` as an argument to `proc` instead of a block.
        "});
    }

    #[test]
    fn corrects_proc_block() {
        test::<SymbolProc>().expect_correction(
            indoc! {"
                proc { |x| x.method }
                     ^^^^^^^^^^^^^^^^ Pass `&:method` as an argument to `proc` instead of a block.
            "},
            "proc(&:method)\n",
        );
    }

    #[test]
    fn flags_proc_new_block() {
        test::<SymbolProc>().expect_offense(indoc! {"
            Proc.new { |x| x.method }
                     ^^^^^^^^^^^^^^^^ Pass `&:method` as an argument to `new` instead of a block.
        "});
    }

    #[test]
    fn corrects_proc_new_block() {
        test::<SymbolProc>().expect_correction(
            indoc! {"
                Proc.new { |x| x.method }
                         ^^^^^^^^^^^^^^^^ Pass `&:method` as an argument to `new` instead of a block.
            "},
            "Proc.new(&:method)\n",
        );
    }
}
murphy_plugin_api::submit_cop!(SymbolProc);
