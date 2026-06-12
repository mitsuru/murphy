//! `Style/ArgumentsForwarding` — use shorthand `...` forwarding syntax.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/ArgumentsForwarding
//! upstream_version_checked: 1.81.6
//! version_added: "0.0"
//! safe: true
//! supports_autocorrect: true
//! status: partial
//! gap_issues:
//!   - murphy-484s
//! notes: >
//!   Implements the forward-all `...` core path (Ruby 2.7+): flags defs
//!   whose restarg/kwrestarg/blockarg all have redundant names and are
//!   forwarded unchanged to all call sites, replacing with `...`.
//!
//!   murphy-jrcs closed two of the three original gaps: (2)
//!   `AllowOnlyRestArgument` is now an option (default `true`) that mirrors
//!   RuboCop's `offensive_block_forwarding?` — when a def declares no
//!   `&block` and `AllowOnlyRestArgument` is true, forward-all is NOT
//!   flagged (because `...` also forwards a block, changing behaviour); set
//!   it to false to flag `*args`/`**kwargs`-only patterns. (3) The three
//!   `Redundant*ArgumentNames` lists (`RedundantRestArgumentNames`,
//!   `RedundantKeywordRestArgumentNames`, `RedundantBlockArgumentNames`) are
//!   now read via `cx.options_or_default`; the empty (anonymous) name and
//!   any name in the configured list count as redundant, matching RuboCop's
//!   `redundant_named_arg` (`[keyword+name …] << keyword`).
//!
//!   REMAINING GAP (murphy-484s): Anonymous forwarding (`*`, `**`, `&`) for
//!   Ruby 3.2+ (`UseAnonymousForwarding`) is not implemented — RuboCop gates
//!   it on `target_ruby_version >= 3.2`, but Murphy's single-surface plugin
//!   ABI exposes `target_rails_version()` and not `target_ruby_version()`
//!   (CxRaw has no Ruby-version field). Adding it would cross the ABI
//!   boundary, so it stays a documented blocker.
//! ```
//!
//! ## Matched shapes
//!
//! Flags `def`/`defs` methods whose restarg, kwrestarg, and/or blockarg all
//! have redundant names and are forwarded unchanged to every descendant
//! `Send`/`Csend`/`Super`/`Yield` call in the body that uses them.
//!
//! ```ruby
//! # offense
//! def foo(*args, **kwargs, &block)
//!   bar(*args, **kwargs, &block)
//! end
//!
//! # good
//! def foo(...)
//!   bar(...)
//! end
//!
//! # no offense — args referenced outside forwarding
//! def foo(*args, &block)
//!   args.do_something
//!   bar(*args, &block)
//! end
//! ```
//!
//! ## Autocorrect
//!
//! Replaces the forwardable portion of the def's argument list with `...`
//! and replaces the matching splat/kwsplat/block-pass sequence in each
//! call site with `...`. Parentheses are added when missing.

use murphy_plugin_api::{CopOptions, Cx, NodeId, NodeKind, Range, SourceTokenKind, cop};

const FORWARDING_MSG: &str = "Use shorthand syntax `...` for arguments forwarding.";

/// Cop options for [`ArgumentsForwarding`]. Read live at dispatch time via
/// [`Cx::options_or_default`]. Mirrors RuboCop's config keys and defaults.
#[derive(CopOptions)]
pub struct ArgumentsForwardingOptions {
    /// `AllowOnlyRestArgument` — when `true` (default), a def that forwards
    /// only a rest and/or kwrest argument (no `&block`) is NOT flagged, because
    /// `...` would also forward a block and change behaviour. When `false`,
    /// these patterns are flagged. Mirrors RuboCop's `offensive_block_forwarding?`
    /// (`@block_arg ? forwarded_block_arg : !allow_only_rest_arguments`).
    #[option(
        name = "AllowOnlyRestArgument",
        default = true,
        description = "Allow forwarding only a rest/kwrest argument (no block) without flagging."
    )]
    pub allow_only_rest_argument: bool,

    /// `RedundantRestArgumentNames` — rest-arg names treated as anonymous-equivalent
    /// (`*args`, `*arguments`), so they may be replaced with `...`.
    #[option(
        name = "RedundantRestArgumentNames",
        default = ["args", "arguments"],
        description = "Rest-argument names considered redundant (forwardable as `...`)."
    )]
    pub redundant_rest_argument_names: Vec<String>,

    /// `RedundantKeywordRestArgumentNames` — kwrest-arg names treated as
    /// anonymous-equivalent (`**kwargs`, `**options`, `**opts`).
    #[option(
        name = "RedundantKeywordRestArgumentNames",
        default = ["kwargs", "options", "opts"],
        description = "Keyword-rest-argument names considered redundant (forwardable as `...`)."
    )]
    pub redundant_keyword_rest_argument_names: Vec<String>,

    /// `RedundantBlockArgumentNames` — block-arg names treated as
    /// anonymous-equivalent (`&blk`, `&block`, `&proc`).
    #[option(
        name = "RedundantBlockArgumentNames",
        default = ["blk", "block", "proc"],
        description = "Block-argument names considered redundant (forwardable as `...`)."
    )]
    pub redundant_block_argument_names: Vec<String>,
}

