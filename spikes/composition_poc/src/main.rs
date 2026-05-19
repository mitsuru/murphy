// Spike 3.2 PoC — COMPOSITION (resolves ADR 0009).
//
// Phase 3 Spike 3.1 (`spikes/live_resolution_poc/`, ADR 0008) proved LIVE
// handle resolution SINGLE-THREADED and explicitly deferred:
//   - thread-safety of the Arc'd `AstContext` (it is `!Send`/`!Sync` because
//     `ruby_prism::ParseResult` holds `NonNull<pm_parser_t>` — ADR 0008
//     finding 4),
//   - abandon-under-real-threads (Mechanism A's timed-out, never-joined cop
//     thread keeping the AST alive for a late native call),
//   - composition with the Phase-2 rayon file-parallel pipeline.
//
// This spike proves all three COMPOSE. With REAL threads:
//
//   * `files.par_iter()` (Phase 2's rayon shape). Each file worker parses ONCE
//     into an `Arc<AstContext>` (Spike-3.1 layout verbatim: walk-order-index
//     handles, `ParseResult` transmuted `'static`, source-outlives-result drop
//     order), runs a trivial native cop, then dispatches mruby cop(s).
//   * Each mruby cop runs on its OWN OS thread with its OWN isolated
//     `mrb_state` under a wall-clock watchdog (the `deadline_poc`
//     recv_timeout/abandon pattern). It reads the LIVE AST via native
//     primitives through `ud` (`Arc::as_ptr` raw `*const AstContext`, re-walk
//     to the indexed node — NO snapshot).
//   * The per-cop worker thread MOVES IN ITS OWN `Arc<AstContext>` clone. An
//     abandoned (timed-out, never-joined) cop thread keeps the AstContext
//     alive for any late native call even after the rayon file-worker dropped
//     its own Arc and moved on.
//   * `unsafe impl Send + Sync for AstContext` with a documented SAFETY
//     justification, stress-verified by heavy concurrent native reads.
//   * Determinism (ADR 0006/0007): a total-order sort makes the JSON-ish
//     output byte-identical across shuffled/repeated runs despite thread
//     interleaving and abandoned threads.
//
// Throwaway spike code. NOT carried into crates/.

// Spike convenience (matches the other spikes): edition-2024 requires
// `unsafe {}` even inside `unsafe extern "C" fn`. Real crates keep the lint;
// a throwaway PoC keeps the FFI shims readable.
#![allow(unsafe_op_in_unsafe_fn)]

use ruby_prism::{parse, ParseResult, Visit};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use rayon::prelude::*;

use mruby3_sys::{
    mrb_class_get, mrb_close, mrb_define_class, mrb_define_module_function, mrb_get_args, mrb_int,
    mrb_load_string, mrb_open, mrb_state, mrb_str_new_cstr, mrb_value,
};

// MRB_ARGS_REQ(n) is not in bindgen output (ADR 0002 Finding 1). Reproduce it:
// ((mrb_aspec)((n)&0x1f) << 18)
const fn args_req(n: u32) -> u32 {
    (n & 0x1f) << 18
}

// Global counter of native primitive calls actually serviced across ALL
// threads. Used only as a stress-soundness witness: under concurrent reads it
// must keep climbing and the per-call results must stay correct.
static NATIVE_CALLS: AtomicU64 = AtomicU64::new(0);

// ===========================================================================
// THE SHARED AST CONTEXT  (Spike-3.1 layout, verbatim contract)
// ===========================================================================
//
// `ruby_prism::parse(src) -> ParseResult<'pr>` borrows `src`. The `'pr` cannot
// be threaded through a C `ud` void* nor an mruby Integer handle. As in Spike
// 3.1 we do NOT solve it with the type system; we solve it with RUNTIME
// OWNERSHIP DISCIPLINE:
//
//   * `AstContext` owns BOTH the source bytes (`Box<[u8]>`, stable heap
//     address) AND the `ParseResult` produced from those bytes.
//   * The stored `ParseResult`'s `'pr` is LIFETIME-LAUNDERED to `'static` via
//     `transmute`. That `'static` IS A LIE — real lifetime is `&self.source`.
//     Validity is upheld by (1) drop order: `parse_result` declared BEFORE
//     `source`, so it drops first; and (2) the whole context lives behind
//     `Arc`, every owner (rayon file worker; abandoned cop thread) holds an
//     `Arc` clone, so source+result die together, never apart.
//   * mruby holds only `Integer` handles. A native primitive resolves a handle
//     by RE-WALKING the LIVE prism tree every call. NOTHING is pre-extracted.
//   * `ud` carries `Arc::as_ptr(&ctx) as *const AstContext` — a raw `*const`,
//     not an Arc handle, not a borrowed reference. The thread that opened the
//     `mrb_state` owns an `Arc` clone that is provably alive for the entire
//     `mrb_load_string` duration (and beyond, if abandoned).
//
// 3.2 DELTA vs 3.1: Spike 3.1's `collected: Vec<String>` proof-sink is DELETED.
// Under threads, mutating shared state through `ud` would be a data race. The
// `AstContext` here is STRICTLY READ-ONLY. Cops return their offenses out-of-
// band through the per-cop mpsc completion message, never through the context.

