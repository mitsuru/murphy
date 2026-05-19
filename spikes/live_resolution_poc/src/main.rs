// Spike 3.1 PoC — LIVE handle resolution (resolves ADR 0002 Finding 4).
//
// Spike 0.2 only proved a SNAPSHOT model: it pre-collected node names into a
// `Vec<String>` and mruby read that Vec. Finding 4 flagged the real question as
// UNPROVEN: can an embedded mruby `.rb` call a Rust native primitive that
// resolves an opaque integer handle to a *LIVE* prism AST node and read its
// real `name()` / `receiver()` nil-ness / `message_loc()` byte range from a
// `ruby_prism::ParseResult<'pr>` that BORROWS the source — with NO serialization
// and NO pre-snapshot? This spike proves YES.
//
// Throwaway spike code. NOT carried into crates/.

// Spike convenience (matches mruby_poc): edition-2024 requires `unsafe {}` even
// inside `unsafe extern "C" fn`. Real crates keep the lint; a throwaway PoC
// keeps the FFI shims readable.
#![allow(unsafe_op_in_unsafe_fn)]

use ruby_prism::{parse, ParseResult, Visit};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::Arc;

use mruby3_sys::{
    mrb_class_get, mrb_close, mrb_define_class, mrb_define_module_function, mrb_get_args, mrb_int,
    mrb_load_string, mrb_open, mrb_state, mrb_str_new_cstr, mrb_value,
};

// MRB_ARGS_REQ(n) is not in bindgen output (ADR 0002 Finding 1). Reproduce it:
// ((mrb_aspec)((n)&0x1f) << 18)
const fn args_req(n: u32) -> u32 {
    (n & 0x1f) << 18
}

// ===========================================================================
// THE LIFETIME / UNSAFE CONTRACT  (this is the keystone the spike proves)
// ===========================================================================
//
// `ruby_prism::parse(src) -> ParseResult<'pr>` borrows `src` (the struct is
// literally `{ source: &'pr [u8], parser, node }`). `Node<'pr>` re-borrows the
// result. That `&'pr` CANNOT be threaded through a C `ud` void* nor an mruby
// Integer handle — exactly the E0106 trap Spike 0.2 hit and dodged with a
// snapshot.
//
// We do NOT solve it with the type system. We solve it with RUNTIME OWNERSHIP
// DISCIPLINE, exactly as ADR 0002 Finding 4 dictates:
//
//   * `AstContext` owns BOTH the source bytes (`Box<[u8]>`, stable heap
//     address for as long as the box lives) AND the `ParseResult` produced
//     from those bytes.
//   * The `'pr` of the stored `ParseResult` is LIFETIME-LAUNDERED to
//     `'static` via `transmute`. **That `'static` is a LIE.** The real
//     lifetime is tied to `AstContext.source` living in the SAME struct.
//     Validity is upheld by two things, not by the borrow checker:
//       (1) Drop order: `parse_result` is declared BEFORE `source`, so it
//           drops first. While `parse_result` is alive, `source` is alive.
//       (2) The whole `AstContext` lives behind `Arc`; every owner (host
//           thread now; an abandoned mruby thread in Spike 3.2) holds an
//           `Arc` clone, so source+result die together, never apart.
//   * mruby holds only `Integer` handles. A native primitive resolves a
//     handle by RE-WALKING the LIVE prism tree (`ctx.parse_result.node()`,
//     which re-derives a fresh `Node` from the live C arena every call) and
//     reading the node's real `name()` / `receiver()` / `message_loc()`.
//     Nothing is pre-extracted; the integer is the ONLY thing that crosses
//     the FFI boundary; no `&'pr` is ever threaded through `ud`.
//   * `ud` carries `Arc::as_ptr(&ctx) as *const AstContext` (a raw `*const`,
//     NOT an Arc handle and NOT a borrowed reference). Native callbacks
//     reconstitute `&AstContext` by deref — no refcount touched. The host's
//     own `Arc` keeps the context alive across every native call on the
//     normal path; reconstitution is sound because that Arc is provably
//     alive for the entire `mrb_load_string` duration.
//
// WHY REVERSING DROP ORDER IS UB (demonstrated at end of main):
//   `mrb_close()` can run GC finalizers and could (in a real cop) be mid
//   native call. If `AstContext` (→ `ParseResult::drop` → `pm_node_destroy`
//   + `pm_parser_free`, and the source buffer) were freed FIRST, a finalizer
//   or in-flight primitive would deref a freed C arena / dangling `&[u8]`.
//   So: close mruby, THEN drop the Arc/AST. Always.
//
// ABANDON PATH (argued here, proven under threads by Spike 3.2):
//   If the mruby work were on a worker thread that times out and is
//   ABANDONED, `mrb_close()` is never called for it (ADR 0003). Safety then
//   comes from the OTHER mechanism: that thread holds its OWN `Arc<AstContext>`
//   clone. The host can drop its Arc and return; the clone keeps the AST +
//   source alive, so a late native call from the zombie thread still derefs
//   valid memory. Both mechanisms target one invariant: "no in-flight native
//   call ever sees freed AST" (ADR 0005 cross-ADR interaction 1). This spike
//   only structures for it (Arc + a demonstrated extra clone); Spike 3.2
//   exercises it under real threads.

