//! `Rails/IndexWith` — prefer `index_with` over manual hash building.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/IndexWith
//! upstream_version_checked: 2.35.0
//! version_added: "2.5"
//! safe: false
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 IndexMethod mixin: `each_with_object({})`
//!   with `h[el] = val`, `to_h { [el, val] }` (Block/Numblock/Itblock),
//!   `map/collect { [el, val] }.to_h`, and `Hash[map/collect { [el, val] }]`
//!   (global Hash only). Noop (`[el, el]` / `h[el] = el`) never flags;
//!   `each_with_object` skips when the value references the memo (`h`)
//!   to avoid wrong rewrites (`h[el] = h.count`). Numblock requires
//!   `_1`-only (`max_n == 1`); Itblock uses `it` (`[it, y]` still flags
//!   per upstream `$_`). Minimum TargetRailsVersion 6.0 is gated (unset
//!   means newest). Ruby 2.6 gating for `to_h` blocks is not gated
//!   (Murphy assumes modern Ruby, like IndexBy). Safe-navigation (`&.`) is
//!   preserved.
//! ```
//!
//! Looks for uses of `each_with_object({})`, `map { ... }.to_h`, and
//! `Hash[map { ... }]` that can be replaced with `index_with`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, Symbol, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct IndexWith;

#[cop(
    name = "Rails/IndexWith",
    description = "Prefer `index_with` over `each_with_object`, `to_h`, or `map`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl IndexWith {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.rails_version_at_least(6, 0) {
            return;
        }
        if let Some(c) = match_each_with_object_block(cx, node) {
            emit_each_with_object(cx, node, c);
            return;
        }
        if let Some(c) = match_to_h_block(cx, node) {
            emit_to_h_block(cx, node, c);
        }
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.rails_version_at_least(6, 0) {
            return;
        }
        if let Some(c) = match_to_h_numblock(cx, node) {
            emit_to_h_block(cx, node, c);
        }
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.rails_version_at_least(6, 0) {
            return;
        }
        if let Some(c) = match_to_h_itblock(cx, node) {
            emit_to_h_block(cx, node, c);
        }
    }

    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.rails_version_at_least(6, 0) {
            return;
        }
        if let Some(c) = match_map_to_h(cx, node) {
            emit_map_to_h(cx, node, c);
            return;
        }
        if let Some(c) = match_hash_brackets(cx, node) {
            emit_hash_brackets(cx, node, c);
        }
    }

    #[on_node(kind = "csend")]
    fn check_csend(&self, node: NodeId, cx: &Cx<'_>) {
        if !cx.rails_version_at_least(6, 0) {
            return;
        }
        if let Some(c) = match_map_to_h(cx, node) {
            emit_map_to_h(cx, node, c);
        }
    }
}

// ---------- captures ----------

struct EachWithObjectCap {
    call: NodeId,
    el: String,
    el_sym: Symbol,
    memo_sym: Symbol,
    value: NodeId,
}

struct ToHBlockCap {
    call: NodeId,
    el_name: String,
    value: NodeId,
    args_node: Option<NodeId>,
}

struct MapToHCap {
    inner_block: NodeId,
    inner_call: NodeId,
    el_name: String,
    value: NodeId,
    inner_args_node: Option<NodeId>,
    inner_body: NodeId,
    is_numblock: bool,
    is_itblock: bool,
}

struct HashBracketsCap {
    inner_block: NodeId,
    inner_call: NodeId,
    el_name: String,
    value: NodeId,
    inner_args_node: Option<NodeId>,
    inner_body: NodeId,
    is_numblock: bool,
    is_itblock: bool,
}

// ---------- matchers ----------

fn call_method(cx: &Cx<'_>, id: NodeId) -> Option<String> {
    match *cx.kind(id) {
        NodeKind::Send { method, .. } | NodeKind::Csend { method, .. } => {
            Some(cx.symbol_str(method).to_owned())
        }
        _ => None,
    }
}

fn call_args(cx: &Cx<'_>, id: NodeId) -> Vec<NodeId> {
    match *cx.kind(id) {
        NodeKind::Send { args, .. } => cx.list(args).to_vec(),
        NodeKind::Csend { args, .. } => cx.list(args).to_vec(),
        _ => Vec::new(),
    }
}