#[derive(Default)]
pub struct ArgumentsForwarding;

#[cop(
    name = "Style/ArgumentsForwarding",
    description = "Use arguments forwarding.",
    default_severity = "warning",
    default_enabled = true,
    options = ArgumentsForwardingOptions,
)]
impl ArgumentsForwarding {
    #[on_node(kind = "def")]
    fn check_def(&self, node: NodeId, cx: &Cx<'_>) {
        check_def_node(node, cx);
    }

    #[on_node(kind = "defs")]
    fn check_defs(&self, node: NodeId, cx: &Cx<'_>) {
        check_def_node(node, cx);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ForwardClass {
    All,
    Partial,
}

struct ForwardSite {
    classification: ForwardClass,
    call_id: NodeId,
    call_range: Range,
    #[allow(dead_code)]
    def_fwd_node: Option<NodeId>,
    first_call_arg: NodeId,
    last_call_arg: NodeId,
}

fn check_def_node(node: NodeId, cx: &Cx<'_>) {
    if cx.is_argument_forwarding(node) {
        return;
    }

    let opts = cx.options_or_default::<ArgumentsForwardingOptions>();

    let Some(args_id) = cx.def_arguments(node).get() else {
        return;
    };
    let NodeKind::Args(args_list) = *cx.kind(args_id) else {
        return;
    };
    let args = cx.list(args_list).to_vec();

    // Extra kwarg/kwoptarg disqualify forward-all.
    let has_extra_kwargs = args
        .iter()
        .any(|&id| matches!(*cx.kind(id), NodeKind::Kwarg { .. } | NodeKind::Kwoptarg { .. }));
    if has_extra_kwargs {
        return;
    }

    let restarg_id = find_arg_kind(&args, cx, |k| matches!(k, NodeKind::Restarg(_)));
    let kwrestarg_id = find_arg_kind(&args, cx, |k| matches!(k, NodeKind::Kwrestarg(_)));
    let blockarg_id = find_arg_kind(&args, cx, |k| matches!(k, NodeKind::Blockarg(_)));

    let fwd_restarg =
        forwardable_restarg(restarg_id, &opts.redundant_rest_argument_names, cx);
    let fwd_kwrestarg = forwardable_kwrestarg(
        kwrestarg_id,
        &opts.redundant_keyword_rest_argument_names,
        cx,
    );
    let fwd_blockarg =
        forwardable_blockarg(blockarg_id, &opts.redundant_block_argument_names, cx);

    if fwd_restarg.is_none() && fwd_kwrestarg.is_none() && fwd_blockarg.is_none() {
        return;
    }

    // If a restarg, kwrestarg, or blockarg EXISTS but has a meaningful name
    // (not in the redundant list), forward-all `...` cannot be used because
    // `...` would forward that argument too but anonymously.
    if restarg_id.is_some() && fwd_restarg.is_none() {
        return;
    }
    if kwrestarg_id.is_some() && fwd_kwrestarg.is_none() {
        return;
    }
    if blockarg_id.is_some() && fwd_blockarg.is_none() {
        return;
    }

    // RuboCop's `offensive_block_forwarding?`: when the def declares a `&block`
    // it must be forwarded (enforced above); when it does NOT declare a block,
    // forward-all is only offensive when `AllowOnlyRestArgument` is false —
    // because `...` also forwards a block, which would change behaviour. So a
    // def forwarding only rest and/or kwrest (no block) is skipped by default.
    if fwd_blockarg.is_none() && opts.allow_only_rest_argument {
        return;
    }

    let Some(body_id) = cx.def_body(node).get() else {
        return;
    };

    let non_forward_refs = collect_non_forwarding_lvar_refs(body_id, cx);

    let rest_name = fwd_restarg.and_then(|id| restarg_name(id, cx));
    let kwrest_name = fwd_kwrestarg.and_then(|id| kwrestarg_name(id, cx));
    let block_name = fwd_blockarg.and_then(|id| blockarg_name(id, cx));

    if rest_name.is_some_and(|n| !n.is_empty() && non_forward_refs.contains(&n)) {
        return;
    }
    if kwrest_name.is_some_and(|n| !n.is_empty() && non_forward_refs.contains(&n)) {
        return;
    }
    if block_name.is_some_and(|n| !n.is_empty() && non_forward_refs.contains(&n)) {
        return;
    }

    let call_nodes = collect_call_nodes(body_id, cx);
    if call_nodes.is_empty() {
        return;
    }

    let mut forward_sites: Vec<ForwardSite> = Vec::new();

    for &call_id in &call_nodes {
        let site = classify_call_site(
            call_id,
            rest_name,
            kwrest_name,
            block_name,
            fwd_restarg,
            fwd_kwrestarg,
            fwd_blockarg,
            cx,
        );

        match site {
            None => {}
            Some(s) if s.classification == ForwardClass::Partial => {
                return;
            }
            Some(s) => {
                forward_sites.push(s);
            }
        }
    }

    if forward_sites.is_empty() {
        return;
    }

    let def_fwd_range = find_def_forward_range(fwd_restarg, fwd_kwrestarg, fwd_blockarg, cx);

    cx.emit_offense(def_fwd_range, FORWARDING_MSG, None);
    emit_def_autocorrect(def_fwd_range, node, cx);

    for site in &forward_sites {
        cx.emit_offense(site.call_range, FORWARDING_MSG, None);
        emit_call_autocorrect(site, cx);
    }
}

fn find_def_forward_range(
    fwd_restarg: Option<NodeId>,
    fwd_kwrestarg: Option<NodeId>,
    fwd_blockarg: Option<NodeId>,
    cx: &Cx<'_>,
) -> Range {
    let first = fwd_restarg.or(fwd_kwrestarg).or(fwd_blockarg);
    let last = fwd_blockarg.or(fwd_kwrestarg).or(fwd_restarg);
    Range {
        start: first.map_or(0, |id| cx.range(id).start),
        end: last.map_or(0, |id| cx.range(id).end),
    }
}

/// Check if a def/defs node's argument list has explicit parentheses.
///
/// `cx.loc(def_node).begin()` does not work for def nodes because the `def`
/// keyword token comes before the `(` and `LocRef::begin()` only checks the
/// single token immediately after `search_from`. Instead, find the method name
/// token and check whether the immediately next token is `(`.
fn def_has_parens(def_node: NodeId, cx: &Cx<'_>) -> bool {
    let sym = match *cx.kind(def_node) {
        NodeKind::Def { name, .. } | NodeKind::Defs { name, .. } => name,
        _ => return false,
    };
    let name_str = cx.symbol_str(sym);
    let node_range = cx.range(def_node);
    let source = cx.source().as_bytes();
    let toks = cx.sorted_tokens();

    // Find the method name token within this def.
    let idx = toks.partition_point(|t| t.range.start < node_range.start);
    let name_tok = toks[idx..]
        .iter()
        .take_while(|t| t.range.start < node_range.end)
        .find(|t| {
            t.kind == SourceTokenKind::Other
                && &source[t.range.start as usize..t.range.end as usize]
                    == name_str.as_bytes()
        });

    let Some(name_tok) = name_tok else {
        return false;
    };

    // The token immediately after the name must be `(`.
    let after_idx = toks.partition_point(|t| t.range.start < name_tok.range.end);
    toks.get(after_idx)
        .is_some_and(|t| t.kind == SourceTokenKind::LeftParen)
}

fn emit_def_autocorrect(def_fwd_range: Range, def_node: NodeId, cx: &Cx<'_>) {
    if def_has_parens(def_node, cx) {
        cx.emit_edit(def_fwd_range, "...");
    } else {
        cx.emit_edit(def_fwd_range, "(...)");
    }
}

fn emit_call_autocorrect(site: &ForwardSite, cx: &Cx<'_>) {
    let replacement_range = Range {
        start: cx.range(site.first_call_arg).start,
        end: cx.range(site.last_call_arg).end,
    };
    let has_parens = cx.loc(site.call_id).begin() != Range::ZERO;
    if has_parens {
        cx.emit_edit(replacement_range, "...");
    } else {
        let source = cx.source().as_bytes();
        let name_end = cx.loc(site.call_id).name.end as usize;
        let is_bracket = source.get(name_end) == Some(&b'[');
        if is_bracket {
            cx.emit_edit(replacement_range, "...");
        } else {
            cx.emit_edit(replacement_range, "(...)");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn classify_call_site(
    call_id: NodeId,
    rest_name: Option<&str>,
    kwrest_name: Option<&str>,
    block_name: Option<&str>,
    fwd_restarg: Option<NodeId>,
    fwd_kwrestarg: Option<NodeId>,
    fwd_blockarg: Option<NodeId>,
    cx: &Cx<'_>,
) -> Option<ForwardSite> {
    let call_args = get_call_args(call_id, cx);

    let mut found_rest: Option<NodeId> = None;
    let mut found_kwrest_hash: Option<NodeId> = None;
    let mut found_block: Option<NodeId> = None;

    for &arg_id in &call_args {
        match *cx.kind(arg_id) {
            NodeKind::Splat(inner) => {
                if let Some(lvar_id) = inner.get()
                    && matches!(*cx.kind(lvar_id), NodeKind::Lvar(_))
                {
                    let name = lvar_name(lvar_id, cx);
                    if rest_name == Some(name) {
                        found_rest = Some(arg_id);
                    }
                }
            }
            NodeKind::Hash(list) => {
                let hash_children = cx.list(list);
                if hash_children.len() == 1 {
                    let child = hash_children[0];
                    if let NodeKind::Kwsplat(inner) = *cx.kind(child)
                        && let Some(lvar_id) = inner.get()
                        && matches!(*cx.kind(lvar_id), NodeKind::Lvar(_))
                    {
                        let name = lvar_name(lvar_id, cx);
                        if kwrest_name == Some(name) {
                            found_kwrest_hash = Some(arg_id);
                        }
                    }
                }
            }
            NodeKind::BlockPass(inner) => {
                if let Some(lvar_id) = inner.get()
                    && matches!(*cx.kind(lvar_id), NodeKind::Lvar(_))
                {
                    let name = lvar_name(lvar_id, cx);
                    if block_name == Some(name) {
                        found_block = Some(arg_id);
                    }
                }
            }
            _ => {}
        }
    }

    let needs_rest = fwd_restarg.is_some() && rest_name.is_some_and(|n| !n.is_empty());
    let needs_kwrest = fwd_kwrestarg.is_some() && kwrest_name.is_some_and(|n| !n.is_empty());
    let needs_block = fwd_blockarg.is_some() && block_name.is_some_and(|n| !n.is_empty());

    let has_rest = found_rest.is_some();
    let has_kwrest = found_kwrest_hash.is_some();
    let has_block = found_block.is_some();

    if !has_rest && !has_kwrest && !has_block {
        return None;
    }

    if (needs_rest && !has_rest) || (needs_kwrest && !has_kwrest) || (needs_block && !has_block) {
        return Some(ForwardSite {
            classification: ForwardClass::Partial,
            call_id,
            call_range: Range::ZERO,
            def_fwd_node: None,
            first_call_arg: call_id,
            last_call_arg: call_id,
        });
    }

    let mut forwarded: Vec<NodeId> = Vec::new();
    if let Some(id) = found_rest {
        forwarded.push(id);
    }
    if let Some(id) = found_kwrest_hash {
        forwarded.push(id);
    }
    if let Some(id) = found_block {
        forwarded.push(id);
    }

    if forwarded.is_empty() {
        return None;
    }

    let first_call_arg = forwarded[0];
    let last_call_arg = *forwarded.last().unwrap();

    let call_range = Range {
        start: cx.range(first_call_arg).start,
        end: cx.range(last_call_arg).end,
    };

    Some(ForwardSite {
        classification: ForwardClass::All,
        call_id,
        call_range,
        def_fwd_node: fwd_restarg.or(fwd_kwrestarg).or(fwd_blockarg),
        first_call_arg,
        last_call_arg,
    })
}

fn get_call_args(call_id: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    match *cx.kind(call_id) {
        NodeKind::Send { args, .. } | NodeKind::Csend { args, .. } => cx.list(args).to_vec(),
        NodeKind::Super(list) => cx.list(list).to_vec(),
        NodeKind::Yield(list) => cx.list(list).to_vec(),
        _ => vec![],
    }
}

fn is_call_node(id: NodeId, cx: &Cx<'_>) -> bool {
    matches!(
        *cx.kind(id),
        NodeKind::Send { .. }
            | NodeKind::Csend { .. }
            | NodeKind::Super(_)
            | NodeKind::Yield(_)
    )
}

fn collect_call_nodes(body_id: NodeId, cx: &Cx<'_>) -> Vec<NodeId> {
    // Include body_id itself (when the body is a single call expression).
    let mut result: Vec<NodeId> = Vec::new();
    if is_call_node(body_id, cx) {
        result.push(body_id);
    }
    let mut descendants: Vec<NodeId> = cx
        .descendants(body_id)
        .into_iter()
        .filter(|&id| is_call_node(id, cx))
        .collect();
    result.append(&mut descendants);
    result
}

fn collect_non_forwarding_lvar_refs<'a>(body_id: NodeId, cx: &Cx<'a>) -> Vec<&'a str> {
    let mut result = Vec::new();
    collect_non_forwarding_recursive(body_id, false, cx, &mut result);
    result.dedup();
    result
}

fn collect_non_forwarding_recursive<'a>(
    id: NodeId,
    in_fwd_ctx: bool,
    cx: &Cx<'a>,
    out: &mut Vec<&'a str>,
) {
    match *cx.kind(id) {
        NodeKind::Splat(_) | NodeKind::Kwsplat(_) | NodeKind::BlockPass(_) => {
            for &child in &cx.children(id) {
                collect_non_forwarding_recursive(child, true, cx, out);
            }
        }
        NodeKind::Lvar(sym) => {
            if !in_fwd_ctx {
                out.push(cx.symbol_str(sym));
            }
        }
        NodeKind::Lvasgn { name: sym, .. } => {
            if !in_fwd_ctx {
                out.push(cx.symbol_str(sym));
            }
            for &child in &cx.children(id) {
                collect_non_forwarding_recursive(child, in_fwd_ctx, cx, out);
            }
        }
        _ => {
            for &child in &cx.children(id) {
                collect_non_forwarding_recursive(child, in_fwd_ctx, cx, out);
            }
        }
    }
}

fn find_arg_kind(args: &[NodeId], cx: &Cx<'_>, pred: impl Fn(&NodeKind) -> bool) -> Option<NodeId> {
    args.iter().copied().find(|&id| pred(cx.kind(id)))
}

/// True when `name` is anonymous (empty — the bare `*`/`**`/`&`) or appears in
/// the configured redundant-name list. Mirrors RuboCop's `redundant_named_arg`,
/// whose candidate list is `[keyword+name for name in config] << keyword`, so
/// the bare keyword (anonymous forwarding) always counts as redundant.
fn is_redundant_name(name: &str, redundant_names: &[String]) -> bool {
    name.is_empty() || redundant_names.iter().any(|n| n == name)
}

fn forwardable_restarg(
    id: Option<NodeId>,
    redundant_names: &[String],
    cx: &Cx<'_>,
) -> Option<NodeId> {
    let id = id?;
    let NodeKind::Restarg(sym) = *cx.kind(id) else {
        return None;
    };
    is_redundant_name(cx.symbol_str(sym), redundant_names).then_some(id)
}

fn forwardable_kwrestarg(
    id: Option<NodeId>,
    redundant_names: &[String],
    cx: &Cx<'_>,
) -> Option<NodeId> {
    let id = id?;
    let NodeKind::Kwrestarg(sym) = *cx.kind(id) else {
        return None;
    };
    is_redundant_name(cx.symbol_str(sym), redundant_names).then_some(id)
}

fn forwardable_blockarg(
    id: Option<NodeId>,
    redundant_names: &[String],
    cx: &Cx<'_>,
) -> Option<NodeId> {
    let id = id?;
    let NodeKind::Blockarg(sym) = *cx.kind(id) else {
        return None;
    };
    is_redundant_name(cx.symbol_str(sym), redundant_names).then_some(id)
}

fn restarg_name<'a>(id: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    if let NodeKind::Restarg(sym) = *cx.kind(id) {
        Some(cx.symbol_str(sym))
    } else {
        None
    }
}

