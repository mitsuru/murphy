//! Read-only live native-primitive IDL — Phase 3 Task 3.
//!
//! These are the native Rust functions an mruby user-cop calls to inspect the
//! **live** prism AST. They promote the *resolution shape* proven by
//! `spikes/live_resolution_poc` (ADR 0008) into `crates/`:
//!
//!   * **Handle = opaque arena node id.** The only thing that ever crosses the
//!     FFI boundary for traversal is an integer; every primitive bounds-checks
//!     it before reading the arena.
//!   * **Resolution RE-WALKS the live tree per native call.**
//!     [`with_call_node`] re-derives a fresh root `Node` from
//!     `AstContext::parse_result().node()` (the live prism C arena) and visits
//!     to the Nth [`ruby_prism::CallNode`], reading its real `name()` /
//!     `receiver()` / `message_loc()` on the spot. NOTHING is pre-extracted.
//!     - ADR 0008 finding 1: a `*const CallNode` is a walk-time temporary and
//!       is UNSOUND to store; we re-walk to an index, never cache a node ptr.
//!     - ADR 0008 finding 2: offset-keyed resolution is REJECTED
//!       (`logger.info(x)` aliases bare `logger` + outer `.info` at the same
//!       `start_offset`). The walk-order **index** is the resolution key; the
//!       Task-3 tests assert handle→node DISTINCTNESS so this cannot regress.
//!     - ADR 0008 finding 5: the re-walk is O(N) per call → O(N²) for an
//!       N-call cop. Accepted for Phase 3; a resumable cursor / single-visit
//!       dispatch is the obvious later optimization (YAGNI now).
//!   * All offsets are **BYTE** offsets (ADR 0001) via [`crate::Range`] /
//!     [`crate::Range::from_prism_location`] — never char indices. A multibyte
//!     hand-derived test pins this.
//!
//! ## Scope fence (Phase 3 plan, Task 3)
//!
//! This module is the **read-only** native-primitive IDL ONLY. It deliberately
//! does NOT implement: the `Murphy::Cop` Ruby base class / `on_call_node`
//! dispatch / `add_offense` / `fix` (Task 4); any cop offense sink (ADR 0009
//! rule 2 — a cop-instance-owned local; Task 4); the per-cop OS thread /
//! watchdog / abandon (Task 5); severity dedupe (Task 6); or pipeline wiring
//! (Task 7). Every primitive here is a pure read accessor over `&AstContext`;
//! no `&mut` is ever formed into the parsed tree (design §4, ADR 0009 rule 3).
//!
//! ## `unsafe_op_in_unsafe_fn`
//!
//! Per the P3 Task 2 review I-2 correction, there is **NO** module-wide
//! `#![allow(unsafe_op_in_unsafe_fn)]`. Every unsafe operation inside the
//! `unsafe extern "C"` callbacks is annotated with its own `unsafe { }` block
//! and a `// SAFETY:` justification. Do not re-add a blanket allow.

use std::ffi::{CStr, CString};

use ruby_prism::Visit;

use murphy_ast::{NodeId, NodeKind, NodeList};

use mruby3_sys::{
    RClass, mrb_class_get, mrb_define_class, mrb_define_module_function, mrb_get_args, mrb_int,
    mrb_load_string, mrb_raise, mrb_state, mrb_str_new, mrb_sym, mrb_sym_name, mrb_value,
};

use crate::Range;
use crate::mruby::AstContext;

// `ruby_string_from_bytes` narrows a slice `len` to `mrb_int` relying on cop
// sources being `u32`-bounded (see `Range::from_prism_location`). That bound
// only fits `mrb_int` if `mrb_int` is at least 64-bit: an `MRB_INT32` mruby
// build (`mrb_int = i32`) would silently truncate a >2GiB source's length.
// Lock the assumption at compile time so such a build fails the build, not a
// user cop at runtime.
const _: () = assert!(std::mem::size_of::<mrb_int>() >= 8);

/// `MRB_ARGS_REQ(n)` is a C macro absent from the `mruby3-sys` bindgen output
/// (ADR 0002 finding 1). Reproduce it: `((mrb_aspec)((n)&0x1f) << 18)`.
const fn args_req(n: u32) -> u32 {
    (n & 0x1f) << 18
}

/// LIVE resolution: walk the real prism tree NOW and pass the Nth
/// (walk-order) [`ruby_prism::CallNode`] to `f`, where `N == handle`.
///
/// Touches the live prism C arena via `ctx.parse_result().node()` on every
/// call — zero snapshot, not even an offset cache; the only thing the handle
/// carries is the integer index (ADR 0008: walk-order index, never a cached
/// `*const CallNode` (finding 1), never offset-keyed (finding 2)). O(N) per
/// call (finding 5) — accepted for Phase 3.
///
/// Returns `None` if `handle` is out of range (no Nth call node).
fn with_call_node<R>(
    ctx: &AstContext,
    handle: usize,
    f: impl FnOnce(&ruby_prism::CallNode<'_>) -> R,
) -> Option<R> {
    /// The one-shot visitor callback, boxed so `Finder` is not generic over
    /// the closure type (a named alias keeps the field type readable).
    type NodeFn<'a, R> = Box<dyn FnOnce(&ruby_prism::CallNode<'_>) -> R + 'a>;
    struct Finder<'a, R> {
        /// Counts call nodes down; act on the one where it reaches 0.
        remaining: usize,
        out: Option<R>,
        f: Option<NodeFn<'a, R>>,
    }
    impl<'pr, 'a, R> Visit<'pr> for Finder<'a, R> {
        fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
            if self.out.is_some() {
                return;
            }
            if self.remaining == 0 {
                if let Some(f) = self.f.take() {
                    self.out = Some(f(node));
                }
                return;
            }
            self.remaining -= 1;
            ruby_prism::visit_call_node(self, node);
        }
    }

    let mut finder = Finder {
        remaining: handle,
        out: None,
        f: Some(Box::new(f)),
    };
    // `node()` re-derives a fresh root `Node` from the LIVE prism C arena.
    finder.visit(&ctx.parse_result().node());
    finder.out
}

/// Count the call nodes in the live tree (sizes the handle space `0..count`).
///
/// This is a re-walk too — NOTHING about node count is cached on `AstContext`
/// (Task 2's `AstContext` is the bare carrier; Task 3 does not modify it). The
/// cop calls `Murphy.node_count` once at script start, so the extra O(N) walk
/// is covered by ADR 0008 finding 5's accepted O(N²) note.
fn count_call_nodes(ctx: &AstContext) -> usize {
    struct Counter {
        count: usize,
    }
    impl<'pr> Visit<'pr> for Counter {
        fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
            self.count += 1;
            ruby_prism::visit_call_node(self, node);
        }
    }
    let mut counter = Counter { count: 0 };
    counter.visit(&ctx.parse_result().node());
    counter.count
}

/// Reconstitute `&AstContext` from `mrb_state.ud`.
///
/// # Safety
///
/// The caller (the per-cop worker, Task 5; the Task-3 tests) MUST have stored
/// the cop-run-owned `CopRun` pointer in `(*mrb).ud` via
/// `crate::MrubyState::set_cop_run` (Task-4 ud-payload widening — `ud` carries
/// a `CopRun`, and `ctx` projects `&(*p).ctx`), and the owning `CopRun` (with
/// its `Arc<AstContext>` clone) MUST be alive for the entire duration of any
/// native call (ADR 0008 / ADR 0009 rule 1 — the `ud` raw pointer is NOT
/// itself a liveness guarantee). The
/// pointee is only ever read here (ADR 0009 rule 3 — no `&mut` is formed).
///
/// `'a` is unconstrained in the signature but is, by caller convention, the
/// synchronous scope of one native callback: every caller binds the result to
/// a local that does not escape its `unsafe extern "C"` body and picks the
/// shortest such scope. It is not actually an unbounded lifetime in practice.
unsafe fn ctx<'a>(mrb: *mut mrb_state) -> &'a AstContext {
    // SAFETY: `mrb` is a valid non-null `mrb_state` passed by mruby into the
    // native callback. Reading the `ud` field is the documented mruby
    // mechanism for native-callback context.
    //
    // Task-4 ud-payload widening: `ud` now carries the cop-run-owned `CopRun`
    // (ADR 0009 rule 2 — it bundles the `Arc<AstContext>` AND the offense
    // sink, and is NOT a `thread_local!`), not a bare `Arc::as_ptr(
    // &AstContext)`. We project the `&AstContext` back out via `CopRun::ctx`.
    // This is the anticipated extension point (Task-3's `register` docstring:
    // "Task 4 lands the first non-test caller"); Task-3's own `#[cfg(test)]`
    // sites construct a `CopRun::for_test` so they keep driving this same
    // contract.
    let ud = unsafe { (*mrb).ud } as *const crate::mruby::sdk::CopRun;
    assert!(
        !ud.is_null(),
        "mrb_state.ud must hold the CopRun pointer (set via set_cop_run)"
    );
    // SAFETY: `ud` is `&CopRun as *const _` (see the fn-level # Safety): a
    // valid, aligned, initialized `*const CopRun` whose owner is alive for the
    // whole native call (ADR 0009 rule 1). The projected `AstContext` is
    // shared-immutable behind `Arc` (ADR 0009 rule 3); we form only a shared
    // `&`, never `&mut`. The returned reference's lifetime is unbounded `'a`
    // but is only ever used within the synchronous body of one native
    // callback, strictly inside the window the owner is alive.
    let run: &crate::mruby::sdk::CopRun = unsafe { &*ud };
    run.ctx()
}

/// Read the single required `i` (handle) argument from an mruby native call.
///
/// # Safety
///
/// `mrb` must be a valid non-null `mrb_state` inside a native callback that
/// was registered with `MRB_ARGS_REQ(1)`.
unsafe fn arg_handle(mrb: *mut mrb_state) -> mrb_int {
    let mut handle: mrb_int = -1;
    let fmt = c"i";
    // SAFETY: `mrb` is a valid non-null `mrb_state`; `fmt` is a valid
    // NUL-terminated format string requesting exactly one `mrb_int`; `handle`
    // is a live, correctly-typed `mrb_int` out-pointer that outlives the call.
    // `mrb_get_args` is the documented mruby argument extractor.
    unsafe {
        mrb_get_args(mrb, fmt.as_ptr(), &mut handle as *mut mrb_int);
    }
    handle
}

/// Build a Ruby `String` from arbitrary bytes (length-delimited; NUL-safe).
///
/// # Safety
///
/// `mrb` must be a valid non-null `mrb_state` inside a native callback.
unsafe fn ruby_string_from_bytes(mrb: *mut mrb_state, bytes: &[u8]) -> mrb_value {
    // SAFETY: `mrb` is a valid non-null `mrb_state`. `bytes.as_ptr()` is valid
    // for `bytes.len()` bytes; `mrb_str_new` copies them into a new Ruby
    // string (it does not retain the pointer), so the borrow ending at the
    // call boundary is sound. `len` fits `mrb_int` (cop sources are bounded by
    // `u32::MAX`, see `Range::from_prism_location`).
    unsafe {
        mrb_str_new(
            mrb,
            bytes.as_ptr() as *const std::os::raw::c_char,
            bytes.len() as mrb_int,
        )
    }
}