fn block_parts(cx: &Cx<'_>, node: NodeId) -> Option<(NodeId, NodeId, Option<NodeId>)> {
    match *cx.kind(node) {
        NodeKind::Block { call, args, body } => Some((call, args, body.get())),
        _ => None,
    }
}

fn arg_names(cx: &Cx<'_>, args: NodeId) -> Vec<Symbol> {
    let NodeKind::Args(list) = *cx.kind(args) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for &p in cx.list(list) {
        if let NodeKind::Arg(s) = *cx.kind(p) {
            out.push(s);
        } else {
            return Vec::new();
        }
    }
    out
}

fn lvar_sym(cx: &Cx<'_>, id: NodeId) -> Option<Symbol> {
    if let NodeKind::Lvar(s) = *cx.kind(id) {
        Some(s)
    } else {
        None
    }
}

fn is_lvar_named(cx: &Cx<'_>, id: NodeId, sym: Symbol) -> bool {
    lvar_sym(cx, id) == Some(sym)
}

fn contains_lvar(cx: &Cx<'_>, root: NodeId, sym: Symbol) -> bool {
    if is_lvar_named(cx, root, sym) {
        return true;
    }
    for ch in cx.children(root) {
        if contains_lvar(cx, ch, sym) {
            return true;
        }
    }
    false
}

fn match_each_with_object_block(cx: &Cx<'_>, node: NodeId) -> Option<EachWithObjectCap> {
    let (call, args_node, body_opt) = block_parts(cx, node)?;
    let body = body_opt?;
    if call_method(cx, call).as_deref() != Some("each_with_object") {
        return None;
    }
    let cargs = call_args(cx, call);
    if cargs.len() != 1 {
        return None;
    }
    if !matches!(*cx.kind(cargs[0]), NodeKind::Hash(_)) {
        return None;
    }
    let anames = arg_names(cx, args_node);
    if anames.len() != 2 {
        return None;
    }
    let el_sym = anames[0];
    let memo_sym = anames[1];
    let el = cx.symbol_str(el_sym).to_owned();
    // Body: `(call (lvar memo) :[]= (lvar el) $!memo)` — key is plain el.
    let NodeKind::Send { receiver, method, .. } = *cx.kind(body) else {
        return None;
    };
    if cx.symbol_str(method) != "[]=" {
        return None;
    }
    let recv = receiver.get()?;
    if !is_lvar_named(cx, recv, memo_sym) {
        return None;
    }
    let bargs = cx.call_arguments(body);
    if bargs.len() != 2 {
        return None;
    }
    let key = bargs[0];
    let value = bargs[1];
    // Key must be plain `el` (keys-transformed `h[el.to_sym] = ...` is IndexBy).
    if !is_lvar_named(cx, key, el_sym) {
        return None;
    }
    // Noop: `h[el] = el`.
    if is_lvar_named(cx, value, el_sym) {
        return None;
    }
    // Value must not reference the memo (`h[el] = h.count` is not rewritable).
    if contains_lvar(cx, value, memo_sym) {
        return None;
    }
    Some(EachWithObjectCap {
        call,
        el,
        el_sym,
        memo_sym,
        value,
    })
}

fn array_value(cx: &Cx<'_>, body: NodeId, el_sym: Symbol) -> Option<NodeId> {
    let NodeKind::Array(list) = *cx.kind(body) else {
        return None;
    };
    let els = cx.list(list);
    if els.len() != 2 {
        return None;
    }
    let key = els[0];
    let value = els[1];
    if !is_lvar_named(cx, key, el_sym) {
        return None;
    }
    if is_lvar_named(cx, value, el_sym) {
        return None;
    }
    Some(value)
}

