//! Read-only live native-primitive IDL — Phase 3 Task 3.
//!
//! These are the native Rust functions an mruby user-cop calls to inspect the
//! **live** prism AST. They promote the *resolution shape* proven by
//! `spikes/live_resolution_poc` (ADR 0008) into `crates/`:
//!
//!   * **Handle = opaque walk-order index `0..node_count`.** Nothing about a
//!     node is cached — not names, receivers, ranges, nor offsets, not even the
//!     count. The only thing that ever crosses the FFI boundary is an integer.
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

use mruby3_sys::{
    RClass, mrb_class_get, mrb_define_class, mrb_define_module_function, mrb_get_args, mrb_int,
    mrb_load_string, mrb_state, mrb_str_new, mrb_value,
};
use murphy_ast::NodeId;
use murphy_pattern::CaptureValue;

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

/// Read two required `i` arguments from an mruby native call.
///
/// # Safety
///
/// `mrb` must be a valid non-null `mrb_state` inside a native callback that
/// was registered with `MRB_ARGS_REQ(2)`.
unsafe fn arg_two_handles(mrb: *mut mrb_state) -> (mrb_int, mrb_int) {
    let mut first: mrb_int = -1;
    let mut second: mrb_int = -1;
    let fmt = c"ii";
    // SAFETY: `mrb` is a valid non-null `mrb_state`; `fmt` requests exactly
    // two `mrb_int`s; both out-pointers are live and correctly typed.
    unsafe {
        mrb_get_args(
            mrb,
            fmt.as_ptr(),
            &mut first as *mut mrb_int,
            &mut second as *mut mrb_int,
        );
    }
    (first, second)
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

/// Copy a `(ptr, len)` mruby string view into an owned `String`.
///
/// # Safety
///
/// `ptr` must be valid for `len` bytes per the `mrb_get_args("s")` contract.
unsafe fn owned_string(ptr: *const std::os::raw::c_char, len: mrb_int) -> String {
    if ptr.is_null() || len <= 0 {
        return String::new();
    }
    // SAFETY: caller guarantees `ptr` is valid for `len` bytes; `len > 0` here.
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
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

/// Build a Ruby array of integer node IDs.
///
/// Uses the same literal-eval style as the existing integer/boolean helpers:
/// mruby3-sys does not expose the inline value constructors in bindgen.
unsafe fn ruby_array_of_node_ids(mrb: *mut mrb_state, ids: &[NodeId]) -> mrb_value {
    let mut src = String::from("[");
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            src.push(',');
        }
        src.push_str(&id.0.to_string());
    }
    src.push(']');
    let lit = CString::new(src).expect("array literal contains only digits and punctuation");
    // SAFETY: `mrb` valid & non-null; `lit` is a NUL-terminated Ruby array
    // literal made only from decimal digits, commas, and brackets.
    unsafe { eval_literal(mrb, &lit) }
}