/// Evaluate a tiny Ruby literal and return its value.
///
/// Used to return `Integer`/`true`/`false`: ADR 0002 finding 1 — the inline
/// `mrb_value` boxers (`mrb_fixnum_value`, …) are absent from the bindgen
/// output, so we round-trip a literal exactly as the proven spike does.
///
/// # Safety
///
/// `mrb` must be a valid non-null `mrb_state`; `lit` a NUL-terminated Ruby
/// literal source.
unsafe fn eval_literal(mrb: *mut mrb_state, lit: &CStr) -> mrb_value {
    // SAFETY: `mrb` is a valid non-null `mrb_state`; `lit` is a valid
    // NUL-terminated C string holding a trivial constant Ruby expression.
    // `mrb_load_string` is the documented string-eval entry point.
    unsafe { mrb_load_string(mrb, lit.as_ptr()) }
}

unsafe fn ruby_nil(mrb: *mut mrb_state) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; "nil" is a trivial literal.
    unsafe { eval_literal(mrb, c"nil") }
}

unsafe fn ruby_empty_array(mrb: *mut mrb_state) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; "[]" is a trivial literal.
    unsafe { eval_literal(mrb, c"[]") }
}

fn node_id(ctx: &AstContext, handle: mrb_int) -> Option<NodeId> {
    let id = usize::try_from(handle).ok()?;
    if id < ctx.ast()?.len() {
        Some(NodeId(id as u32))
    } else {
        None
    }
}

unsafe fn ruby_int(mrb: *mut mrb_state, value: u32) -> mrb_value {
    let lit = CString::new(value.to_string()).expect("decimal digits, no NUL");
    // SAFETY: `mrb` valid & non-null; `lit` is a NUL-terminated decimal integer literal.
    unsafe { eval_literal(mrb, &lit) }
}

unsafe fn ruby_i64(mrb: *mut mrb_state, value: i64) -> mrb_value {
    let lit = CString::new(value.to_string()).expect("decimal digits, no NUL");
    // SAFETY: `mrb` valid & non-null; `lit` is a NUL-terminated decimal integer literal.
    unsafe { eval_literal(mrb, &lit) }
}

unsafe fn ruby_float(mrb: *mut mrb_state, value: f64) -> mrb_value {
    if !value.is_finite() {
        // SAFETY: non-finite floats have no safe generated Ruby literal here.
        return unsafe { ruby_nil(mrb) };
    }
    let lit = CString::new(format!("{value:?}")).expect("finite float literal, no NUL");
    // SAFETY: `mrb` valid & non-null; `lit` is a generated finite float literal.
    unsafe { eval_literal(mrb, &lit) }
}

unsafe fn ruby_bool(mrb: *mut mrb_state, value: bool) -> mrb_value {
    let lit = if value { c"true" } else { c"false" };
    // SAFETY: `mrb` valid & non-null; `lit` is a boolean literal.
    unsafe { eval_literal(mrb, lit) }
}

unsafe fn ruby_int_array(mrb: *mut mrb_state, ids: impl IntoIterator<Item = NodeId>) -> mrb_value {
    let mut lit = String::from("[");
    for (i, id) in ids.into_iter().enumerate() {
        if i > 0 {
            lit.push(',');
        }
        lit.push_str(&id.0.to_string());
    }
    lit.push(']');
    let lit = CString::new(lit).expect("decimal array literal, no NUL");
    // SAFETY: `mrb` valid & non-null; `lit` is a generated integer-array literal.
    unsafe { eval_literal(mrb, &lit) }
}

fn node_kind_symbol_name(kind: &murphy_ast::NodeKind) -> &'static str {
    match kind {
        murphy_ast::NodeKind::Error => "error",
        murphy_ast::NodeKind::Nil => "nil",
        murphy_ast::NodeKind::True_ => "true",
        murphy_ast::NodeKind::False_ => "false",
        murphy_ast::NodeKind::SelfExpr => "self",
        murphy_ast::NodeKind::Int(_) => "int",
        murphy_ast::NodeKind::Float(_) => "float",
        murphy_ast::NodeKind::Str(_) => "str",
        murphy_ast::NodeKind::Sym(_) => "sym",
        murphy_ast::NodeKind::Lvar(_) => "lvar",
        murphy_ast::NodeKind::Ivar(_) => "ivar",
        murphy_ast::NodeKind::Cvar(_) => "cvar",
        murphy_ast::NodeKind::Gvar(_) => "gvar",
        murphy_ast::NodeKind::Const { .. } => "const",
        murphy_ast::NodeKind::Lvasgn { .. } => "lvasgn",
        murphy_ast::NodeKind::Ivasgn { .. } => "ivasgn",
        murphy_ast::NodeKind::Casgn { .. } => "casgn",
        murphy_ast::NodeKind::Send { .. } => "send",
        murphy_ast::NodeKind::Csend { .. } => "csend",
        murphy_ast::NodeKind::Block { .. } => "block",
        murphy_ast::NodeKind::BlockPass(_) => "block_pass",
        murphy_ast::NodeKind::Splat(_) => "splat",
        murphy_ast::NodeKind::Array(_) => "array",
        murphy_ast::NodeKind::Hash(_) => "hash",
        murphy_ast::NodeKind::Pair { .. } => "pair",
        murphy_ast::NodeKind::If { .. } => "if",
        murphy_ast::NodeKind::Case { .. } => "case",
        murphy_ast::NodeKind::When { .. } => "when",
        murphy_ast::NodeKind::Begin(_) => "begin",
        murphy_ast::NodeKind::Return(_) => "return",
        murphy_ast::NodeKind::And { .. } => "and",
        murphy_ast::NodeKind::Or { .. } => "or",
        murphy_ast::NodeKind::Def { .. } => "def",
        murphy_ast::NodeKind::Class { .. } => "class",
        murphy_ast::NodeKind::Module { .. } => "module",
        murphy_ast::NodeKind::Args(_) => "args",
        murphy_ast::NodeKind::Arg(_) => "arg",
        murphy_ast::NodeKind::Unknown => "unknown",
        murphy_ast::NodeKind::Gvasgn { .. } => "gvasgn",
        murphy_ast::NodeKind::Cvasgn { .. } => "cvasgn",
        murphy_ast::NodeKind::Optarg { .. } => "optarg",
        murphy_ast::NodeKind::Restarg(_) => "restarg",
        murphy_ast::NodeKind::Kwarg(_) => "kwarg",
        murphy_ast::NodeKind::Kwoptarg { .. } => "kwoptarg",
        murphy_ast::NodeKind::Kwrestarg(_) => "kwrestarg",
        murphy_ast::NodeKind::Blockarg(_) => "blockarg",
        murphy_ast::NodeKind::Kwsplat(_) => "kwsplat",
        murphy_ast::NodeKind::While { .. } => "while",
        murphy_ast::NodeKind::Until { .. } => "until",
        murphy_ast::NodeKind::RangeExpr { .. } => "irange",
        murphy_ast::NodeKind::Sclass { .. } => "sclass",
        murphy_ast::NodeKind::Break(_) => "break",
        murphy_ast::NodeKind::Next(_) => "next",
        murphy_ast::NodeKind::Yield(_) => "yield",
        murphy_ast::NodeKind::Super(_) => "super",
        murphy_ast::NodeKind::Zsuper => "zsuper",
        murphy_ast::NodeKind::Defined(_) => "defined?",
        murphy_ast::NodeKind::Rescue { .. } => "rescue",
        murphy_ast::NodeKind::Resbody { .. } => "resbody",
        murphy_ast::NodeKind::Ensure { .. } => "ensure",
        murphy_ast::NodeKind::OpAsgn { .. } => "op_asgn",
        murphy_ast::NodeKind::OrAsgn { .. } => "or_asgn",
        murphy_ast::NodeKind::AndAsgn { .. } => "and_asgn",
        murphy_ast::NodeKind::Dstr(_) => "dstr",
        murphy_ast::NodeKind::Dsym(_) => "dsym",
        murphy_ast::NodeKind::Xstr(_) => "xstr",
        murphy_ast::NodeKind::Regexp { .. } => "regexp",
        murphy_ast::NodeKind::Masgn { .. } => "masgn",
        murphy_ast::NodeKind::Mlhs(_) => "mlhs",
    }
}