fn kwrestarg_name<'a>(id: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    if let NodeKind::Kwrestarg(sym) = *cx.kind(id) {
        Some(cx.symbol_str(sym))
    } else {
        None
    }
}

fn blockarg_name<'a>(id: NodeId, cx: &Cx<'a>) -> Option<&'a str> {
    if let NodeKind::Blockarg(sym) = *cx.kind(id) {
        Some(cx.symbol_str(sym))
    } else {
        None
    }
}

fn lvar_name<'a>(id: NodeId, cx: &Cx<'a>) -> &'a str {
    if let NodeKind::Lvar(sym) = *cx.kind(id) {
        cx.symbol_str(sym)
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::{ArgumentsForwarding, ArgumentsForwardingOptions};
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_restarg_and_block_arg() {
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*args, &block)
                        ^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(*args, &block)
                      ^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                end
            "#},
            indoc! {r#"
                def foo(...)
                  bar(...)
                end
            "#},
        );
    }

    #[test]
    fn flags_restarg_kwrestarg_and_block_arg() {
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*args, **kwargs, &block)
                        ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(*args, **kwargs, &block)
                      ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                end
            "#},
            indoc! {r#"
                def foo(...)
                  bar(...)
                end
            "#},
        );
    }

    #[test]
    fn flags_with_extra_non_forwarding_call() {
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*args, **kwargs, &block)
                        ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(*args, **kwargs, &block)
                      ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  baz(1, 2, 3)
                end
            "#},
            indoc! {r#"
                def foo(...)
                  bar(...)
                  baz(1, 2, 3)
                end
            "#},
        );
    }

    #[test]
    fn flags_with_two_forwarding_calls() {
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*args, **kwargs, &block)
                        ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(*args, **kwargs, &block)
                      ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  baz(*args, **kwargs, &block)
                      ^^^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                end
            "#},
            indoc! {r#"
                def foo(...)
                  bar(...)
                  baz(...)
                end
            "#},
        );
    }

    #[test]
    fn flags_with_redundant_opts_name() {
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*args, **opts, &block)
                        ^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(*args, **opts, &block)
                      ^^^^^^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                end
            "#},
            indoc! {r#"
                def foo(...)
                  bar(...)
                end
            "#},
        );
    }

    #[test]
    fn flags_with_redundant_blk_name() {
        test::<ArgumentsForwarding>().expect_correction(
            indoc! {r#"
                def foo(*args, &blk)
                        ^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                  bar(*args, &blk)
                      ^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                end
            "#},
            indoc! {r#"
                def foo(...)
                  bar(...)
                end
            "#},
        );
    }

    #[test]
    fn accepts_already_forwarding() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(...)
              bar(...)
            end
        "#});
    }

    #[test]
    fn accepts_args_used_outside_forwarding() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, **kwargs, &block)
              args.do_something
              bar(*args, **kwargs, &block)
            end
        "#});
    }

    #[test]
    fn accepts_args_reassigned() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, **kwargs, &block)
              args = new_args
              bar(*args, **kwargs, &block)
            end
        "#});
    }

    #[test]
    fn accepts_empty_body() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, &block)
            end
        "#});
    }

    #[test]
    fn accepts_meaningful_rest_name() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*meaningful_args, &block)
              bar(*meaningful_args, &block)
            end
        "#});
    }

    #[test]
    fn accepts_not_always_forwarding_block() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, &block)
              bar(*args, &block)
              baz(*args)
            end
        "#});
    }

    #[test]
    fn accepts_not_always_forwarding_all_three() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, **kwargs, &block)
              bar(*args, **kwargs, &block)
              bar(*args, &block)
              bar(**kwargs, &block)
            end
        "#});
    }

    #[test]
    fn accepts_block_forwarded_to_separate_call() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, &block)
              bar(*args).baz(&block)
            end
        "#});
    }

    #[test]
    fn accepts_kwargs_with_additional_kwarg() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(first:, **kwargs, &block)
              forwarded(**kwargs, &block)
            end
        "#});
    }

    #[test]
    fn accepts_meaningful_kwrest_name() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(**my_special_kwargs, &block)
              bar(**my_special_kwargs, &block)
            end
        "#});
    }

    #[test]
    fn accepts_meaningful_block_name() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, &my_callback)
              bar(*args, &my_callback)
            end
        "#});
    }

    #[test]
    fn accepts_only_rest_arg_by_default() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args)
              bar(*args)
            end
        "#});
    }

    #[test]
    fn accepts_only_kwrest_arg_by_default() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(**kwargs)
              bar(**kwargs)
            end
        "#});
    }

    #[test]
    fn accepts_args_forwarded_to_separate_receiver_methods() {
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, **kwargs, &block)
              bar(first(*args), second(**kwargs), third(&block))
            end
        "#});
    }

    // ── AllowOnlyRestArgument ───────────────────────────────────────────────

    #[test]
    fn options_defaults_match_rubocop() {
        let d = ArgumentsForwardingOptions::default();
        assert!(d.allow_only_rest_argument);
        assert_eq!(d.redundant_rest_argument_names, ["args", "arguments"]);
        assert_eq!(
            d.redundant_keyword_rest_argument_names,
            ["kwargs", "options", "opts"]
        );
        assert_eq!(d.redundant_block_argument_names, ["blk", "block", "proc"]);
    }

    #[test]
    fn accepts_only_rest_and_kwrest_without_block_by_default() {
        // RuboCop's `offensive_block_forwarding?`: with no `&block` declared and
        // `AllowOnlyRestArgument: true` (default), forward-all is NOT offensive,
        // because `...` would also forward a block and change behaviour.
        test::<ArgumentsForwarding>().expect_no_offenses(indoc! {r#"
            def foo(*args, **kwargs)
              bar(*args, **kwargs)
            end
        "#});
    }

    #[test]
    fn flags_only_rest_arg_when_allow_only_rest_argument_false() {
        test::<ArgumentsForwarding>()
            .with_options(&ArgumentsForwardingOptions {
                allow_only_rest_argument: false,
                ..Default::default()
            })
            .expect_correction(
                indoc! {r#"
                    def foo(*args)
                            ^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(*args)
                          ^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }

    #[test]
    fn flags_only_kwrest_arg_when_allow_only_rest_argument_false() {
        test::<ArgumentsForwarding>()
            .with_options(&ArgumentsForwardingOptions {
                allow_only_rest_argument: false,
                ..Default::default()
            })
            .expect_correction(
                indoc! {r#"
                    def foo(**kwargs)
                            ^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(**kwargs)
                          ^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }

    #[test]
    fn flags_rest_and_kwrest_without_block_when_allow_only_rest_argument_false() {
        test::<ArgumentsForwarding>()
            .with_options(&ArgumentsForwardingOptions {
                allow_only_rest_argument: false,
                ..Default::default()
            })
            .expect_correction(
                indoc! {r#"
                    def foo(*args, **kwargs)
                            ^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(*args, **kwargs)
                          ^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }

    // ── Redundant*ArgumentNames ─────────────────────────────────────────────

    #[test]
    fn flags_custom_redundant_rest_name() {
        test::<ArgumentsForwarding>()
            .with_options(&ArgumentsForwardingOptions {
                redundant_rest_argument_names: vec!["sploosh".to_string()],
                ..Default::default()
            })
            .expect_correction(
                indoc! {r#"
                    def foo(*sploosh, &block)
                            ^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(*sploosh, &block)
                          ^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }

    #[test]
    fn accepts_default_rest_name_when_not_in_custom_list() {
        // With a custom `RedundantRestArgumentNames` that omits `args`, the
        // default `*args` name is now meaningful and forward-all is rejected.
        test::<ArgumentsForwarding>()
            .with_options(&ArgumentsForwardingOptions {
                redundant_rest_argument_names: vec!["sploosh".to_string()],
                ..Default::default()
            })
            .expect_no_offenses(indoc! {r#"
                def foo(*args, &block)
                  bar(*args, &block)
                end
            "#});
    }

    #[test]
    fn flags_custom_redundant_block_name() {
        test::<ArgumentsForwarding>()
            .with_options(&ArgumentsForwardingOptions {
                redundant_block_argument_names: vec!["callback".to_string()],
                ..Default::default()
            })
            .expect_correction(
                indoc! {r#"
                    def foo(*args, &callback)
                            ^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                      bar(*args, &callback)
                          ^^^^^^^^^^^^^^^^ Use shorthand syntax `...` for arguments forwarding.
                    end
                "#},
                indoc! {r#"
                    def foo(...)
                      bar(...)
                    end
                "#},
            );
    }
}

murphy_plugin_api::submit_cop!(ArgumentsForwarding);
