//! `Style/ExplicitBlockArgument` — use `&block` instead of block literals that
//! just pass arguments to `yield`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ExplicitBlockArgument
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Detection covers the core pattern: a block node whose body is a bare
//!   `yield` and whose block args match the yield args exactly (same names,
//!   same order).
//!   Autocorrect:
//!     - Removes the block literal (from call_end to block_end).
//!     - Inserts `&block` into the call and into the enclosing def signature.
//!   Block name: if the enclosing def already has a `&blk` argument, its
//!   name is reused; otherwise the name `block` is used.
//!   Gap: `super`/`zsuper` forms are not supported.
//!   Gap: Multiple-dispatch (the same def_node's signature updated only once
//!   across multiple yield-blocks) is not tracked here; each offense emits
//!   its own def edit.
//! ```
//!
//! ## Matched shapes
//!
//! ```ruby
//! # bad (flagged)
//! def nine_times
//!   9.times { yield }
//! end
//!
//! # bad (flagged)
//! def with_dir
//!   Dir.chdir(tmp_dir) { |dir| yield dir }
//! end
//!
//! # good
//! def nine_times(&block)
//!   9.times(&block)
//! end
//! ```

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, SourceTokenKind, cop};

/// Stateless unit struct.
#[derive(Default)]
pub struct ExplicitBlockArgument;

const MSG: &str =
    "Consider using explicit block argument in the surrounding method's signature over `yield`.";

#[cop(
    name = "Style/ExplicitBlockArgument",
    description = "Consider using explicit block argument to avoid writing block literal that just passes its arguments to another block.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl ExplicitBlockArgument {
    #[on_node(kind = "yield")]
    fn check_yield(&self, node: NodeId, cx: &Cx<'_>) {
        check(node, cx);
    }
}

fn check(yield_node: NodeId, cx: &Cx<'_>) {
    // The yield node must be the direct body of a block
    let Some(block_node) = cx.parent(yield_node).get() else {
        return;
    };
    let NodeKind::Block { call: block_call, args: block_args_node, body } =
        *cx.kind(block_node)
    else {
        return;
    };

    // The block's body must be exactly this yield node
    let Some(body_node) = body.get() else {
        return;
    };
    if body_node != yield_node {
        return;
    }

    // Get block args (from the Args node)
    let block_args = match *cx.kind(block_args_node) {
        NodeKind::Args(list) => cx.list(list).to_vec(),
        _ => return,
    };

    // Get yield args
    let NodeKind::Yield(yield_list) = *cx.kind(yield_node) else {
        return;
    };
    let yield_args = cx.list(yield_list).to_vec();

    // Check that block args match yield args exactly
    if !yielding_arguments_match(&block_args, &yield_args, cx) {
        return;
    }

    // Must be inside a method def
    let Some(def_node) = cx
        .ancestors(yield_node)
        .find(|&a| matches!(*cx.kind(a), NodeKind::Def { .. } | NodeKind::Defs { .. }))
    else {
        return;
    };

    // Determine block name from existing def signature or default to "block"
    let block_name = extract_block_name(def_node, cx);

    cx.emit_offense(cx.range(block_node), MSG, None);

    // Autocorrect
    emit_autocorrect(block_node, block_call, def_node, &block_name, cx);
}

/// Check that each block arg name matches the corresponding yield arg lvar name.
fn yielding_arguments_match(block_args: &[NodeId], yield_args: &[NodeId], cx: &Cx<'_>) -> bool {
    if block_args.len() != yield_args.len() {
        return false;
    }
    for (&ya, &ba) in yield_args.iter().zip(block_args.iter()) {
        let ya_name = lvar_name(ya, cx);
        let ba_name = arg_name(ba, cx);
        match (ya_name, ba_name) {
            (Some(y), Some(b)) if y == b => {}
            _ => return false,
        }
    }
    true
}

fn lvar_name<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(node) {
        NodeKind::Lvar(sym) => Some(cx.symbol_str(sym)),
        _ => None,
    }
}

fn arg_name<'a>(node: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    match *cx.kind(node) {
        NodeKind::Arg(sym) | NodeKind::Restarg(sym) => {
            let s = cx.symbol_str(sym);
            if s.is_empty() { None } else { Some(s) }
        }
        _ => None,
    }
}