/// The shared, Arc'd AST context. In Phase 3 this is what the Core clones to
/// each per-cop worker thread.
///
/// FIELD ORDER IS LOAD-BEARING: `parse_result` MUST be declared before
/// `source` so Rust's drop-in-declaration-order frees the prism arena while
/// the source bytes it conceptually borrows are still alive. (`ParseResult`'s
/// own Drop only frees the C arena, but keeping the honest ordering documents
/// and future-proofs the borrow-validity story.)
struct AstContext {
    /// `ParseResult<'static>` — the `'static` IS A LIE (see contract above).
    /// Real lifetime is `&self.source`. Read LIVE on every native call.
    parse_result: ParseResult<'static>,
    /// The owned source buffer. `ParseResult` conceptually borrows this.
    /// Stable heap address for as long as this box lives.
    #[allow(dead_code)]
    source: Box<[u8]>,
    /// Number of call-node handles. The handle IS the walk-order index
    /// (0..node_count). NOTHING about the nodes is cached — not names, not
    /// ranges, not even offsets. Resolution re-walks the LIVE tree every call
    /// and returns the Nth call node. This is the strongest "no snapshot"
    /// form: the handle is a bare integer, the node is found live each time.
    ///
    /// (Offset-keying was tried and REJECTED: `logger.info(x)` has the bare
    /// `logger` CallNode and the outer `.info` CallNode sharing start_offset
    /// 10, so offset-first-match aliased two distinct handles to one node.
    /// Walk-order index resolves every handle to its own distinct node.)
    node_count: usize,
    /// Proof sink: what the `.rb` reported back, for Rust-side assertions.
    collected: Vec<String>,
}