/// `Murphy.node_count -> Integer`. The size of the handle space `0..count`.
/// Resolved by a live re-walk; nothing is cached (ADR 0008).
unsafe extern "C" fn native_node_count(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: inside a native callback; `mrb` valid & non-null (mruby
    // guarantee); `ud` set to the live `Arc<AstContext>` ptr by the caller.
    let n = count_call_nodes(unsafe { ctx(mrb) });
    let lit = CString::new(n.to_string()).expect("decimal digits, no NUL");
    // SAFETY: `mrb` valid & non-null; `lit` is a NUL-terminated decimal
    // integer literal.
    unsafe { eval_literal(mrb, &lit) }
}

/// `Murphy.compile_pattern(src) -> Integer`. Parse/lower a node pattern once
/// per process and return the shared IR handle.
unsafe extern "C" fn native_compile_pattern(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut ptr: *const std::os::raw::c_char = std::ptr::null();
    let mut len: mrb_int = 0;
    // SAFETY: `mrb` is a valid non-null `mrb_state`; `fmt` requests one string
    // as a pointer/length pair; out-pointers are live for the call.
    unsafe {
        mrb_get_args(
            mrb,
            c"s".as_ptr(),
            &mut ptr as *mut *const std::os::raw::c_char,
            &mut len as *mut mrb_int,
        );
    }
    // SAFETY: `mrb_get_args("s")` guarantees `ptr` is valid for `len` bytes
    // for the duration of this callback.
    let src = unsafe { owned_string(ptr, len) };
    let Ok(handle) = crate::mruby::pattern_registry::PatternIrRegistry::global().intern(&src)
    else {
        // Full load-time error reporting is wired with the mruby proxy. For the
        // primitive surface, invalid patterns degrade to nil rather than panic.
        return unsafe { eval_literal(mrb, c"nil") };
    };
    let lit = CString::new(handle.to_string()).expect("decimal digits, no NUL");
    // SAFETY: `mrb` valid & non-null; `lit` is a decimal integer literal.
    unsafe { eval_literal(mrb, &lit) }
}

/// `Murphy.match(ir_handle, node_id) -> Array<Integer> | nil`. Runs a compiled
/// runtime node pattern against the arena AST. This first slice returns capture
/// node IDs only; no-match and invalid handles degrade to nil.
unsafe extern "C" fn native_match(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with two required `i` arguments.
    let (ir_handle, node_id) = unsafe { arg_two_handles(mrb) };
    let (Ok(ir_handle), Ok(node_id)) = (u32::try_from(ir_handle), u32::try_from(node_id)) else {
        // SAFETY: `mrb` valid & non-null; nil literal.
        return unsafe { eval_literal(mrb, c"nil") };
    };

    let Some(ir) = crate::mruby::pattern_registry::PatternIrRegistry::global().get(ir_handle)
    else {
        // SAFETY: `mrb` valid & non-null; nil literal.
        return unsafe { eval_literal(mrb, c"nil") };
    };
    let ast = c.arena_ast();
    if node_id as usize >= ast.len() {
        // SAFETY: `mrb` valid & non-null; nil literal.
        return unsafe { eval_literal(mrb, c"nil") };
    }

    let mut predicates = murphy_pattern::NoPredicates;
    let Some(captures) = murphy_pattern::matches(&ir, ast, NodeId(node_id), &mut predicates) else {
        // SAFETY: `mrb` valid & non-null; nil literal.
        return unsafe { eval_literal(mrb, c"nil") };
    };

    let mut ids = Vec::new();
    for capture in captures.as_slice() {
        match capture {
            CaptureValue::Node(id) => ids.push(*id),
            CaptureValue::Seq(seq) => ids.extend(seq.iter().copied()),
        }
    }
    // SAFETY: `mrb` valid & non-null; helper builds a safe Ruby literal.
    unsafe { ruby_array_of_node_ids(mrb, &ids) }
}

/// `Murphy.node_descendants(node_id) -> Array<Integer> | nil`. Returns arena
/// descendant node IDs in DFS pre-order, excluding `node_id` itself.
unsafe extern "C" fn native_node_descendants(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    // SAFETY: native callback; `mrb` valid & non-null; `ud` is the live ctx.
    let c = unsafe { ctx(mrb) };
    // SAFETY: native callback registered with one required `i` argument.
    let node_id = unsafe { arg_handle(mrb) };
    let Ok(node_id) = u32::try_from(node_id) else {
        // SAFETY: `mrb` valid & non-null; nil literal.
        return unsafe { eval_literal(mrb, c"nil") };
    };

    let ast = c.arena_ast();
    if node_id as usize >= ast.len() {
        // SAFETY: `mrb` valid & non-null; nil literal.
        return unsafe { eval_literal(mrb, c"nil") };
    }

    let ids = ast.descendants(NodeId(node_id)).collect::<Vec<_>>();
    // SAFETY: `mrb` valid & non-null; helper builds a safe Ruby literal.
    unsafe { ruby_array_of_node_ids(mrb, &ids) }
}

/// `Murphy.node_name(handle) -> String`. Resolves the handle to the LIVE
/// prism call node and reads its real `name()`. Only the integer crossed FFI.
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
        def(c"node_count", native_node_count, 0);
        def(c"node_name", native_node_name, 1);
        def(c"node_receiver_nil?", native_node_receiver_nil, 1);
        def(c"node_msg_start", native_node_msg_start, 1);
        def(c"node_msg_end", native_node_msg_end, 1);
        def(c"source_slice", native_source_slice, 2);
        def(c"compile_pattern", native_compile_pattern, 1);
        def(c"match", native_match, 2);
        def(c"node_descendants", native_node_descendants, 1);
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
            st.eval(DRIVER);
            // `st` drops here → mrb_close, BEFORE the CopRun / Arc clones drop
            // (normal-path ordering, ADR 0009 rule 4).
        }
        drop(cop_run);
        drop(worker);
        drop(ctx);
        drain_sink()
    }

    #[test]
    fn compile_pattern_primitive_reuses_registered_handles() {
        let _guard = lock_sink();
        {
            let mut st = MrubyState::open();
            unsafe {
                register(st.raw());
                register_test_report(st.raw());
            }
            st.eval(
                r##"
                a = Murphy.compile_pattern("(send nil? :puts $...)")
                b = Murphy.compile_pattern("(send nil? :puts $...)")
                Murphy.__test_report("#{a}|#{b}")
            "##,
            );
        }

        assert_eq!(drain_sink(), vec!["0|0"]);
    }

    #[test]
    fn match_primitive_returns_capture_node_ids() {
        let _guard = lock_sink();
        let ctx = AstContext::new(b"puts x\nlogger.info(x)\n".to_vec());
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
            st.eval(
                r##"
                ir = Murphy.compile_pattern("(send nil? :puts $...)")
                captures = Murphy.match(ir, 1)
                if captures
                  Murphy.__test_report("hit:#{captures[0]}")
                else
                  Murphy.__test_report("miss")
                end
            "##,
            );
        }
        drop(cop_run);
        drop(worker);
        drop(ctx);

        assert_eq!(drain_sink(), vec!["hit:0"]);
    }

    #[test]
    fn match_primitive_invalid_and_miss_paths_return_nil() {
        let _guard = lock_sink();
        let ctx = AstContext::new(b"puts x\nlogger.info(x)\n".to_vec());
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
            st.eval(
                r##"
                ir = Murphy.compile_pattern("(send nil? :puts $...)")
                Murphy.__test_report("bad_ir=#{Murphy.match(999999, 1).nil?}")
                Murphy.__test_report("bad_node=#{Murphy.match(ir, 999999).nil?}")
                Murphy.__test_report("miss=#{Murphy.match(ir, 0).nil?}")
            "##,
            );
        }
        drop(cop_run);
        drop(worker);
        drop(ctx);

        assert_eq!(
            drain_sink(),
            vec!["bad_ir=true", "bad_node=true", "miss=true"]
        );
    }

    #[test]
    fn node_descendants_primitive_returns_dfs_descendant_ids() {
        let _guard = lock_sink();
        let ctx = AstContext::new(b"puts x\nlogger.info(x)\n".to_vec());
        let root = ctx.arena_ast().root();
        let expected = ctx
            .arena_ast()
            .descendants(root)
            .map(|id| id.0.to_string())
            .collect::<Vec<_>>()
            .join(",");
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
            st.eval(&format!(
                r##"
                Murphy.__test_report(Murphy.node_descendants({}).join(","))
            "##,
                root.0
            ));
        }
        drop(cop_run);
        drop(worker);
        drop(ctx);

        assert_eq!(drain_sink(), vec![expected]);
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