fn ruby_single_quoted_literal_body(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn ruby_double_quoted_literal_body(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\0' => out.push_str("\\0"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '#' => out.push_str("\\#"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

unsafe fn ruby_symbol(mrb: *mut mrb_state, name: &str) -> mrb_value {
    let Ok(lit) = CString::new(format!(":'{}'", ruby_single_quoted_literal_body(name))) else {
        // SAFETY: symbol names with interior NUL cannot be represented as a C
        // eval string; degrade rather than panic in a user-triggerable native.
        return unsafe { ruby_nil(mrb) };
    };
    // SAFETY: `mrb` valid & non-null; generated single-quoted symbol literal.
    // Single quotes avoid Ruby interpolation of interned names like `#{...}`.
    unsafe { eval_literal(mrb, &lit) }
}

unsafe fn ruby_node_id(mrb: *mut mrb_state, id: NodeId) -> mrb_value {
    // SAFETY: `mrb` valid & non-null; generated decimal integer literal.
    unsafe { ruby_int(mrb, id.0) }
}

unsafe fn ruby_opt_node_id(mrb: *mut mrb_state, id: murphy_ast::OptNodeId) -> mrb_value {
    match id.get() {
        // SAFETY: `mrb` valid & non-null; generated decimal integer literal.
        Some(id) => unsafe { ruby_node_id(mrb, id) },
        // SAFETY: `mrb` valid & non-null; `nil` literal.
        None => unsafe { ruby_nil(mrb) },
    }
}

unsafe fn ruby_symbol_field(
    mrb: *mut mrb_state,
    ast: &murphy_ast::Ast,
    symbol: murphy_ast::Symbol,
) -> mrb_value {
    // SAFETY: `mrb` valid & non-null; generated quoted symbol literal.
    unsafe { ruby_symbol(mrb, ast.interner().resolve(symbol.0)) }
}

unsafe fn ruby_regexp_options(
    mrb: *mut mrb_state,
    ast: &murphy_ast::Ast,
    opts: murphy_ast::Symbol,
) -> mrb_value {
    let mut flags = 0_u32;
    for ch in ast.interner().resolve(opts.0).chars() {
        match ch {
            'i' => flags |= 1,
            'x' => flags |= 2,
            'm' => flags |= 4,
            _ => {}
        }
    }
    // SAFETY: `mrb` valid & non-null; generated decimal integer literal.
    unsafe { ruby_int(mrb, flags) }
}

unsafe fn ruby_string_field(
    mrb: *mut mrb_state,
    ast: &murphy_ast::Ast,
    string: murphy_ast::StringId,
) -> mrb_value {
    // SAFETY: `mrb` valid & non-null; byte slice is copied by mruby.
    unsafe { ruby_string_from_bytes(mrb, ast.interner().resolve(string.0).as_bytes()) }
}

unsafe fn raise_argument_error(mrb: *mut mrb_state, message: &'static CStr) -> mrb_value {
    // SAFETY: `mrb` valid & non-null; ArgumentError is a core class in mruby.
    let error_class = unsafe { mrb_class_get(mrb, c"ArgumentError".as_ptr()) };
    // SAFETY: `error_class` names a valid exception class and `message` is a
    // static NUL-terminated string. mruby raises non-locally; returning nil is
    // only for the C ABI type if control comes back.
    unsafe { mrb_raise(mrb, error_class, message.as_ptr()) };
    // SAFETY: fallback value after raise.
    unsafe { ruby_nil(mrb) }
}

unsafe fn ruby_node_list_field(
    mrb: *mut mrb_state,
    ast: &murphy_ast::Ast,
    list: NodeList,
) -> mrb_value {
    match node_list(ast, list) {
        // SAFETY: `mrb` valid & non-null; generated integer-array literal.
        Some(ids) => unsafe { ruby_int_array(mrb, ids.iter().copied()) },
        // SAFETY: malformed side-table reference degrades to nil rather than panicking.
        None => unsafe { ruby_nil(mrb) },
    }
}

unsafe fn ruby_range_array(mrb: *mut mrb_state, range: murphy_ast::Range) -> mrb_value {
    let lit = CString::new(format!("[{},{}]", range.start, range.end))
        .expect("decimal range array literal, no NUL");
    // SAFETY: `mrb` valid & non-null; `lit` is a generated two-integer array literal.
    unsafe { eval_literal(mrb, &lit) }
}

fn node_list(ast: &murphy_ast::Ast, list: NodeList) -> Option<&[NodeId]> {
    let raw = ast.raw_parts().node_lists;
    let start = list.start as usize;
    let len = list.len as usize;
    let end = start.checked_add(len)?;
    raw.get(start..end)
}

unsafe extern "C" fn native_node_kind(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let handle = unsafe { arg_handle(mrb) };

    match c
        .ast()
        .and_then(|ast| node_id(c, handle).map(|id| node_kind_symbol_name(ast.kind(id))))
    {
        // SAFETY: `mrb` valid & non-null; generated quoted symbol literal.
        Some(name) => unsafe { ruby_symbol(mrb, name) },
        // SAFETY: `mrb` valid & non-null; `nil` literal.
        None => unsafe { ruby_nil(mrb) },
    }
}

unsafe extern "C" fn native_node_parent(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let handle = unsafe { arg_handle(mrb) };

    match c
        .ast()
        .and_then(|ast| node_id(c, handle).and_then(|id| ast.parent(id).get()))
    {
        // SAFETY: `mrb` valid & non-null; generated decimal integer literal.
        Some(parent) => unsafe { ruby_int(mrb, parent.0) },
        // SAFETY: `mrb` valid & non-null; `nil` literal.
        None => unsafe { ruby_nil(mrb) },
    }
}

unsafe extern "C" fn native_node_children(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let handle = unsafe { arg_handle(mrb) };

    match c
        .ast()
        .and_then(|ast| node_id(c, handle).map(|id| (ast, id)))
    {
        // SAFETY: `mrb` valid & non-null; generated integer-array literal.
        Some((ast, id)) => unsafe { ruby_int_array(mrb, ast.children(id)) },
        // SAFETY: `mrb` valid & non-null; empty-array literal.
        None => unsafe { ruby_empty_array(mrb) },
    }
}

unsafe extern "C" fn native_node_ancestors(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let handle = unsafe { arg_handle(mrb) };

    match c
        .ast()
        .and_then(|ast| node_id(c, handle).map(|id| (ast, id)))
    {
        // SAFETY: `mrb` valid & non-null; generated integer-array literal.
        Some((ast, id)) => unsafe { ruby_int_array(mrb, ast.ancestors(id)) },
        // SAFETY: `mrb` valid & non-null; empty-array literal.
        None => unsafe { ruby_empty_array(mrb) },
    }
}

unsafe extern "C" fn native_node_descendants(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let handle = unsafe { arg_handle(mrb) };

    match c
        .ast()
        .and_then(|ast| node_id(c, handle).map(|id| (ast, id)))
    {
        // SAFETY: `mrb` valid & non-null; generated integer-array literal.
        Some((ast, id)) => unsafe { ruby_int_array(mrb, ast.descendants(id)) },
        // SAFETY: `mrb` valid & non-null; empty-array literal.
        None => unsafe { ruby_empty_array(mrb) },
    }
}

unsafe extern "C" fn native_node_range(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let handle = unsafe { arg_handle(mrb) };

    match c
        .ast()
        .and_then(|ast| node_id(c, handle).map(|id| ast.range(id)))
    {
        // SAFETY: `mrb` valid & non-null; generated two-integer array literal.
        Some(range) => unsafe { ruby_range_array(mrb, range) },
        // SAFETY: `mrb` valid & non-null; `nil` literal.
        None => unsafe { ruby_nil(mrb) },
    }
}

unsafe extern "C" fn native_node_field(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };

    let mut handle: mrb_int = -1;
    let mut field: mrb_sym = 0;
    // SAFETY: `mrb` is valid & non-null; `fmt` requests one integer and one
    // string/symbol intern id. `mrb_get_args("n")` accepts Ruby Symbol and
    // String, so both direct `Murphy.node_field(id, :method)` calls and the
    // prelude's wrapper path share the same native implementation.
    unsafe {
        mrb_get_args(
            mrb,
            c"in".as_ptr(),
            &mut handle as *mut mrb_int,
            &mut field as *mut mrb_sym,
        );
    }

    // SAFETY: `field` is an interned symbol id returned by mruby for this VM;
    // `mrb_sym_name` returns a NUL-terminated pointer valid for the callback.
    let field_name = unsafe { mrb_sym_name(mrb, field) };
    let Some(field) = (if field_name.is_null() {
        None
    } else {
        // SAFETY: see the `mrb_sym_name` safety note above.
        unsafe { CStr::from_ptr(field_name) }.to_str().ok()
    }) else {
        // SAFETY: `mrb` valid & non-null; `nil` literal.
        return unsafe { ruby_nil(mrb) };
    };

    let Some((ast, id)) = c
        .ast()
        .and_then(|ast| node_id(c, handle).map(|id| (ast, id)))
    else {
        // SAFETY: `mrb` valid & non-null; `nil` literal.
        return unsafe { ruby_nil(mrb) };
    };

    match (ast.kind(id), field) {
        (NodeKind::Int(value), "value") => unsafe { ruby_i64(mrb, *value) },
        (NodeKind::Float(value), "value") => unsafe { ruby_float(mrb, *value) },
        (NodeKind::Str(value), "value") => unsafe { ruby_string_field(mrb, ast, *value) },
        (NodeKind::Sym(value), "value") => unsafe { ruby_symbol_field(mrb, ast, *value) },

        (
            NodeKind::Lvar(name)
            | NodeKind::Ivar(name)
            | NodeKind::Cvar(name)
            | NodeKind::Gvar(name),
            "name",
        ) => unsafe { ruby_symbol_field(mrb, ast, *name) },
        (NodeKind::Const { scope, .. }, "scope" | "parent") => unsafe {
            ruby_opt_node_id(mrb, *scope)
        },
        (NodeKind::Const { name, .. }, "name") => unsafe { ruby_symbol_field(mrb, ast, *name) },

        (
            NodeKind::Lvasgn { name, .. }
            | NodeKind::Ivasgn { name, .. }
            | NodeKind::Gvasgn { name, .. }
            | NodeKind::Cvasgn { name, .. },
            "name",
        ) => unsafe { ruby_symbol_field(mrb, ast, *name) },
        (
            NodeKind::Lvasgn { value, .. }
            | NodeKind::Ivasgn { value, .. }
            | NodeKind::Gvasgn { value, .. }
            | NodeKind::Cvasgn { value, .. },
            "value",
        ) => unsafe { ruby_opt_node_id(mrb, *value) },
        (NodeKind::Casgn { scope, .. }, "scope" | "parent") => unsafe {
            ruby_opt_node_id(mrb, *scope)
        },
        (NodeKind::Casgn { name, .. }, "name") => unsafe { ruby_symbol_field(mrb, ast, *name) },
        (NodeKind::Casgn { value, .. }, "value") => unsafe { ruby_opt_node_id(mrb, *value) },

        (NodeKind::Send { receiver, .. }, "receiver") => unsafe {
            ruby_opt_node_id(mrb, *receiver)
        },
        (NodeKind::Csend { receiver, .. }, "receiver") => unsafe { ruby_node_id(mrb, *receiver) },
        (NodeKind::Send { method, .. } | NodeKind::Csend { method, .. }, "method") => unsafe {
            ruby_symbol_field(mrb, ast, *method)
        },
        (NodeKind::Send { args, .. } | NodeKind::Csend { args, .. }, "args" | "arguments") => unsafe {
            ruby_node_list_field(mrb, ast, *args)
        },
        (NodeKind::Send { .. } | NodeKind::Csend { .. }, "block") => unsafe { ruby_nil(mrb) },
        (NodeKind::Block { call, .. }, "call") => unsafe { ruby_node_id(mrb, *call) },
        (NodeKind::Block { args, .. }, "args" | "arguments") => unsafe { ruby_node_id(mrb, *args) },
        (NodeKind::Block { body, .. }, "body") => unsafe { ruby_opt_node_id(mrb, *body) },
        (
            NodeKind::BlockPass(value) | NodeKind::Splat(value) | NodeKind::Kwsplat(value),
            "value",
        ) => unsafe { ruby_opt_node_id(mrb, *value) },

        (NodeKind::Array(elements), "elements" | "children")
        | (NodeKind::Hash(elements), "elements" | "pairs")
        | (NodeKind::Begin(elements), "statements" | "children" | "body")
        | (NodeKind::Args(elements), "children" | "arguments" | "args")
        | (NodeKind::Mlhs(elements), "children" | "elements" | "targets") => unsafe {
            ruby_node_list_field(mrb, ast, *elements)
        },
        (NodeKind::Pair { key, .. }, "key") => unsafe { ruby_node_id(mrb, *key) },
        (NodeKind::Pair { value, .. }, "value") => unsafe { ruby_node_id(mrb, *value) },

        (NodeKind::If { cond, .. }, "cond" | "condition") => unsafe { ruby_node_id(mrb, *cond) },
        (NodeKind::If { then_, .. }, "then" | "then_") => unsafe { ruby_opt_node_id(mrb, *then_) },
        (NodeKind::If { else_, .. }, "else" | "else_") => unsafe { ruby_opt_node_id(mrb, *else_) },
        (NodeKind::Case { subject, .. }, "subject") => unsafe { ruby_opt_node_id(mrb, *subject) },
        (NodeKind::Case { whens, .. }, "whens") => unsafe {
            ruby_node_list_field(mrb, ast, *whens)
        },
        (NodeKind::Case { else_, .. }, "else" | "else_") => unsafe {
            ruby_opt_node_id(mrb, *else_)
        },
        (NodeKind::When { conds, .. }, "conds" | "conditions") => unsafe {
            ruby_node_list_field(mrb, ast, *conds)
        },
        (NodeKind::When { body, .. }, "body") => unsafe { ruby_opt_node_id(mrb, *body) },
        (NodeKind::Return(value) | NodeKind::Break(value) | NodeKind::Next(value), "value") => unsafe {
            ruby_opt_node_id(mrb, *value)
        },
        (NodeKind::And { lhs, .. } | NodeKind::Or { lhs, .. }, "lhs" | "left") => unsafe {
            ruby_node_id(mrb, *lhs)
        },
        (NodeKind::And { rhs, .. } | NodeKind::Or { rhs, .. }, "rhs" | "right") => unsafe {
            ruby_node_id(mrb, *rhs)
        },

        (NodeKind::Def { receiver, .. }, "receiver") => unsafe { ruby_opt_node_id(mrb, *receiver) },
        (NodeKind::Def { name, .. }, "name") => unsafe { ruby_symbol_field(mrb, ast, *name) },
        (NodeKind::Def { args, .. }, "args" | "arguments") => unsafe { ruby_node_id(mrb, *args) },
        (NodeKind::Def { body, .. }, "body") => unsafe { ruby_opt_node_id(mrb, *body) },
        (NodeKind::Class { name, .. } | NodeKind::Module { name, .. }, "name") => unsafe {
            ruby_node_id(mrb, *name)
        },
        (NodeKind::Class { superclass, .. }, "superclass") => unsafe {
            ruby_opt_node_id(mrb, *superclass)
        },
        (NodeKind::Class { body, .. } | NodeKind::Module { body, .. }, "body") => unsafe {
            ruby_opt_node_id(mrb, *body)
        },
        (NodeKind::Sclass { expr, .. }, "expr" | "expression") => unsafe {
            ruby_node_id(mrb, *expr)
        },
        (NodeKind::Sclass { body, .. }, "body") => unsafe { ruby_opt_node_id(mrb, *body) },

        (
            NodeKind::Arg(name)
            | NodeKind::Restarg(name)
            | NodeKind::Kwarg(name)
            | NodeKind::Kwrestarg(name)
            | NodeKind::Blockarg(name),
            "name",
        ) => unsafe { ruby_symbol_field(mrb, ast, *name) },
        (NodeKind::Optarg { name, .. } | NodeKind::Kwoptarg { name, .. }, "name") => unsafe {
            ruby_symbol_field(mrb, ast, *name)
        },
        (NodeKind::Optarg { default, .. } | NodeKind::Kwoptarg { default, .. }, "default") => unsafe {
            ruby_node_id(mrb, *default)
        },

        (NodeKind::While { cond, .. } | NodeKind::Until { cond, .. }, "cond" | "condition") => unsafe {
            ruby_node_id(mrb, *cond)
        },
        (NodeKind::While { body, .. } | NodeKind::Until { body, .. }, "body") => unsafe {
            ruby_opt_node_id(mrb, *body)
        },
        (NodeKind::While { post, .. } | NodeKind::Until { post, .. }, "post") => unsafe {
            ruby_bool(mrb, *post)
        },
        (NodeKind::RangeExpr { begin_, .. }, "begin" | "begin_") => unsafe {
            ruby_opt_node_id(mrb, *begin_)
        },
        (NodeKind::RangeExpr { end_, .. }, "end" | "end_") => unsafe {
            ruby_opt_node_id(mrb, *end_)
        },
        (NodeKind::RangeExpr { exclusive, .. }, "exclusive") => unsafe {
            ruby_bool(mrb, *exclusive)
        },

        (NodeKind::Yield(args) | NodeKind::Super(args), "args" | "arguments") => unsafe {
            ruby_node_list_field(mrb, ast, *args)
        },
        (NodeKind::Defined(expr), "expr" | "value") => unsafe { ruby_node_id(mrb, *expr) },

        (NodeKind::Rescue { body, .. }, "body") => unsafe { ruby_opt_node_id(mrb, *body) },
        (NodeKind::Rescue { resbodies, .. }, "resbodies" | "rescues") => unsafe {
            ruby_node_list_field(mrb, ast, *resbodies)
        },
        (NodeKind::Rescue { else_, .. }, "else" | "else_") => unsafe {
            ruby_opt_node_id(mrb, *else_)
        },
        (NodeKind::Resbody { exceptions, .. }, "exceptions") => unsafe {
            ruby_node_list_field(mrb, ast, *exceptions)
        },
        (NodeKind::Resbody { var, .. }, "var") => unsafe { ruby_opt_node_id(mrb, *var) },
        (NodeKind::Resbody { body, .. }, "body") => unsafe { ruby_opt_node_id(mrb, *body) },
        (NodeKind::Ensure { body, .. }, "body") => unsafe { ruby_opt_node_id(mrb, *body) },
        (NodeKind::Ensure { ensure_, .. }, "ensure" | "ensure_") => unsafe {
            ruby_opt_node_id(mrb, *ensure_)
        },

        (
            NodeKind::OpAsgn { target, .. }
            | NodeKind::OrAsgn { target, .. }
            | NodeKind::AndAsgn { target, .. },
            "target",
        ) => unsafe { ruby_node_id(mrb, *target) },
        (NodeKind::OpAsgn { op, .. }, "op") => unsafe { ruby_symbol_field(mrb, ast, *op) },
        (
            NodeKind::OpAsgn { value, .. }
            | NodeKind::OrAsgn { value, .. }
            | NodeKind::AndAsgn { value, .. },
            "value",
        ) => unsafe { ruby_node_id(mrb, *value) },

        (NodeKind::Dstr(parts) | NodeKind::Dsym(parts) | NodeKind::Xstr(parts), "parts") => unsafe {
            ruby_node_list_field(mrb, ast, *parts)
        },
        (NodeKind::Regexp { parts, .. }, "parts") => unsafe {
            ruby_node_list_field(mrb, ast, *parts)
        },
        (NodeKind::Regexp { opts, .. }, "opts") => unsafe { ruby_symbol_field(mrb, ast, *opts) },
        (NodeKind::Regexp { opts, .. }, "options") => unsafe {
            ruby_regexp_options(mrb, ast, *opts)
        },
        (NodeKind::Masgn { lhs, .. }, "lhs") => unsafe { ruby_node_id(mrb, *lhs) },
        (NodeKind::Masgn { rhs, .. }, "rhs") => unsafe { ruby_node_id(mrb, *rhs) },

        _ => unsafe { ruby_nil(mrb) },
    }
}

unsafe extern "C" fn native_symbol_str(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let handle = unsafe { arg_handle(mrb) };

    let Some(ast) = c.ast() else {
        return unsafe { raise_argument_error(mrb, c"invalid symbol handle") };
    };
    let Ok(index) = u32::try_from(handle) else {
        return unsafe { raise_argument_error(mrb, c"invalid symbol handle") };
    };
    if index as usize >= ast.interner().len() {
        return unsafe { raise_argument_error(mrb, c"invalid symbol handle") };
    }
    // SAFETY: `mrb` valid & non-null; interner string is valid UTF-8 bytes and copied by mruby.
    unsafe { ruby_string_from_bytes(mrb, ast.interner().resolve(index).as_bytes()) }
}

unsafe extern "C" fn native_string_str(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // The arena interner stores Symbol and StringId in the same index space.
    // SAFETY: same native callback contract as `native_symbol_str`.
    unsafe { native_symbol_str(mrb, _self) }
}

unsafe extern "C" fn native_raw_source(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };

    let mut start: mrb_int = -1;
    let mut end: mrb_int = -1;
    // SAFETY: `mrb` valid & non-null; `fmt` requests exactly two `mrb_int`s;
    // `start`/`end` are live correctly-typed out-pointers outliving the call.
    unsafe {
        mrb_get_args(
            mrb,
            c"ii".as_ptr(),
            &mut start as *mut mrb_int,
            &mut end as *mut mrb_int,
        );
    }

    let Some(ast) = c.ast() else {
        // SAFETY: `mrb` valid & non-null; `nil` literal.
        return unsafe { ruby_nil(mrb) };
    };
    let source = ast.source();
    let slice = if start < 0 || end < 0 {
        None
    } else {
        let (start, end) = (start as usize, end as usize);
        if start <= end
            && end <= source.len()
            && source.is_char_boundary(start)
            && source.is_char_boundary(end)
        {
            Some(&source.as_bytes()[start..end])
        } else {
            None
        }
    };

    match slice {
        // SAFETY: `mrb` valid & non-null; byte slice is copied by mruby.
        Some(bytes) => unsafe { ruby_string_from_bytes(mrb, bytes) },
        // SAFETY: `mrb` valid & non-null; `nil` literal.
        None => unsafe { ruby_nil(mrb) },
    }
}

