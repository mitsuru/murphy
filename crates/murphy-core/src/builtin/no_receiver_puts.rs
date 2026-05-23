//! `Murphy/NoReceiverPuts` on the single-surface plugin API (ADR 0038).
//!
//! Flags a `puts` / `print` / `p` call with no explicit receiver. Ruby's
//! bare debug-output methods almost always belong on a logger; an explicit
//! receiver (`logger.info "x"`, `obj.puts "x"`) is fine and is filtered
//! out by Gate 1 below.
//!
//! The offense `range` is the full `Send` node range. The pre-arena
//! implementation used prism's `message_loc()` (just the `puts` selector
//! token); the arena AST does not retain a selector-only sub-range and
//! re-deriving one would require source scanning. The selector-only
//! narrowing was not part of the offense JSON contract (ADR 0006 — only
//! `cop_name` / `message` / `range` / `severity` are frozen) and the
//! coarser `Send` range is unambiguously the same offense site, so this
//! is the intended .22 trade-off.

use murphy_ast::{NodeId, NodeKind, OptNodeId};
use murphy_plugin_api::{
    Cop, CopOptions, Cx, CxRaw, NoOptions, NodeCop, NodeKindTag, PluginCopV1, RawSlice, Severity,
    tristate_to_wire,
};

/// Stateless unit struct, matching the const-metadata cop pattern (ADR 0035).
pub struct NoReceiverPuts;

impl Cop for NoReceiverPuts {
    type Options = NoOptions;
    const NAME: &'static str = "Murphy/NoReceiverPuts";
    const DESCRIPTION: &'static str =
        "Flag receiver-less `puts`/`print`/`p` calls; use a logger instead.";
    const DEFAULT_SEVERITY: Option<Severity> = Some(Severity::Warning);
    const DEFAULT_ENABLED: Option<bool> = Some(true);
}

/// `NodeKind::Send` tag — declaration order is frozen by ADR 0037; see
/// `murphy_ast::KIND_PATTERN_NAMES` where `"send"` is bound to `17`.
const SEND_TAG: NodeKindTag = NodeKindTag(17);

/// One-element `KINDS` slice. Held in a named `static` so `KINDS_PTR /
/// KINDS_LEN` in `COP` resolve to it at const-eval time (slice literal in
/// place would also work; a named slice keeps the size symbol explicit).
static KINDS: &[NodeKindTag] = &[SEND_TAG];

impl NodeCop for NoReceiverPuts {
    const KINDS: &'static [NodeKindTag] = KINDS;

    fn check(&self, node: NodeId, cx: &Cx<'_>) {
        // Pattern-match defends against the dispatcher feeding us a non-Send
        // node (it should not — `KINDS = [Send]` — but the `let-else` is
        // free, and a future kind aliasing accident here would silently
        // misreport without it).
        let NodeKind::Send {
            receiver, method, ..
        } = *cx.kind(node)
        else {
            return;
        };
        // Gate 1 (ADR 0001): an explicit receiver is intentional output.
        if receiver != OptNodeId::NONE {
            return;
        }
        // Gate 2: only the three bare-output method names. Compare against
        // the interner's resolved string; `cx.symbol_str` is a pure read.
        if !matches!(cx.symbol_str(method), "puts" | "print" | "p") {
            return;
        }
        cx.emit_offense(cx.range(node), "Use a logger instead of puts", None);
    }
}

/// The cop's per-node dispatch thunk. `unsafe extern "C"` so it crosses
/// the plugin ABI cleanly; `catch_unwind` traps any panic in
/// [`NoReceiverPuts::check`] so a buggy check cannot unwind across the
/// boundary (ADR 0038 safety contract).
unsafe extern "C" fn dispatch_thunk(node: NodeId, cx_raw: *const CxRaw) -> i32 {
    let result = std::panic::catch_unwind(|| {
        // Safety: the host upholds the `CxRaw` validity contract for the
        // duration of the call (ADR 0038); `Cx::from_raw` re-establishes
        // the safe lifetime.
        let cx = unsafe { Cx::from_raw(&*cx_raw) };
        NoReceiverPuts.check(node, &cx);
    });
    if result.is_ok() { 0 } else { 1 }
}