fn extract_block_name(def_node: NodeId, cx: &Cx<'_>) -> String {
    let args_node = match *cx.kind(def_node) {
        NodeKind::Def { args, .. } | NodeKind::Defs { args, .. } => args,
        _ => return "block".to_string(),
    };
    let NodeKind::Args(list) = *cx.kind(args_node) else {
        return "block".to_string();
    };
    for &arg in cx.list(list) {
        if let NodeKind::Blockarg(sym) = *cx.kind(arg) {
            let name = cx.symbol_str(sym);
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    "block".to_string()
}

fn emit_autocorrect(
    block_node: NodeId,
    call_node: NodeId,
    def_node: NodeId,
    block_name: &str,
    cx: &Cx<'_>,
) {
    // NOTE: In Murphy's AST, the Send node's range covers the ENTIRE expression
    // including the block (because Prism's CallNode location covers the whole
    // call+block). So `cx.range(call_node).end == cx.range(block_node).end`.
    // We must find the block opener token (`{` or `do`) to determine where the
    // send actually ends and the block begins.

    let block_end = cx.range(block_node).end;
    let selector_end = cx.node(call_node).loc.name.end;

    // Find the `{` or `do` token that opens the block.
    let toks = cx.sorted_tokens();
    let source = cx.source().as_bytes();
    let search_start = selector_end;
    let search_idx = toks.partition_point(|t| t.range.start < search_start);
    let block_opener = toks[search_idx..]
        .iter()
        .take_while(|t| t.range.start < block_end)
        .find(|t| {
            t.kind == SourceTokenKind::LeftBrace
                || (t.kind == SourceTokenKind::Other
                    && &source[t.range.start as usize..t.range.end as usize] == b"do")
        });

    let Some(opener) = block_opener else {
        // Cannot find block opener -- bail
        return;
    };

    // Find the token just before the block opener (skip whitespace).
    let before_opener = toks[..toks.partition_point(|t| t.range.end <= opener.range.start)]
        .last();

    // Edit 1: Replace block literal.
    // Determine if the token before the block opener is `)` (parenthesized call with args).
    let is_paren_with_args = before_opener
        .is_some_and(|t| t.kind == SourceTokenKind::RightParen);
    let is_empty_paren = cx.is_parenthesized(call_node) && !cx.has_call_arguments(call_node);

    if is_paren_with_args {
        // `call(args) { ... }` -> `call(args, &block)`
        // Replace from the `)` to the end of block with `, &block)`
        let rp = before_opener.unwrap();
        cx.emit_edit(
            Range { start: rp.range.start, end: block_end },
            &format!(", &{})", block_name),
        );
    } else if is_empty_paren {
        // `call() { ... }` -> `call(&block)`
        // Find `(` token
        let open_paren = toks[search_idx..]
            .iter()
            .take_while(|t| t.range.start < opener.range.start)
            .find(|t| t.kind == SourceTokenKind::LeftParen);
        if let Some(op) = open_paren {
            cx.emit_edit(
                Range { start: op.range.start, end: block_end },
                &format!("(&{})", block_name),
            );
        } else {
            // Fallback
            cx.emit_edit(
                Range { start: opener.range.start, end: block_end },
                &format!("(&{})", block_name),
            );
        }
    } else {
        // `call { ... }` or `call arg { ... }` -> `call(&block)` or `call arg, &block`
        // Simple case: replace from space+opener to block_end with `(&block)`
        // Find the whitespace before the opener to include it in the replacement
        let ws_start = opener.range.start;
        // Check if there's a whitespace char right before opener
        let actual_start = if ws_start > 0 
            && source[ws_start as usize - 1] == b' ' {
            ws_start - 1
        } else {
            ws_start
        };
        cx.emit_edit(
            Range { start: actual_start, end: block_end },
            &format!("(&{})", block_name),
        );
    }

    // Edit 2: Add `&block` to def signature
    let has_block_arg = def_has_block_arg(def_node, cx);
    if !has_block_arg {
        add_block_to_def(def_node, block_name, cx);
    }
}

fn def_has_block_arg(def_node: NodeId, cx: &Cx<'_>) -> bool {
    let args_node = match *cx.kind(def_node) {
        NodeKind::Def { args, .. } | NodeKind::Defs { args, .. } => args,
        _ => return false,
    };
    let NodeKind::Args(list) = *cx.kind(args_node) else {
        return false;
    };
    cx.list(list)
        .iter()
        .any(|&a| matches!(*cx.kind(a), NodeKind::Blockarg(_)))
}

fn add_block_to_def(def_node: NodeId, block_name: &str, cx: &Cx<'_>) {
    let (name_sym, args_node) = match *cx.kind(def_node) {
        NodeKind::Def { name, args, .. } | NodeKind::Defs { name, args, .. } => (name, args),
        _ => return,
    };

    // Find method name token range using token search
    let name_str = cx.symbol_str(name_sym);
    let name_bytes = name_str.as_bytes();
    let node_range = cx.range(def_node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    let start_idx = toks.partition_point(|t| t.range.start < node_range.start);
    let name_range = toks[start_idx..]
        .iter()
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize] == name_bytes
        })
        .map(|t| t.range);

    let Some(name_range) = name_range else {
        return;
    };

    let NodeKind::Args(args_list) = *cx.kind(args_node) else {
        return;
    };
    let args = cx.list(args_list);
    let has_args = !args.is_empty();

    // Check if there's a paren after the name
    let after_name_idx = toks.partition_point(|t| t.range.start < name_range.end);
    let has_paren = toks
        .get(after_name_idx)
        .is_some_and(|t| t.range.start <= node_range.end && t.kind == SourceTokenKind::LeftParen);

    if has_args {
        // Find the last arg and insert `, &block` after it
        let last_arg = args[args.len() - 1];
        let last_arg_end = cx.range(last_arg).end;
        cx.emit_edit(
            Range { start: last_arg_end, end: last_arg_end },
            &format!(", &{}", block_name),
        );
    } else if has_paren {
        // `def foo()` -> `def foo(&block)`
        // Find the matching `)` after `(`
        let open_paren_tok = toks.get(after_name_idx).unwrap();
        let close_paren_idx = toks[after_name_idx..]
            .iter()
            .position(|t| t.kind == SourceTokenKind::RightParen)
            .map(|i| after_name_idx + i);
        if let Some(ci) = close_paren_idx {
            let close_paren_range = toks[ci].range;
            cx.emit_edit(
                Range {
                    start: open_paren_tok.range.start,
                    end: close_paren_range.end,
                },
                &format!("(&{})", block_name),
            );
        }
    } else {
        // `def foo` -> `def foo(&block)` — insert after name
        cx.emit_edit(
            Range { start: name_range.end, end: name_range.end },
            &format!("(&{})", block_name),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ExplicitBlockArgument;
    use murphy_plugin_api::test_support::{indoc, test};

    // --- No offense ---

    #[test]
    fn accepts_block_with_different_arg_names() {
        test::<ExplicitBlockArgument>().expect_no_offenses(indoc! {"
            def foo
              bar { |x| yield y }
            end
        "});
    }

    #[test]
    fn accepts_block_with_body_that_does_more_than_yield() {
        test::<ExplicitBlockArgument>().expect_no_offenses(indoc! {"
            def foo
              bar { |x| puts x; yield x }
            end
        "});
    }

    #[test]
    fn accepts_explicit_block_arg_already_present() {
        test::<ExplicitBlockArgument>().expect_no_offenses(indoc! {"
            def foo(&block)
              9.times(&block)
            end
        "});
    }

    #[test]
    fn accepts_yield_outside_block() {
        test::<ExplicitBlockArgument>().expect_no_offenses(indoc! {"
            def foo
              yield
            end
        "});
    }

    #[test]
    fn accepts_mismatched_arg_count() {
        test::<ExplicitBlockArgument>().expect_no_offenses(indoc! {"
            def foo
              bar { |x, y| yield x }
            end
        "});
    }

    // --- Offense ---

    #[test]
    fn flags_no_arg_block_yield() {
        test::<ExplicitBlockArgument>().expect_offense(indoc! {"
            def nine_times
              9.times { yield }
              ^^^^^^^^^^^^^^^^^ Consider using explicit block argument in the surrounding method's signature over `yield`.
            end
        "});
    }

    #[test]
    fn flags_block_passing_args() {
        test::<ExplicitBlockArgument>().expect_offense(indoc! {"
            def with_dir
              Dir.chdir(tmp_dir) { |dir| yield dir }
              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Consider using explicit block argument in the surrounding method's signature over `yield`.
            end
        "});
    }

    // --- Autocorrect ---

    #[test]
    fn corrects_no_arg_block_yield() {
        test::<ExplicitBlockArgument>().expect_correction(
            indoc! {"
                def nine_times
                  9.times { yield }
                  ^^^^^^^^^^^^^^^^^ Consider using explicit block argument in the surrounding method's signature over `yield`.
                end
            "},
            indoc! {"
                def nine_times(&block)
                  9.times(&block)
                end
            "},
        );
    }

    #[test]
    fn corrects_block_with_single_arg() {
        test::<ExplicitBlockArgument>().expect_correction(
            indoc! {"
                def with_dir
                  Dir.chdir(tmp_dir) { |dir| yield dir }
                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Consider using explicit block argument in the surrounding method's signature over `yield`.
                end
            "},
            indoc! {"
                def with_dir(&block)
                  Dir.chdir(tmp_dir, &block)
                end
            "},
        );
    }
}

murphy_plugin_api::submit_cop!(ExplicitBlockArgument);
