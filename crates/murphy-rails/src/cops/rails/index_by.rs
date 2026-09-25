//! `Rails/IndexBy` — prefer `index_by` over manual hash building.
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop-rails
//! upstream_cop: Rails/IndexBy
//! upstream_version_checked: 2.35.0
//! version_added: "2.5"
//! safe: true
//! supports_autocorrect: true
//! status: verified
//! gap_issues: []
//! notes: >
//!   Mirrors rubocop-rails 2.35.0 IndexMethod mixin: `each_with_object({})`
//!   with `h[key] = el`, `to_h { [key, el] }` (Block/Numblock/Itblock),
//!   `map/collect { [key, el] }.to_h`, and `Hash[map/collect { [key, el] }]`
//!   (global Hash only). Noop (`[el, el]`) never flags; `each_with_object`
//!   skips when the key references the memo (`h`) to avoid wrong rewrites.
//!   Numblock requires `_1`-only (`max_n == 1`); Itblock uses `it`.
//!   Offense is the whole node when single-line, else the triggering call
//!   (to keep annotations single-line). Autocorrect is surgical: replace
//!   the builder (`each_with_object({})`/`to_h`/`map`) with `index_by`,
//!   narrow args to `|el|`, replace body with the key, and strip the
//!   trailing `.to_h` / `Hash[`/`]` wrappers. Ruby 2.6 gating for `to_h`
//!   blocks is not gated (Murphy assumes modern Ruby). Safe-navigation
//!   (`&.`) is preserved.
//! ```
//!
//! Looks for uses of `each_with_object({})`, `map { ... }.to_h`, and
//! `Hash[map { ... }]` that can be replaced with `index_by`.

use murphy_plugin_api::{Cx, NoOptions, NodeId, NodeKind, Range, Symbol, cop};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
#[derive(Default)]
pub struct IndexBy;

#[cop(
    name = "Rails/IndexBy",
    description = "Prefer `index_by` over `each_with_object`, `to_h`, or `map`.",
    default_severity = "warning",
    default_enabled = true,
    options = NoOptions,
)]
impl IndexBy {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        // `each_with_object` and `to_h` with block.
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
        if let Some(c) = match_to_h_numblock(cx, node) {
            emit_to_h_block(cx, node, c);
        }
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        if let Some(c) = match_to_h_itblock(cx, node) {
            emit_to_h_block(cx, node, c);
        }
    }

    #[on_node(kind = "send")]
    fn check_send(&self, node: NodeId, cx: &Cx<'_>) {
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
    key: NodeId,
}

struct ToHBlockCap {
    call: NodeId,
    el_name: String,
    key: NodeId,
    args_node: Option<NodeId>,
}

struct MapToHCap {
    inner_block: NodeId,
    inner_call: NodeId,
    el_name: String,
    key: NodeId,
    inner_args_node: Option<NodeId>,
    inner_body: NodeId,
    is_numblock: bool,
    is_itblock: bool,
}

struct HashBracketsCap {
    inner_block: NodeId,
    inner_call: NodeId,
    el_name: String,
    key: NodeId,
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
    // `(hash)` — any hash arg (typically `{}`).
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
    // Body: `(call (lvar memo) :[]= key (lvar el))`
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
    let val = bargs[1];
    if !is_lvar_named(cx, val, el_sym) {
        return None;
    }
    // `$!memo` — key must not be the memo itself; broader: key must not
    // reference the memo at all (`h[h[el]] = el` is not rewritable).
    if contains_lvar(cx, key, memo_sym) {
        return None;
    }
    // Noop: `h[el] = el` (key is just el).
    if is_lvar_named(cx, key, el_sym) {
        return None;
    }
    Some(EachWithObjectCap {
        call,
        el,
        el_sym,
        memo_sym,
        key,
    })
}