fn match_to_h_block(cx: &Cx<'_>, node: NodeId) -> Option<ToHBlockCap> {
    let (call, args_node, body_opt) = block_parts(cx, node)?;
    let body = body_opt?;
    if call_method(cx, call).as_deref() != Some("to_h") {
        return None;
    }
    if !call_args(cx, call).is_empty() {
        return None;
    }
    let anames = arg_names(cx, args_node);
    if anames.len() != 1 {
        return None;
    }
    let el_sym = anames[0];
    let el_name = cx.symbol_str(el_sym).to_owned();
    let value = array_value(cx, body, el_sym)?;
    Some(ToHBlockCap {
        call,
        el_name,
        value,
        args_node: Some(args_node),
    })
}

fn match_to_h_numblock(cx: &Cx<'_>, node: NodeId) -> Option<ToHBlockCap> {
    let NodeKind::Numblock { send, max_n, body } = *cx.kind(node) else {
        return None;
    };
    if call_method(cx, send).as_deref() != Some("to_h") {
        return None;
    }
    if !call_args(cx, send).is_empty() {
        return None;
    }
    if max_n != 1 {
        return None;
    }
    let body = body.get()?;
    let NodeKind::Array(list) = *cx.kind(body) else {
        return None;
    };
    let els = cx.list(list);
    if els.len() != 2 {
        return None;
    }
    // Key must be plain `_1`; value must not be bare `_1` (noop `[_1, _1]`).
    if !is_n1(cx, els[0]) || is_n1(cx, els[1]) {
        return None;
    }
    Some(ToHBlockCap {
        call: send,
        el_name: "_1".to_owned(),
        value: els[1],
        args_node: None,
    })
}

fn is_n1(cx: &Cx<'_>, id: NodeId) -> bool {
    if let NodeKind::Lvar(s) = *cx.kind(id) {
        cx.symbol_str(s) == "_1"
    } else {
        false
    }
}

fn is_it(cx: &Cx<'_>, id: NodeId) -> bool {
    if let NodeKind::Send { receiver, method, args } = *cx.kind(id) {
        return receiver.get().is_none()
            && cx.symbol_str(method) == "it"
            && cx.list(args).is_empty();
    }
    false
}

fn match_to_h_itblock(cx: &Cx<'_>, node: NodeId) -> Option<ToHBlockCap> {
    let NodeKind::Itblock { send, body } = *cx.kind(node) else {
        return None;
    };
    if call_method(cx, send).as_deref() != Some("to_h") {
        return None;
    }
    if !call_args(cx, send).is_empty() {
        return None;
    }
    let body = body.get()?;
    let NodeKind::Array(list) = *cx.kind(body) else {
        return None;
    };
    let els = cx.list(list);
    if els.len() != 2 {
        return None;
    }
    if !is_it(cx, els[0]) || is_it(cx, els[1]) {
        return None;
    }
    Some(ToHBlockCap {
        call: send,
        el_name: "it".to_owned(),
        value: els[1],
        args_node: None,
    })
}

type MapBlockMatch = (NodeId, String, NodeId, Option<NodeId>, NodeId, bool, bool);

fn inner_map_block(cx: &Cx<'_>, block: NodeId) -> Option<MapBlockMatch> {
    // Returns (call, el_name, value, args_node, body, is_numblock, is_itblock)
    match *cx.kind(block) {
        NodeKind::Block { call, args, body } => {
            let m = call_method(cx, call)?;
            if m != "map" && m != "collect" {
                return None;
            }
            let anames = arg_names(cx, args);
            if anames.len() != 1 {
                return None;
            }
            let el_sym = anames[0];
            let el_name = cx.symbol_str(el_sym).to_owned();
            let body_id = body.get()?;
            let value = array_value(cx, body_id, el_sym)?;
            Some((call, el_name, value, Some(args), body_id, false, false))
        }
        NodeKind::Numblock { send, max_n, body } => {
            let m = call_method(cx, send)?;
            if m != "map" && m != "collect" {
                return None;
            }
            if max_n != 1 {
                return None;
            }
            let body_id = body.get()?;
            let NodeKind::Array(list) = *cx.kind(body_id) else {
                return None;
            };
            let els = cx.list(list);
            if els.len() != 2 {
                return None;
            }
            if !is_n1(cx, els[0]) || is_n1(cx, els[1]) {
                return None;
            }
            Some((send, "_1".to_owned(), els[1], None, body_id, true, false))
        }
        NodeKind::Itblock { send, body } => {
            let m = call_method(cx, send)?;
            if m != "map" && m != "collect" {
                return None;
            }
            let body_id = body.get()?;
            let NodeKind::Array(list) = *cx.kind(body_id) else {
                return None;
            };
            let els = cx.list(list);
            if els.len() != 2 {
                return None;
            }
            // Upstream `$_` for the value: `[it, y]` still flags, only
            // `[it, it]` (noop) is excluded. Key must be plain `it`;
            // a non-`it` key (`[y, it.to_sym]`) never matches.
            if !is_it(cx, els[0]) || is_it(cx, els[1]) {
                return None;
            }
            Some((send, "it".to_owned(), els[1], None, body_id, false, true))
        }
        _ => None,
    }
}

