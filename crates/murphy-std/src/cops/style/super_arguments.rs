//! `Style/SuperArguments` — flags redundant arg forwarding in `super`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/SuperArguments
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Keyword shorthand omission (`def foo(a:); super(a:)`) lowers the pair value
//!   to Unknown in Murphy. These are detected via symbol-name matching when the
//!   value is Unknown and the key sym matches the def kwarg name.
//!   Anonymous forwarding (`*`, `**`, `&`) is supported.
//!   `...` forwarding via ForwardArgs/ForwardedArgs is handled.
//!   super() in a block that could be define_method is conservatively skipped.
//!   super() in a block whose call is the super node itself (inline block) is
//!   handled — the MSG_INLINE_BLOCK message is used when the def has a &blk arg
//!   that is not forwarded.
//!   Block reassignment detection covers Lvasgn and OrAsgn on the block arg name.
//! ```
//!
//! ## Matched shapes
//!
//! `Super(args)` (NodeKind::Super) nodes where the args are identical to the
//! enclosing `Def`'s argument list.
//!
//! ## Message
//!
//! - Same args: "Call `super` without arguments and parentheses when the
//!   signature is identical."
//! - Inline block (block arg not forwarded): "Call `super` without arguments
//!   and parentheses when all positional and keyword arguments are forwarded."
//!
//! ## Autocorrect
//!
//! Replaces the entire `super(...)` expression with `super`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Symbol, cop};

const MSG: &str = "Call `super` without arguments and parentheses when the signature is identical.";
const MSG_INLINE_BLOCK: &str = "Call `super` without arguments and parentheses when all positional and keyword arguments are forwarded.";

#[derive(Default)]
pub struct SuperArguments;