/// The shared, immutable, Arc'd AST context. Each per-cop worker thread owns an
/// independent `Arc` clone.
///
/// FIELD ORDER IS LOAD-BEARING: `parse_result` MUST be declared before
/// `source` so drop-in-declaration-order frees the prism C arena while the
/// source bytes it conceptually borrows are still alive.
struct AstContext {
    /// `ParseResult<'static>` — the `'static` IS A LIE (see contract above).
    /// Real lifetime is `&self.source`. Read LIVE on every native call.
    parse_result: ParseResult<'static>,
    /// The owned source buffer. `ParseResult` conceptually borrows this.
    /// Stable heap address for as long as this box lives.
    #[allow(dead_code)]
    source: Box<[u8]>,
    /// Number of call-node handles. The handle IS the walk-order index
    /// (0..node_count). NOTHING about the nodes is cached. Resolution
    /// re-walks the LIVE tree every call.
    node_count: usize,
    /// The "file" name this context belongs to (for offense attribution).
    file: String,
}

// ===========================================================================
// THE UNSAFE Send + Sync IMPL  (ADR 0008 finding 4 — the deferred gap)
// ===========================================================================
//
// `AstContext` is `!Send`/`!Sync` ONLY because `ParseResult` holds a
// `NonNull<pm_parser_t>` (a raw pointer into prism's C arena). Without the
// impl below, moving `Arc<AstContext>` into a `thread::spawn` closure and
// `par_iter`-ing over them does not compile (E0277: `*mut pm_parser_t` /
// `NonNull<...>` cannot be sent/shared between threads safely). This was
// verified empirically — see `docs`/report: commenting the impl out yields the
// `NonNull<pm_parser_t>` E0277 errors; restoring it compiles.
//
// SAFETY: the prism C arena behind `ParseResult` is READ-ONLY for the entire
// lifetime of every cop run. After `parse()` completes the tree is never
// mutated: cops are read-only traversal + text-edit *suggestions* (design §3),
// there is no AST mutation anywhere, and no `&mut` to the parser/nodes is ever
// formed (every native primitive takes `&AstContext` and only *reads* via
// `parse_result.node()` re-walks). Concurrent shared `&` reads of an immutable
// C arena from many threads is sound (it is exactly `&T: Sync` for a `T`
// containing only never-written raw pointers). The `Arc` is dropped only after
// ALL reader threads — including abandoned (timed-out, never-joined) ones —
// are gone, because every such thread MOVES IN its own `Arc` clone; the arena
// is therefore never freed under an in-flight or zombie native read. No
// aliased `&mut`, no interior mutability, no writes: `Send + Sync` holds.
unsafe impl Send for AstContext {}
unsafe impl Sync for AstContext {}

