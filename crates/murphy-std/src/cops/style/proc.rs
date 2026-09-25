//! `Style/Proc` — use `proc` instead of `Proc.new`.
//!
//! ## RuboCop parity
//!
//! ```murphy-parity
//! upstream: rubocop
//! upstream_cop: Style/Proc
//! upstream_version_checked: 1.86.2
//! status: partial
//! gap_issues: []
//! notes: >
//!   Flags `Proc.new { ... }` and `::Proc.new { ... }` when used with a block,
//!   suggesting `proc` instead. `Proc.new` without a block is not flagged.
//!   Handles `block`, `numblock`, and `itblock` forms.
//!
//!   The offense range covers `Proc.new` (from the receiver start to the
//!   selector end). The autocorrect replaces that range with `proc`.
//! ```
//!
//! ## Matched shapes
//!
//! `Block`, `Numblock`, and `Itblock` nodes whose call child is
//! `(send (const {nil? cbase} :Proc) :new)` with no arguments to `new`.
//!
//! ## Enforcement logic
//!
//! Flag unconditionally when the shape matches. Offense range is just
//! `Proc.new` (receiver start to selector end). Autocorrect replaces it
//! with `proc`.

use murphy_plugin_api::{Cx, NodeId, NodeKind, Range, cop, def_node_matcher};

// RuboCop parity: `Style/Proc` inner send is
// `(send (const {nil? cbase} :Proc) :new)` (zero args, send-only).
// In Murphy `::Proc` collapses to `Const{scope:None}`: `nil?` covers bare +
// `::` (pinned by pre-existing `flags_cbase_proc_new`). Namespaced
// `Foo::Proc` still accepts (pinned by `boundary_accepts_namespaced_proc_new`).
// `send` covers `Send` only (not `Csend`), matching the `Send`-only check
// (pinned by `boundary_accepts_csend_proc_new`). Block extraction
// (`block`/`numblock`/`itblock`) and offense-range logic stay hand-rolled.
def_node_matcher!(proc_new, "(send (const nil? :Proc) :new)");

/// Stateless unit struct.
#[derive(Default)]
pub struct Proc;

const MSG: &str = "Use `proc` instead of `Proc.new`.";

#[cop(
    name = "Style/Proc",
    description = "Use `proc` instead of `Proc.new`.",
    default_severity = "warning",
    default_enabled = true,
)]
impl Proc {
    #[on_node(kind = "block")]
    fn check_block(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_call(node, cx);
    }

    #[on_node(kind = "numblock")]
    fn check_numblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_call(node, cx);
    }

    #[on_node(kind = "itblock")]
    fn check_itblock(&self, node: NodeId, cx: &Cx<'_>) {
        check_block_call(node, cx);
    }
}

fn check_block_call(node: NodeId, cx: &Cx<'_>) {
    // Extract the call child of the block node.
    let call = match *cx.kind(node) {
        NodeKind::Block { call, .. } => call,
        NodeKind::Numblock { send, .. } => send,
        NodeKind::Itblock { send, .. } => send,
        _ => return,
    };

    // `(send (const nil? :Proc) :new)` (zero args, `Send` only).
    // `nil?` covers `Proc` + `::Proc`; `send` excludes `Csend`.
    if !proc_new(call, cx) {
        return;
    }

    // Receiver for the offense range (`Proc.new` span). Always present when
    // the matcher above passes.
    let Some(recv_id) = cx.call_receiver(call).get() else {
        return;
    };

    // Offense range: from the start of the receiver to the end of the
    // `new` selector. This covers exactly `Proc.new` (or `::Proc.new`).
    let offense_range = Range {
        start: cx.range(recv_id).start,
        end: cx.selector(call).end,
    };
    cx.emit_offense(offense_range, MSG, None);
    cx.emit_edit(offense_range, "proc");
}

#[cfg(test)]
mod tests {
    use super::Proc;
    use murphy_plugin_api::test_support::{indoc, test};

    #[test]
    fn flags_proc_new_block() {
        test::<Proc>().expect_correction(
            indoc! {"
                f = Proc.new { |x| puts x }
                    ^^^^^^^^ Use `proc` instead of `Proc.new`.
            "},
            "f = proc { |x| puts x }\n",
        );
    }

    #[test]
    fn flags_cbase_proc_new() {
        test::<Proc>().expect_correction(
            indoc! {"
                f = ::Proc.new { |x| puts x }
                    ^^^^^^^^^^ Use `proc` instead of `Proc.new`.
            "},
            "f = proc { |x| puts x }\n",
        );
    }

    #[test]
    fn accepts_proc_new_without_block() {
        test::<Proc>().expect_no_offenses("p = Proc.new\n");
    }

    #[test]
    fn accepts_cbase_proc_new_without_block() {
        test::<Proc>().expect_no_offenses("p = ::Proc.new\n");
    }

    #[test]
    fn flags_numblock_proc_new() {
        test::<Proc>().expect_correction(
            indoc! {"
                f = Proc.new { puts _1 }
                    ^^^^^^^^ Use `proc` instead of `Proc.new`.
            "},
            "f = proc { puts _1 }\n",
        );
    }

    // --- Boundary characterization (murphy-ft88.6): pin the exact node set
    // the hand-rolled `Proc` guard matches, so the verbatim
    // `(send (const nil? :Proc) :new)` refactor can be proven equivalent.
    // `::Proc` collapses to `Const{scope:None}` in Murphy: `nil?` covers
    // bare + `::` (pinned by pre-existing `flags_cbase_proc_new`).
    // Namespaced `Foo::Proc` still accepts; `&.` is a `csend` node and
    // `send` covers `Send` only.

    #[test]
    fn boundary_accepts_namespaced_proc_new() {
        // Upstream `(const {nil? cbase} :Proc)` matches top-level only.
        test::<Proc>().expect_no_offenses("f = Foo::Proc.new { |x| x }\n");
    }

    #[test]
    fn boundary_accepts_csend_proc_new() {
        // `&.` is a `csend` node; `send` covers `Send` only.
        test::<Proc>().expect_no_offenses("f = Proc&.new { |x| x }\n");
    }
}
murphy_plugin_api::submit_cop!(Proc);