/// Static `PluginCopV1` consumed by the dispatch host (murphy-9cr.22).
/// Hand-written rather than via `register_cops!` — `murphy-core` cannot
/// take a `murphy-plugin-macros` dependency (design §4.7).
pub static COP: PluginCopV1 = PluginCopV1 {
    size: std::mem::size_of::<PluginCopV1>(),
    name: RawSlice::from_str(<NoReceiverPuts as Cop>::NAME),
    description: RawSlice::from_str(<NoReceiverPuts as Cop>::DESCRIPTION),
    default_severity: Severity::to_wire(<NoReceiverPuts as Cop>::DEFAULT_SEVERITY),
    default_enabled: tristate_to_wire(<NoReceiverPuts as Cop>::DEFAULT_ENABLED),
    options_ptr: <NoOptions as CopOptions>::SCHEMA.as_ptr(),
    options_len: <NoOptions as CopOptions>::SCHEMA.len(),
    kinds_ptr: KINDS.as_ptr(),
    kinds_len: KINDS.len(),
    dispatch: dispatch_thunk,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::{OffenseSink, run_cops};
    use murphy_ast::{AstBuilder, NodeKind, OptNodeId, Range};

    /// Build a one-`Send` arena: `[receiver].method(args)`, with `args` as a
    /// pre-built list of string nodes.
    fn build_send(
        source: &str,
        receiver: OptNodeId,
        method: &str,
        args: &[&str],
    ) -> murphy_ast::Ast {
        let mut b = AstBuilder::new(source, "t.rb");
        let arg_ids: Vec<_> = args
            .iter()
            .map(|s| {
                let sid = b.intern_string(s);
                b.push(NodeKind::Str(sid), Range { start: 0, end: 0 })
            })
            .collect();
        let args_list = b.push_list(&arg_ids);
        let m = b.intern_symbol(method);
        let send = b.push(
            NodeKind::Send {
                receiver,
                method: m,
                args: args_list,
            },
            Range {
                start: 0,
                end: source.len() as u32,
            },
        );
        b.finish(send)
    }

    #[test]
    fn flags_receiver_less_puts() {
        let ast = build_send("puts \"x\"", OptNodeId::NONE, "puts", &["x"]);
        let mut sink = OffenseSink::new("t.rb");
        run_cops(&ast, &[&COP], &mut sink);

        let offs = sink.into_offenses();
        assert_eq!(offs.len(), 1, "puts without a receiver must be flagged");
        assert_eq!(offs[0].cop_name, "Murphy/NoReceiverPuts");
        assert_eq!(offs[0].message, "Use a logger instead of puts");
    }

    #[test]
    fn flags_receiver_less_print_and_p() {
        for method in ["print", "p"] {
            let src = format!("{method} \"x\"");
            let ast = build_send(&src, OptNodeId::NONE, method, &["x"]);
            let mut sink = OffenseSink::new("t.rb");
            run_cops(&ast, &[&COP], &mut sink);
            assert_eq!(
                sink.into_offenses().len(),
                1,
                "receiver-less `{method}` must be flagged"
            );
        }
    }

    #[test]
    fn does_not_flag_call_with_explicit_receiver() {
        // Build `logger.puts "x"`.
        let mut b = AstBuilder::new("logger.puts \"x\"", "t.rb");
        let recv_sym = b.intern_symbol("logger");
        let recv = b.push(NodeKind::Lvar(recv_sym), Range { start: 0, end: 6 });
        let arg_str = b.intern_string("x");
        let arg = b.push(NodeKind::Str(arg_str), Range { start: 13, end: 16 });
        let args = b.push_list(&[arg]);
        let method = b.intern_symbol("puts");
        let send = b.push(
            NodeKind::Send {
                receiver: OptNodeId::some(recv),
                method,
                args,
            },
            Range { start: 0, end: 16 },
        );
        let ast = b.finish(send);

        let mut sink = OffenseSink::new("t.rb");
        run_cops(&ast, &[&COP], &mut sink);

        assert!(
            sink.into_offenses().is_empty(),
            "`logger.puts` has an explicit receiver and must not be flagged"
        );
    }

    #[test]
    fn does_not_flag_other_methods() {
        let ast = build_send("info \"x\"", OptNodeId::NONE, "info", &["x"]);
        let mut sink = OffenseSink::new("t.rb");
        run_cops(&ast, &[&COP], &mut sink);

        assert!(
            sink.into_offenses().is_empty(),
            "`info` is not in the bare-output gate and must not be flagged"
        );
    }

    #[test]
    fn cop_static_carries_correct_metadata() {
        assert_eq!(COP.size, std::mem::size_of::<PluginCopV1>());
        assert_eq!(unsafe { COP.name.as_bytes() }, b"Murphy/NoReceiverPuts");
        // Severity::Warning → wire 0.
        assert_eq!(COP.default_severity, 0);
        // Enabled by default → tristate true wire = 1.
        assert_eq!(COP.default_enabled, 1);
        assert_eq!(COP.kinds_len, 1);
        assert_eq!(unsafe { *COP.kinds_ptr }, SEND_TAG);
    }
}