#[cop(
    name = "Style/SuperArguments",
    description = "Call `super` without arguments when the signature is identical.",
    default_severity = "warning",
    default_enabled = false,
    options = NoOptions,
)]
impl SuperArguments {
    #[on_node(kind = "super")]
    fn check_super(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

/// Check if `super_node` (a `NodeKind::Super`) has redundant forwarding.
fn check(super_node: NodeId, cx: &Cx<'_>) {
    let NodeKind::Super(super_args_list) = *cx.kind(super_node) else {
        return;
    };

    // Find the enclosing def. Stop at block boundaries (possible define_method
    // delegation means implicit super is not safe).
    let Some(def_node) = find_def_node(super_node, cx) else {
        return;
    };

    let def_node_args = collect_def_args(def_node, cx);
    // If collect_super_args returns None, an unrecognized arg shape was
    // encountered — skip the check to avoid false positives.
    let Some(super_args) = collect_super_args(super_args_list, cx) else {
        return;
    };

    let inline_block = is_super_with_inline_block(super_node, cx);

    if !arguments_identical(
        def_node,
        super_node,
        &def_node_args,
        &super_args,
        inline_block,
        cx,
    ) {
        return;
    }

    // When there is an inline block and def has a block arg not forwarded,
    // use the inline-block message.
    let message = if def_node_args.len() != super_args.len() {
        MSG_INLINE_BLOCK
    } else {
        MSG
    };

    // Compute the offense range. When there is an inline block
    // (`super(a, b) { x }`), the super node's expression range includes the
    // block. We scan tokens to find the closing `)` of the super args list.
    let node_range = cx.range(super_node);
    let offense_range = if inline_block {
        // Find the closing `)` of the super args list by scanning tokens.
        // If the call is unparenthesized (super a, b { x }), we cannot safely
        // bound the autocorrect range — skip the offense to avoid deleting the
        // block body.
        match super_call_range(super_node, node_range, cx) {
            Some(r) => r,
            None => return, // unparenthesized inline-block: cannot safely correct
        }
    } else {
        node_range
    };
    cx.emit_offense(offense_range, message, None);
    cx.emit_edit(offense_range, "super");
}

/// Walk ancestors from `super_node` to find the nearest enclosing `Def`/`Defs`.
/// Returns `None` if:
/// - We encounter any block-type node (Block/Numblock/Itblock) whose call is
///   NOT the super_node itself (potential define_method delegation).
/// - No def is found.
fn find_def_node(super_node: NodeId, cx: &Cx<'_>) -> Option<NodeId> {
    for ancestor in cx.ancestors(super_node) {
        if cx.is_any_def_type(ancestor) {
            return Some(ancestor);
        }
        if cx.is_any_block_type(ancestor) {
            // If the block's call is the super node itself (inline block),
            // continue walking — this is the `super(a) { x }` case.
            if let Some(call) = cx.block_call(ancestor).get()
                && call == super_node
            {
                continue;
            }
            // super inside a block body — potential define_method. Stop.
            return None;
        }
    }
    None
}

/// A compact representation of a def argument for matching.
#[derive(Debug)]
enum DefArg {
    /// Plain positional: `def foo(a)` or `def foo(a = 1)`.
    Positional(Symbol),
    /// Splat: `def foo(*args)`. `None` = anonymous `*`.
    Splat(Option<Symbol>),
    /// Required keyword: `def foo(a:)`. `(name, is_optional)`.
    Keyword { name: Symbol },
    /// Optional keyword: `def foo(a: 1)`.
    KeywordOpt { name: Symbol },
    /// Keyword splat: `def foo(**kwargs)`. `None` = anonymous `**`.
    KwSplat(Option<Symbol>),
    /// Block param: `def foo(&blk)`. `None` = anonymous `&`.
    BlockParam(Option<Symbol>),
    /// `...` forwarding.
    ForwardArgs,
}

/// Collect def args from the `Args` child of a `Def`/`Defs` node.
fn collect_def_args(def_node: NodeId, cx: &Cx<'_>) -> Vec<DefArg> {
    let Some(args_id) = cx.def_arguments(def_node).get() else {
        return vec![];
    };
    let NodeKind::Args(args_list) = *cx.kind(args_id) else {
        return vec![];
    };
    let arg_ids = cx.list(args_list);

    let mut result = Vec::with_capacity(arg_ids.len());
    for &arg_id in arg_ids.iter() {
        match *cx.kind(arg_id) {
            NodeKind::Arg(sym) => result.push(DefArg::Positional(sym)),
            NodeKind::Optarg { name, .. } => result.push(DefArg::Positional(name)),
            NodeKind::Restarg(sym) => {
                let name = cx.symbol_str(sym);
                result.push(DefArg::Splat(if name.is_empty() {
                    None
                } else {
                    Some(sym)
                }));
            }
            NodeKind::Kwarg(sym) => result.push(DefArg::Keyword { name: sym }),
            NodeKind::Kwoptarg { name, .. } => result.push(DefArg::KeywordOpt { name }),
            NodeKind::Kwrestarg(sym) => {
                let name = cx.symbol_str(sym);
                result.push(DefArg::KwSplat(if name.is_empty() {
                    None
                } else {
                    Some(sym)
                }));
            }
            NodeKind::Blockarg(sym) => {
                let name = cx.symbol_str(sym);
                result.push(DefArg::BlockParam(if name.is_empty() {
                    None
                } else {
                    Some(sym)
                }));
            }
            NodeKind::ForwardArgs => result.push(DefArg::ForwardArgs),
            _ => {}
        }
    }
    result
}

/// A compact representation of a super argument for matching.
#[derive(Debug)]
enum SuperArg {
    /// Plain lvar: `super(a)`.
    Lvar(Symbol),
    /// Splat: `super(*args)` → Lvar inside, or anonymous `super(*)`.
    Splat(Option<Symbol>),
    /// Keyword pair: `super(a: a)` or shorthand `super(a:)` (value=Unknown).
    KeyPair { key: Symbol, value: Symbol },
    /// Keyword splat: `super(**kwargs)` → Lvar inside, or anonymous.
    KwSplat(Option<Symbol>),
    /// Block pass: `super(&blk)` → Lvar inside, or anonymous.
    BlockPass(Option<Symbol>),
    /// `...` forwarding.
    ForwardedArgs,
}

/// Collect super args, flattening bare hashes (keyword args without braces).
/// Returns `None` if any argument has an unrecognized shape (indicating a
/// non-forwarding arg that could cause a false positive).
fn collect_super_args(
    super_args_list: murphy_plugin_api::NodeList,
    cx: &Cx<'_>,
) -> Option<Vec<SuperArg>> {
    let raw = cx.list(super_args_list);
    let mut result = Vec::new();

    for &arg_id in raw.iter() {
        match *cx.kind(arg_id) {
            NodeKind::Lvar(sym) => result.push(SuperArg::Lvar(sym)),
            NodeKind::Splat(inner) => {
                if let Some(inner_id) = inner.get() {
                    if let NodeKind::Lvar(sym) = *cx.kind(inner_id) {
                        result.push(SuperArg::Splat(Some(sym)));
                    } else {
                        // Not a simple lvar — unrecognized forwarding shape.
                        return None;
                    }
                } else {
                    // Anonymous splat `*`
                    result.push(SuperArg::Splat(None));
                }
            }
            NodeKind::Hash(pairs_list) => {
                // Keyword args without braces — flatten into individual pairs.
                for &pair_id in cx.list(pairs_list).iter() {
                    match *cx.kind(pair_id) {
                        NodeKind::Pair { key, value } => {
                            let key_sym = match *cx.kind(key) {
                                NodeKind::Sym(s) => s,
                                _ => return None, // non-symbol key — not forwarding
                            };
                            // Value may be Lvar (explicit forward) or Unknown
                            // (shorthand `a:` in Ruby 3.1+).
                            let val_sym = match *cx.kind(value) {
                                NodeKind::Lvar(s) => s,
                                NodeKind::Unknown => key_sym, // shorthand: same name
                                _ => return None,             // non-lvar value — not forwarding
                            };
                            result.push(SuperArg::KeyPair {
                                key: key_sym,
                                value: val_sym,
                            });
                        }
                        NodeKind::Kwsplat(inner) => {
                            if let Some(inner_id) = inner.get() {
                                if let NodeKind::Lvar(sym) = *cx.kind(inner_id) {
                                    result.push(SuperArg::KwSplat(Some(sym)));
                                } else {
                                    return None;
                                }
                            } else {
                                result.push(SuperArg::KwSplat(None));
                            }
                        }
                        _ => return None, // unrecognized hash entry — not forwarding
                    }
                }
            }
            NodeKind::BlockPass(inner) => {
                if let Some(inner_id) = inner.get() {
                    if let NodeKind::Lvar(sym) = *cx.kind(inner_id) {
                        result.push(SuperArg::BlockPass(Some(sym)));
                    } else {
                        // Block pass with non-lvar — not simple forwarding.
                        return None;
                    }
                } else {
                    // Anonymous block pass `&`
                    result.push(SuperArg::BlockPass(None));
                }
            }
            NodeKind::ForwardedArgs => result.push(SuperArg::ForwardedArgs),
            // `Unknown` in a super arg list corresponds to `...` forwarding
            // (super(...)) or Ruby 3.1+ shorthand kwargs (a:). The Unknown
            // case for triple-dot forwarding is a single Unknown arg.
            NodeKind::Unknown => result.push(SuperArg::ForwardedArgs),
            // Unrecognized argument shape — not a forwarding pattern.
            _ => return None,
        }
    }
    Some(result)
}

/// Returns true when the super node has an inline block (e.g. `super(a) { x }`).
/// Uses `cx.block_node` which handles Block, Numblock, and Itblock.
fn is_super_with_inline_block(super_node: NodeId, cx: &Cx<'_>) -> bool {
    cx.block_node(super_node).get().is_some()
}

/// Compare def args against super args, returning true when they're identical.
///
/// When `inline_block` is true, the def may have a trailing `&blk` that is NOT
/// forwarded (because there's an inline block replacing it). In that case we
/// exclude the def's block arg from the comparison.
fn arguments_identical(
    def_node: NodeId,
    super_node: NodeId,
    def_args: &[DefArg],
    super_args: &[SuperArg],
    inline_block: bool,
    cx: &Cx<'_>,
) -> bool {
    // Determine the effective def args (potentially excluding trailing block arg
    // when inline_block is true).
    let effective_def_args: &[DefArg] = if inline_block {
        // If the last def arg is a block param, exclude it for size comparison.
        if def_args
            .last()
            .is_some_and(|a| matches!(a, DefArg::BlockParam(_)))
        {
            &def_args[..def_args.len() - 1]
        } else {
            def_args
        }
    } else {
        def_args
    };

    if effective_def_args.len() != super_args.len() {
        return false;
    }

    for (def_arg, super_arg) in effective_def_args.iter().zip(super_args.iter()) {
        if !arg_pair_matches(def_node, super_node, def_arg, super_arg, cx) {
            return false;
        }
    }

    true
}

fn arg_pair_matches(
    def_node: NodeId,
    _super_node: NodeId,
    def_arg: &DefArg,
    super_arg: &SuperArg,
    cx: &Cx<'_>,
) -> bool {
    match (def_arg, super_arg) {
        // Positional: def arg `a` matches super arg `lvar(a)`
        (DefArg::Positional(def_sym), SuperArg::Lvar(super_sym)) => def_sym == super_sym,

        // Splat: def `*args` matches super `*args`
        (DefArg::Splat(def_name), SuperArg::Splat(super_name)) => {
            match (def_name, super_name) {
                (None, None) => true,         // both anonymous
                (Some(d), Some(s)) => d == s, // named, same name
                _ => false,
            }
        }

        // Keyword: def `a:` or `a: 1` matches super `a: a`
        (
            DefArg::Keyword { name } | DefArg::KeywordOpt { name },
            SuperArg::KeyPair { key, value },
        ) => name == key && key == value,

        // Keyword splat: def `**kwargs` matches super `**kwargs`
        (DefArg::KwSplat(def_name), SuperArg::KwSplat(super_name)) => {
            match (def_name, super_name) {
                (None, None) => true,
                (Some(d), Some(s)) => d == s,
                _ => false,
            }
        }

        // Block param: def `&blk` matches super `&blk` (not reassigned)
        (DefArg::BlockParam(def_name), SuperArg::BlockPass(super_name)) => {
            match (def_name, super_name) {
                (None, None) => true, // both anonymous
                (Some(d), Some(s)) => d == s && !block_arg_reassigned(def_node, *d, cx),
                _ => false,
            }
        }

        // ForwardArgs: def `(...)` matches super `(...)`
        (DefArg::ForwardArgs, SuperArg::ForwardedArgs) => true,

        _ => false,
    }
}

/// Returns true when the block argument `blk_sym` is reassigned (via `lvasgn`
/// or `or_asgn`) anywhere within `def_node`. Mirrors RuboCop's
/// `block_reassigned?`.
fn block_arg_reassigned(def_node: NodeId, blk_sym: Symbol, cx: &Cx<'_>) -> bool {
    cx.descendants(def_node)
        .iter()
        .any(|&desc| match *cx.kind(desc) {
            NodeKind::Lvasgn { name, .. } => name == blk_sym,
            NodeKind::OrAsgn { target, .. } => {
                // target is a write node (Lvasgn with no value)
                matches!(*cx.kind(target), NodeKind::Lvasgn { name, .. } if name == blk_sym)
            }
            _ => false,
        })
}

/// Find the range of just the `super(...)` call, excluding any attached block.
///
/// When `super(a, b) { x }` is used, the Murphy AST stores the super node with
/// the full range including the block. This function scans the token stream to
/// find the matching `)` for the opening `(` of the super args, returning only
/// `super(...)`.
fn super_call_range(
    _super_node: NodeId,
    node_range: murphy_plugin_api::Range,
    cx: &Cx<'_>,
) -> Option<murphy_plugin_api::Range> {
    let toks = cx.sorted_tokens();
    let source = cx.source().as_bytes();

    // Find the `(` after the `super` keyword.
    let idx = toks.partition_point(|t| t.range.start < node_range.start);
    let paren_start = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| t.kind == murphy_plugin_api::SourceTokenKind::LeftParen)?;

    // Scan forward, counting paren depth, to find the matching `)`.
    let search_from = paren_start.range.end;
    let mut depth: i32 = 1;
    let close_paren = toks
        .iter()
        .skip(toks.partition_point(|t| t.range.start < search_from))
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| match t.kind {
            murphy_plugin_api::SourceTokenKind::LeftParen => {
                depth += 1;
                false
            }
            murphy_plugin_api::SourceTokenKind::RightParen => {
                depth -= 1;
                depth == 0
            }
            _ => false,
        })?;