fn match_map_to_h(cx: &Cx<'_>, node: NodeId) -> Option<MapToHCap> {
    let m = call_method(cx, node)?;
    if m != "to_h" {
        return None;
    }
    if !call_args(cx, node).is_empty() {
        return None;
    }
    let recv = match *cx.kind(node) {
        NodeKind::Send { receiver, .. } => receiver.get()?,
        NodeKind::Csend { receiver, .. } => receiver,
        _ => return None,
    };
    let (inner_call, el_name, value, args_node, body, is_num, is_it) =
        inner_map_block(cx, recv)?;
    Some(MapToHCap {
        inner_block: recv,
        inner_call,
        el_name,
        value,
        inner_args_node: args_node,
        inner_body: body,
        is_numblock: is_num,
        is_itblock: is_it,
    })
}

fn match_hash_brackets(cx: &Cx<'_>, node: NodeId) -> Option<HashBracketsCap> {
    let NodeKind::Send { receiver, method, .. } = *cx.kind(node) else {
        return None;
    };
    if cx.symbol_str(method) != "[]" {
        return None;
    }
    let recv = receiver.get()?;
    let NodeKind::Const { scope, name } = *cx.kind(recv) else {
        return None;
    };
    if cx.symbol_str(name) != "Hash" {
        return None;
    }
    match scope.get() {
        None => {}
        Some(s) if matches!(*cx.kind(s), NodeKind::Cbase) => {}
        _ => return None,
    }
    let args = cx.call_arguments(node);
    if args.len() != 1 {
        return None;
    }
    let inner = args[0];
    let (inner_call, el_name, value, args_node, body, is_num, is_it) =
        inner_map_block(cx, inner)?;
    Some(HashBracketsCap {
        inner_block: inner,
        inner_call,
        el_name,
        value,
        inner_args_node: args_node,
        inner_body: body,
        is_numblock: is_num,
        is_itblock: is_it,
    })
}

// ---------- emits ----------

fn first_line_range(cx: &Cx<'_>, whole: Range) -> Range {
    let src = cx.raw_source(whole);
    if let Some(pos) = src.find('\n') {
        Range {
            start: whole.start,
            end: whole.start + pos as u32,
        }
    } else {
        whole
    }
}

fn block_receiver_source(cx: &Cx<'_>, call: NodeId) -> Option<(String, String)> {
    let (recv_opt, is_csend) = match *cx.kind(call) {
        NodeKind::Send { receiver, .. } => (receiver.get(), false),
        NodeKind::Csend { receiver, .. } => (Some(receiver), true),
        _ => return None,
    };
    let recv = recv_opt?;
    let op = if is_csend { "&." } else { "." };
    Some((cx.raw_source(cx.range(recv)).to_owned(), op.to_owned()))
}