fn array_key_value(cx: &Cx<'_>, body: NodeId, el_sym: Symbol) -> Option<NodeId> {
    let NodeKind::Array(list) = *cx.kind(body) else {
        return None;
    };
    let els = cx.list(list);
    if els.len() != 2 {
        return None;
    }
    let key = els[0];
    let val = els[1];
    if !is_lvar_named(cx, val, el_sym) {
        return None;
    }
    if is_lvar_named(cx, key, el_sym) {
        return None;
    }
    Some(key)
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
    let key = array_key_value(cx, body, el_sym)?;
    Some(ToHBlockCap {
        call,
        el_name,
        key,
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
    // Body `[key, _1]`
    let NodeKind::Array(list) = *cx.kind(body) else {
        return None;
    };
    let els = cx.list(list);
    if els.len() != 2 {
        return None;
    }
    let key = els[0];
    let val = els[1];
    // val must be `_1`
    if !is_n1(cx, val) {
        return None;
    }
    if is_n1(cx, key) {
        return None;
    }
    Some(ToHBlockCap {
        call: send,
        el_name: "_1".to_owned(),
        key,
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
    // Murphy translates bare `it` (Ruby 3.4 implicit block param) as
    // `(send nil :it)` to match parser-gem/RuboCop, not `(lvar :it)`.
    // `_1` stays `(lvar :_1)`.
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
    let key = els[0];
    let val = els[1];
    if !is_it(cx, val) {
        return None;
    }
    if is_it(cx, key) {
        return None;
    }
    Some(ToHBlockCap {
        call: send,
        el_name: "it".to_owned(),
        key,
        args_node: None,
    })
}

type MapBlockMatch = (NodeId, String, NodeId, Option<NodeId>, NodeId, bool, bool);

fn inner_map_block(cx: &Cx<'_>, block: NodeId) -> Option<MapBlockMatch> {
    // Returns (call, el_name, key, args_node, body, is_numblock, is_itblock)
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
            let key = array_key_value(cx, body_id, el_sym)?;
            Some((call, el_name, key, Some(args), body_id, false, false))
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
            if !is_n1(cx, els[1]) || is_n1(cx, els[0]) {
                return None;
            }
            Some((send, "_1".to_owned(), els[0], None, body_id, true, false))
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
            if !is_it(cx, els[1]) || is_it(cx, els[0]) {
                return None;
            }
            // For itblock map, `y` in key is allowed (matches upstream
            // `x.map { [it.to_sym, it] }` and `x.to_h { [y.to_sym, it] }`
            // parity); only the `it`/`it` noop is excluded above.
            // Note: `Hash[x.map { [it.to_sym, y] }]` (value not `it`) is
            // already excluded by the `!is_it(val)` check.
            Some((send, "it".to_owned(), els[0], None, body_id, false, true))
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
    // Receiver must be a map block.
    let recv = match *cx.kind(node) {
        NodeKind::Send { receiver, .. } => receiver.get()?,
        NodeKind::Csend { receiver, .. } => receiver,
        _ => return None,
    };
    let (inner_call, el_name, key, args_node, body, is_num, is_it) =
        inner_map_block(cx, recv)?;
    Some(MapToHCap {
        inner_block: recv,
        inner_call,
        el_name,
        key,
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
    // `(const {nil? cbase} :Hash)`
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
    let (inner_call, el_name, key, args_node, body, is_num, is_it) =
        inner_map_block(cx, inner)?;
    Some(HashBracketsCap {
        inner_block: inner,
        inner_call,
        el_name,
        key,
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
    // Returns (receiver_source, operator "." or "&.")
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
    // Heuristic: `do`/`end` blocks contain ` do ` or `do|` and end with `end`.
    // Braces contain `{`/`}`. Key expressions rarely contain standalone `do`.
    let src = cx.raw_source(whole);
    src.contains(" do ") || src.contains(" do|") || src.contains("\ndo ") || src.contains("\ndo|")
}

fn emit_each_with_object(cx: &Cx<'_>, block: NodeId, cap: EachWithObjectCap) {
    let whole = cx.range(block);
    cx.emit_offense(
        first_line_range(cx, whole),
        "Prefer `index_by` over `each_with_object`.",
        None,
    );
    let Some((recv_src, op)) = block_receiver_source(cx, cap.call) else {
        return;
    };
    let key_src = cx.raw_source(cx.range(cap.key)).to_owned();
    let replacement = if is_do_block(cx, whole) {
        let base = base_indent(cx, whole.start);
        let bind = body_indent(cx, cap.key);
        // Use original body indent to preserve nesting.
        format!("{recv_src}{op}index_by do |{}|\n{bind}{key_src}\n{base}end", cap.el)
    } else {
        format!("{recv_src}{op}index_by {{ |{}| {key_src} }}", cap.el)
    };
    cx.emit_edit(whole, &replacement);
    let _ = cap.el_sym;
    let _ = cap.memo_sym;
}

fn emit_to_h_block(cx: &Cx<'_>, block: NodeId, cap: ToHBlockCap) {
    let whole = cx.range(block);
    cx.emit_offense(
        first_line_range(cx, whole),
        "Prefer `index_by` over `to_h { ... }`.",
        None,
    );
    let Some((recv_src, op)) = block_receiver_source(cx, cap.call) else {
        return;
    };
    let key_src = cx.raw_source(cx.range(cap.key)).to_owned();
    let replacement = if cap.args_node.is_none() {
        // Numblock (`_1`) / Itblock (`it`) — no explicit args.
        if is_do_block(cx, whole) {
            let base = base_indent(cx, whole.start);
            let bind = body_indent(cx, cap.key);
            format!("{recv_src}{op}index_by do\n{bind}{key_src}\n{base}end")
        } else {
            format!("{recv_src}{op}index_by {{ {key_src} }}")
        }
    } else if is_do_block(cx, whole) {
        let base = base_indent(cx, whole.start);
        let bind = body_indent(cx, cap.key);
        format!("{recv_src}{op}index_by do |{}|\n{bind}{key_src}\n{base}end", cap.el_name)
    } else {
        format!("{recv_src}{op}index_by {{ |{}| {key_src} }}", cap.el_name)
    };
    cx.emit_edit(whole, &replacement);
}

fn emit_map_to_h(cx: &Cx<'_>, outer: NodeId, cap: MapToHCap) {
    let whole = cx.range(outer);
    cx.emit_offense(
        first_line_range(cx, whole),
        "Prefer `index_by` over `map { ... }.to_h`.",
        None,
    );
    let Some((recv_src, op)) = block_receiver_source(cx, cap.inner_call) else {
        return;
    };
    let key_src = cx.raw_source(cx.range(cap.key)).to_owned();
    // Preserve do/end vs braces based on the inner block.
    let inner_whole = cx.range(cap.inner_block);
    let replacement = if cap.is_numblock || cap.is_itblock {
        if is_do_block(cx, inner_whole) {
            let base = base_indent(cx, whole.start);
            let bind = body_indent(cx, cap.key);
            format!("{recv_src}{op}index_by do\n{bind}{key_src}\n{base}end")
        } else {
            format!("{recv_src}{op}index_by {{ {key_src} }}")
        }
    } else if is_do_block(cx, inner_whole) {
        let base = base_indent(cx, whole.start);
        let bind = body_indent(cx, cap.key);
        format!("{recv_src}{op}index_by do |{}|\n{bind}{key_src}\n{base}end", cap.el_name)
    } else {
        format!("{recv_src}{op}index_by {{ |{}| {key_src} }}", cap.el_name)
    };
    cx.emit_edit(whole, &replacement);
    let _ = cap.inner_args_node;
    let _ = cap.inner_body;
}

fn emit_hash_brackets(cx: &Cx<'_>, outer: NodeId, cap: HashBracketsCap) {
    let whole = cx.range(outer);
    cx.emit_offense(
        first_line_range(cx, whole),
        "Prefer `index_by` over `Hash[map { ... }]`.",
        None,
    );
    let Some((recv_src, op)) = block_receiver_source(cx, cap.inner_call) else {
        return;
    };
    let key_src = cx.raw_source(cx.range(cap.key)).to_owned();
    let inner_whole = cx.range(cap.inner_block);
    let replacement = if cap.is_numblock || cap.is_itblock {
        if is_do_block(cx, inner_whole) {
            let base = base_indent(cx, whole.start);
            let bind = body_indent(cx, cap.key);
            format!("{recv_src}{op}index_by do\n{bind}{key_src}\n{base}end")
        } else {
            format!("{recv_src}{op}index_by {{ {key_src} }}")
        }
    } else if is_do_block(cx, inner_whole) {
        let base = base_indent(cx, whole.start);
        let bind = body_indent(cx, cap.key);
        format!("{recv_src}{op}index_by do |{}|\n{bind}{key_src}\n{base}end", cap.el_name)
    } else {
        format!("{recv_src}{op}index_by {{ |{}| {key_src} }}", cap.el_name)
    };
    cx.emit_edit(whole, &replacement);
    let _ = cap.inner_args_node;
    let _ = cap.inner_body;
}

#[cfg(test)]
mod tests {
    use super::IndexBy;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_each_with_object() {
        test::<IndexBy>().expect_offense(indoc! {r#"
            x.each_with_object({}) { |el, h| h[foo(el)] = el }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `each_with_object`.
        "#});
    }

    #[test]
    fn corrects_each_with_object() {
        test::<IndexBy>().expect_correction(
            indoc! {r#"
                x.each_with_object({}) { |el, h| h[foo(el)] = el }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `each_with_object`.
            "#},
            "x.index_by { |el| foo(el) }\n",
        );
    }

    #[test]
    fn corrects_each_with_object_multiline() {
        test::<IndexBy>().expect_correction(
            indoc! {r#"
                x.each_with_object({}) do |el, memo|
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `each_with_object`.
                  memo[el.to_sym] = el
                end
            "#},
            "x.index_by do |el|\n  el.to_sym\nend\n",
        );
    }

    #[test]
    fn corrects_safe_navigation_each_with_object() {
        test::<IndexBy>().expect_correction(
            indoc! {r#"
                x&.each_with_object({}) { |el, h| h[foo(el)] = el }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `each_with_object`.
            "#},
            "x&.index_by { |el| foo(el) }\n",
        );
    }

    #[test]
    fn does_not_flag_values_transformed() {
        test::<IndexBy>().expect_no_offenses(
            "x.each_with_object({}) { |el, h| h[el.to_sym] = foo(el) }\n",
        );
    }

    #[test]
    fn does_not_flag_keys_not_transformed() {
        test::<IndexBy>()
            .expect_no_offenses("x.each_with_object({}) { |el, h| h[el] = el }\n");
    }

    #[test]
    fn does_not_flag_hash_not_used() {
        test::<IndexBy>().expect_no_offenses(indoc! {r#"
            x.each_with_object({}) { |el, h| other_h[el.to_sym] = el }
        "#});
    }

    #[test]
    fn does_not_flag_hash_used_in_key() {
        test::<IndexBy>().expect_no_offenses(indoc! {r#"
            x.each_with_object({}) { |el, h| h[h[el]] = el }
        "#});
    }

    #[test]
    fn flags_map_to_h() {
        test::<IndexBy>().expect_offense(indoc! {r#"
            x.map { |el| [el.to_sym, el] }.to_h
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `map { ... }.to_h`.
        "#});
    }

    #[test]
    fn corrects_map_to_h() {
        test::<IndexBy>().expect_correction(
            indoc! {r#"
                x.map { |el| [el.to_sym, el] }.to_h
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `map { ... }.to_h`.
            "#},
            "x.index_by { |el| el.to_sym }\n",
        );
    }

    #[test]
    fn flags_hash_brackets() {
        test::<IndexBy>().expect_offense(indoc! {r#"
            Hash[x.map { |el| [el.to_sym, el] }]
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `Hash[map { ... }]`.
        "#});
    }

    #[test]
    fn corrects_hash_brackets() {
        test::<IndexBy>().expect_correction(
            indoc! {r#"
                Hash[x.map { |el| [el.to_sym, el] }]
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `Hash[map { ... }]`.
            "#},
            "x.index_by { |el| el.to_sym }\n",
        );
    }

    #[test]
    fn does_not_flag_cbase_hash_mismatch() {
        test::<IndexBy>().expect_no_offenses("Foo::Hash[x.map { |el| [el.to_sym, el] }]\n");
    }

    #[test]
    fn flags_cbase_hash() {
        test::<IndexBy>().expect_offense(indoc! {r#"
            ::Hash[x.map { |el| [el.to_sym, el] }]
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `Hash[map { ... }]`.
        "#});
    }

    #[test]
    fn flags_to_h_block() {
        test::<IndexBy>().expect_offense(indoc! {r#"
            x.to_h { |el| [el.to_sym, el] }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `to_h { ... }`.
        "#});
    }

    #[test]
    fn corrects_to_h_block() {
        test::<IndexBy>().expect_correction(
            indoc! {r#"
                x.to_h { |el| [el.to_sym, el] }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `to_h { ... }`.
            "#},
            "x.index_by { |el| el.to_sym }\n",
        );
    }

    #[test]
    fn does_not_flag_plain_map() {
        test::<IndexBy>()
            .expect_no_offenses("x.map { |el| [el.to_sym, el] }\n");
    }

    #[test]
    fn flags_collect_to_h() {
        test::<IndexBy>().expect_offense(indoc! {r#"
            x.collect { |el| [el.to_sym, el] }.to_h
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `map { ... }.to_h`.
        "#});
    }

    #[test]
    fn flags_numblock_to_h() {
        test::<IndexBy>().expect_offense(indoc! {r#"
            x.to_h { [_1.to_sym, _1] }
            ^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `to_h { ... }`.
        "#});
    }

    #[test]
    fn corrects_numblock_to_h() {
        test::<IndexBy>().expect_correction(
            indoc! {r#"
                x.to_h { [_1.to_sym, _1] }
                ^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `to_h { ... }`.
            "#},
            "x.index_by { _1.to_sym }\n",
        );
    }

    #[test]
    fn does_not_flag_numblock_other_param() {
        test::<IndexBy>()
            .expect_no_offenses("x.to_h { [_2.to_sym, _1] }\n");
    }

    #[test]
    fn flags_itblock_map_to_h() {
        test::<IndexBy>().expect_offense(indoc! {r#"
            x.map { [it.to_sym, it] }.to_h
            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Prefer `index_by` over `map { ... }.to_h`.
        "#});
    }
}
murphy_plugin_api::submit_cop!(IndexBy);
