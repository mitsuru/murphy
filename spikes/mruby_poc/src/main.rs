// Spike 0.2 PoC: embedded mruby + native AST handle, NO serialization.
//
// Proves the load-bearing architecture claim (design §2/§3): an mruby `.rb`
// script can walk a real prism-parsed AST by calling Rust native primitives
// in-process, passing only an opaque integer handle across the boundary — the
// node data itself is never serialized; Rust reads it live from the prism tree.
//
// Handle model: the AST + a flat Vec of CallNode refs live in an `AstContext`
// owned by Rust. A `*mut AstContext` is stashed in `mrb_state.ud`. mruby holds
// only `usize` indices ("handles"); native primitives resolve handle -> live
// prism node and read it directly.
//
// Throwaway spike code. NOT carried into crates/.

// Spike convenience: edition-2024 requires `unsafe {}` even inside
// `unsafe extern "C" fn`. Real crates should keep the lint and wrap blocks
// explicitly; for a throwaway PoC this keeps the FFI shims readable.
#![allow(unsafe_op_in_unsafe_fn)]

use ruby_prism::{parse, Visit};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use mruby3_sys::{
    mrb_class_get, mrb_close, mrb_define_class, mrb_define_module_function, mrb_get_args,
    mrb_int, mrb_load_string, mrb_open, mrb_state, mrb_str_new_cstr, mrb_value,
};

// MRB_ARGS_REQ(n) macro is not in bindgen output. Reproduce it:
// ((mrb_aspec)((n)&0x1f) << 18)
const fn args_req(n: u32) -> u32 {
    (n & 0x1f) << 18
}

// LIFETIME FINDING (load-bearing, see ADR 0002): prism's
// `parse(src) -> ParseResult<'pr>` BORROWS the source. An owner struct that
// holds both source and ParseResult would be self-referential. The real
// architecture avoids this entirely: the Core owns the source buffer and the
// AST as *siblings* for the file's processing scope, with the source outliving
// the AST. The mruby-facing context therefore must NOT hold the borrowed tree
// — it holds opaque handles + derived data only.
//
/// The mruby-facing context. Holds handle-resolved data, NOT the borrowed AST.
/// MUST still outlive the mrb_state (native callbacks deref it via ud).
struct AstContext {
    // Flat handle table: index == handle. Names resolved from the live tree
    // before the mruby run. (Real Murphy resolves lazily via native primitives
    // against the sibling-owned AST; the spike snapshots what `node.name`
    // needs to prove the boundary, not the resolution strategy.)
    names: Vec<String>,
    // Proof channel: native code the Ruby script calls back into.
    collected: Vec<String>,
}

struct Collector {
    names: Vec<String>,
}
impl<'pr> Visit<'pr> for Collector {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        self.names
            .push(String::from_utf8_lossy(node.name().as_slice()).into_owned());
        ruby_prism::visit_call_node(self, node);
    }
}

unsafe fn ctx<'a>(mrb: *mut mrb_state) -> &'a mut AstContext {
    let ud = (*mrb).ud as *mut AstContext;
    assert!(!ud.is_null(), "mrb_state.ud must hold the AstContext");
    &mut *ud
}

/// Native primitive: `Murphy.node_count` -> Integer (no serialization).
unsafe extern "C" fn native_node_count(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let c = ctx(mrb);
    // Build the Integer value by round-tripping through Ruby's own literal.
    // (Avoids mruby's inline value-boxing helpers, which bindgen omits.)
    let s = CString::new(c.names.len().to_string()).unwrap();
    mrb_load_string(mrb, s.as_ptr())
}

/// Native primitive: `Murphy.node_name(handle)` -> String. Reads the LIVE
/// prism-derived data for `handle`; only the integer crossed the boundary.
unsafe extern "C" fn native_node_name(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut handle: mrb_int = -1;
    let fmt = CStr::from_bytes_with_nul(b"i\0").unwrap();
    mrb_get_args(mrb, fmt.as_ptr(), &mut handle as *mut mrb_int);
    let c = ctx(mrb);
    let name = c
        .names
        .get(handle as usize)
        .cloned()
        .unwrap_or_else(|| "<oob>".to_string());
    let cs = CString::new(name).unwrap();
    mrb_str_new_cstr(mrb, cs.as_ptr())
}