fn base_indent(cx: &Cx<'_>, pos: u32) -> String {
    let src = cx.source();
    let upto = pos as usize;
    let line_start = src[..upto].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_head = &src[line_start..upto];
    line_head
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

fn body_indent(cx: &Cx<'_>, body: NodeId) -> String {
    base_indent(cx, cx.range(body).start)
}

fn is_do_block(cx: &Cx<'_>, whole: Range) -> bool {
    let src = cx.raw_source(whole);
    src.contains(" do ") || src.contains(" do|") || src.contains("\ndo ") || src.contains("\ndo|")
}

fn emit_each_with_object(cx: &Cx<'_>, block: NodeId, cap: EachWithObjectCap) {
    let whole = cx.range(block);
    cx.emit_offense(
        first_line_range(cx, whole),
        "Prefer `index_with` over `each_with_object`.",
        None,
    );
    let Some((recv_src, op)) = block_receiver_source(cx, cap.call) else {
        return;
    };
    let value_src = cx.raw_source(cx.range(cap.value)).to_owned();
    // Wrap brace-less hash values (`[el, value: el]` → `{ value: el }`).
    let value_src = maybe_brace_hash(cx, cap.value, value_src);
    let replacement = if is_do_block(cx, whole) {
        let base = base_indent(cx, whole.start);
        let bind = body_indent(cx, cap.value);
        format!("{recv_src}{op}index_with do |{}|\n{bind}{value_src}\n{base}end", cap.el)
    } else {
        format!("{recv_src}{op}index_with {{ |{}| {value_src} }}", cap.el)
    };
    cx.emit_edit(whole, &replacement);
    let _ = cap.el_sym;
    let _ = cap.memo_sym;
}

fn maybe_brace_hash(cx: &Cx<'_>, value: NodeId, src: String) -> String {
    if matches!(*cx.kind(value), NodeKind::Hash(_)) {
        let raw = cx.raw_source(cx.range(value));
        if !(raw.trim_start().starts_with('{') || raw.trim_start().starts_with('}')) {
            // Heuristic: bare `k: v` hash without braces needs wrapping.
            // A Hash node whose source doesn't start with `{` is bare.
            return format!("{{ {src} }}");
        }
    }
    src
}

fn emit_to_h_block(cx: &Cx<'_>, block: NodeId, cap: ToHBlockCap) {
    let whole = cx.range(block);
    cx.emit_offense(
        first_line_range(cx, whole),
        "Prefer `index_with` over `to_h { ... }`.",
        None,
    );
    let Some((recv_src, op)) = block_receiver_source(cx, cap.call) else {
        return;
    };
    let value_src = maybe_brace_hash(cx, cap.value, cx.raw_source(cx.range(cap.value)).to_owned());
    let replacement = if cap.args_node.is_none() {
        if is_do_block(cx, whole) {
            let base = base_indent(cx, whole.start);
            let bind = body_indent(cx, cap.value);
            format!("{recv_src}{op}index_with do\n{bind}{value_src}\n{base}end")
        } else {
            format!("{recv_src}{op}index_with {{ {value_src} }}")
        }
    } else if is_do_block(cx, whole) {
        let base = base_indent(cx, whole.start);
        let bind = body_indent(cx, cap.value);
        format!("{recv_src}{op}index_with do |{}|\n{bind}{value_src}\n{base}end", cap.el_name)
    } else {
        format!("{recv_src}{op}index_with {{ |{}| {value_src} }}", cap.el_name)
    };
    cx.emit_edit(whole, &replacement);
}

fn emit_map_to_h(cx: &Cx<'_>, outer: NodeId, cap: MapToHCap) {
    let whole = cx.range(outer);
    cx.emit_offense(
        first_line_range(cx, whole),
        "Prefer `index_with` over `map { ... }.to_h`.",
        None,
    );
    let Some((recv_src, op)) = block_receiver_source(cx, cap.inner_call) else {
        return;
    };
    let value_src = maybe_brace_hash(cx, cap.value, cx.raw_source(cx.range(cap.value)).to_owned());
    let inner_whole = cx.range(cap.inner_block);
    let replacement = if cap.is_numblock || cap.is_itblock {
        if is_do_block(cx, inner_whole) {
            let base = base_indent(cx, whole.start);
            let bind = body_indent(cx, cap.value);
            format!("{recv_src}{op}index_with do\n{bind}{value_src}\n{base}end")
        } else {
            format!("{recv_src}{op}index_with {{ {value_src} }}")
        }
    } else if is_do_block(cx, inner_whole) {
        let base = base_indent(cx, whole.start);
        let bind = body_indent(cx, cap.value);
        format!("{recv_src}{op}index_with do |{}|\n{bind}{value_src}\n{base}end", cap.el_name)
    } else {
        format!("{recv_src}{op}index_with {{ |{}| {value_src} }}", cap.el_name)
    };
    cx.emit_edit(whole, &replacement);
    let _ = cap.inner_args_node;
    let _ = cap.inner_body;
}

fn emit_hash_brackets(cx: &Cx<'_>, outer: NodeId, cap: HashBracketsCap) {
    let whole = cx.range(outer);
    cx.emit_offense(
        first_line_range(cx, whole),
        "Prefer `index_with` over `Hash[map { ... }]`.",
        None,
    );
    let Some((recv_src, op)) = block_receiver_source(cx, cap.inner_call) else {
        return;
    };
    let value_src = maybe_brace_hash(cx, cap.value, cx.raw_source(cx.range(cap.value)).to_owned());
    let inner_whole = cx.range(cap.inner_block);
    let replacement = if cap.is_numblock || cap.is_itblock {
        if is_do_block(cx, inner_whole) {
            let base = base_indent(cx, whole.start);
            let bind = body_indent(cx, cap.value);
            format!("{recv_src}{op}index_with do\n{bind}{value_src}\n{base}end")
        } else {
            format!("{recv_src}{op}index_with {{ {value_src} }}")
        }
    } else if is_do_block(cx, inner_whole) {
        let base = base_indent(cx, whole.start);
        let bind = body_indent(cx, cap.value);
        format!("{recv_src}{op}index_with do |{}|\n{bind}{value_src}\n{base}end", cap.el_name)
    } else {
        format!("{recv_src}{op}index_with {{ |{}| {value_src} }}", cap.el_name)
    };
    cx.emit_edit(whole, &replacement);
    let _ = cap.inner_args_node;
    let _ = cap.inner_body;
}

#[cfg(test)]
mod tests {
    use super::IndexWith;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_each_with_object() {
        test::<IndexWith>().expect_offense(indoc! {r#"
            x.each_with_object({}) { |el, h| h[el] = foo(el) }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `each_with_object`.
        "#});
    }

    #[test]
    fn corrects_each_with_object() {
        test::<IndexWith>().expect_correction(
            indoc! {r#"
                x.each_with_object({}) { |el, h| h[el] = foo(el) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `each_with_object`.
            "#},
            "x.index_with { |el| foo(el) }\n",
        );
    }

    #[test]
    fn corrects_each_with_object_multiline() {
        test::<IndexWith>().expect_correction(
            indoc! {r#"
                x.each_with_object({}) do |el, memo|
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `each_with_object`.
                  memo[el] = el.to_sym
                end
            "#},
            "x.index_with do |el|\n  el.to_sym\nend\n",
        );
    }

    #[test]
    fn corrects_safe_navigation_each_with_object() {
        test::<IndexWith>().expect_correction(
            indoc! {r#"
                x&.each_with_object({}) { |el, h| h[el] = foo(el) }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `each_with_object`.
            "#},
            "x&.index_with { |el| foo(el) }\n",
        );
    }

    #[test]
    fn does_not_flag_keys_transformed() {
        test::<IndexWith>().expect_no_offenses(
            "x.each_with_object({}) { |el, h| h[el.to_sym] = foo(el) }\n",
        );
    }

    #[test]
    fn does_not_flag_values_not_transformed() {
        test::<IndexWith>()
            .expect_no_offenses("x.each_with_object({}) { |el, h| h[el] = el }\n");
    }

    #[test]
    fn does_not_flag_hash_not_used() {
        test::<IndexWith>().expect_no_offenses(indoc! {r#"
            x.each_with_object({}) { |el, h| other_h[el] = el.to_sym }
        "#});
    }

    #[test]
    fn does_not_flag_hash_used_in_value() {
        test::<IndexWith>().expect_no_offenses(indoc! {r#"
            x.each_with_object({}) { |el, h| h[el] = h.count }
        "#});
    }

    #[test]
    fn flags_map_to_h() {
        test::<IndexWith>().expect_offense(indoc! {r#"
            x.map { |el| [el, el.to_sym] }.to_h
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `map { ... }.to_h`.
        "#});
    }

    #[test]
    fn corrects_map_to_h() {
        test::<IndexWith>().expect_correction(
            indoc! {r#"
                x.map { |el| [el, el.to_sym] }.to_h
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `map { ... }.to_h`.
            "#},
            "x.index_with { |el| el.to_sym }\n",
        );
    }

    #[test]
    fn corrects_bare_hash_value() {
        test::<IndexWith>().expect_correction(
            indoc! {r#"
                x.map { |el| [el, value: el] }.to_h
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `map { ... }.to_h`.
            "#},
            "x.index_with { |el| { value: el } }\n",
        );
    }

    #[test]
    fn flags_hash_brackets() {
        test::<IndexWith>().expect_offense(indoc! {r#"
            Hash[x.map { |el| [el, el.to_sym] }]
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `Hash[map { ... }]`.
        "#});
    }

    #[test]
    fn corrects_hash_brackets() {
        test::<IndexWith>().expect_correction(
            indoc! {r#"
                Hash[x.map { |el| [el, el.to_sym] }]
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `Hash[map { ... }]`.
            "#},
            "x.index_with { |el| el.to_sym }\n",
        );
    }

    #[test]
    fn does_not_flag_cbase_hash_mismatch() {
        test::<IndexWith>().expect_no_offenses("Foo::Hash[x.map { |el| [el, el.to_sym] }]\n");
    }

    #[test]
    fn flags_cbase_hash() {
        test::<IndexWith>().expect_offense(indoc! {r#"
            ::Hash[x.map { |el| [el, el.to_sym] }]
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `Hash[map { ... }]`.
        "#});
    }

    #[test]
    fn flags_to_h_block() {
        test::<IndexWith>().expect_offense(indoc! {r#"
            x.to_h { |el| [el, el.to_sym] }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `to_h { ... }`.
        "#});
    }

    #[test]
    fn corrects_to_h_block() {
        test::<IndexWith>().expect_correction(
            indoc! {r#"
                x.to_h { |el| [el, el.to_sym] }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `to_h { ... }`.
            "#},
            "x.index_with { |el| el.to_sym }\n",
        );
    }

    #[test]
    fn does_not_flag_plain_map() {
        test::<IndexWith>()
            .expect_no_offenses("x.map { |el| [el, el.to_sym] }\n");
    }

    #[test]
    fn flags_collect_to_h() {
        test::<IndexWith>().expect_offense(indoc! {r#"
            x.collect { |el| [el, el.to_sym] }.to_h
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `map { ... }.to_h`.
        "#});
    }

    #[test]
    fn flags_numblock_to_h() {
        test::<IndexWith>().expect_offense(indoc! {r#"
            x.to_h { [_1, _1.to_sym] }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `to_h { ... }`.
        "#});
    }

    #[test]
    fn corrects_numblock_to_h() {
        test::<IndexWith>().expect_correction(
            indoc! {r#"
                x.to_h { [_1, _1.to_sym] }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `to_h { ... }`.
            "#},
            "x.index_with { _1.to_sym }\n",
        );
    }

    #[test]
    fn does_not_flag_numblock_other_param() {
        test::<IndexWith>()
            .expect_no_offenses("x.to_h { [_1, _2.to_sym] }\n");
    }

    #[test]
    fn flags_itblock_map_to_h() {
        test::<IndexWith>().expect_offense(indoc! {r#"
            x.map { [it, it.to_sym] }.to_h
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `map { ... }.to_h`.
        "#});
    }

    #[test]
    fn flags_itblock_to_h_with_other_value() {
        // Upstream `$_` allows `[it, y.to_sym]` to flag.
        test::<IndexWith>().expect_offense(indoc! {r#"
            x.to_h { [it, y.to_sym] }
            ^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_with` over `to_h { ... }`.
        "#});
    }

    #[test]
    fn does_not_fire_on_old_rails() {
        test::<IndexWith>()
            .with_target_rails_version(5, 2)
            .expect_no_offenses("x.each_with_object({}) { |el, h| h[el] = foo(el) }\n");
    }
}
murphy_plugin_api::submit_cop!(IndexWith);
