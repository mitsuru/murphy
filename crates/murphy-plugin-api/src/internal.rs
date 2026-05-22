//! Doc-hidden plumbing for the `register_cops!` proc macro (ADR 0035).
//!
//! `register_cops!` expands to references into this module so the
//! generated code stays a thin list-to-table transform: the const fns
//! and the panic-trapping dispatch thunk live here, in a normal crate
//! that unit tests can exercise rather than in macro-generated code.

use std::panic::{AssertUnwindSafe, catch_unwind};

use murphy_ast::NodeId;

use crate::abi::{CxRaw, PluginCopV1, RawSlice};
use crate::cx::Cx;
use crate::node_cop::NodeCop;
use crate::options::CopOptions;
use crate::severity::{Severity, tristate_to_wire};

/// Pack one cop type's `Cop` + `NodeCop` metadata into a [`PluginCopV1`]
/// registration descriptor.
///
/// A `const fn` so `register_cops!` can build a `static` cop table
/// (ADR 0035): every input is read through associated `const`s, and the
/// per-cop dispatch entry is the monomorphized [`dispatch_thunk`].
///
/// `C: Default` because [`NodeCop::check`] takes `&self`; the thunk
/// constructs a fresh, stateless cop value per matched node.
pub const fn build_cop<C: NodeCop + Default>() -> PluginCopV1 {
    PluginCopV1 {
        size: std::mem::size_of::<PluginCopV1>(),
        name: RawSlice::from_str(C::NAME),
        description: RawSlice::from_str(C::DESCRIPTION),
        default_severity: Severity::to_wire(C::DEFAULT_SEVERITY),
        default_enabled: tristate_to_wire(C::DEFAULT_ENABLED),
        options_ptr: <C::Options as CopOptions>::SCHEMA.as_ptr(),
        options_len: <C::Options as CopOptions>::SCHEMA.len(),
        kinds_ptr: <C as NodeCop>::KINDS.as_ptr(),
        kinds_len: <C as NodeCop>::KINDS.len(),
        dispatch: dispatch_thunk::<C>,
    }
}

/// The per-cop dispatch entry `register_cops!` stores in
/// [`PluginCopV1::dispatch`]. Monomorphized once per registered cop.
///
/// Builds a [`Cx`] from the raw context, constructs the (stateless) cop
/// via [`Default`], and runs [`NodeCop::check`]. A panic is trapped here
/// — it must not unwind across the ABI boundary (ADR 0038). Returns `0`
/// on success and a non-zero code on a null context or a trapped panic.
///
/// # Safety
/// `cx`, when non-null, must point to a [`CxRaw`] whose pointer/length
/// fields describe live, immutable data for the duration of the call
/// (the ADR 0038 safety contract the host upholds per dispatch).
pub unsafe extern "C" fn dispatch_thunk<C: NodeCop + Default>(
    node: NodeId,
    cx: *const CxRaw,
) -> i32 {
    if cx.is_null() {
        return 1;
    }
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // Safety: the caller's contract guarantees `cx` is a valid,
        // non-null `CxRaw` for this call.
        let raw: &CxRaw = unsafe { &*cx };
        let cx = unsafe { Cx::from_raw(raw) };
        C::default().check(node, &cx);
    }));
    match outcome {
        Ok(()) => 0,
        Err(_) => 2,
    }
}

/// Const-panic if any two of `names` are equal.
///
/// `register_cops!` wraps a call in a `const _: () = …;` block so a
/// duplicate cop `NAME` surfaces as a compile error.
pub const fn assert_unique_cop_names<const N: usize>(names: [&str; N]) {
    let mut i = 0;
    while i < N {
        let mut j = i + 1;
        while j < N {
            if str_eq(names[i], names[j]) {
                panic!("register_cops!: two registered cops share the same NAME");
            }
            j += 1;
        }
        i += 1;
    }
}