unsafe extern "C" fn native_comments(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    let Some(ast) = c.ast() else {
        // SAFETY: `mrb` valid & non-null; empty-array literal.
        return unsafe { ruby_empty_array(mrb) };
    };

    let mut lit = String::from("[");
    for (i, comment) in ast.comments().iter().enumerate() {
        if i > 0 {
            lit.push(',');
        }
        let text = ast.raw_source(comment.range);
        lit.push('[');
        lit.push_str(&comment.range.start.to_string());
        lit.push(',');
        lit.push_str(&comment.range.end.to_string());
        lit.push(',');
        lit.push('"');
        lit.push_str(&ruby_double_quoted_literal_body(text));
        lit.push_str("\"]");
    }
    lit.push(']');

    let Ok(lit) = CString::new(lit) else {
        // SAFETY: comments containing interior NUL cannot be represented in the
        // generated eval literal. Degrade rather than panic.
        return unsafe { ruby_nil(mrb) };
    };
    // SAFETY: `mrb` valid & non-null; generated array literal contains only
    // decimal offsets and single-quoted, escaped source text.
    unsafe { eval_literal(mrb, &lit) }
}

/// `Murphy.node_count -> Integer`. The size of the handle space `0..count`.
/// Resolved by a live re-walk; nothing is cached (ADR 0008).
#[allow(dead_code)]
unsafe extern "C" fn native_node_count(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: inside a native callback; `mrb` valid & non-null (mruby
    // guarantee); `ud` set to the live `Arc<AstContext>` ptr by the caller.
    let n = count_call_nodes(unsafe { ctx(mrb) });
    let lit = CString::new(n.to_string()).expect("decimal digits, no NUL");
    // SAFETY: `mrb` valid & non-null; `lit` is a NUL-terminated decimal
    // integer literal.
    unsafe { eval_literal(mrb, &lit) }
}

/// `Murphy.node_name(handle) -> String`. Resolves the handle to the LIVE
/// prism call node and reads its real `name()`. Only the integer crossed FFI.
#[allow(dead_code)]
unsafe extern "C" fn native_node_name(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let handle = unsafe { arg_handle(mrb) };

    // A negative handle is out of range exactly like a too-large one: route it
    // through the same `None` path (do NOT `.max(0)`-clamp — that would alias a
    // negative handle to handle 0 and return a REAL WRONG node, ADR 0008).
    let name = usize::try_from(handle)
        .ok()
        .and_then(|h| with_call_node(c, h, |n| n.name().as_slice().to_vec()));

    match name {
        // SAFETY: `mrb` valid & non-null; byte slice is copied by mruby.
        Some(bytes) => unsafe { ruby_string_from_bytes(mrb, &bytes) },
        None => unsafe { eval_literal(mrb, c"nil") },
    }
}

/// `Murphy.node_receiver_nil?(handle) -> true/false`. Reads the LIVE
/// `node.receiver().is_none()` (an out-of-range handle reads as `true`/nil-ish
/// — there is no node, so there is no receiver).
#[allow(dead_code)]
unsafe extern "C" fn native_node_receiver_nil(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let handle = unsafe { arg_handle(mrb) };

    // Negative handle == out of range (no `.max(0)` aliasing). OOB (including
    // negative) reads as `true` — no node, so no receiver — unchanged from the
    // positive-OOB behavior; only negatives are made consistent with it.
    let is_nil = usize::try_from(handle)
        .ok()
        .and_then(|h| with_call_node(c, h, |n| n.receiver().is_none()))
        .unwrap_or(true);

    let lit: &CStr = if is_nil { c"true" } else { c"false" };
    // SAFETY: `mrb` valid & non-null; `lit` is a NUL-terminated Ruby boolean
    // literal.
    unsafe { eval_literal(mrb, lit) }
}

fn node_message_loc(ctx: &AstContext, handle: mrb_int) -> Option<Range> {
    usize::try_from(handle).ok().and_then(|h| {
        with_call_node(ctx, h, |n| {
            n.message_loc().map(|loc| Range::from_prism_location(&loc))
        })
        .flatten()
    })
}

/// `Murphy.node_msg_start(handle) -> Integer` (BYTE offset, ADR 0001).
///
/// Reads the LIVE `node.message_loc()` through [`crate::Range`] (the single
/// audited `usize -> u32` byte narrowing, ADR 0001 — never char). Missing,
/// negative, or out-of-range handles return `-1`; Ruby glue converts any
/// negative offset to `nil` for `Node#message_loc`.
#[allow(dead_code)]
unsafe extern "C" fn native_node_msg_start(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let handle = unsafe { arg_handle(mrb) };

    let start = node_message_loc(c, handle)
        .map(|r| mrb_int::from(r.start_offset))
        .unwrap_or(-1);
    let lit = CString::new(start.to_string()).expect("decimal digits, no NUL");
    // SAFETY: `mrb` valid & non-null; `lit` is a NUL-terminated decimal
    // integer literal.
    unsafe { eval_literal(mrb, &lit) }
}

/// `Murphy.node_msg_end(handle) -> Integer` (BYTE offset, ADR 0001).
///
/// Same contract as [`native_node_msg_start`], returning the exclusive end
/// offset or `-1` when the message location is absent/out of range.
#[allow(dead_code)]
unsafe extern "C" fn native_node_msg_end(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let handle = unsafe { arg_handle(mrb) };

    let end = node_message_loc(c, handle)
        .map(|r| mrb_int::from(r.end_offset))
        .unwrap_or(-1);
    let lit = CString::new(end.to_string()).expect("decimal digits, no NUL");
    // SAFETY: `mrb` valid & non-null; `lit` is a NUL-terminated decimal
    // integer literal.
    unsafe { eval_literal(mrb, &lit) }
}

/// `Murphy.source_slice(start, end) -> String`. The BYTE slice
/// `source[start..end]` of the original parsed source (ADR 0001 byte offsets;
/// pair with `node_msg_start`/`node_msg_end`). Returns `nil` on an out-of-range or inverted
/// range rather than panicking — a user cop must not be able to crash the
/// engine with a bad range.
#[allow(dead_code)]
unsafe extern "C" fn native_source_slice(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };

    let mut start: mrb_int = -1;
    let mut end: mrb_int = -1;
    let fmt = c"ii";
    // SAFETY: `mrb` valid & non-null; `fmt` requests exactly two `mrb_int`s;
    // `start`/`end` are live correctly-typed out-pointers outliving the call.
    unsafe {
        mrb_get_args(
            mrb,
            fmt.as_ptr(),
            &mut start as *mut mrb_int,
            &mut end as *mut mrb_int,
        );
    }

    let src = c.source();
    let slice = if start < 0 || end < 0 {
        None
    } else {
        let (s, e) = (start as usize, end as usize);
        if s <= e && e <= src.len() {
            Some(&src[s..e])
        } else {
            None
        }
    };

    match slice {
        // SAFETY: `mrb` valid & non-null; the byte slice is copied by mruby
        // (length-delimited, NUL-safe).
        Some(bytes) => unsafe { ruby_string_from_bytes(mrb, bytes) },
        None => unsafe { eval_literal(mrb, c"nil") },
    }
}

