//! Parser-driven cop test harness, gated by the `test-support` feature.
//!
//! Any plugin pack — `murphy-std`, `murphy-example-pack`,
//! `murphy-rspec`, third-party packs — can enable this feature in its
//! `[dev-dependencies]` and write `#[cfg(test)] mod tests` against
//! its own cops without rebuilding the `CxRaw` + offense-sink
//! plumbing every time.
//!
//! Production plugin binaries never touch this module — `murphy-translate`
//! (the runtime parser) is an optional dep activated only by the
//! feature. With the feature off, this file is `#[cfg]`-gated out of
//! compilation entirely.
//!
//! # Example
//!
//! ```ignore
//! use murphy_plugin_api::test_support::run_cop;
//! use my_pack::MyCop;
//!
//! #[test]
//! fn flags_the_thing() {
//!     let offenses = run_cop::<MyCop>("def foo; end\n");
//!     assert_eq!(offenses.len(), 1);
//!     assert_eq!(offenses[0].cop_name, "Plugin/MyCop");
//! }
//! ```
//!
//! # Dispatch
//!
//! For per-kind cops (`KINDS = &[..]`) every `NodeId` in the arena is
//! handed to `check`; the macro-generated dispatch routes only matching
//! kinds. For file-visit / investigation cops (`KINDS = &[]`) `check`
//! is called once with `cx.root()`, matching the
//! `murphy-core::dispatch::run_cops` contract.

use std::cell::RefCell;

use murphy_ast::Ast;

use crate::{
    Cop, Cx, CxRaw, FnTable, NodeCop, NodeId, Range, RawEdit, RawOffense, RawSlice, Severity,
};

/// Re-export of [`indoc::indoc!`] so plugin packs writing
/// `#[cfg(test)] mod tests` can lift their Ruby fixture strings out of
/// the surrounding Rust indentation without re-declaring the dep. The
/// macro strips the common leading whitespace at compile time.
pub use indoc::indoc;

/// One offense captured by [`run_cop`]. Fields are owned `String`s so
/// callers can inspect them after the underlying `Ast` / `CxRaw` are
/// dropped (the cop receives a borrowed `&Cx<'_>`; we copy out at
/// emission time).
#[derive(Debug, Clone)]
pub struct CapturedOffense {
    pub cop_name: String,
    pub message: String,
    pub range: Range,
    /// `None` when the cop didn't override (host applies its default);
    /// otherwise the cop's declared severity.
    pub severity: Option<Severity>,
}

/// Mutable scratch the FFI callbacks borrow through a `*mut c_void`.
struct Sink {
    offenses: Vec<CapturedOffense>,
}

unsafe extern "C" fn record_offense(sink_ptr: *mut std::ffi::c_void, o: *const RawOffense) {
    let sink = unsafe { &*(sink_ptr as *const RefCell<Sink>) };
    let o = unsafe { &*o };
    let cop_name = String::from_utf8(unsafe { o.cop_name.as_bytes() }.to_vec())
        .expect("cop_name must be UTF-8");
    let message =
        String::from_utf8(unsafe { o.message.as_bytes() }.to_vec()).expect("message must be UTF-8");
    sink.borrow_mut().offenses.push(CapturedOffense {
        cop_name,
        message,
        range: o.range,
        severity: Severity::from_wire(o.severity),
    });
}

unsafe extern "C" fn ignore_edit(_sink: *mut std::ffi::c_void, _e: *const RawEdit) {
    // Autocorrect edits are not captured by the basic harness. Cops
    // that emit edits should write a richer test that records them;
    // this default keeps the FnTable valid.
}

/// Parse `source` as Ruby, drive `T::check` over every relevant node,
/// and return the captured offenses in emission order.
///
/// The cop is instantiated via `T::default()` — matches the stateless
/// `#[derive(Default)]` shape every Murphy cop uses (ADR 0035).
pub fn run_cop<T: NodeCop + Default>(source: &str) -> Vec<CapturedOffense> {
    let ast = murphy_translate::translate(source, "t.rb");
    let cop = T::default();
    let cop_name = RawSlice::from_str(<T as Cop>::NAME);
    let sink = RefCell::new(Sink {
        offenses: Vec::new(),
    });
    let fns = FnTable {
        emit_offense: record_offense,
        emit_edit: ignore_edit,
    };
    let raw = cx_raw_for(&ast, &fns, cop_name, &sink);
    let cx = unsafe { Cx::from_raw(&raw) };

    if T::KINDS.is_empty() {
        // File-visit / investigation dispatch — single call with root,
        // matching the host's `KINDS = &[]` contract.
        cop.check(ast.root(), &cx);
    } else {
        // Per-kind dispatch — feed every node; the macro-generated
        // `check` filters by tag.
        let node_count = ast.raw_parts().nodes.len();
        for i in 0..node_count {
            cop.check(NodeId(i as u32), &cx);
        }
    }

    sink.into_inner().offenses
}

/// Build a `CxRaw` borrowing from `ast`, `fns`, and `sink`. The
/// returned value contains raw pointers; the caller keeps all three
/// alive for the duration of the dispatch.
fn cx_raw_for(ast: &Ast, fns: &FnTable, cop_name: RawSlice, sink: &RefCell<Sink>) -> CxRaw {
    let p = ast.raw_parts();
    CxRaw {
        nodes: p.nodes.as_ptr(),
        nodes_len: p.nodes.len(),
        lists: p.node_lists.as_ptr(),
        lists_len: p.node_lists.len(),
        interner_blob: p.interner_blob.as_ptr(),
        interner_blob_len: p.interner_blob.len(),
        interner_offsets: p.interner_offsets.as_ptr(),
        interner_offsets_len: p.interner_offsets.len(),
        comments: p.comments.as_ptr(),
        comments_len: p.comments.len(),
        source: p.source.as_ptr(),
        source_len: p.source.len(),
        root: p.root,
        cop_name,
        fns: fns as *const FnTable,
        sink: sink as *const _ as *mut std::ffi::c_void,
    }
}