/// `const`-evaluable string equality (`str::eq` is not `const`).
const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{CxRaw, FnTable, PluginCopV1, RawEdit, RawOffense, RawSlice};
    use crate::cop::Cop;
    use crate::cx::Cx;
    use crate::node_cop::{NodeCop, NodeKindTag};
    use crate::options::NoOptions;
    use crate::severity::{Severity, tristate_to_wire};
    use murphy_ast::{Ast, AstBuilder, NodeId, NodeKind, Range};
    use std::cell::RefCell;

    /// A minimal cop: metadata + an empty `check`.
    #[derive(Default)]
    struct StubCop;
    impl Cop for StubCop {
        type Options = NoOptions;
        const NAME: &'static str = "Plugin/Stub";
        const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    }
    impl NodeCop for StubCop {
        const KINDS: &'static [NodeKindTag] = &[NodeKindTag(1)];
        fn check(&self, _node: NodeId, _cx: &Cx<'_>) {}
    }

    /// `build_cop` must be usable in `const` context — the load-bearing
    /// assumption that `register_cops!` builds a `static` cop table.
    const _: PluginCopV1 = build_cop::<StubCop>();

    /// Distinct names must pass at const-eval time.
    const _: () = assert_unique_cop_names(["Plugin/A", "Plugin/B"]);

    #[test]
    fn build_cop_packs_cop_and_node_cop_metadata() {
        let cop = build_cop::<StubCop>();
        assert_eq!(unsafe { cop.name.as_bytes() }, b"Plugin/Stub");
        assert_eq!(unsafe { cop.description.as_bytes() }, b"");
        assert_eq!(
            cop.default_severity,
            Severity::to_wire(Some(Severity::Warning))
        );
        assert_eq!(cop.default_enabled, tristate_to_wire(None));
        assert_eq!(cop.kinds_len, 1);
        assert_eq!(cop.options_len, 0);
        assert_eq!(cop.size, std::mem::size_of::<PluginCopV1>());
    }

    #[test]
    fn assert_unique_cop_names_accepts_distinct_names() {
        assert_unique_cop_names(["Plugin/One", "Plugin/Two", "Plugin/Three"]);
    }

    #[test]
    #[should_panic(expected = "same NAME")]
    fn assert_unique_cop_names_panics_on_a_duplicate() {
        assert_unique_cop_names(["Plugin/Dup", "Plugin/Other", "Plugin/Dup"]);
    }

    // --- dispatch_thunk -----------------------------------------------

    /// Records offenses forwarded through a `FnTable`.
    struct Sink {
        offenses: Vec<String>,
    }

    unsafe extern "C" fn record_offense(sink: *mut std::ffi::c_void, o: *const RawOffense) {
        let sink = unsafe { &*(sink as *const RefCell<Sink>) };
        let o = unsafe { &*o };
        sink.borrow_mut()
            .offenses
            .push(String::from_utf8(unsafe { o.message.as_bytes() }.to_vec()).unwrap());
    }

    unsafe extern "C" fn noop_edit(_: *mut std::ffi::c_void, _: *const RawEdit) {}

    /// A cop whose `check` emits one offense, proving the thunk ran it.
    #[derive(Default)]
    struct EmittingCop;
    impl Cop for EmittingCop {
        type Options = NoOptions;
        const NAME: &'static str = "Plugin/Emit";
    }
    impl NodeCop for EmittingCop {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, node: NodeId, cx: &Cx<'_>) {
            cx.emit_offense(cx.range(node), "thunk ran check", None);
        }
    }

    /// A cop whose `check` panics, exercising the thunk's unwind trap.
    #[derive(Default)]
    struct PanickingCop;
    impl Cop for PanickingCop {
        type Options = NoOptions;
        const NAME: &'static str = "Plugin/Panic";
    }
    impl NodeCop for PanickingCop {
        const KINDS: &'static [NodeKindTag] = &[];
        fn check(&self, _node: NodeId, _cx: &Cx<'_>) {
            panic!("cop check deliberately panicked");
        }
    }

    fn single_node_ast() -> (Ast, NodeId) {
        let mut b = AstBuilder::new("nil", "t.rb".to_string());
        let root = b.push(NodeKind::Nil, Range { start: 0, end: 3 });
        (b.finish(root), root)
    }

    fn cx_raw_for(ast: &Ast, fns: &FnTable, sink: *mut std::ffi::c_void) -> CxRaw {
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
            cop_name: RawSlice::from_str("Plugin/Emit"),
            fns: fns as *const FnTable,
            sink,
        }
    }

    #[test]
    fn dispatch_thunk_builds_a_cx_and_runs_check() {
        let (ast, root) = single_node_ast();
        let fns = FnTable {
            emit_offense: record_offense,
            emit_edit: noop_edit,
        };
        let sink = RefCell::new(Sink {
            offenses: Vec::new(),
        });
        let raw = cx_raw_for(&ast, &fns, &sink as *const _ as *mut std::ffi::c_void);

        let rc = unsafe { dispatch_thunk::<EmittingCop>(root, &raw) };

        assert_eq!(rc, 0);
        assert_eq!(sink.borrow().offenses, vec!["thunk ran check".to_string()]);
    }

    #[test]
    fn dispatch_thunk_traps_a_panic_and_returns_non_zero() {
        let (ast, root) = single_node_ast();
        let fns = FnTable {
            emit_offense: record_offense,
            emit_edit: noop_edit,
        };
        let raw = cx_raw_for(&ast, &fns, std::ptr::null_mut());

        // Silence the panic print for this deliberate panic.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let rc = unsafe { dispatch_thunk::<PanickingCop>(root, &raw) };
        std::panic::set_hook(prev);

        assert_ne!(rc, 0);
    }

    #[test]
    fn dispatch_thunk_rejects_a_null_context() {
        let rc = unsafe { dispatch_thunk::<StubCop>(NodeId(0), std::ptr::null()) };
        assert_ne!(rc, 0);
    }
}