/// Register the read-only native primitives on `mrb` as module functions of a
/// `Murphy` class (matching the proven spike's `Murphy.node_*` surface).
///
/// The caller MUST have already stored the live `CopRun` pointer in
/// `(*mrb).ud` (via `crate::MrubyState::set_cop_run`; `ctx` projects
/// `&(*p).ctx`) and MUST keep that `CopRun` (with its `Arc<AstContext>`
/// clone) alive for the whole duration of any subsequent `eval` (ADR 0009
/// rule 1). This only *defines* the functions; it reads nothing.
///
/// # Safety
///
/// `mrb` must be a valid non-null `mrb_state` (e.g. from
/// [`crate::MrubyState`]). This must be called before any `eval` that invokes
/// the primitives.
///
/// `pub(crate)`: every caller (Task 4/5/7 dispatch/pipeline, these tests) is
/// in-crate — same discipline Task 2 set for `MrubyState::raw()`. Nothing
/// outside `murphy-core` should register the primitive surface. There is no
/// non-test caller yet (the only callers are this module's `#[cfg(test)]`
/// tests); Task 4 lands the first production caller.
// Task 4 lands the first non-test caller; remove this allow when wired.
#[allow(dead_code)]
pub(crate) unsafe fn register(mrb: *mut mrb_state) {
    // COUPLING: `cop_prelude.rb` REOPENS `Murphy` with `class Murphy` — it must
    // stay a class. Do NOT switch this to `mrb_define_module` without updating
    // the prelude, or the prelude eval raises `TypeError` (class vs module).
    // SAFETY: `mrb` is a valid non-null `mrb_state`; "Object" is a built-in
    // class always present in a fresh mruby state; `mrb_class_get` /
    // `mrb_define_class` are the documented class-definition entry points.
    let murphy: *mut RClass = unsafe {
        let obj = mrb_class_get(mrb, c"Object".as_ptr());
        mrb_define_class(mrb, c"Murphy".as_ptr(), obj)
    };

    // SAFETY: `mrb` valid & non-null; `murphy` is the class just defined;
    // every `name` is a static NUL-terminated identifier; each function
    // pointer matches the mruby native-callback ABI; `args_req` reproduces
    // `MRB_ARGS_REQ` (ADR 0002 finding 1).
    unsafe {
        let def = |name: &CStr,
                   f: unsafe extern "C" fn(*mut mrb_state, mrb_value) -> mrb_value,
                   argc: u32| {
            mrb_define_module_function(mrb, murphy, name.as_ptr(), Some(f), args_req(argc));
        };
        def(c"node_kind", native_node_kind, 1);
        def(c"node_parent", native_node_parent, 1);
        def(c"node_children", native_node_children, 1);
        def(c"node_ancestors", native_node_ancestors, 1);
        def(c"node_descendants", native_node_descendants, 1);
        def(c"node_range", native_node_range, 1);
        def(c"node_field", native_node_field, 2);
        def(c"symbol_str", native_symbol_str, 1);
        def(c"string_str", native_string_str, 1);
        def(c"raw_source", native_raw_source, 2);
        def(c"comments", native_comments, 0);
    }
}

#[cfg(test)]
mod tests {
    //! `.rb`-driven tests for the read-only live native-primitive IDL.
    //!
    //! Each test parses a known snippet into a real `AstContext`, opens an
    //! `MrubyState`, stores the live `Arc<AstContext>` ptr in `ud`, registers
    //! the production primitives, then `eval`s a small `.rb` script that drives
    //! each primitive over every handle and reports the live-resolved data back
    //! to Rust for assertion against HAND-DERIVED BYTE values (ADR 0001).
    //!
    //! The report channel is **test instrumentation only** — a `__test_report`
    //! native registered ONLY here, backed by a process-wide `Mutex<Vec<…>>`.
    //! It is NOT the cop offense sink (ADR 0009 rule 2 — a cop-instance-owned
    //! local; Task 4). The two report-using tests serialize via `SINK_GUARD` so
    //! the shared sink is never raced (the production primitives carry no sink).

    use super::*;
    use crate::{AstContext, MrubyState};
    use std::sync::{Mutex, MutexGuard};

    /// Test-instrumentation sink. Process-wide; `SINK_GUARD` serializes the
    /// two tests that use it so it is filled/drained by exactly one at a time.
    static SINK: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static SINK_GUARD: Mutex<()> = Mutex::new(());