impl AstContext {
    /// LIVE resolution: walk the real prism tree NOW and return the Nth
    /// (walk-order) CallNode, where N == handle. Touches the live C arena via
    /// `parse_result.node()` every call — zero snapshot.
    fn with_call_node<R>(
        &self,
        handle: usize,
        f: impl FnOnce(&ruby_prism::CallNode<'_>) -> R,
    ) -> Option<R> {
        if handle >= self.node_count {
            return None;
        }

        struct Finder<'a, R> {
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
/// there from `Arc::as_ptr`; the thread that opened this `mrb_state` owns an
/// `Arc` clone alive for the whole run (and, if abandoned, indefinitely), so
/// this deref is sound. No Arc refcount is touched here.
unsafe fn ctx<'a>(mrb: *mut mrb_state) -> &'a AstContext {
    let ud = (*mrb).ud as *const AstContext;
    assert!(!ud.is_null(), "mrb_state.ud must hold the AstContext ptr");
    &*ud
}

/// `Murphy.node_count` -> Integer.
unsafe extern "C" fn native_node_count(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    NATIVE_CALLS.fetch_add(1, Ordering::Relaxed);
    let n = ctx(mrb).node_count;
    let s = CString::new(n.to_string()).unwrap();
    mrb_load_string(mrb, s.as_ptr())
}

/// `Murphy.node_name(handle)` -> String. Resolves the handle to the LIVE prism
/// node and reads its real `name()`. Only the integer crossed FFI.
unsafe extern "C" fn native_node_name(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    NATIVE_CALLS.fetch_add(1, Ordering::Relaxed);
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
    NATIVE_CALLS.fetch_add(1, Ordering::Relaxed);
    let mut handle: mrb_int = -1;
    let fmt = CStr::from_bytes_with_nul(b"i\0").unwrap();
    mrb_get_args(mrb, fmt.as_ptr(), &mut handle as *mut mrb_int);

    let is_nil = ctx(mrb)
        .with_call_node(handle as usize, |n| n.receiver().is_none())
        .unwrap_or(true);

    let lit = if is_nil {
        b"true\0".as_ref()
    } else {
        b"false\0".as_ref()
    };
    mrb_load_string(mrb, CStr::from_bytes_with_nul(lit).unwrap().as_ptr())
}

/// `Murphy.node_msg_range(handle)` -> "start,end" byte offsets (ADR 0001).
unsafe extern "C" fn native_node_msg_range(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    NATIVE_CALLS.fetch_add(1, Ordering::Relaxed);
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

// `Murphy.offense(handle, message)` — a cop reporting an offense. We stash
// (handle, message) into a thread-local sink the worker drains AFTER
// `mrb_load_string` returns and BEFORE `mrb_close`, so the offense never
// crosses through the shared (read-only) `AstContext`.
thread_local! {
    static COP_OFFENSES: std::cell::RefCell<Vec<(usize, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

unsafe extern "C" fn native_offense(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    NATIVE_CALLS.fetch_add(1, Ordering::Relaxed);
    let mut handle: mrb_int = -1;
    let mut p: *const c_char = std::ptr::null();
    let fmt = CStr::from_bytes_with_nul(b"iz\0").unwrap();
    mrb_get_args(
        mrb,
        fmt.as_ptr(),
        &mut handle as *mut mrb_int,
        &mut p as *mut *const c_char,
    );
    let msg = CStr::from_ptr(p).to_string_lossy().into_owned();
    COP_OFFENSES.with(|c| c.borrow_mut().push((handle as usize, msg)));
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
    def(
        CStr::from_bytes_with_nul(b"node_count\0").unwrap(),
        native_node_count,
        0,
    );
    def(
        CStr::from_bytes_with_nul(b"node_name\0").unwrap(),
        native_node_name,
        1,
    );
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
    def(
        CStr::from_bytes_with_nul(b"offense\0").unwrap(),
        native_offense,
        2,
    );
}

/// Pre-walk ONLY to COUNT call nodes (to size the handle space 0..count).
/// Stores NO node data — every read happens LIVE in the native primitives.
struct NodeCounter {
    count: usize,
}
impl<'pr> Visit<'pr> for NodeCounter {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        self.count += 1;
        ruby_prism::visit_call_node(self, node);
    }
}

// ===========================================================================
// Offense model + total-order sort (ADR 0006/0007 determinism)
// ===========================================================================

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Offense {
    file: String,
    start_offset: usize,
    end_offset: usize,
    cop_name: String,
    message: String,
    severity: Severity,
}

/// Phase-2 `aggregate`'s total-order key: (file, start, end, cop, message,
/// severity). Total ⇒ output is input-order-independent.
fn aggregate(mut offenses: Vec<Offense>) -> Vec<Offense> {
    offenses.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.start_offset.cmp(&b.start_offset))
            .then(a.end_offset.cmp(&b.end_offset))
            .then(a.cop_name.cmp(&b.cop_name))
            .then(a.message.cmp(&b.message))
            .then(a.severity.cmp(&b.severity))
    });
    offenses
}

/// A stable byte serialization of the aggregated offenses (stand-in for the
/// Phase-2 JSON array) so determinism can be asserted byte-for-byte.
fn serialize(offenses: &[Offense]) -> String {
    let mut s = String::new();
    for o in offenses {
        s.push_str(&format!(
            "{}|{}|{}|{}|{:?}|{}\n",
            o.file, o.start_offset, o.end_offset, o.cop_name, o.severity, o.message
        ));
    }
    s
}

// ===========================================================================
// The per-cop watchdog worker (Mechanism A, ADR 0003 / deadline_poc)
// ===========================================================================

enum CopResult {
    Completed(Vec<Offense>),
    /// The cop hit the wall-clock deadline; its thread is ABANDONED.
    TimedOut,
    /// The cop raised an mruby exception (design §6 exception isolation).
    Raised,
}

/// Run ONE mruby cop under a wall-clock deadline on its OWN OS thread with its
/// OWN isolated `mrb_state`. The thread MOVES IN its OWN `Arc<AstContext>`
/// clone (`worker_ctx`) so an abandoned thread keeps the AST alive for a late
/// native call even after the rayon file-worker dropped its Arc.
///
/// `cop_name`/`cop_src` are `'static` (fixture strings); the AST is shared via
/// the Arc clone.
fn run_cop_with_deadline(
    cop_name: &'static str,
    cop_src: &'static str,
    file_ctx: &Arc<AstContext>,
    deadline: Duration,
) -> CopResult {
    let (tx, rx) = mpsc::channel::<Result<Vec<(usize, String)>, ()>>();
    // THE LOAD-BEARING LINE: the spawned cop thread owns its OWN Arc clone.
    // If this thread times out and is abandoned, the rayon file-worker can
    // drop `file_ctx` and return; this clone keeps source+ParseResult alive,
    // so a still-running zombie native call derefs valid memory.
    let worker_ctx: Arc<AstContext> = Arc::clone(file_ctx);

    thread::spawn(move || {
        // The whole mruby lifecycle lives ON THIS THREAD with an isolated
        // state. `worker_ctx` is captured by move; it stays alive for as long
        // as this closure (well-behaved OR zombie) runs.
        let result = unsafe {
            let mrb = mrb_open();
            if mrb.is_null() {
                let _ = tx.send(Err(()));
                return;
            }
            // ud := raw *const into THIS THREAD's Arc clone (not the file
            // worker's). Sound: `worker_ctx` outlives every native call here.
            (*mrb).ud = Arc::as_ptr(&worker_ctx) as *mut c_void;
            define_primitives(mrb);

            COP_OFFENSES.with(|c| c.borrow_mut().clear());

            let cscript = CString::new(cop_src).unwrap();
            // For the pathological `while true; end` cop this NEVER returns —
            // the thread blocks here forever inside mruby C. That is the point;
            // the host abandons it via recv_timeout.
            mrb_load_string(mrb, cscript.as_ptr());

            // EXCEPTION ISOLATION (design §6): mruby exceptions do NOT unwind
            // into Rust. After load, a non-null `(*mrb).exc` means the cop
            // raised. We MUST check explicitly or a raising cop looks
            // Completed and the exception test passes for the wrong reason.
            let raised = !(*mrb).exc.is_null();

            let offenses: Vec<(usize, String)> =
                COP_OFFENSES.with(|c| std::mem::take(&mut *c.borrow_mut()));

            // DROP ORDER (ADR 0002/0005): close mruby BEFORE this thread's
            // Arc clone drops. mrb_close can run GC finalizers; reversing is
            // UB (a finalizer could deref a freed prism arena).
            mrb_close(mrb);

            if raised {
                Err(())
            } else {
                Ok(offenses)
            }
        };
        // If the host already moved on (timed out), the receiver is gone and
        // this send fails harmlessly — the thread just exits.
        let _ = tx.send(result);
        // `worker_ctx` drops HERE, on this thread, after mrb_close. For an
        // abandoned thread this point is reached late (or, for the infinite
        // loop, never — process exit reaps it); either way the AST outlived
        // every native call this thread could make.
    });

    match rx.recv_timeout(deadline) {
        Ok(Ok(raw)) => {
            // Map raw (handle, message) into real Offenses by live-resolving
            // each handle's byte range against the file's AST (host side; the
            // file_ctx Arc is alive here).
            let mut out = Vec::new();
            for (h, msg) in raw {
                let (s, e) = file_ctx
                    .with_call_node(h, |n| match n.message_loc() {
                        Some(m) => (m.start_offset(), m.end_offset()),
                        None => (0, 0),
                    })
                    .unwrap_or((0, 0));
                out.push(Offense {
                    file: file_ctx.file.clone(),
                    start_offset: s,
                    end_offset: e,
                    cop_name: cop_name.to_string(),
                    message: msg,
                    severity: Severity::Warning,
                });
            }
            CopResult::Completed(out)
        }
        Ok(Err(())) => CopResult::Raised,
        Err(mpsc::RecvTimeoutError::Timeout) => CopResult::TimedOut, // thread abandoned
        Err(mpsc::RecvTimeoutError::Disconnected) => CopResult::Raised,
    }
}

// ===========================================================================
// Fixtures
// ===========================================================================

/// A "file": name + Ruby source. Shared `&'static str` so cop/source strings
/// are trivially movable into threads.
struct Fixture {
    name: &'static str,
    source: &'static str,
    /// Which mruby cops run on this file. `(cop_name, cop_src)`.
    cops: &'static [(&'static str, &'static str)],
}

// A well-behaved cop: flag every `puts` with no receiver (mirrors the real
// NoReceiverPuts native cop, but written as a user `.rb`).
const COP_NO_RECEIVER_PUTS: &str = r##"
    class Node
      def initialize(h) = @h = h
      def name          = Murphy.node_name(@h)
      def receiver_nil? = Murphy.node_receiver_nil?(@h)
    end
    Murphy.node_count.times do |i|
      n = Node.new(i)
      if n.name == "puts" && n.receiver_nil?
        Murphy.offense(i, "puts without explicit receiver")
      end
    end
"##;

// Another well-behaved cop: flag any call named `eval`.
const COP_NO_EVAL: &str = r##"
    Murphy.node_count.times do |i|
      if Murphy.node_name(i) == "eval"
        Murphy.offense(i, "avoid eval")
      end
    end
"##;

// The pathological cop: ZERO yield points, no native callback per iteration.
// Only thread-abandon (Mechanism A) bounds this — a cooperative flag scheme
// would not.
const COP_RUNAWAY: &str = "while true; end";

// The raising cop: design §6 exception isolation. mruby exceptions do not
// unwind to Rust; the worker detects via (*mrb).exc.
const COP_RAISES: &str = r##"
    Murphy.node_count.times do |i|
      Murphy.node_name(i)   # do some real native work first
    end
    raise "cop blew up on purpose"
"##;

fn fixtures() -> Vec<Fixture> {
    // Multiple files: clean ones, the looping one, the raising one, and ones
    // with genuine offenses — so determinism/aggregation has real content.
    vec![
        Fixture {
            name: "clean_a.rb",
            source: "x = 1 + 2\nFoo.bar(1)\n",
            cops: &[("NoReceiverPuts", COP_NO_RECEIVER_PUTS), ("NoEval", COP_NO_EVAL)],
        },
        Fixture {
            name: "has_puts.rb",
            source: "puts \"hi\"\nlogger.info(x)\nputs(42)\n",
            cops: &[("NoReceiverPuts", COP_NO_RECEIVER_PUTS), ("NoEval", COP_NO_EVAL)],
        },
        Fixture {
            name: "has_eval.rb",
            source: "eval(\"1\")\nputs \"y\"\n",
            cops: &[("NoReceiverPuts", COP_NO_RECEIVER_PUTS), ("NoEval", COP_NO_EVAL)],
        },
        Fixture {
            name: "looping_cop.rb",
            source: "puts \"this file's mruby cop loops forever\"\n",
            cops: &[
                ("RunawayCop", COP_RUNAWAY),
                // a good cop ALSO on this file — must still produce its offense
                ("NoReceiverPuts", COP_NO_RECEIVER_PUTS),
            ],
        },
        Fixture {
            name: "raising_cop.rb",
            source: "puts \"this file's mruby cop raises\"\nFoo.baz\n",
            cops: &[
                ("RaisingCop", COP_RAISES),
                ("NoReceiverPuts", COP_NO_RECEIVER_PUTS),
            ],
        },
        Fixture {
            name: "clean_b.rb",
            source: "Account.new\nx.y.z\n",
            cops: &[("NoReceiverPuts", COP_NO_RECEIVER_PUTS), ("NoEval", COP_NO_EVAL)],
        },
    ]
}

/// A trivial NATIVE cop (Rust, runs synchronously on the file worker's Arc,
/// no mruby): flag every call node that HAS a receiver (a stand-in
/// "ExplicitReceiverCall" check). Iterates the full handle space 0..node_count
/// reading each node LIVE — proves native + mruby offenses from the SAME
/// shared AST aggregate together. (`with_call_node` returns `None` only at
/// out-of-bounds; for an in-bounds handle it returns `Some(inner)` where
/// `inner` is the closure's own `Option` — so we explicitly index 0..count.)
fn native_cop(ctx: &Arc<AstContext>) -> Vec<Offense> {
    let mut out = Vec::new();
    for h in 0..ctx.node_count {
        let hit = ctx.with_call_node(h, |n| {
            if n.receiver().is_some() {
                let name = String::from_utf8_lossy(n.name().as_slice()).into_owned();
                n.message_loc()
                    .map(|m| (name, m.start_offset(), m.end_offset()))
            } else {
                None
            }
        });
        if let Some(Some((name, s, e))) = hit {
            out.push(Offense {
                file: ctx.file.clone(),
                start_offset: s,
                end_offset: e,
                cop_name: "NativeExplicitReceiver".to_string(),
                message: format!("explicit-receiver call `{name}`"),
                severity: Severity::Warning,
            });
        }
    }
    out
}

/// Lint ONE file: parse once into an `Arc<AstContext>`, run the native cop,
/// then dispatch each mruby cop on its own watchdog thread. Returns the file's
/// offenses (good cops) + any error offenses (looping/raising cops).
///
/// `side_probe`: for the one designated "looping_cop.rb" file, the caller
/// passes an `Arc` it will keep, so AFTER this returns it can assert the AST
/// is still alive for the zombie (strong_count >= 2: side probe + zombie).
fn lint_one_file(
    fx: &Fixture,
    deadline: Duration,
    side_probe: Option<&std::sync::Mutex<Option<Arc<AstContext>>>>,
) -> Vec<Offense> {
    // 1. Real prism parse — OWNED source, Spike-3.1 layout + drop order.
    let source: Box<[u8]> = fx.source.as_bytes().to_vec().into_boxed_slice();
    let result: ParseResult<'_> = parse(&source);
    assert!(
        result.errors().next().is_none(),
        "fixture {} must parse cleanly",
        fx.name
    );
    let mut counter = NodeCounter { count: 0 };
    counter.visit(&result.node());

    // 2. LIFETIME LAUNDER (Spike 3.1 unsafe core): ParseResult<'borrow source>
    //    -> ParseResult<'static>. Sound only because source+result move into
    //    the SAME AstContext, parse_result declared first (drop order), Arc'd.
    let parse_result: ParseResult<'static> =
        unsafe { std::mem::transmute::<ParseResult<'_>, ParseResult<'static>>(result) };

    let ctx: Arc<AstContext> = Arc::new(AstContext {
        parse_result,
        source,
        node_count: counter.count,
        file: fx.name.to_string(),
    });

    // Side probe: hand a clone to the caller-held slot BEFORE we dispatch the
    // looping cop, so the caller can prove the zombie keeps the AST alive.
    if let Some(slot) = side_probe {
        *slot.lock().unwrap() = Some(Arc::clone(&ctx));
    }

    let mut offenses: Vec<Offense> = Vec::new();

    // 3. NATIVE cop runs synchronously on this worker's Arc (no thread).
    offenses.extend(native_cop(&ctx));

    // 4. Each mruby cop on its own watchdog thread + isolated mrb_state, each
    //    owning its own Arc clone (moved into the thread).
    for (cop_name, cop_src) in fx.cops {
        match run_cop_with_deadline(cop_name, cop_src, &ctx, deadline) {
            CopResult::Completed(o) => offenses.extend(o),
            CopResult::TimedOut => offenses.push(Offense {
                file: fx.name.to_string(),
                start_offset: 0,
                end_offset: 0,
                cop_name: cop_name.to_string(),
                message: format!("error: cop `{cop_name}` exceeded deadline (abandoned)"),
                severity: Severity::Error,
            }),
            CopResult::Raised => offenses.push(Offense {
                file: fx.name.to_string(),
                start_offset: 0,
                end_offset: 0,
                cop_name: cop_name.to_string(),
                message: format!("error: cop `{cop_name}` raised an exception"),
                severity: Severity::Error,
            }),
        }
    }

    // 5. The rayon file worker's `ctx` Arc drops HERE. For the looping file,
    //    its RunawayCop thread is abandoned and STILL HOLDS its own clone, so
    //    the AstContext is NOT freed — proven by the caller's side probe.
    offenses
}

// ===========================================================================
// main: compose rayon × watchdog × Arc; assert everything
// ===========================================================================

fn main() {
    // Tight deadline: 5 shuffled runs × stress must not become a coffee break.
    let deadline = Duration::from_millis(150);

    // ---- One full pipeline run over all fixtures, in parallel. ------------
    // `side_slot` lets us observe the looping file's AST liveness after its
    // rayon worker returned and dropped its Arc.
    let side_slot: std::sync::Mutex<Option<Arc<AstContext>>> = std::sync::Mutex::new(None);

    let run_pipeline = |fxs: &[Fixture],
                        side: Option<&std::sync::Mutex<Option<Arc<AstContext>>>>|
     -> Vec<Offense> {
        // Phase-2's rayon shape: par_iter over files, flatten, aggregate.
        let per_file: Vec<Vec<Offense>> = fxs
            .par_iter()
            .map(|fx| {
                let s = if fx.name == "looping_cop.rb" { side } else { None };
                lint_one_file(fx, deadline, s)
            })
            .collect();
        aggregate(per_file.into_iter().flatten().collect())
    };

    let host_start = Instant::now();
    let fxs = fixtures();
    let offenses = run_pipeline(&fxs, Some(&side_slot));
    let host_elapsed = host_start.elapsed();

    println!("=== Composition pipeline: 1 run, {} files ===", fxs.len());
    for o in &offenses {
        println!(
            "  {:<16} {:>3}..{:<3} {:<20} [{:?}] {}",
            o.file, o.start_offset, o.end_offset, o.cop_name, o.severity, o.message
        );
    }
    println!("host wall time: {host_elapsed:?}");
    println!("native primitive calls serviced so far: {}", NATIVE_CALLS.load(Ordering::Relaxed));

    // ---- ASSERTION 1: host NOT held hostage by the infinite-loop cop. -----
    // 6 files in parallel; the only multi-deadline cost is the looping cop's
    // single 150ms timeout. Generous bound = 6× deadline covers cold mruby
    // init + rayon scheduling without ever approaching "infinite".
    assert!(
        host_elapsed < deadline * 6,
        "host must not be held hostage by the runaway cop (got {host_elapsed:?}, \
         deadline {deadline:?}) — it bounded, did not hang"
    );

    // ---- ASSERTION 2: abandon-under-threads — zombie keeps AST alive. -----
    // The rayon worker for looping_cop.rb has returned and dropped its Arc.
    // The RunawayCop thread is abandoned (still spinning in `while true`),
    // holding its own clone. The side probe + that zombie => strong_count >= 2.
    let probe = side_slot.lock().unwrap().take().expect("side probe was set");
    let sc = Arc::strong_count(&probe);
    println!(
        "looping_cop.rb AstContext strong_count after worker returned: {sc} \
         (side probe + abandoned zombie thread's own clone)"
    );
    assert!(
        sc >= 2,
        "abandoned RunawayCop thread MUST still hold its own Arc clone — \
         AST alive for any late zombie native call (got strong_count {sc})"
    );
    drop(probe); // we relinquish the probe; zombie's clone still keeps it alive

    // ---- ASSERTION 3: looping cop → exactly one error offense. ------------
    let runaway_errs: Vec<&Offense> = offenses
        .iter()
        .filter(|o| o.file == "looping_cop.rb" && o.cop_name == "RunawayCop")
        .collect();
    assert_eq!(
        runaway_errs.len(),
        1,
        "looping cop → exactly one error offense for that cop×file"
    );
    assert_eq!(runaway_errs[0].severity, Severity::Error);
    assert!(runaway_errs[0].message.contains("exceeded deadline"));

    // The OTHER (good) cop on the looping file MUST still have run.
    assert!(
        offenses
            .iter()
            .any(|o| o.file == "looping_cop.rb" && o.cop_name == "NoReceiverPuts"),
        "the good cop on the looping file must still produce its offense — \
         one runaway cop does not poison its sibling cop on the same file"
    );

    // ---- ASSERTION 4: raising cop → exactly one error offense. ------------
    let raise_errs: Vec<&Offense> = offenses
        .iter()
        .filter(|o| o.file == "raising_cop.rb" && o.cop_name == "RaisingCop")
        .collect();
    assert_eq!(
        raise_errs.len(),
        1,
        "raising cop → exactly one error offense for that cop×file"
    );
    assert_eq!(raise_errs[0].severity, Severity::Error);
    assert!(raise_errs[0].message.contains("raised an exception"));
    assert!(
        offenses
            .iter()
            .any(|o| o.file == "raising_cop.rb" && o.cop_name == "NoReceiverPuts"),
        "the good cop on the raising file must still run (exception isolated)"
    );

    // GLOBAL error-count discriminator: across the WHOLE parallel run there
    // must be EXACTLY two Error offenses — RunawayCop (timeout) and
    // RaisingCop (exception). If any good cop spuriously errored (e.g. the
    // exc-null check were vestigial and silently mis-classified), this count
    // would not be 2.
    let err_count = offenses
        .iter()
        .filter(|o| o.severity == Severity::Error)
        .count();
    assert_eq!(
        err_count, 2,
        "exactly RunawayCop + RaisingCop produce error offenses; every other \
         cop×file completes normally (got {err_count})"
    );

    // ---- ASSERTION 5: good cops produced their real offenses. -------------
    // has_puts.rb: `puts "hi"` (no recv) and `puts(42)` (no recv) → 2 offenses
    // from NoReceiverPuts; `logger.info` has a receiver → NOT flagged.
    let puts_offs: Vec<&Offense> = offenses
        .iter()
        .filter(|o| o.file == "has_puts.rb" && o.cop_name == "NoReceiverPuts")
        .collect();
    assert_eq!(
        puts_offs.len(),
        2,
        "has_puts.rb has two receiverless `puts` → two NoReceiverPuts offenses \
         (got {:?})",
        puts_offs
    );
    // has_eval.rb: NoEval flags the `eval` call.
    assert!(
        offenses
            .iter()
            .any(|o| o.file == "has_eval.rb" && o.cop_name == "NoEval"),
        "NoEval must flag the eval call in has_eval.rb"
    );
    // Native cop fired on Foo.bar / Account.new etc.
    assert!(
        offenses
            .iter()
            .any(|o| o.cop_name == "NativeExplicitReceiver"),
        "the native (Rust) cop must also contribute offenses from the SAME \
         shared AST — native + mruby offenses aggregate together"
    );

    // ---- ASSERTION 6: determinism across 5 shuffled/repeated runs. --------
    // Re-run the WHOLE parallel pipeline 5 times with the fixture order
    // ROTATED each time (a deterministic shuffle). Despite thread
    // interleaving, abandoned threads, and rayon work-stealing, the
    // total-order-sorted serialization MUST be byte-identical every time.
    let reference = serialize(&offenses);
    for run in 1..=5 {
        let mut shuffled = fixtures();
        shuffled.rotate_left(run); // deterministic distinct permutation
        let again = run_pipeline(&shuffled, None);
        let bytes = serialize(&again);
        assert_eq!(
            bytes, reference,
            "run {run}: shuffled-order output MUST be byte-identical to the \
             reference (determinism under thread interleaving + abandoned \
             threads, ADR 0006/0007)"
        );
    }
    println!("determinism: 5 shuffled/repeated runs → byte-identical output ✓");

    // ---- ASSERTION 7: Send+Sync soundness under HEAVY concurrent reads. ---
    // TSan is not used here (see report — ThreadSanitizer requires a nightly
    // toolchain / rebuilt std not available in this sandbox). Instead: a heavy
    // concurrent stress — many files × many mruby cops, each cop driving many
    // live native re-walks of the SAME Arc'd C arena from distinct threads —
    // repeated, with results checked against a single-threaded reference.
    //
    // Single-threaded reference for one stress fixture:
    let stress_fx = Fixture {
        name: "stress.rb",
        source: "puts \"a\"\nputs \"b\"\nFoo.bar\nlogger.info(z)\nputs(c)\neval(\"e\")\n",
        cops: &[
            ("NoReceiverPuts", COP_NO_RECEIVER_PUTS),
            ("NoEval", COP_NO_EVAL),
        ],
    };
    let ref_stress = {
        // Run it once, alone, as the source of truth.
        let v = lint_one_file(&stress_fx, deadline, None);
        serialize(&aggregate(v))
    };
    let n_files = 80usize;
    let stress_set: Vec<Fixture> = (0..n_files)
        .map(|_| Fixture {
            name: "stress.rb",
            source: stress_fx.source,
            cops: stress_fx.cops,
        })
        .collect();
    for round in 1..=5 {
        let v: Vec<Vec<Offense>> = stress_set
            .par_iter()
            .map(|fx| lint_one_file(fx, deadline, None))
            .collect();
        // All 80 files are identical → each must yield exactly the reference
        // offense set; aggregate over one representative and compare.
        let mut one = v[0].clone();
        one = aggregate(one);
        assert_eq!(
            serialize(&one),
            ref_stress,
            "stress round {round}: a concurrently-read Arc'd AST yielded WRONG \
             data → a data race / unsound Send+Sync would surface here"
        );
        // Every one of the 80 parallel results must match too (no partial /
        // torn reads from any worker).
        for (i, o) in v.iter().enumerate() {
            assert_eq!(
                serialize(&aggregate(o.clone())),
                ref_stress,
                "stress round {round}: parallel file #{i} diverged — concurrent \
                 native reads of the shared Arc'd C arena must all be correct"
            );
        }
    }
    let total_calls = NATIVE_CALLS.load(Ordering::Relaxed);
    println!(
        "Send+Sync stress: {n_files} files × {} cops × 5 rounds, all results == \
         single-threaded reference; {total_calls} native primitive calls \
         serviced concurrently with NO incorrect result",
        stress_fx.cops.len()
    );
    assert!(
        total_calls > 1000,
        "stress must have driven a substantial volume of concurrent native \
         reads (got {total_calls})"
    );

    println!("\nALL COMPOSITION ASSERTIONS PASSED");
}