impl AstContext {
    /// LIVE resolution: walk the real prism tree NOW and return the Nth
    /// (walk-order) CallNode, where N == handle. Touches the live C arena via
    /// `parse_result.node()` every call — zero snapshot, not even an offset
    /// cache; the only thing the handle carries is the integer index.
    fn with_call_node<R>(
        &self,
        handle: usize,
        f: impl FnOnce(&ruby_prism::CallNode<'_>) -> R,
    ) -> Option<R> {
        if handle >= self.node_count {
            return None;
        }

        struct Finder<'a, R> {
            // Counts call nodes down; act on the one where it reaches 0.
            remaining: usize,
            out: Option<R>,
            f: Option<Box<dyn FnOnce(&ruby_prism::CallNode<'_>) -> R + 'a>>,
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
        // `node()` re-derives a fresh root `Node` from the LIVE C tree.
        finder.visit(&self.parse_result.node());
        finder.out
    }
}

/// Reconstitute `&AstContext` from `mrb_state.ud`. The raw `*const` was put
/// there from `Arc::as_ptr`; the host's Arc is alive for the whole run, so
/// this deref is sound (see contract). No Arc refcount is touched here.
unsafe fn ctx<'a>(mrb: *mut mrb_state) -> &'a AstContext {
    let ud = (*mrb).ud as *const AstContext;
    assert!(!ud.is_null(), "mrb_state.ud must hold the AstContext ptr");
    &*ud
}

/// Proof-sink mutable access. The context is shared-immutable behind Arc; the
/// ONLY mutation is `collected` (the report sink), and it happens
/// synchronously from the single-threaded `.rb` run with no concurrent native
/// reader. Phase 3 uses a real channel; this is a PoC sink.
unsafe fn ctx_mut_collected<'a>(mrb: *mut mrb_state) -> &'a mut Vec<String> {
    let ud = (*mrb).ud as *const AstContext as *mut AstContext;
    &mut (*ud).collected
}

/// `Murphy.node_count` -> Integer. (Round-trip a Ruby literal for the Integer
/// value; ADR 0002 Finding 1: inline value boxers are absent from bindgen.)
unsafe extern "C" fn native_node_count(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let n = ctx(mrb).node_count;
    let s = CString::new(n.to_string()).unwrap();
    mrb_load_string(mrb, s.as_ptr())
}

/// `Murphy.node_name(handle)` -> String. Resolves the handle to the LIVE
/// prism node and reads its real `name()`. Only the integer crossed FFI.
unsafe extern "C" fn native_node_name(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut handle: mrb_int = -1;
    let fmt = CStr::from_bytes_with_nul(b"i\0").unwrap();
    mrb_get_args(mrb, fmt.as_ptr(), &mut handle as *mut mrb_int);

    let name = ctx(mrb)
        .with_call_node(handle as usize, |n| {
            String::from_utf8_lossy(n.name().as_slice()).into_owned()
        })
        .unwrap_or_else(|| "<oob>".to_string());

    let cs = CString::new(name).unwrap();
    mrb_str_new_cstr(mrb, cs.as_ptr())
}

/// `Murphy.node_receiver_nil?(handle)` -> true/false. Reads LIVE
/// `node.receiver().is_none()`.
unsafe extern "C" fn native_node_receiver_nil(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut handle: mrb_int = -1;
    let fmt = CStr::from_bytes_with_nul(b"i\0").unwrap();
    mrb_get_args(mrb, fmt.as_ptr(), &mut handle as *mut mrb_int);

    let is_nil = ctx(mrb)
        .with_call_node(handle as usize, |n| n.receiver().is_none())
        .unwrap_or(true);

    let lit = if is_nil { b"true\0".as_ref() } else { b"false\0".as_ref() };
    mrb_load_string(mrb, CStr::from_bytes_with_nul(lit).unwrap().as_ptr())
}

/// `Murphy.node_msg_range(handle)` -> "start,end" (byte offsets, ADR 0001).
/// Reads LIVE `node.message_loc()`. Returned as a small string the `.rb`
/// splits — proves the byte range survives the round-trip unserialized AST.
unsafe extern "C" fn native_node_msg_range(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut handle: mrb_int = -1;
    let fmt = CStr::from_bytes_with_nul(b"i\0").unwrap();
    mrb_get_args(mrb, fmt.as_ptr(), &mut handle as *mut mrb_int);

    let range = ctx(mrb)
        .with_call_node(handle as usize, |n| match n.message_loc() {
            Some(m) => format!("{},{}", m.start_offset(), m.end_offset()),
            None => "nil".to_string(),
        })
        .unwrap_or_else(|| "oob".to_string());

    let cs = CString::new(range).unwrap();
    mrb_str_new_cstr(mrb, cs.as_ptr())
}

/// `Murphy.report(str)` — proof sink: the `.rb` reports each node's
/// live-resolved data back; Rust asserts on it afterward.
unsafe extern "C" fn native_report(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut p: *const c_char = std::ptr::null();
    let fmt = CStr::from_bytes_with_nul(b"z\0").unwrap();
    mrb_get_args(mrb, fmt.as_ptr(), &mut p as *mut *const c_char);
    let s = CStr::from_ptr(p).to_string_lossy().into_owned();
    ctx_mut_collected(mrb).push(s);
    mrb_load_string(mrb, CStr::from_bytes_with_nul(b"nil\0").unwrap().as_ptr())
}

unsafe fn define_primitives(mrb: *mut mrb_state) {
    let obj = mrb_class_get(mrb, CStr::from_bytes_with_nul(b"Object\0").unwrap().as_ptr());
    let murphy = mrb_define_class(
        mrb,
        CStr::from_bytes_with_nul(b"Murphy\0").unwrap().as_ptr(),
        obj,
    );
    let def = |name: &CStr, f: unsafe extern "C" fn(*mut mrb_state, mrb_value) -> mrb_value, argc| {
        mrb_define_module_function(mrb, murphy, name.as_ptr(), Some(f), args_req(argc));
    };
    def(CStr::from_bytes_with_nul(b"node_count\0").unwrap(), native_node_count, 0);
    def(CStr::from_bytes_with_nul(b"node_name\0").unwrap(), native_node_name, 1);
    def(
        CStr::from_bytes_with_nul(b"node_receiver_nil?\0").unwrap(),
        native_node_receiver_nil,
        1,
    );
    def(
        CStr::from_bytes_with_nul(b"node_msg_range\0").unwrap(),
        native_node_msg_range,
        1,
    );
    def(CStr::from_bytes_with_nul(b"report\0").unwrap(), native_report, 1);
}

/// Pre-walk ONLY to COUNT call nodes (to size the handle space 0..count).
/// This stores NO node data at all — not names, not receivers, not ranges,
/// not even offsets. Every actual node read happens LIVE inside the native
/// primitives at call time via an independent re-walk.
struct NodeCounter {
    count: usize,
}
impl<'pr> Visit<'pr> for NodeCounter {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        self.count += 1;
        ruby_prism::visit_call_node(self, node);
    }
}

fn main() {
    // 1. Real prism parse. Source is OWNED here as Box<[u8]> so AstContext can
    //    own it alongside the ParseResult (the sibling-ownership rule, ADR
    //    0002 Finding 2 — unified here in one Arc'd struct).
    //
    //    Hand-verified byte offsets for assertions:
    //      "puts \"hi\"\n"  → bytes 0..4 = "puts" (no receiver), newline at 9
    //      "logger.info(x)\n" starts at byte 10; "info" is the message token
    //      "Foo.bar(1)\n"   → "bar" is the message token (has receiver)
    let src_string = "puts \"hi\"\nlogger.info(x)\nFoo.bar(1)\n".to_string();
    let source: Box<[u8]> = src_string.clone().into_bytes().into_boxed_slice();

    // Parse from the OWNED bytes. `result` borrows `source` here.
    let result: ParseResult<'_> = parse(&source);
    assert!(
        result.errors().next().is_none(),
        "fixture must parse cleanly before we transmute the lifetime"
    );

    // Count handles only (NO node data, NO offsets cached).
    let mut counter = NodeCounter { count: 0 };
    counter.visit(&result.node());
    println!(
        "prism found {} call nodes; handles = 0..{} (bare indices, nothing cached)",
        counter.count, counter.count
    );

    // 2. LIFETIME LAUNDER: `result: ParseResult<'_ borrowing source>` ->
    //    `ParseResult<'static>`. This is the unsafe core. It is sound ONLY
    //    because we are about to move BOTH `source` and `result` into the
    //    SAME `AstContext`, with `parse_result` declared before `source`
    //    (drop order), the whole thing Arc'd so they live and die together.
    //    The `'static` is a LIE; real lifetime = `&AstContext.source`.
    let parse_result: ParseResult<'static> =
        unsafe { std::mem::transmute::<ParseResult<'_>, ParseResult<'static>>(result) };

    let ctx_arc: Arc<AstContext> = Arc::new(AstContext {
        parse_result, // declared FIRST → dropped FIRST (load-bearing)
        source,       // declared AFTER → dropped AFTER parse_result
        node_count: counter.count,
        collected: Vec::new(),
    });

    // 3. Abandon-path STRUCTURE (argued, not exercised here — Spike 3.2):
    //    a worker would get its own clone so the host dropping its Arc can't
    //    free the AST under a zombie native call. We make one to demonstrate
    //    the structure is mechanically present.
    let worker_clone: Arc<AstContext> = Arc::clone(&ctx_arc);
    println!(
        "Arc strong_count = {} (host + simulated worker clone) — abandon-path \
         safety net is structurally present",
        Arc::strong_count(&ctx_arc)
    );

    unsafe {
        // 4. Embed mruby; stash a RAW *const to the Arc'd context in ud.
        //    NOT an Arc, NOT a &reference — a raw pointer reached via ud,
        //    lifetime upheld by the host Arc being alive for the whole run.
        let mrb = mrb_open();
        assert!(!mrb.is_null(), "mrb_open failed");
        (*mrb).ud = Arc::as_ptr(&ctx_arc) as *mut c_void;
        define_primitives(mrb);

        // 5. The user-cop-style `.rb`, run AS-IS via mrb_load_string. The
        //    walk reads like Ruby; every datum is resolved LIVE in Rust from
        //    the prism tree at call time. Only integers cross the boundary.
        // NOTE: `r##"..."##` (double hash) is REQUIRED here: the Ruby string
        // interpolation `"#{i}` contains the byte sequence `"#`, which would
        // terminate a single-hash `r#"..."#` raw string early (the bug this
        // delimiter choice fixes).
        let script = r##"
            class Node
              def initialize(h) = @h = h
              def name          = Murphy.node_name(@h)
              def receiver_nil? = Murphy.node_receiver_nil?(@h)
              def msg_range     = Murphy.node_msg_range(@h)
            end
            Murphy.node_count.times do |i|
              n = Node.new(i)
              Murphy.report("#{i}|#{n.name}|#{n.receiver_nil?}|#{n.msg_range}")
            end
        "##;
        let cscript = CString::new(script).unwrap();
        mrb_load_string(mrb, cscript.as_ptr());

        // 6. Second independent mrb_state (per-cop isolation precondition,
        //    design §6) opened while the first is live, then closed.
        let mrb2 = mrb_open();
        assert!(!mrb2.is_null() && mrb2 != mrb, "second independent mrb_state");
        mrb_load_string(mrb2, CString::new("1 + 1").unwrap().as_ptr());
        mrb_close(mrb2);

        // 7. DROP ORDER (load-bearing, ADR 0002 item 3 / ADR 0005):
        //    close mruby FIRST. After this, no GC finalizer / native call
        //    can run, so it is now safe for the AstContext to drop.
        //    Reversing (drop AstContext, then mrb_close) is UB: mrb_close's
        //    GC could deref the freed prism C arena via a still-defined
        //    primitive / finalizer.
        mrb_close(mrb);
    }

    // 8. Assertions: the `.rb` walked the LIVE AST via native resolution.
    //    Reported lines look like "i|name|recv_nil|start,end".
    let collected = &ctx_arc.collected;
    println!("native-collected from Ruby (live-resolved): {collected:#?}");
    assert!(!collected.is_empty(), "must have walked at least one node");

    // Parse the reported tuples.
    let mut by_name: std::collections::HashMap<String, (bool, String)> =
        std::collections::HashMap::new();
    for line in collected {
        let parts: Vec<&str> = line.split('|').collect();
        assert_eq!(parts.len(), 4, "report line shape: i|name|recv_nil|range");
        let name = parts[1].to_string();
        let recv_nil = parts[2] == "true";
        let range = parts[3].to_string();
        by_name.insert(name, (recv_nil, range));
    }

    // DISTINCTNESS: every handle must resolve to its OWN node. The fixture has
    // 5 call nodes with 5 distinct (name, range) tuples — if two handles
    // aliased one node (the offset-keying bug we rejected) this set would have
    // < 5 entries. This assertion is what makes the `logger`/`info` shared-
    // start-offset case load-bearing instead of silently skipped.
    let distinct: std::collections::HashSet<&String> = collected.iter().collect();
    assert_eq!(
        distinct.len(),
        collected.len(),
        "each handle must live-resolve to a DISTINCT node (no aliasing)"
    );
    assert_eq!(
        collected.len(),
        5,
        "fixture has exactly 5 call nodes; all 5 handles must resolve"
    );

    // `puts` — no receiver; message token is bytes [0,4) == "puts".
    let (puts_nil, puts_range) = by_name.get("puts").expect("a `puts` call");
    assert_eq!(*puts_nil, true, "puts must have NO receiver (receiver_nil?)");
    assert_eq!(puts_range, "0,4", "puts msg byte range must be 0,4");
    assert_eq!(&src_string[0..4], "puts", "byte slice sanity");

    // `info` — `logger.info`, HAS a receiver; msg token is "info".
    let (info_nil, info_range) = by_name.get("info").expect("an `info` call");
    assert_eq!(*info_nil, false, "logger.info MUST have a receiver");
    {
        let (a, b) = info_range.split_once(',').unwrap();
        let (a, b): (usize, usize) = (a.parse().unwrap(), b.parse().unwrap());
        assert_eq!(
            &src_string[a..b],
            "info",
            "live-resolved msg range must slice to `info`"
        );
    }

    // `bar` — `Foo.bar`, HAS a receiver.
    let (bar_nil, bar_range) = by_name.get("bar").expect("a `bar` call");
    assert_eq!(*bar_nil, false, "Foo.bar MUST have a receiver");
    {
        let (a, b) = bar_range.split_once(',').unwrap();
        let (a, b): (usize, usize) = (a.parse().unwrap(), b.parse().unwrap());
        assert_eq!(&src_string[a..b], "bar", "live-resolved range slices to `bar`");
    }

    // The bare `logger` call (implicit-receiver CallNode, shares start_offset
    // 10 with the outer `logger.info` call). UNCONDITIONAL: this is the exact
    // node the rejected offset-keying aliased away — it MUST now resolve to
    // its own distinct node, proving handle→THE-live-node, not handle→first.
    let (lg_nil, lg_range) = by_name
        .get("logger")
        .expect("bare `logger` CallNode MUST resolve to its own handle");
    assert_eq!(*lg_nil, true, "bare `logger` has no receiver");
    {
        let (a, b) = lg_range.split_once(',').unwrap();
        let (a, b): (usize, usize) = (a.parse().unwrap(), b.parse().unwrap());
        assert_eq!(
            &src_string[a..b],
            "logger",
            "live-resolved range slices to `logger`"
        );
    }

    // Abandon-path structure still intact: drop host Arc, the worker clone
    // keeps the AstContext alive (count goes 2 -> 1, not 0). This is the
    // mechanism Spike 3.2 will exercise under a real abandoned thread.
    drop(ctx_arc);
    assert_eq!(
        Arc::strong_count(&worker_clone),
        1,
        "after host drops its Arc, the worker clone alone keeps AST alive — \
         a late native call from a zombie thread would still see valid memory"
    );
    // Now the last owner drops; source+result die TOGETHER, ordered.
    drop(worker_clone);

    println!("\nALL LIVE-RESOLUTION ASSERTIONS PASSED");
}