    let _ = source; // suppress unused warning
    Some(murphy_plugin_api::Range {
        start: node_range.start,
        end: close_paren.range.end,
    })
}

#[cfg(test)]
mod tests {
    use super::SuperArguments;
    use murphy_plugin_api::test_support::{indoc, test};

    // ---- Basic offense cases ----

    #[test]
    fn flags_super_with_no_args_method() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo
                  super()
                  ^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo
                  super
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_single_positional_arg() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(a)
                  super(a)
                  ^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(a)
                  super
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_multiple_positional_args() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(a, b)
                  super(a, b)
                  ^^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(a, b)
                  super
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_splat_args() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(*args)
                  super(*args)
                  ^^^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(*args)
                  super
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_kwargs() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(a:)
                  super(a: a)
                  ^^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(a:)
                  super
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_kwsplat() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(**kwargs)
                  super(**kwargs)
                  ^^^^^^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(**kwargs)
                  super
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_block_arg() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(&blk)
                  super(&blk)
                  ^^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(&blk)
                  super
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_mixed_args() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(*args, **kwargs)
                  super(*args, **kwargs)
                  ^^^^^^^^^^^^^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(*args, **kwargs)
                  super
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_optarg() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(a, b, c = 1)
                  super(a, b, c)
                  ^^^^^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(a, b, c = 1)
                  super
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_kwoptarg() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(a, b: 1)
                  super(a, b: b)
                  ^^^^^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(a, b: 1)
                  super
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_inline_block_and_block_arg_not_forwarded() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(a, &blk)
                  super(a) { x }
                  ^^^^^^^^ Call `super` without arguments and parentheses when all positional and keyword arguments are forwarded.
                end
            "},
            indoc! {"
                def foo(a, &blk)
                  super { x }
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_inline_block_and_all_args_forwarded() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(a, b)
                  super(a, b) { x }
                  ^^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(a, b)
                  super { x }
                end
            "},
        );
    }

    #[test]
    fn flags_super_with_triple_dot_forwarding() {
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(...)
                  super(...)
                  ^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(...)
                  super
                end
            "},
        );
    }

    #[test]
    fn flags_super_after_hash_mutation() {
        // The hash argument itself is mutated but the local var forwarding
        // is still identical — should flag.
        test::<SuperArguments>().expect_correction(
            indoc! {"
                def foo(options, &block)
                  options[:key] ||= 1
                  super(options, &block)
                  ^^^^^^^^^^^^^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "},
            indoc! {"
                def foo(options, &block)
                  options[:key] ||= 1
                  super
                end
            "},
        );
    }

    // ---- No-offense cases ----

    #[test]
    fn accepts_super_with_no_parentheses() {
        // `super` (zsuper) without parens is already correct.
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo(a)
              super
            end
        "});
    }

    #[test]
    fn accepts_super_with_explicit_empty_parens() {
        // `super()` means forward NO args, differs from `super` which forwards all.
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo(a)
              super()
            end
        "});
    }

    #[test]
    fn accepts_super_with_subset_of_args() {
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo(a, b)
              super(a)
            end
        "});
    }

    #[test]
    fn accepts_super_with_different_order() {
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo(a, b)
              super(b, a)
            end
        "});
    }

    #[test]
    fn accepts_super_in_block() {
        // super inside a block (possible define_method) — must skip.
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo(a)
              some_method do
                super(a)
              end
            end
        "});
    }

    #[test]
    fn accepts_super_in_dsl_block() {
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            describe 'example' do
              subject { super() }
            end
        "});
    }

    #[test]
    fn accepts_super_with_block_arg_not_forwarded() {
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def bar(a, b, &blk)
              super(a, b)
            end
        "});
    }

    #[test]
    fn accepts_super_with_block_arg_different_name() {
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo(&blk)
              super(&other_blk)
            end
        "});
    }

    #[test]
    fn accepts_super_with_keyword_mixing() {
        // def foo(a, b) but super(a, b: b) — type mismatch.
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo(a, b)
              super(a, b: b)
            end
        "});
    }

    #[test]
    fn accepts_super_when_block_arg_reassigned() {
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo(&blk)
              blk = proc {}
              super(&blk)
            end
        "});
    }

    #[test]
    fn accepts_super_when_block_arg_or_assigned() {
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo(&blk)
              blk ||= proc {}
              super(&blk)
            end
        "});
    }

    #[test]
    fn flags_nested_def_independently() {
        // Each def has its own super check.
        test::<SuperArguments>()
            .expect_offense(indoc! {"
                def foo(a)
                  def bar(b:)
                    super(b: b)
                    ^^^^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                  end
                  super(a)
                  ^^^^^^^^ Call `super` without arguments and parentheses when the signature is identical.
                end
            "});
    }

    #[test]
    fn accepts_unparenthesized_super_with_inline_block() {
        // Unparenthesized `super a, b { x }` — cannot safely bound the
        // autocorrect range (the super node's expression includes the block),
        // so we skip the offense entirely to avoid deleting the block body.
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo(a, b)
              super a, b do
                x
              end
            end
        "});
    }

    #[test]
    fn accepts_super_with_extra_non_forwarded_args() {
        // `super(a, extra_call)` — `extra_call` is not a forwarding lvar.
        // This must NOT be flagged (it would change the behavior).
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo(a)
              super(a, some_extra_call)
            end
        "});
    }
    #[test]
    fn accepts_super_with_no_inline_block_and_no_args() {
        // `super { x }` with no args in a no-arg def — already bare super.
        test::<SuperArguments>().expect_no_offenses(indoc! {"
            def foo
              super { x }
            end
        "});
    }
}
murphy_plugin_api::submit_cop!(SuperArguments);