    fn lock_sink() -> MutexGuard<'static, ()> {
        let g = SINK_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        SINK.lock().unwrap_or_else(|e| e.into_inner()).clear();
        g
    }

    fn drain_sink() -> Vec<String> {
        std::mem::take(&mut *SINK.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// `Murphy.__test_report(str)` — test-only. Pushes the reported string
    /// into `SINK`. NOT a production primitive (no offense sink in Task 3).
    unsafe extern "C" fn native_test_report(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
        let mut p: *const std::os::raw::c_char = std::ptr::null();
        // SAFETY: native callback; `mrb` valid & non-null; `c"z"` requests one
        // C string; `p` is a live out-pointer outliving the call.
        unsafe {
            mrb_get_args(
                mrb,
                c"z".as_ptr(),
                &mut p as *mut *const std::os::raw::c_char,
            )
        };
        // SAFETY: mruby guarantees `p` points at a NUL-terminated C string for
        // the duration of the callback when `mrb_get_args` succeeds with "z".
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        SINK.lock().unwrap_or_else(|e| e.into_inner()).push(s);
        // SAFETY: `mrb` valid & non-null; "nil" is a trivial literal.
        unsafe { eval_literal(mrb, c"nil") }
    }

    /// Register the test-only `__test_report` on the `Murphy` class (defined
    /// by `register`, called first).
    unsafe fn register_test_report(mrb: *mut mrb_state) {
        // SAFETY: `mrb` valid & non-null; `Murphy` exists (register ran first);
        // the name is a static NUL-terminated id; the fn matches the ABI.
        unsafe {
            let murphy = mrb_class_get(mrb, c"Murphy".as_ptr());
            mrb_define_module_function(
                mrb,
                murphy,
                c"__test_report".as_ptr(),
                Some(native_test_report),
                args_req(1),
            );
        }
    }

    /// Drive every handle through name/receiver_nil?/msg_start/msg_end/source_slice
    /// and report `"i|name|recv_nil|start,end|slice"` per node.
    const DRIVER: &str = r##"
        Murphy.node_count.times do |i|
          name  = Murphy.node_name(i)
          rnil  = Murphy.node_receiver_nil?(i)
          start = Murphy.node_msg_start(i)
          stop  = Murphy.node_msg_end(i)
          # Negative offsets are the missing/OOB sentinel. Every handle here is
          # in range with a real message_loc, so this reports real byte offsets.
          if start >= 0 && stop >= 0
            slice = Murphy.source_slice(start, stop)
            Murphy.__test_report("#{i}|#{name}|#{rnil}|#{start},#{stop}|#{slice}")
          else
            Murphy.__test_report("#{i}|#{name}|#{rnil}|nil|nil")
          end
        end
    "##;

    fn run_driver_over(src: &str) -> Vec<String> {
        run_script_over(src, DRIVER)
    }

    fn run_script_over(src: &str, script: &str) -> Vec<String> {
        let ctx = AstContext::new(src.as_bytes().to_vec());
        // ADR 0009 rule 1: the worker owns its own Arc clone (here the test is
        // the "worker"); `ud` is not the liveness guarantee.
        let worker = std::sync::Arc::clone(&ctx);
        // Task-4 ud-payload: `ud` now carries a `CopRun` (not a bare
        // `Arc<AstContext>`); the primitives project `&AstContext` via
        // `CopRun::ctx`. Task-3's tests only need the `ctx` projection, so a
        // minimal `CopRun::for_test` exercises the exact same contract Task 4
        // ships. The `CopRun` is owned by this scope and outlives the `eval`
        // (ADR 0009 rule 1), dropping AFTER `st` (mrb_close).
        let cop_run = crate::mruby::sdk::CopRun::for_test(std::sync::Arc::clone(&worker));
        {
            let mut st = MrubyState::open();
            st.set_cop_run(&cop_run);
            // SAFETY: `st.raw()` is a valid non-null `mrb_state` (lives as
            // long as `st`); `ud` was set to the live `CopRun` ptr above and
            // `cop_run`/`worker`/`ctx` stay alive across the `eval`.
            // `register` only defines functions, reads nothing.
            unsafe {
                register(st.raw());
                register_test_report(st.raw());
            }
            st.eval(script);
            // `st` drops here → mrb_close, BEFORE the CopRun / Arc clones drop
            // (normal-path ordering, ADR 0009 rule 4).
        }
        drop(cop_run);
        drop(worker);
        drop(ctx);
        drain_sink()
    }

    fn all_node_kind_variants() -> Vec<murphy_ast::NodeKind> {
        use murphy_ast::{NodeId, NodeKind, NodeList, OptNodeId, StringId, Symbol};
        let n = NodeId(0);
        let s = Symbol(0);
        vec![
            NodeKind::Error,
            NodeKind::Nil,
            NodeKind::True_,
            NodeKind::False_,
            NodeKind::SelfExpr,
            NodeKind::Int(0),
            NodeKind::Float(0.0),
            NodeKind::Str(StringId(0)),
            NodeKind::Sym(s),
            NodeKind::Lvar(s),
            NodeKind::Ivar(s),
            NodeKind::Cvar(s),
            NodeKind::Gvar(s),
            NodeKind::Const {
                scope: OptNodeId::NONE,
                name: s,
            },
            NodeKind::Lvasgn {
                name: s,
                value: OptNodeId::NONE,
            },
            NodeKind::Ivasgn {
                name: s,
                value: OptNodeId::NONE,
            },
            NodeKind::Casgn {
                scope: OptNodeId::NONE,
                name: s,
                value: OptNodeId::NONE,
            },
            NodeKind::Send {
                receiver: OptNodeId::NONE,
                method: s,
                args: NodeList::EMPTY,
            },
            NodeKind::Csend {
                receiver: n,
                method: s,
                args: NodeList::EMPTY,
            },
            NodeKind::Block {
                call: n,
                args: n,
                body: OptNodeId::NONE,
            },
            NodeKind::BlockPass(OptNodeId::NONE),
            NodeKind::Splat(OptNodeId::NONE),
            NodeKind::Array(NodeList::EMPTY),
            NodeKind::Hash(NodeList::EMPTY),
            NodeKind::Pair { key: n, value: n },
            NodeKind::If {
                cond: n,
                then_: OptNodeId::NONE,
                else_: OptNodeId::NONE,
            },
            NodeKind::Case {
                subject: OptNodeId::NONE,
                whens: NodeList::EMPTY,
                else_: OptNodeId::NONE,
            },
            NodeKind::When {
                conds: NodeList::EMPTY,
                body: OptNodeId::NONE,
            },
            NodeKind::Begin(NodeList::EMPTY),
            NodeKind::Return(OptNodeId::NONE),
            NodeKind::And { lhs: n, rhs: n },
            NodeKind::Or { lhs: n, rhs: n },
            NodeKind::Def {
                receiver: OptNodeId::NONE,
                name: s,
                args: n,
                body: OptNodeId::NONE,
            },
            NodeKind::Class {
                name: n,
                superclass: OptNodeId::NONE,
                body: OptNodeId::NONE,
            },
            NodeKind::Module {
                name: n,
                body: OptNodeId::NONE,
            },
            NodeKind::Args(NodeList::EMPTY),
            NodeKind::Arg(s),
            NodeKind::Unknown,
            NodeKind::Gvasgn {
                name: s,
                value: OptNodeId::NONE,
            },
            NodeKind::Cvasgn {
                name: s,
                value: OptNodeId::NONE,
            },
            NodeKind::Optarg {
                name: s,
                default: n,
            },
            NodeKind::Restarg(s),
            NodeKind::Kwarg(s),
            NodeKind::Kwoptarg {
                name: s,
                default: n,
            },
            NodeKind::Kwrestarg(s),
            NodeKind::Blockarg(s),
            NodeKind::Kwsplat(OptNodeId::NONE),
            NodeKind::While {
                cond: n,
                body: OptNodeId::NONE,
                post: false,
            },
            NodeKind::Until {
                cond: n,
                body: OptNodeId::NONE,
                post: false,
            },
            NodeKind::RangeExpr {
                begin_: OptNodeId::NONE,
                end_: OptNodeId::NONE,
                exclusive: false,
            },
            NodeKind::Sclass {
                expr: n,
                body: OptNodeId::NONE,
            },
            NodeKind::Break(OptNodeId::NONE),
            NodeKind::Next(OptNodeId::NONE),
            NodeKind::Yield(NodeList::EMPTY),
            NodeKind::Super(NodeList::EMPTY),
            NodeKind::Zsuper,
            NodeKind::Defined(n),
            NodeKind::Rescue {
                body: OptNodeId::NONE,
                resbodies: NodeList::EMPTY,
                else_: OptNodeId::NONE,
            },
            NodeKind::Resbody {
                exceptions: NodeList::EMPTY,
                var: OptNodeId::NONE,
                body: OptNodeId::NONE,
            },
            NodeKind::Ensure {
                body: OptNodeId::NONE,
                ensure_: OptNodeId::NONE,
            },
            NodeKind::OpAsgn {
                target: n,
                op: s,
                value: n,
            },
            NodeKind::OrAsgn {
                target: n,
                value: n,
            },
            NodeKind::AndAsgn {
                target: n,
                value: n,
            },
            NodeKind::Dstr(NodeList::EMPTY),
            NodeKind::Dsym(NodeList::EMPTY),
            NodeKind::Xstr(NodeList::EMPTY),
            NodeKind::Regexp {
                parts: NodeList::EMPTY,
                opts: s,
            },
            NodeKind::Masgn { lhs: n, rhs: n },
            NodeKind::Mlhs(NodeList::EMPTY),
        ]
    }

    #[test]
    fn kind_symbol_mapping() {
        let expected = [
            "error",
            "nil",
            "true",
            "false",
            "self",
            "int",
            "float",
            "str",
            "sym",
            "lvar",
            "ivar",
            "cvar",
            "gvar",
            "const",
            "lvasgn",
            "ivasgn",
            "casgn",
            "send",
            "csend",
            "block",
            "block_pass",
            "splat",
            "array",
            "hash",
            "pair",
            "if",
            "case",
            "when",
            "begin",
            "return",
            "and",
            "or",
            "def",
            "class",
            "module",
            "args",
            "arg",
            "unknown",
            "gvasgn",
            "cvasgn",
            "optarg",
            "restarg",
            "kwarg",
            "kwoptarg",
            "kwrestarg",
            "blockarg",
            "kwsplat",
            "while",
            "until",
            "irange",
            "sclass",
            "break",
            "next",
            "yield",
            "super",
            "zsuper",
            "defined?",
            "rescue",
            "resbody",
            "ensure",
            "op_asgn",
            "or_asgn",
            "and_asgn",
            "dstr",
            "dsym",
            "xstr",
            "regexp",
            "masgn",
            "mlhs",
        ];

        let variants = all_node_kind_variants();
        assert_eq!(variants.len(), expected.len());
        for (kind, expected_name) in variants.iter().zip(expected) {
            assert_eq!(node_kind_symbol_name(kind), expected_name, "{kind:?}");
        }
    }

    #[test]
    fn node_kind_primitive_returns_symbols_for_arena_nodes() {
        let _g = lock_sink();
        let src = "defined?(x)\n1..2\nfoo(3)\n";
        let expected_ast = murphy_translate::translate(src, "<test>");
        let script = format!(
            r##"
            0.upto({}) do |i|
              kind = Murphy.node_kind(i)
              Murphy.__test_report("#{{i}}|#{{kind.is_a?(Symbol)}}|#{{kind}}")
            end
            Murphy.__test_report("bad=#{{Murphy.node_kind(-1).inspect}}/#{{Murphy.node_kind(999).inspect}}")
            "##,
            expected_ast.len() - 1
        );

        let mut expected = (0..expected_ast.len())
            .map(|i| {
                let kind = expected_ast.kind(NodeId(i as u32));
                format!("{i}|true|{}", node_kind_symbol_name(kind))
            })
            .collect::<Vec<_>>();
        expected.push("bad=nil/nil".to_string());

        assert_eq!(run_script_over(src, &script), expected);
    }

    #[test]
    fn arena_traversal_primitives_report_parent_children_ancestors_descendants_and_range() {
        let _g = lock_sink();
        let src = "if foo\n  bar(1)\nend\n";
        let expected_ast = murphy_translate::translate(src, "<test>");
        let max = expected_ast.len() - 1;
        let script = format!(
            r##"
            0.upto({max}) do |i|
              range = Murphy.node_range(i)
              Murphy.__test_report([
                i,
                Murphy.node_parent(i).inspect,
                Murphy.node_children(i).join(","),
                Murphy.node_ancestors(i).join(","),
                Murphy.node_descendants(i).join(","),
                range ? range.join(",") : "nil",
              ].join("|"))
            end
            Murphy.__test_report("bad_parent=#{{Murphy.node_parent(-1).inspect}}/#{{Murphy.node_parent(999).inspect}}")
            Murphy.__test_report("bad_children=#{{Murphy.node_children(-1).inspect}}/#{{Murphy.node_children(999).inspect}}")
            Murphy.__test_report("bad_ancestors=#{{Murphy.node_ancestors(-1).inspect}}/#{{Murphy.node_ancestors(999).inspect}}")
            Murphy.__test_report("bad_descendants=#{{Murphy.node_descendants(-1).inspect}}/#{{Murphy.node_descendants(999).inspect}}")
            Murphy.__test_report("bad_range=#{{Murphy.node_range(-1).inspect}}/#{{Murphy.node_range(999).inspect}}")
            "##
        );

        let mut expected = (0..expected_ast.len())
            .map(|i| {
                let id = murphy_ast::NodeId(i as u32);
                let parent = expected_ast
                    .parent(id)
                    .get()
                    .map(|p| p.0.to_string())
                    .unwrap_or_else(|| "nil".to_string());
                let children = expected_ast
                    .children(id)
                    .map(|c| c.0.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let ancestors = expected_ast
                    .ancestors(id)
                    .map(|a| a.0.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let descendants = expected_ast
                    .descendants(id)
                    .map(|d| d.0.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let range = expected_ast.range(id);
                format!(
                    "{i}|{parent}|{children}|{ancestors}|{descendants}|{},{}",
                    range.start, range.end
                )
            })
            .collect::<Vec<_>>();
        expected.extend([
            "bad_parent=nil/nil".to_string(),
            "bad_children=[]/[]".to_string(),
            "bad_ancestors=[]/[]".to_string(),
            "bad_descendants=[]/[]".to_string(),
            "bad_range=nil/nil".to_string(),
        ]);

        assert_eq!(run_script_over(src, &script), expected);
    }

    #[test]
    fn node_field_accepts_symbol_fields_for_send() {
        let _g = lock_sink();
        let src = "obj.foo(1, 2)\n";
        let expected_ast = murphy_translate::translate(src, "<test>");
        let send_id = (0..expected_ast.len())
            .map(|i| NodeId(i as u32))
            .find(|&id| match expected_ast.kind(id) {
                NodeKind::Send { method, .. } => expected_ast.interner().resolve(method.0) == "foo",
                _ => false,
            })
            .expect("fixture has the outer foo send node");
        let script = format!(
            r##"
            id = {id}
            receiver = Murphy.node_field(id, :receiver)
            method = Murphy.node_field(id, :method)
            arguments = Murphy.node_field(id, :arguments)
            block = Murphy.node_field(id, :block)
            Murphy.__test_report("receiver=#{{receiver.inspect}}")
            Murphy.__test_report("method=#{{method.is_a?(Symbol)}}/#{{method}}")
            Murphy.__test_report("arguments=#{{arguments.is_a?(Array)}}/#{{arguments.length}}")
            Murphy.__test_report("block=#{{block.inspect}}")
            Murphy.__test_report("bad=#{{Murphy.node_field(id, :missing).inspect}}/#{{Murphy.node_field(-1, :method).inspect}}")
            "##,
            id = send_id.0
        );

        let out = run_script_over(src, &script);
        assert!(out[0].starts_with("receiver="), "{out:?}");
        assert_ne!(out[0], "receiver=nil", "{out:?}");
        assert_eq!(out[1], "method=true/foo");
        assert_eq!(out[2], "arguments=true/2");
        assert_eq!(out[3], "block=nil");
        assert_eq!(out[4], "bad=nil/nil");
    }

    #[test]
    fn node_field_covers_representative_payload_families() {
        let _g = lock_sink();
        let src = r#"
class Child < Parent
  def self.m(a, b = 1, *rest, k:, kk: 2, **kw, &blk)
    @x ||= 1
    @@c &&= 2
    $g += 3
    1...2
    [1, "s", :sym, *rest]
    { k => /a#{b}/imx, **kw }
    if a && b then return b else nil end
  rescue RuntimeError => e
    break e
  ensure
    next
  end
end
"#;
        let expected_ast = murphy_translate::translate(src, "<test>");
        let find = |pred: &dyn Fn(NodeId, &NodeKind) -> bool| {
            (0..expected_ast.len())
                .map(|i| NodeId(i as u32))
                .find(|&id| pred(id, expected_ast.kind(id)))
                .unwrap_or_else(|| panic!("fixture missing expected node"))
        };
        let def_id = find(&|_, k| matches!(k, NodeKind::Def { .. }));
        let optarg_id = find(&|_, k| matches!(k, NodeKind::Optarg { .. }));
        let array_id = find(&|_, k| matches!(k, NodeKind::Array(_)));
        let pair_id = find(&|_, k| matches!(k, NodeKind::Pair { .. }));
        let if_id = find(&|_, k| matches!(k, NodeKind::If { .. }));
        let return_id = find(&|_, k| matches!(k, NodeKind::Return(_)));
        let range_id = find(&|_, k| matches!(k, NodeKind::RangeExpr { .. }));
        let rescue_id = find(&|_, k| matches!(k, NodeKind::Rescue { .. }));
        let op_asgn_id = find(&|_, k| matches!(k, NodeKind::OpAsgn { .. }));
        let or_asgn_id = find(&|_, k| matches!(k, NodeKind::OrAsgn { .. }));
        let dstr_id = find(&|_, k| matches!(k, NodeKind::Dstr(_) | NodeKind::Regexp { .. }));

        let script = format!(
            r##"
            def report(id, field)
              v = Murphy.node_field(id, field)
              Murphy.__test_report("#{{field}}=#{{v.inspect}}/#{{v.class}}")
            end

            report({def_id}, :name)
            report({def_id}, :receiver)
            report({def_id}, :args)
            report({optarg_id}, :default)
            report({array_id}, :elements)
            report({pair_id}, :key)
            report({pair_id}, :value)
            report({if_id}, :cond)
            report({return_id}, :value)
            report({range_id}, :exclusive)
            report({rescue_id}, :resbodies)
            report({op_asgn_id}, :op)
            report({or_asgn_id}, :target)
            report({dstr_id}, :parts)
            "##,
            def_id = def_id.0,
            optarg_id = optarg_id.0,
            array_id = array_id.0,
            pair_id = pair_id.0,
            if_id = if_id.0,
            return_id = return_id.0,
            range_id = range_id.0,
            rescue_id = rescue_id.0,
            op_asgn_id = op_asgn_id.0,
            or_asgn_id = or_asgn_id.0,
            dstr_id = dstr_id.0,
        );

        let out = run_script_over(src, &script);
        assert_eq!(out[0], "name=:m/Symbol");
        assert!(out[1].ends_with("/Integer"), "{out:?}");
        assert!(out[2].ends_with("/Integer"), "{out:?}");
        assert!(out[3].ends_with("/Integer"), "{out:?}");
        assert!(out[4].starts_with("elements=["), "{out:?}");
        assert!(out[5].ends_with("/Integer"), "{out:?}");
        assert!(out[6].ends_with("/Integer"), "{out:?}");
        assert!(out[7].ends_with("/Integer"), "{out:?}");
        assert!(out[8].ends_with("/Integer"), "{out:?}");
        assert!(
            out[9] == "exclusive=true/TrueClass" || out[9] == "exclusive=false/FalseClass",
            "{out:?}"
        );
        assert!(out[10].starts_with("resbodies=["), "{out:?}");
        assert!(out[11].ends_with("/Symbol"), "{out:?}");
        assert!(out[12].ends_with("/Integer"), "{out:?}");
        assert!(out[13].starts_with("parts=["), "{out:?}");
    }

    #[test]
    fn node_field_returns_nil_for_unsupported_field_or_kind() {
        let _g = lock_sink();
        let src = "nil\n1\nfoo(2)\n";
        let expected_ast = murphy_translate::translate(src, "<test>");
        let find = |pred: &dyn Fn(&NodeKind) -> bool| {
            (0..expected_ast.len())
                .map(|i| NodeId(i as u32))
                .find(|&id| pred(expected_ast.kind(id)))
                .unwrap_or_else(|| panic!("fixture missing expected node"))
        };
        let nil_id = find(&|k| matches!(k, NodeKind::Nil));
        let int_id = find(&|k| matches!(k, NodeKind::Int(_)));
        let send_id = find(&|k| matches!(k, NodeKind::Send { .. }));
        let script = format!(
            r##"
            Murphy.__test_report("int_receiver=#{{Murphy.node_field({int_id}, :receiver).inspect}}")
            Murphy.__test_report("send_value=#{{Murphy.node_field({send_id}, :value).inspect}}")
            Murphy.__test_report("nil_value=#{{Murphy.node_field({nil_id}, :value).inspect}}")
            "##,
            int_id = int_id.0,
            send_id = send_id.0,
            nil_id = nil_id.0,
        );

        assert_eq!(
            run_script_over(src, &script),
            ["int_receiver=nil", "send_value=nil", "nil_value=nil"]
        );
    }

    #[test]
    fn source_interner_and_comments_primitives() {
        let _g = lock_sink();
        let src = "# hi\nfoo(:bar, \"baz\")\n";
        let expected_ast = murphy_translate::translate(src, "<test>");
        let mut sym_handle = None;
        let mut str_handle = None;
        for i in 0..expected_ast.len() {
            match expected_ast.kind(NodeId(i as u32)) {
                NodeKind::Sym(s) => sym_handle = Some(s.0),
                NodeKind::Str(s) => str_handle = Some(s.0),
                _ => {}
            }
        }
        let sym_handle = sym_handle.expect("fixture has a symbol literal");
        let str_handle = str_handle.expect("fixture has a string literal");
        let script = format!(
            r##"
            Murphy.__test_report("sym=#{{Murphy.symbol_str({sym_handle})}}")
            Murphy.__test_report("str=#{{Murphy.string_str({str_handle})}}")
            Murphy.__test_report("raw=#{{Murphy.raw_source(5, 8)}}")
            Murphy.__test_report("bad_raw=#{{Murphy.raw_source(9, 5).inspect}}/#{{Murphy.raw_source(-1, 1).inspect}}")
            comments = Murphy.comments
            Murphy.__test_report("comments=#{{comments.length}}/#{{comments[0][0]}},#{{comments[0][1]}}/#{{comments[0][2]}}")
            begin
              Murphy.symbol_str(999999)
            rescue ArgumentError
              Murphy.__test_report("bad_symbol=ArgumentError")
            end
            begin
              Murphy.string_str(-1)
            rescue ArgumentError
              Murphy.__test_report("bad_string=ArgumentError")
            end
            "##
        );

        assert_eq!(
            run_script_over(src, &script),
            vec![
                "sym=bar".to_string(),
                "str=baz".to_string(),
                "raw=foo".to_string(),
                "bad_raw=nil/nil".to_string(),
                "comments=1/0,4/# hi".to_string(),
                "bad_symbol=ArgumentError".to_string(),
                "bad_string=ArgumentError".to_string(),
            ]
        );
    }

    #[test]
    fn raw_source_rejects_non_utf8_boundaries() {
        let _g = lock_sink();
        let script = r##"
            Murphy.__test_report("partial=#{Murphy.raw_source(0, 1).inspect}")
            Murphy.__test_report("whole=#{Murphy.raw_source(0, 2)}")
        "##;

        assert_eq!(
            run_script_over("é\n", script),
            vec!["partial=nil".to_string(), "whole=é".to_string()]
        );
    }

    #[test]
    fn comments_preserve_source_order_and_multiline_text() {
        let _g = lock_sink();
        let src = "# first\n=begin\nsecond\n=end\nfoo\n";
        let script = r##"
            comments = Murphy.comments
            Murphy.__test_report("count=#{comments.length}")
            comments.each_with_index do |comment, i|
              Murphy.__test_report("#{i}|#{comment[0]},#{comment[1]}|#{comment[2].inspect}")
            end
            Murphy.__test_report("oob=#{Murphy.raw_source(0, 999).inspect}")
        "##;

        assert_eq!(
            run_script_over(src, script),
            vec![
                "count=2".to_string(),
                "0|0,7|\"# first\"".to_string(),
                "1|8,27|\"=begin\\nsecond\\n=end\\n\"".to_string(),
                "oob=nil".to_string(),
            ]
        );
    }

    /// MULTIBYTE hand-derivation (ADR 0001 — bytes, NOT chars) + handle→node
    /// DISTINCTNESS (ADR 0008 finding 2 — `logger.info(x)` must NOT alias bare
    /// `logger` with outer `.info`).
    ///
    /// Source: `# コメント\nputs "x"\nlogger.info(x)\n`
    ///
    /// HAND-DERIVED byte offsets (verified against `ruby_prism` 1.9.0):
    ///   * line 1 `# コメント\n` = `#`(1) + ` `(1) + コ/メ/ン/ト (4 chars ×
    ///     3 bytes UTF-8 = 12) + `\n`(1) = **15 bytes** (offsets 0..15).
    ///     CHAR length of that line is 7 — so a char-indexed regression would
    ///     report `puts` at 7, not 15. This is the load-bearing gate.
    ///   * `puts` msg token: bytes **[15, 19)** — no receiver.
    ///   * `logger.info(x)` starts at byte 25 (after `puts "x"\n` =
    ///     15 + len(`puts "x"\n`)=10 → 25). Walk order: `puts`(0),
    ///     outer `.info`(1), bare `logger`(2), arg `x`(3) → 4 call nodes.
    ///   * bare `logger` msg: **[24, 30)**, no receiver — its OWN distinct node.
    ///   * `info` msg: **[31, 35)**, HAS a receiver (`logger`). Distinct
    ///     (name, range) from `logger` → not aliased (finding 2 regression
    ///     guard).
    ///   * arg `x` msg: **[36, 37)**, no receiver.
    #[test]
    #[ignore = "obsolete live-prism primitive test; arena primitive tests replace this in murphy-9cr.24.2"]
    fn multibyte_byte_ranges_and_handle_distinctness() {
        let _g = lock_sink();
        let src = "# \u{30b3}\u{30e1}\u{30f3}\u{30c8}\nputs \"x\"\nlogger.info(x)\n";

        // Hand-checked invariants of the fixture itself (byte, not char).
        assert_eq!(src.len(), 39, "fixture is 39 BYTES");
        assert_eq!(&src[15..19], "puts", "bytes [15,19) ARE `puts` (not 7..)");
        assert_eq!(
            src.chars().take_while(|&c| c != 'p').count(),
            7,
            "the multibyte line is 7 CHARS — char-indexing would say 7, byte says 15"
        );

        let mut lines = run_driver_over(src);
        lines.sort();

        // Exactly 4 call nodes (ADR 0008 finding 2 regression guard).
        assert_eq!(lines.len(), 4, "4 call nodes: puts, info, logger, x");

        let mut by_name = std::collections::HashMap::new();
        let mut node_identities = std::collections::HashSet::new();
        for l in &lines {
            let p: Vec<&str> = l.split('|').collect();
            assert_eq!(p.len(), 5, "report shape i|name|recv_nil|range|slice");
            // Identity = (name, byte-range) EXCLUDING the handle index, so
            // this set genuinely shrinks if two handles aliased one node
            // (the rejected offset-keying bug — ADR 0008 finding 2). Including
            // the `i` prefix would make every line trivially distinct and the
            // guard vacuous.
            node_identities.insert((p[1].to_string(), p[3].to_string()));
            by_name.insert(
                p[1].to_string(),
                (p[2].to_string(), p[3].to_string(), p[4].to_string()),
            );
        }
        assert_eq!(
            node_identities.len(),
            lines.len(),
            "each handle must live-resolve to a DISTINCT (name,byte-range) node \
             — NO offset aliasing (ADR 0008 finding 2)"
        );

        // `puts` — no receiver, BYTE msg range [15,19), slice "puts".
        let (rnil, rng, slice) = by_name.get("puts").expect("a `puts` call");
        assert_eq!(rnil, "true", "puts has NO receiver");
        assert_eq!(
            rng, "15,19",
            "puts msg BYTE range (multibyte gate: NOT 7,11)"
        );
        assert_eq!(slice, "puts", "source_slice over the byte range");

        // bare `logger` — own distinct node, no receiver, [24,30) → "logger".
        let (lnil, lrng, lslice) = by_name
            .get("logger")
            .expect("bare `logger` CallNode resolves to its OWN handle (finding 2)");
        assert_eq!(lnil, "true", "bare logger has no receiver");
        assert_eq!(lrng, "24,30");
        assert_eq!(lslice, "logger");

        // `info` — `logger.info`, HAS a receiver, [31,35) → "info". Distinct
        // (name+range) from `logger` though they share a start region: proves
        // the offset-aliasing bug (ADR 0008 finding 2) cannot regress.
        let (inil, irng, islice) = by_name.get("info").expect("an `info` call");
        assert_eq!(inil, "false", "logger.info HAS a receiver");
        assert_eq!(irng, "31,35");
        assert_eq!(islice, "info");
        assert_ne!(
            (lrng, lslice),
            (irng, islice),
            "logger and info are DISTINCT live nodes (not aliased)"
        );

        // arg `x` — no receiver, [36,37) → "x".
        let (xnil, xrng, xslice) = by_name.get("x").expect("the arg `x` call");
        assert_eq!(xnil, "true");
        assert_eq!(xrng, "36,37");
        assert_eq!(xslice, "x");
    }

    /// Out-of-range handle / range are read-only, panic-free, and degrade
    /// gracefully (a user cop must not crash the engine with a bad handle).
    #[test]
    #[ignore = "obsolete live-prism primitive test; arena primitive tests replace this in murphy-9cr.24.2"]
    fn out_of_range_handle_and_slice_are_safe() {
        let _g = lock_sink();
        let ctx = AstContext::new(b"puts 1\n".to_vec());
        let worker = std::sync::Arc::clone(&ctx);
        let cop_run = crate::mruby::sdk::CopRun::for_test(std::sync::Arc::clone(&worker));
        {
            let mut st = MrubyState::open();
            st.set_cop_run(&cop_run);
            // SAFETY: see `run_driver_over` — valid state, live CopRun in ud.
            unsafe {
                register(st.raw());
                register_test_report(st.raw());
            }
            // node_count is 1 (`puts`). Handle 99 is OOB; bad slice range too.
            // A NEGATIVE handle (-1, -5) must behave EXACTLY like positive OOB
            // — never `.max(0)`-alias to handle 0 and return `puts`'s real
            // value (ADR 0008; the latent wrong-answer bug this test pins).
            st.eval(
                r##"
                Murphy.__test_report("count=#{Murphy.node_count}")
                Murphy.__test_report("oob_name=#{Murphy.node_name(99).inspect}")
                Murphy.__test_report("oob_msg_start=#{Murphy.node_msg_start(99)}")
                Murphy.__test_report("oob_msg_end=#{Murphy.node_msg_end(99)}")
                Murphy.__test_report("oob_recv=#{Murphy.node_receiver_nil?(99)}")
                Murphy.__test_report("bad_slice=#{Murphy.source_slice(100, 200).inspect}")
                Murphy.__test_report("inv_slice=#{Murphy.source_slice(5, 1).inspect}")
                Murphy.__test_report("good_slice=#{Murphy.source_slice(0, 4)}")
                # Negative-handle aliasing pin: handle 0 IS the real `puts`
                # node, so a `.max(0)` regression would make these report
                # "puts" / "true" / "0,4" instead of the nil/OOB sentinels.
                Murphy.__test_report("neg1_name=#{Murphy.node_name(-1).inspect}")
                Murphy.__test_report("neg5_name=#{Murphy.node_name(-5).inspect}")
                Murphy.__test_report("neg1_msg_start=#{Murphy.node_msg_start(-1)}")
                Murphy.__test_report("neg5_msg_start=#{Murphy.node_msg_start(-5)}")
                Murphy.__test_report("neg1_msg_end=#{Murphy.node_msg_end(-1)}")
                Murphy.__test_report("neg5_msg_end=#{Murphy.node_msg_end(-5)}")
                Murphy.__test_report("neg1_recv=#{Murphy.node_receiver_nil?(-1)}")
                Murphy.__test_report("neg5_recv=#{Murphy.node_receiver_nil?(-5)}")
            "##,
            );
        }
        drop(cop_run);
        drop(worker);
        drop(ctx);
        let out = drain_sink();
        assert!(out.contains(&"count=1".to_string()), "{out:?}");
        assert!(out.contains(&"oob_name=nil".to_string()), "{out:?}");
        assert!(out.contains(&"oob_msg_start=-1".to_string()), "{out:?}");
        assert!(out.contains(&"oob_msg_end=-1".to_string()), "{out:?}");
        assert!(out.contains(&"oob_recv=true".to_string()), "{out:?}");
        assert!(out.contains(&"bad_slice=nil".to_string()), "{out:?}");
        assert!(out.contains(&"inv_slice=nil".to_string()), "{out:?}");
        assert!(out.contains(&"good_slice=puts".to_string()), "{out:?}");

        // Negative handles behave EXACTLY like positive OOB — NOT aliased to
        // handle 0. If `.max(0)`-clamping regressed, `node_name(-1/-5)` would
        // return `"puts"` (handle 0's real name), msg offsets `0`/`4`,
        // and `node_receiver_nil?` `true` for a REAL node — caught here.
        assert!(out.contains(&"neg1_name=nil".to_string()), "{out:?}");
        assert!(out.contains(&"neg5_name=nil".to_string()), "{out:?}");
        assert!(out.contains(&"neg1_msg_start=-1".to_string()), "{out:?}");
        assert!(out.contains(&"neg5_msg_start=-1".to_string()), "{out:?}");
        assert!(out.contains(&"neg1_msg_end=-1".to_string()), "{out:?}");
        assert!(out.contains(&"neg5_msg_end=-1".to_string()), "{out:?}");
        // receiver_nil? OOB (incl. negative) stays `true` — matches the
        // positive-OOB contract; only consistency with negatives is new.
        assert!(out.contains(&"neg1_recv=true".to_string()), "{out:?}");
        assert!(out.contains(&"neg5_recv=true".to_string()), "{out:?}");
    }
}