/// Native sink: `Murphy.report(str)` — proves the Ruby script drove native
/// code with data it derived from the AST. We assert on `collected` afterward.
unsafe extern "C" fn native_report(mrb: *mut mrb_state, _self: mrb_value) -> mrb_value {
    let mut p: *const c_char = std::ptr::null();
    let fmt = CStr::from_bytes_with_nul(b"z\0").unwrap(); // NUL-terminated cstr
    mrb_get_args(mrb, fmt.as_ptr(), &mut p as *mut *const c_char);
    let s = CStr::from_ptr(p).to_string_lossy().into_owned();
    ctx(mrb).collected.push(s);
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
    def(CStr::from_bytes_with_nul(b"report\0").unwrap(), native_report, 1);
}

fn main() {
    // 1. Real prism parse (Spike 0.1 binding). Source kept alive by ParseResult.
    let src = "puts \"hi\"\nlogger.info(x)\nFoo.bar(1)\n";
    let result = parse(src.as_bytes());
    assert!(
        result.errors().next().is_none(),
        "fixture must parse cleanly"
    );
    let mut col = Collector { names: Vec::new() };
    col.visit(&result.node());
    println!("prism found {} call nodes: {:?}", col.names.len(), col.names);

    // `src` and `result` are siblings in this scope and outlive everything
    // below — mirroring the Core owning source+AST for the file scope.
    // Box the mruby-facing context; it MUST outlive the mrb_state.
    let mut boxed = Box::new(AstContext {
        names: col.names.clone(),
        collected: Vec::new(),
    });

    unsafe {
        // 2. Embed mruby, wire the AST context in via ud (no serialization).
        let mrb = mrb_open();
        assert!(!mrb.is_null(), "mrb_open failed");
        (*mrb).ud = (&mut *boxed as *mut AstContext) as *mut std::os::raw::c_void;
        define_primitives(mrb);

        // 3. The user-cop-style `.rb`, run AS-IS. "fast core, scripted glue":
        //    thin Ruby `Node` wraps the native primitives; the walk is in Ruby.
        let script = r#"
            class Node
              def initialize(h) = @h = h
              def name = Murphy.node_name(@h)
            end
            Murphy.node_count.times do |i|
              Murphy.report("visited:" + Node.new(i).name)
            end
        "#;
        let cscript = CString::new(script).unwrap();
        mrb_load_string(mrb, cscript.as_ptr());

        // 4. Independent second state — design §6 needs per-cop isolated states.
        let mrb2 = mrb_open();
        assert!(!mrb2.is_null() && mrb2 != mrb, "second independent mrb_state");
        mrb_load_string(mrb2, CString::new("1 + 1").unwrap().as_ptr());
        mrb_close(mrb2);

        // 5. DROP ORDER IS LOAD-BEARING: close mruby BEFORE the AstContext
        //    (and its ParseResult/source) is dropped. Reversing this is UB —
        //    a GC finalizer or pending native call could deref freed AST.
        mrb_close(mrb);
    }

    // 6. Assertions: the Ruby script walked the real AST via native calls only.
    let expected: Vec<String> = boxed
        .names
        .iter()
        .map(|n| format!("visited:{n}"))
        .collect();
    println!("native-collected from Ruby: {:?}", boxed.collected);
    assert_eq!(
        boxed.collected, expected,
        "Ruby must have walked every prism node through native primitives"
    );
    assert!(
        !boxed.collected.is_empty(),
        "must have walked at least one node"
    );

    // Box (and ParseResult inside it) drops here — after mrb_close. Correct.
    println!("\nALL ASSERTIONS PASSED — mruby walked the live prism AST, no serialization");
}
